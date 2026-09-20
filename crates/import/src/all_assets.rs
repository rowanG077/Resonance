//! Discover source packages once, then bind runtime assets from shared decoded inputs.
mod archive;
mod attachment;
#[cfg(test)]
pub(crate) use archive::Directory as PhysicalDirectory;
pub(crate) use archive::{FieldDirectory, MemberKind, entries as archive_entries};
mod audio_tables;
mod credits;
mod embedded;
#[cfg(test)]
pub(crate) use embedded::cook_tables;
pub(crate) use embedded::{
    cooking_ui, ex_skills, figurine_catalogue, inventory_ui, monster_catalogue, options_ui,
    rename_ui, save_menu, shop_ui, status_ui, strategy_ui, synopsis, technique_ui, title_catalogue,
    ui_style, world_map,
};
mod exclusions;
mod field;
pub(crate) use field::CameraTrack;
pub(crate) use field::camera;
pub(crate) mod field_unlocks;
pub(crate) mod geometry;
mod overworld_collision;
mod overworld_encounters;
pub(crate) mod physical_scene;
pub(crate) mod pool;
mod preparation;
pub(crate) mod reuse;
pub(crate) mod roles;
pub(crate) mod skits;

use crate::{
    media::{self, library as audio},
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use roles::Role;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(crate) type MemberResults = Vec<(String, Result<Vec<String>>)>;

#[derive(Clone, Debug, Serialize)]
pub struct Failure {
    pub path: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeferredAsset {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    pub reason: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct Deferred(pub &'static str);

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub cooked: usize,
    pub duplicates: usize,
    pub reused: usize,
    pub failures: Vec<Failure>,
    pub deferred: Vec<DeferredAsset>,
}

impl Report {
    fn record(&mut self, path: &str, result: Result<()>) {
        match result {
            Ok(()) => self.cooked += 1,
            Err(error) if error.is::<Deferred>() => self.deferred.push(DeferredAsset {
                path: path.into(),
                source_sha256: None,
                reason: error.to_string(),
            }),
            Err(error) => self.fail(Failure {
                path: path.into(),
                error: format!("{error:#}"),
            }),
        }
    }
    fn fail(&mut self, failure: Failure) {
        eprintln!("{}: {}", failure.path, failure.error);
        self.failures.push(failure);
    }
    fn merge(&mut self, other: Self) {
        self.cooked += other.cooked;
        self.duplicates += other.duplicates;
        self.reused += other.reused;
        self.failures.extend(other.failures);
        self.deferred.extend(other.deferred);
    }
}

pub struct Options<'a> {
    pub discs: &'a [PathBuf],
    pub output: &'a Path,
    pub coefficients: &'a Path,
    /// Maximum number of concurrent asset converters.
    pub jobs: usize,
}

struct File {
    relative: String,
    hash: String,
    role: Option<Role>,
}
struct Disc<'a> {
    extracted: &'a Path,
    audio: Result<audio::Cooker>,
    movies: Vec<File>,
    executable_hash: String,
    skit_key: String,
    reuse_key: Option<String>,
}
enum Job {
    File(File),
    Voice {
        member: audio::ArchiveMember,
        hash: String,
    },
}

#[derive(Clone)]
struct CookedJob {
    key: String,
    paths: Vec<String>,
    report: Report,
    recovered: Arc<crate::scene::recovered::RecoveredModels>,
}

impl std::fmt::Debug for CookedJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookedJob")
            .field("report", &self.report)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("asset conversion failed")]
struct FailedJob(CookedJob);

impl Job {
    fn label(&self) -> String {
        match self {
            Self::File(file) => file.relative.clone(),
            Self::Voice { member, .. } => member.label.clone(),
        }
    }

    fn reuse_key(&self, cache: &reuse::Cache, source: &Disc<'_>) -> Result<Option<String>> {
        let Some(context) = &source.reuse_key else {
            return Ok(None);
        };
        let key = match self {
            Self::File(file) => cache.key(&(
                context,
                "file",
                &file.hash,
                &file.relative,
                file.role.map(|role| role as u8),
            ))?,
            Self::Voice { member, hash } => cache.key(&(context, "voice", hash, &member.label))?,
        };
        Ok(Some(key))
    }

    fn output_key(&self) -> &str {
        match self {
            Self::File(file) => &file.hash,
            Self::Voice { hash, .. } => hash,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Exclusion {
    NativeCode,
    BuildMetadata,
    DiscMetadata,
    BattleSemantics,
}

/// Bound simultaneous decoded assets while leaving headroom for codec workers.
pub const MAX_WORKERS: usize = 11;

fn source_discs(paths: &[PathBuf]) -> Result<BTreeMap<u8, &Path>> {
    ensure!(!paths.is_empty(), "no extracted discs supplied");
    let mut discs = BTreeMap::new();
    for path in paths {
        let disc = crate::disc_number(path)?;
        ensure!(
            discs.insert(disc, path.as_path()).is_none(),
            "duplicate extracted disc {disc}: {}",
            path.display()
        );
    }
    Ok(discs)
}

pub fn cook(options: &Options<'_>) -> Result<Report> {
    let discs = source_discs(options.discs)?;
    ensure!(
        (1..=MAX_WORKERS).contains(&options.jobs),
        "worker count must be 1..={MAX_WORKERS}"
    );
    // Workers convert assets in-process; the caller collects results.
    let workers = options.jobs;
    // Resolve roles across both discs before content deduplication. An alias
    // encountered first must get the same reader as its declared resource.
    let mut declared = Vec::new();
    for extracted in discs.values() {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let fields = crate::field_catalogue::map_paths(extracted)?
            .into_iter()
            .map(|path| (path, Role::Field));
        let fonts = crate::font_directory::Directory::read(&executable)?
            .paths(&extracted.join("files"))?
            .into_iter()
            .map(|path| (path, Role::Font));
        declared.extend(
            fields
                .chain(fonts)
                .chain(roles::battle_paths(extracted)?)
                .chain(roles::audio_paths(extracted, &executable)?)
                .chain([(roles::credits_path(extracted, &executable)?, Role::Credits)])
                .map(|(path, role)| (extracted.join("files").join(path), role)),
        );
    }
    let mut declared_hashes = BTreeMap::new();
    let mut hashing = pool::Dag::new();
    for (path, _) in &declared {
        hashing.add(path.display().to_string(), [], move |_, _| {
            Ok((path.clone(), media::hash_file(path)?))
        });
    }
    for status in hashing.run(
        workers,
        || (),
        |completion| {
            if let Ok(value) = completion.result::<(PathBuf, String)>() {
                declared_hashes.insert(value.0.clone(), value.1.clone());
            }
        },
    )? {
        status?;
    }
    let mut roles = BTreeMap::new();
    for (path, role) in &declared {
        let hash = &declared_hashes[path];
        if let Some(previous) = roles.insert(hash.clone(), *role) {
            ensure!(
                previous == *role,
                "conflicting resource roles for {}: {previous:?} and {role:?}",
                path.display()
            );
        }
    }
    let output_session = media::OutputSession::open(options.output)?;
    let _publications = crate::publication::Session::start_if_needed(options.output)?;
    let cache = reuse::Cache::open(options.output)?;
    let catalogue = options.output.join("sources.json");
    match fs::remove_file(&catalogue) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("invalidate previous source catalogue"),
    }
    let mut report = Report::default();
    let mut seen = BTreeSet::new();
    let mut tables = BTreeMap::<String, Vec<String>>::new();
    let mut skit_tables = BTreeMap::<String, Vec<String>>::new();
    let mut embedded_cache = embedded::Cache::default();
    let mut sources = BTreeMap::new();
    let mut excluded = BTreeMap::new();
    let mut outputs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut all_jobs = Vec::new();
    let mut pending = BTreeMap::new();
    for (&disc, &extracted) in &discs {
        let files = walk(&extracted.join("files"), &mut report);
        let mut hashed = BTreeMap::new();
        let mut hashing = pool::Dag::new();
        for path in &files {
            let declared_hashes = &declared_hashes;
            hashing.add(path.display().to_string(), [], move |_, _| {
                let relative = path
                    .strip_prefix(extracted.join("files"))?
                    .to_str()
                    .context("non-UTF8 disc path")?
                    .to_owned();
                let hash = declared_hashes
                    .get(path)
                    .cloned()
                    .map_or_else(|| media::hash_file(path), Ok)?;
                Ok((relative, hash))
            });
        }
        let statuses = hashing.run(
            workers,
            || (),
            |completion| {
                if let Ok(value) = completion.result::<(String, String)>() {
                    hashed.insert(value.0.clone(), value.1.clone());
                }
            },
        )?;
        for (path, status) in files.iter().zip(statuses) {
            if let Err(error) = status {
                report.record(&path.display().to_string(), Err(error));
            }
        }
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let executable_hash = crate::digest(&executable);
        let portrait = crate::skit::portrait_path(extracted, &executable)?;
        let skit_key = crate::digest(&serde_json::to_vec(&(
            &executable_hash,
            &portrait,
            hashed
                .get(&portrait)
                .context("portrait archive absent from inventory")?,
        ))?);
        let executable_label = format!("disc{}/sys/main.dol", disc);
        sources.insert(executable_label.clone(), executable_hash.clone());
        excluded.insert(executable_label, Exclusion::NativeCode);
        for path in walk(&extracted.join("sys"), &mut report) {
            let relative = path.strip_prefix(extracted)?.to_string_lossy();
            if relative == "sys/main.dol" {
                continue;
            }
            let label = format!("disc{}/{relative}", disc);
            let result = (|| {
                sources.insert(label.clone(), media::hash_file(&path)?);
                let reason = match relative.as_ref() {
                    "sys/apploader.img" => Exclusion::NativeCode,
                    "sys/boot.bin" | "sys/bi2.bin" | "sys/fst.bin" => Exclusion::DiscMetadata,
                    _ => anyhow::bail!("unclassified disc system file"),
                };
                excluded.insert(label.clone(), reason);
                Ok(())
            })();
            if result.is_err() {
                report.record(&label, result);
            }
        }
        let mut jobs = Vec::new();
        let deferred_sources =
            crate::source_assets::Sources::read_with(extracted, &executable)?.deferred_paths();
        let audio = audio::Cooker::new(extracted, &output_session, options.coefficients, &hashed);
        // The audio environment covers executable/REL constants, inherited pools,
        // coefficient contents and song setup aliases. Boot bytes also select text encodings.
        let reuse_key = audio
            .as_ref()
            .ok()
            .map(|audio| {
                cache.key(&(
                    audio.resource_key(&executable_hash),
                    media::hash_file(&extracted.join("sys/boot.bin"))?,
                ))
            })
            .transpose()?;
        let mut movies = Vec::new();
        for (relative, mut hash) in hashed {
            let source = extracted.join("files").join(&relative);
            let extension = extension(&relative);
            let mut role = roles.get(&hash).copied();
            let label = format!("disc{}/{relative}", disc);
            sources.insert(label.clone(), hash.clone());
            if role == Some(Role::Font) || (role.is_none() && embedded::owns(&relative)) {
                continue;
            }
            if role.is_none() && matches!(extension.as_str(), "rel" | "sfv" | "str") {
                if extension != "rel"
                    && let Err(error) = exclusions::validate_build_metadata(&source)
                {
                    report.record(&label, Err(error));
                    continue;
                }
                if extension == "rel"
                    && Path::new(&relative)
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().contains("Top2Btl"))
                {
                    report.deferred.push(DeferredAsset {
                        path: label.clone(), source_sha256: Some(hash.clone()),
                        reason: "battle source tables and native gameplay preparation; embedded artwork is still decoded".into(),
                    });
                }
                excluded.insert(
                    label,
                    if extension == "rel" {
                        Exclusion::NativeCode
                    } else {
                        Exclusion::BuildMetadata
                    },
                );
                continue;
            }
            if deferred_sources.contains(&relative)
                || matches!(role, Some(Role::Victory | Role::BattleSkit))
            {
                excluded.insert(label.clone(), Exclusion::BattleSemantics);
                report.deferred.push(DeferredAsset {
                    path: label,
                    source_sha256: Some(hash.clone()),
                    reason: "battle gameplay, effects and presentation are deferred; shared menu/audio readers may consume this source".into(),
                });
                continue;
            }
            let header = match read_header(&source) {
                Ok(header) => header,
                Err(error) => {
                    report.record(&label, Err(error.into()));
                    continue;
                }
            };
            if role.is_none() {
                match audio::file_role(&source) {
                    Ok(detected) => role = detected,
                    Err(error) => {
                        report.record(&label, Err(error));
                        continue;
                    }
                }
            }
            if matches!(role, Some(Role::SoundBank | Role::Song | Role::VoiceBank)) {
                match &audio {
                    Ok(audio) => hash = audio.resource_key(&hash),
                    Err(error) => {
                        report.record(&label, Err(anyhow::anyhow!("{error:#}")));
                        continue;
                    }
                }
                sources.insert(label.clone(), hash.clone());
            }
            let fresh = seen.insert(hash.clone());
            if !fresh {
                report.duplicates += 1;
            }
            let file = File {
                relative,
                hash,
                role,
            };
            if role.is_none() && media::is_movie(&header) {
                if fresh {
                    movies.push(file);
                }
                continue;
            }
            let result = (|| -> Result<bool> {
                if role.is_some() || !header.starts_with(b"AFS\0") {
                    return Ok(false);
                }
                if !fresh {
                    return Ok(true);
                }
                let audio = audio.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))?;
                let (index, members) = audio.archive_members(&file.relative, &file.hash)?;
                outputs.entry(file.hash.clone()).or_default().push(index);
                jobs.extend(members.into_iter().map(|member| Job::Voice {
                    member,
                    hash: file.hash.clone(),
                }));
                Ok(true)
            })();
            match result {
                Ok(false) if fresh => jobs.push(Job::File(file)),
                Ok(_) => {}
                Err(error) => report.record(&label, Err(error)),
            }
        }
        all_jobs.extend(jobs.into_iter().map(|job| (disc, job)));
        pending.insert(
            disc,
            Disc {
                extracted,
                audio,
                movies,
                executable_hash,
                skit_key,
                reuse_key,
            },
        );
    }
    let fields = preparation::discover(&discs, &mut sources, &mut report)?;
    let required = fields
        .iter()
        .flat_map(|field| field.dependencies.iter().cloned())
        .collect::<BTreeSet<_>>();
    let maps = fields
        .iter()
        .map(|field| (field.hash.clone(), Arc::clone(&field.archive)))
        .collect::<BTreeMap<_, _>>();
    // Publish independent packages before retaining the shared inputs and field archives.
    all_jobs.sort_by_key(|(_, job)| {
        (
            maps.contains_key(job.output_key()),
            required.contains(job.output_key()),
        )
    });
    let aliases = sources.iter().fold(
        BTreeMap::<String, Vec<String>>::new(),
        |mut aliases, (path, hash)| {
            if let Some((_, path)) = path.split_once('/') {
                aliases
                    .entry(hash.clone())
                    .or_default()
                    .push(path.to_owned());
            }
            aliases
        },
    );
    let primary = *discs.first_key_value().context("no source disc")?.1;
    let mut packages = BTreeMap::new();
    eprintln!(
        "Cooking general assets: {} jobs on {workers} workers",
        all_jobs.len()
    );
    let started = AtomicUsize::new(0);
    let mut dag = pool::Dag::new();
    let mut physical_dependencies = Vec::new();
    let mut shared = None;
    for (disc, job) in &all_jobs {
        let (pending, cache, started, maps, aliases) =
            (&pending, &cache, &started, &maps, &aliases);
        let retain = required.contains(job.output_key());
        if retain && shared.is_none() {
            let discs = &discs;
            let sources = &sources;
            let prepared = dag.add(
                "shared presentation",
                physical_dependencies.clone(),
                move |_, _| {
                    let shared = crate::shared::prepare(primary, options.output, sources)?;
                    for extracted in discs.values() {
                        ensure!(
                            crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?
                                == shared.catalogue,
                            "resource catalogues disagree across source discs"
                        );
                    }
                    Ok(shared)
                },
            );
            dag.estimate(prepared, 16 * 1024 * 1024, 64 * 1024 * 1024)?;
            shared = Some(prepared);
        }
        let job_count = all_jobs.len();
        let handle = dag.add(
            format!("disc{disc}/{}", job.label()),
            shared.map(|shared| shared.dependency()),
            move |_, _| {
                let source = &pending[disc];
                let extracted = source.extracted;
                let audio = &source.audio;
                let reuse_key = job.reuse_key(cache, source)?;
                if !retain && let Some(key) = &reuse_key {
                    if let Some(receipt) = cache.restore(key) {
                        receipt.register(options.output)?;
                        return Ok(CookedJob {
                            key: job.output_key().to_owned(),
                            paths: receipt.paths,
                            recovered: Arc::default(),
                            report: Report {
                                cooked: receipt.units,
                                reused: 1,
                                ..Report::default()
                            },
                        });
                    }
                    cache.invalidate(key)?;
                }
                let capture = reuse_key.as_ref().map(|_| reuse::Capture::start());
                let count = started.fetch_add(1, Ordering::Relaxed) + 1;
                if count == 1 || count.is_multiple_of(100) {
                    eprintln!("Starting job {count}/{}", job_count);
                }
                let mut local = Report::default();
                let mut recovered = crate::scene::recovered::RecoveredModels::default();
                let paths = match job {
                    Job::Voice { member, .. } => audio
                        .as_ref()
                        .map_err(|error| anyhow::anyhow!("{error:#}"))?
                        .cook_member(member)?,
                    Job::File(file) => match cook_file(
                        options,
                        extracted,
                        file,
                        audio,
                        &mut local,
                        retain.then_some(Recovery {
                            map: maps.get(&file.hash).map(Arc::as_ref),
                            models: &mut recovered,
                            aliases: aliases
                                .get(&file.hash)
                                .map(Vec::as_slice)
                                .unwrap_or_default(),
                        }),
                    ) {
                        Ok(paths) => paths,
                        Err(error) => {
                            local.record(&format!("disc{disc}/{}", file.relative), Err(error));
                            Vec::new()
                        }
                    },
                };
                if let Job::File(file) = job {
                    for deferred in &mut local.deferred {
                        if !deferred.path.starts_with(&format!("disc{disc}/")) {
                            deferred.path.insert_str(0, &format!("disc{disc}/"));
                        }
                        deferred.source_sha256 = Some(file.hash.clone());
                    }
                }
                let succeeded = local.failures.is_empty();
                local.cooked += usize::from(succeeded && !paths.is_empty());
                if succeeded
                    && local.deferred.is_empty()
                    && let (Some(key), Some(capture)) = (reuse_key, capture)
                {
                    cache.publish(&key, &paths, local.cooked, capture.finish()?)?;
                }
                let result = CookedJob {
                    key: job.output_key().to_owned(),
                    paths,
                    report: local,
                    recovered: Arc::new(recovered),
                };
                if succeeded {
                    Ok(result)
                } else {
                    Err(FailedJob(result).into())
                }
            },
        );
        dag.estimate(
            handle,
            if retain { 16 * 1024 * 1024 } else { 4096 },
            64 * 1024 * 1024,
        )?;
        if !retain {
            physical_dependencies.push(handle.dependency());
        }
        packages.insert(job.output_key().to_owned(), handle);
    }
    preparation::add(
        &mut dag,
        &fields,
        &packages,
        shared.context("no field preparation jobs")?,
        options.output,
    )?;
    dag.run_bounded(
        workers,
        2 * 1024 * 1024 * 1024,
        || (),
        |completion| {
            if let Some(error) = completion.error() {
                if let Some(failure) = error.downcast_ref::<FailedJob>() {
                    report.merge(failure.0.report.clone());
                } else {
                    report.fail(Failure {
                        path: completion.name.into(),
                        error: format!("{error:#}"),
                    });
                }
            } else if let Ok(result) = completion.result::<CookedJob>() {
                outputs
                    .entry(result.key.clone())
                    .or_default()
                    .extend(result.paths.clone());
                if !result.report.deferred.is_empty() {
                    excluded.insert(completion.name.into(), Exclusion::BattleSemantics);
                }
                report.merge(result.report.clone());
            }
        },
    )?;
    write_atomic(
        &options.output.join("progress.json"),
        &serde_json::to_vec(&serde_json::json!({
            "finished_asset_queues_for_discs": pending.keys().collect::<Vec<_>>(),
            "converted_units": report.cooked,
            "failures": report.failures,
        }))?,
    )?;
    for (
        disc,
        Disc {
            extracted,
            movies,
            executable_hash,
            skit_key,
            ..
        },
    ) in pending
    {
        // Convert movies separately to bound disk use by full-frame intermediates.
        for file in movies {
            let name = format!("assets/{}", file.hash);
            eprintln!("Cooking disc{}/{}", disc, file.relative);
            let result = media::cook_movie_file(
                extracted,
                Path::new(&file.relative),
                &options.output.join(&name),
            );
            if result.is_ok() {
                outputs.insert(file.hash, vec![name]);
            }
            report.record(
                &format!("disc{}/{}", disc, file.relative),
                result.map(|_| ()),
            );
        }
        let data = options.output.join("data");
        if let Some(paths) = tables.get(&executable_hash) {
            outputs
                .entry(executable_hash.clone())
                .or_default()
                .extend(paths.clone());
            report.duplicates += 1;
        } else {
            let prefix = if tables.is_empty() {
                "data".into()
            } else {
                format!("data/variants/{executable_hash}")
            };
            let table_output = options.output.join(&prefix);
            let mut paths = Vec::new();
            for (label, result) in [
                (
                    "session tables",
                    crate::session::cook(extracted, &table_output).map(|path| vec![path]),
                ),
                (
                    "localized text",
                    crate::session::cook_text(extracted, &table_output).map(|path| vec![path]),
                ),
                ("audio tables", audio_tables::cook(extracted, &table_output)),
                (
                    "music directory",
                    crate::music_directory::Directory::cook(extracted, &table_output),
                ),
                (
                    "voice directory",
                    crate::voice_directory::Directory::cook(extracted, &table_output),
                ),
                (
                    "event bank directory",
                    crate::event_bank_directory::Directory::cook(extracted, &table_output),
                ),
                (
                    "stream mixer",
                    crate::stream_mixer::Tables::cook(extracted, &table_output),
                ),
                (
                    "font directory",
                    crate::font_directory::Directory::cook(extracted, &table_output),
                ),
            ] {
                report.record(
                    &format!("disc{}/{label}", disc),
                    result.map(|cooked| {
                        paths.extend(cooked.into_iter().map(|path| format!("{prefix}/{path}")))
                    }),
                );
            }
            outputs
                .entry(executable_hash.clone())
                .or_default()
                .extend(paths.clone());
            tables.insert(executable_hash.clone(), paths);
        }
        let skits = if let Some(paths) = skit_tables.get(&skit_key) {
            report.duplicates += 1;
            Ok(paths.clone())
        } else {
            skits::cook(extracted, &data).map(|paths| {
                let paths: Vec<_> = paths
                    .into_iter()
                    .map(|path| format!("data/{path}"))
                    .collect();
                skit_tables.insert(skit_key, paths.clone());
                paths
            })
        };
        report.record(
            &format!("disc{disc}/skit tables"),
            skits.map(|paths| {
                outputs
                    .entry(executable_hash.clone())
                    .or_default()
                    .extend(paths);
            }),
        );
        let hits = embedded_cache.hits;
        let prefix = format!("disc{disc}/");
        let source_hashes = sources
            .iter()
            .filter_map(|(source, hash)| {
                source
                    .strip_prefix(&prefix)
                    .map(|path| (path.to_owned(), hash.clone()))
            })
            .collect();
        let result = embedded::cook(
            extracted,
            &data,
            workers,
            &mut embedded_cache,
            &mut |path, result| {
                report.record(&format!("disc{}/{path}", disc), result);
            },
            &source_hashes,
        );
        report.duplicates += embedded_cache.hits - hits;
        match result {
            Ok(embedded) => {
                for (source, paths) in embedded {
                    let hash = media::hash_file(&extracted.join(&source))?;
                    let label = format!(
                        "disc{}/{}",
                        disc,
                        source.strip_prefix("files/").unwrap_or(&source)
                    );
                    sources.insert(label.clone(), hash.clone());
                    if source == "sys/main.dol" {
                        excluded.insert(label, Exclusion::NativeCode);
                    }
                    outputs
                        .entry(hash)
                        .or_default()
                        .extend(paths.into_iter().map(|path| format!("data/{path}")));
                }
            }
            Err(error) => report.record(&format!("disc{}/embedded assets", disc), Err(error)),
        }
    }
    for paths in outputs.values_mut() {
        paths.sort();
        paths.dedup();
    }
    let sources = sources
        .into_iter()
        .map(|(source, hash)| {
            let paths = outputs.get(&hash).cloned().unwrap_or_default();
            (source, paths)
        })
        .collect::<BTreeMap<_, _>>();
    for (source, paths) in &sources {
        if !excluded.contains_key(source) && paths.is_empty() {
            report.record(
                source,
                Err(anyhow::anyhow!("asset produced no cooked output")),
            );
        }
        for path in paths {
            if !options.output.join(path).exists() {
                report.record(
                    source,
                    Err(anyhow::anyhow!("cooked output is missing: {path}")),
                );
            }
        }
    }
    write_atomic(
        &options.output.join("excluded.json"),
        &serde_json::to_vec_pretty(&excluded)?,
    )?;
    write_atomic(
        &options.output.join("deferred.json"),
        &serde_json::to_vec_pretty(&report.deferred)?,
    )?;
    report.record(
        "runtime preparation",
        preparation::finish(options, &fields, &output_session, &sources),
    );
    write_atomic(
        &options.output.join("failures.json"),
        &serde_json::to_vec_pretty(&report.failures)?,
    )?;
    write_atomic(
        &options.output.join("summary.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "converted_units": report.cooked, "source_files": sources.len(),
            "excluded_files": excluded.len(), "duplicates": report.duplicates,
            "reused_jobs": report.reused, "scope": "general-assets", "deferred": report.deferred.len(),
            "failures": report.failures.len()
        }))?,
    )?;
    fs::remove_file(options.output.join("progress.json"))?;
    if report.failures.is_empty() {
        write_atomic(&catalogue, &serde_json::to_vec_pretty(&sources)?)?;
    }
    Ok(report)
}

fn extension(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn read_header(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut header = Vec::with_capacity(16);
    fs::File::open(path)?.take(16).read_to_end(&mut header)?;
    Ok(header)
}

#[cfg(test)]
pub(crate) fn cook_source(
    extracted: &Path,
    output: &Path,
    relative: &str,
    role: Option<Role>,
) -> Result<Vec<String>> {
    let options = Options {
        discs: &[],
        output,
        coefficients: Path::new("unused"),
        jobs: 1,
    };
    let file = File {
        relative: relative.into(),
        hash: media::hash_file(&extracted.join("files").join(relative))?,
        role,
    };
    let mut report = Report::default();
    let paths = cook_file(
        &options,
        extracted,
        &file,
        &Err(anyhow::anyhow!("audio not requested")),
        &mut report,
        None,
    )?;
    ensure!(
        report.failures.is_empty(),
        "file cook failed: {:?}",
        report.failures
    );
    Ok(paths)
}

struct Recovery<'a> {
    map: Option<&'a crate::field::MapArchive>,
    models: &'a mut crate::scene::recovered::RecoveredModels,
    aliases: &'a [String],
}

fn cook_file(
    options: &Options<'_>,
    extracted: &Path,
    file: &File,
    audio: &Result<audio::Cooker>,
    report: &mut Report,
    mut recovery: Option<Recovery<'_>>,
) -> Result<Vec<String>> {
    eprintln!("Cooking {}", file.relative);
    let source = extracted.join("files").join(&file.relative);
    let stream = file.role.is_none() && audio::is_stream(&read_header(&source)?);
    let bytes = if stream
        || matches!(
            file.role,
            Some(Role::VoiceBank | Role::SoundBank | Role::Song)
        ) {
        None
    } else {
        Some(Arc::new(fs::read(&source)?))
    };
    if let (Some(recovery), Some(bytes)) = (&mut recovery, &bytes) {
        for alias in recovery.aliases {
            recovery.models.remember_source(alias, Arc::clone(bytes));
        }
    }
    let bytes = bytes.as_deref().map(Vec::as_slice);
    let role = file.role.or_else(|| bytes.and_then(audio::structural_role));
    if stream || matches!(role, Some(Role::VoiceBank | Role::SoundBank | Role::Song)) {
        let audio = audio.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))?;
        let members = if stream {
            audio
                .cook_stream(&file.relative)
                .map(|files| vec![(file.relative.clone(), Ok(files))])
        } else {
            match role {
                Some(Role::VoiceBank) => audio.cook_bank_archive(&file.relative),
                Some(Role::SoundBank) => audio.cook_bank(&file.relative),
                Some(Role::Song) => audio.cook_song(&file.relative),
                _ => unreachable!(),
            }
        }?;
        let mut outputs = Vec::new();
        for (member, result) in members {
            report.record(
                &format!("{}/{member}", file.relative),
                result.map(|files| outputs.extend(files)),
            );
        }
        return Ok(outputs);
    }
    let bytes = bytes.context("non-audio resource has no payload")?;
    let name = format!("assets/{}", file.hash);
    if file.role.is_none() && matches!(bytes.get(..4), Some(b"BNR1" | b"BNR2")) {
        let boot = fs::read(extracted.join("sys/boot.bin"))?;
        return embedded::cook_banner(bytes, boot.get(3) == Some(&b'J'), &name, options.output);
    }
    let credits = match file.role {
        Some(Role::Credits) => Some(credits::decode(bytes)?),
        None => credits::detect(bytes),
        _ => None,
    };
    if let Some(credits) = credits {
        let path = format!("{name}/credits.json");
        write_atomic(&options.output.join(&path), &serde_json::to_vec(&credits)?)?;
        return Ok(vec![path]);
    }
    if matches!(file.role, Some(Role::Victory | Role::BattleSkit)) {
        return Err(Deferred("battle result and dialogue programs").into());
    }
    ensure!(
        file.role != Some(Role::Font),
        "font reached ordinary asset queue"
    );
    let input = if file.role == Some(Role::Field) {
        geometry::Input::Field
    } else {
        geometry::Input::File
    };
    let (bytes, child) = if let Some(map) = recovery.as_ref().and_then(|recovery| recovery.map) {
        write_atomic(
            &options.output.join(&name).join("cabinet.json"),
            &serde_json::to_vec(&[&map.member])?,
        )?;
        (map.bytes.as_slice(), format!("{name}/{}", map.member))
    } else {
        (bytes, name.clone())
    };
    let mut record =
        |member: &str, result| report.record(&format!("{}/{member}", file.relative), result);
    let recognized = match recovery {
        Some(recovery) => geometry::cook_recovered(
            bytes,
            &child,
            options.output,
            audio.as_ref().ok(),
            input,
            recovery.models,
            &mut record,
        ),
        None => geometry::cook(
            bytes,
            &child,
            options.output,
            audio.as_ref().ok(),
            input,
            &mut record,
        ),
    };
    ensure!(recognized, "no decoder for this asset format");
    Ok(if options.output.join(&name).exists() {
        vec![name]
    } else {
        Vec::new()
    })
}

fn walk(root: &Path, report: &mut Report) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            report.record(&root.display().to_string(), Err(error.into()));
            return paths;
        }
    };
    for entry in entries {
        let result = (|| -> Result<()> {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                paths.extend(walk(&entry.path(), report));
            } else if kind.is_file() {
                paths.push(entry.path());
            } else {
                anyhow::bail!(
                    "unsupported disc filesystem entry: {}",
                    entry.path().display()
                );
            }
            Ok(())
        })();
        if result.is_err() {
            report.record(&root.display().to_string(), result);
        }
    }
    paths.sort();
    paths
}

fn script_header(bytes: &[u8]) -> Option<symphonia_script::scenario::Header> {
    let header = symphonia_script::scenario::parse_header(bytes).ok()?;
    (header.code_base() >= 8 + usize::from(header.registry_count) * 12
        && header.auxiliary_offset() > header.code_base()
        && header.auxiliary_offset() <= bytes.len())
    .then_some(header)
}

pub(crate) struct DecodedScript<'a> {
    pub bytes: &'a [u8],
    pub messages: Vec<symphonia_script::message::Message>,
}

/// Validate the original raw or wrapped program once, independently of publication.
pub(crate) fn decode_script(bytes: &[u8]) -> Result<Option<DecodedScript<'_>>> {
    use symphonia_script::{Program, message};
    let wrapped = bytes.get(..4) == Some(&2u32.to_be_bytes())
        && bytes.get(4..8) == Some(&32u32.to_be_bytes());
    let container = bytes;
    let bytes = if wrapped {
        let Some(payload) = bytes.get(32..) else {
            return Ok(None);
        };
        payload
    } else {
        bytes
    };
    let Some(header) = script_header(bytes) else {
        return Ok(None);
    };
    // Two-member model archives share this prefix. Check the payload's script
    // header before interpreting the third word as a length instead of an offset.
    if wrapped {
        ensure!(
            crate::read::u32(container, 8)? as usize == bytes.len(),
            "script package size does not match its payload"
        );
    }
    Program::decode(bytes)?;
    let messages = message::parse(&bytes[header.auxiliary_offset()..])?;
    Ok(Some(DecodedScript { bytes, messages }))
}

/// Keep validated legacy bytecode and decoded messages without native-call admission.
pub(crate) fn cook_script(bytes: &[u8], name: &str, output: &Path) -> Result<bool> {
    let Some(script) = decode_script(bytes)? else {
        return Ok(false);
    };
    write_atomic(&output.join(name).join("script.ssb"), script.bytes)?;
    write_atomic(
        &output.join(name).join("messages.json"),
        &serde_json::to_vec(&script.messages)?,
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(output: &Path) -> Options<'_> {
        Options {
            discs: &[],
            output,
            coefficients: Path::new("unused"),
            jobs: 1,
        }
    }

    #[test]
    fn declared_roles_override_filename_formats_and_reject_damaged_inputs() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("declared-asset-routing"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files/renamed"))?;
            let output = root.join("output");
            let options = options(&output);
            let audio = Err(anyhow::anyhow!("declared resources must not use audio"));
            let mut bytes = vec![0; 2048];
            for base in bytes[..16].chunks_exact_mut(4) {
                base.copy_from_slice(&128u32.to_be_bytes());
            }
            let mut file = File {
                relative: "renamed/archive.song".into(),
                hash: crate::digest(&bytes),
                role: Some(Role::BattleSkit),
            };
            fs::write(root.join("files").join(&file.relative), &bytes)?;
            let mut report = Report::default();
            let error = cook_file(&options, &root, &file, &audio, &mut report, None).unwrap_err();
            assert!(error.is::<Deferred>());
            assert_eq!(report.cooked, 0);
            assert!(!output.exists());

            file.relative = "renamed/field.txt".into();
            file.role = Some(Role::Field);
            file.hash = crate::digest(&[0; 8]);
            fs::write(root.join("files").join(&file.relative), [0; 8])?;
            assert!(cook_file(&options, &root, &file, &audio, &mut report, None)?.is_empty());
            assert_eq!(report.failures.len(), 1);
            assert!(!output.join(format!("assets/{}", file.hash)).exists());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires both extracted discs; reads only file headers"]
    fn original_media_signatures_match_every_physical_file() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in 1..=2 {
            let mut report = Report::default();
            let files = walk(&root.join(format!("disc{disc}/files")), &mut report);
            assert!(report.failures.is_empty() && !files.is_empty());
            for path in files {
                let header = read_header(&path)?;
                let extension = extension(path.to_str().context("invalid source path")?);
                assert_eq!(
                    media::is_movie(&header),
                    extension == "h4m",
                    "{}",
                    path.display()
                );
                assert_eq!(
                    audio::is_stream(&header),
                    matches!(extension.as_str(), "adx" | "ahx"),
                    "{}",
                    path.display()
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires an extracted disc; converts only two banner images"]
    fn banners_cook_by_signature_at_arbitrary_paths() -> Result<()> {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let root = crate::temporary_path(&std::env::temp_dir().join("banner-routing"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files/nested"))?;
            fs::create_dir_all(root.join("sys"))?;
            fs::copy(source.join("sys/boot.bin"), root.join("sys/boot.bin"))?;
            let original = fs::read(source.join("files/opening.bnr"))?;
            let output = root.join("output");
            let options = options(&output);
            for (magic, languages) in [(b"BNR1", 1), (b"BNR2", 6)] {
                let mut bytes = original[..0x1820].to_vec();
                bytes[..4].copy_from_slice(magic);
                for _ in 0..languages {
                    bytes.extend_from_slice(&original[0x1820..0x1960]);
                }
                let file = File {
                    relative: "nested/unrelated.data".into(),
                    hash: crate::digest(&bytes),
                    role: None,
                };
                fs::write(root.join("files").join(&file.relative), &bytes)?;
                let mut report = Report::default();
                let audio = Err(anyhow::anyhow!("banner must not use audio"));
                let paths = cook_file(&options, &root, &file, &audio, &mut report, None)?;
                assert_eq!(paths.len(), 2);
                assert!(paths.iter().all(|path| options.output.join(path).is_file()));
                let metadata: serde_json::Value =
                    serde_json::from_slice(&fs::read(options.output.join(&paths[1]))?)?;
                assert_eq!(metadata["comments"].as_array().unwrap().len(), languages);
                assert_eq!(metadata["texture"], paths[0]);
                assert!(report.failures.is_empty());
                fs::write(
                    root.join("files").join(&file.relative),
                    &bytes[..bytes.len() - 1],
                )?;
                assert!(
                    cook_file(&options, &root, &file, &audio, &mut report, None)
                        .unwrap_err()
                        .to_string()
                        .contains("truncated banner metadata")
                );
            }
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn deferred_members_are_not_successes_or_failures() {
        let mut report = Report::default();
        report.record(
            "source/member",
            Err(anyhow::Error::new(Deferred("battle actions")).context("decode member")),
        );
        assert_eq!(report.cooked, 0);
        assert!(report.failures.is_empty());
        assert_eq!(report.deferred.len(), 1);
        assert_eq!(report.deferred[0].path, "source/member");
    }

    #[test]
    fn action_like_generic_data_does_not_hide_a_format_failure() -> Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("files"))?;
        let mut bytes = vec![0; 128];
        for value in bytes[..16].chunks_exact_mut(4) {
            value.copy_from_slice(&128u32.to_be_bytes());
        }
        fs::write(root.path().join("files/unknown.bin"), &bytes)?;
        let mut report = Report::default();
        let error = cook_file(
            &options(&root.path().join("output")),
            root.path(),
            &File {
                relative: "unknown.bin".into(),
                hash: crate::digest(&bytes),
                role: None,
            },
            &Err(anyhow::anyhow!("no audio in fixture")),
            &mut report,
            None,
        )
        .unwrap_err();
        assert!(!error.is::<Deferred>());
        assert!(error.to_string().contains("no decoder"));
        assert!(report.deferred.is_empty());
        assert_eq!(report.cooked, 0);
        Ok(())
    }

    #[test]
    fn disc_preflight_preserves_identity_and_rejects_duplicates_before_output() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-disc-order"));
        fs::create_dir(&root)?;
        let result = (|| -> Result<()> {
            let paths = [root.join("one"), root.join("two")];
            for (path, header) in paths.iter().zip([b"GQSEAF\0\0", b"GQSEAF\x01\0"]) {
                fs::create_dir_all(path.join("sys"))?;
                fs::write(path.join("sys/boot.bin"), header)?;
            }
            let reversed = [paths[1].clone(), paths[0].clone()];
            let discs = source_discs(&reversed)?;
            assert_eq!(discs.keys().copied().collect::<Vec<_>>(), [1, 2]);
            assert_eq!(discs[&1], paths[0].as_path());
            assert_eq!(discs[&2], paths[1].as_path());
            let second_only = source_discs(&paths[1..])?;
            assert_eq!(second_only.keys().copied().collect::<Vec<_>>(), [2]);
            assert_eq!(second_only[&2], paths[1].as_path());
            assert!(source_discs(&[]).is_err());

            let alias = root.join("another-disc-two");
            fs::create_dir_all(alias.join("sys"))?;
            fs::write(alias.join("sys/boot.bin"), b"GQSEAF\x01\0")?;
            let duplicate = [paths[1].clone(), alias];
            let output = root.join("output");
            let absent_tool = root.join("no-codec-or-other-assets");
            let error = cook(&Options {
                discs: &duplicate,
                output: &output,
                coefficients: &absent_tool,
                jobs: 1,
            })
            .err()
            .context("duplicate disc unexpectedly admitted")?;
            assert!(error.to_string().contains("duplicate extracted disc 2"));
            assert!(!output.exists());

            for header in [
                b"GQSPAF\0\0".as_slice(),
                b"GQSEAF\0\x01",
                b"GQSEAF\x02\0",
                b"GQSEAF",
            ] {
                fs::write(paths[1].join("sys/boot.bin"), header)?;
                assert!(source_discs(&paths).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn two_member_geometry_is_not_a_script_wrapper() {
        let mut bytes = [0; 128];
        for (index, value) in [2u32, 32, 64].into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        assert!(!super::cook_script(&bytes, "unused", std::path::Path::new("")).unwrap());
    }
}
