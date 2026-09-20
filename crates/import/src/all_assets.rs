//! Cook physical disc contents first. Runtime bindings are a separate concern.
mod archive;
pub(crate) use archive::Directory as PhysicalDirectory;
mod audio_tables;
mod credits;
mod embedded;
pub(crate) use embedded::defeat_ui::Catalogue as DefeatUi;
#[cfg(test)]
pub(crate) use embedded::defeat_ui::cook as cook_defeat_ui;
pub(crate) use embedded::{
    cooking_ui, ex_skills, figurine_catalogue, inventory_ui, monster_catalogue, options_ui,
    rename_ui, save_menu, shop_ui, status_ui, strategy_ui, synopsis, technique_ui, title_catalogue,
    ui_style, world_map,
};
mod exclusions;
mod field;
pub(crate) use field::CameraTrack;
#[cfg(test)]
pub(crate) use field::camera;
mod field_unlocks;
pub(crate) mod geometry;
mod overworld_collision;
mod overworld_encounters;
pub(crate) mod physical_scene;
mod pool;
pub(crate) mod roles;
mod skits;
mod victory;

use crate::{
    battle::{all as battle, audio::all as audio},
    media, write_atomic,
};
use anyhow::{Context, Result, ensure};
use roles::Role;
pub(crate) use roles::voice_bank_path;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

pub(crate) type MemberResults = Vec<(String, Result<Vec<String>>)>;

#[derive(Serialize)]
pub struct Failure {
    pub path: String,
    pub error: String,
}

#[derive(Default)]
pub struct Report {
    pub cooked: usize,
    pub duplicates: usize,
    pub failures: Vec<Failure>,
}

impl Report {
    fn record(&mut self, path: &str, result: Result<()>) {
        match result {
            Ok(()) => self.cooked += 1,
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
        self.failures.extend(other.failures);
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
enum Job {
    File(File),
    Voice {
        member: audio::ArchiveMember,
        hash: String,
    },
    Battle {
        job: battle::Job,
        namespace: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Exclusion {
    NativeCode,
    BuildMetadata,
    DiscMetadata,
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
    let mut roles = BTreeMap::new();
    for ((path, role), hash) in declared.iter().zip(pool::map(
        &declared,
        workers,
        || (),
        |_, (path, _)| media::hash_file(path),
    )) {
        let hash = hash?;
        if let Some(previous) = roles.insert(hash.clone(), *role) {
            ensure!(
                previous == *role,
                "conflicting resource roles for {}: {previous:?} and {role:?}",
                path.display()
            );
        }
        declared_hashes.insert(path.clone(), hash);
    }
    fs::create_dir_all(options.output)?;
    let mut report = Report::default();
    let mut seen = BTreeSet::new();
    let mut battles = BTreeSet::new();
    let mut battle_outputs = BTreeMap::<String, Vec<String>>::new();
    let mut tables = BTreeMap::<String, Vec<String>>::new();
    let mut skit_tables = BTreeMap::<String, Vec<String>>::new();
    let mut embedded_cache = embedded::Cache::default();
    let mut sources = BTreeMap::new();
    let mut excluded = BTreeMap::new();
    let mut outputs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (disc, extracted) in discs {
        let files = walk(&extracted.join("files"), &mut report);
        let mut hashed = BTreeMap::new();
        for (path, result) in files.iter().zip(pool::map(
            &files,
            workers,
            || (),
            |_, path| {
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
            },
        )) {
            match result {
                Ok((relative, hash)) => {
                    hashed.insert(relative, hash);
                }
                Err(error) => report.record(&path.display().to_string(), Err(error)),
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
        let identities = battle::Identities::read(extracted, &hashed)?;
        let discovered = battle::jobs(extracted, identities.sources(), &mut |f| report.fail(f));
        for job in discovered.jobs {
            let identity = identities.identity(&job)?;
            let namespace = format!("assets/{identity}");
            for source in identities.sources().source_paths(&job) {
                ensure!(
                    hashed.contains_key(source),
                    "missing battle source {source}"
                );
                // Equal archive bytes can use different external dependencies.
                // Keep each source's bindings separate from payload sharing.
                battle_outputs
                    .entry(format!("disc{disc}/{source}"))
                    .or_default()
                    .push(namespace.clone());
            }
            if battles.insert(identity) {
                jobs.push(Job::Battle { job, namespace });
            } else {
                report.duplicates += 1;
            }
        }
        let audio = audio::Cooker::new(extracted, options.output, options.coefficients, &hashed);
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
            if discovered.owned_sources.contains(&relative) {
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
        eprintln!(
            "Cooking disc{}: {} jobs on {workers} workers",
            disc,
            jobs.len()
        );
        let started = AtomicUsize::new(0);
        let results = pool::map(
            &jobs,
            workers,
            || None,
            |state, job| {
                if !matches!(job, Job::Battle { .. }) {
                    *state = None;
                }
                let count = started.fetch_add(1, Ordering::Relaxed) + 1;
                if count == 1 || count.is_multiple_of(100) {
                    eprintln!("Starting job {count}/{}", jobs.len());
                }
                let mut local = Report::default();
                let (key, result) = match job {
                    Job::Battle { job, namespace } => {
                        let state = state.get_or_insert_with(|| {
                            audio
                                .as_ref()
                                .map_err(|error| anyhow::anyhow!("{error:#}"))
                                .and_then(|audio| {
                                    battle::Worker::new(extracted, options.output, audio.pools())
                                })
                        });
                        match state {
                            Ok(worker) => {
                                for failure in
                                    worker.cook_into(job, &options.output.join(namespace))
                                {
                                    local.fail(failure);
                                }
                                local.cooked += usize::from(local.failures.is_empty());
                            }
                            Err(error) => {
                                local.record("battle worker", Err(anyhow::anyhow!("{error:#}")))
                            }
                        }
                        return (None, Vec::new(), local);
                    }
                    Job::Voice { member, hash } => (
                        hash,
                        audio
                            .as_ref()
                            .map_err(|e| anyhow::anyhow!("{e:#}"))
                            .and_then(|a| a.cook_member(member)),
                    ),
                    Job::File(file) => (
                        &file.hash,
                        cook_file(options, extracted, file, &audio, &mut local),
                    ),
                };
                let paths = match result {
                    Ok(paths) => {
                        local.cooked += usize::from(local.failures.is_empty());
                        paths
                    }
                    Err(error) => {
                        let label = match job {
                            Job::File(f) => &f.relative,
                            Job::Voice { member, .. } => &member.label,
                            Job::Battle { .. } => unreachable!(),
                        };
                        local.record(&format!("disc{}/{label}", disc), Err(error));
                        Vec::new()
                    }
                };
                (Some(key.clone()), paths, local)
            },
        );
        for (key, paths, local) in results {
            if let Some(key) = key {
                outputs.entry(key).or_default().extend(paths);
            }
            report.merge(local);
        }
        write_atomic(
            &options.output.join("progress.json"),
            &serde_json::to_vec(&serde_json::json!({
                "finished_asset_queues_for_disc": disc,
                "converted_units": report.cooked,
                "failures": report.failures,
            }))?,
        )?;
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
        let result = embedded::cook(
            extracted,
            &data,
            &mut embedded_cache,
            &mut |path, result| {
                report.record(&format!("disc{}/{path}", disc), result);
            },
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
    for paths in outputs.values_mut().chain(battle_outputs.values_mut()) {
        paths.sort();
        paths.dedup();
    }
    let sources = sources
        .into_iter()
        .map(|(source, hash)| {
            let paths = battle_outputs
                .remove(&source)
                .unwrap_or_else(|| outputs.get(&hash).cloned().unwrap_or_default());
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
        &options.output.join("sources.json"),
        &serde_json::to_vec_pretty(&sources)?,
    )?;
    write_atomic(
        &options.output.join("failures.json"),
        &serde_json::to_vec_pretty(&report.failures)?,
    )?;
    write_atomic(
        &options.output.join("summary.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "converted_units": report.cooked, "source_files": sources.len(),
            "excluded_files": excluded.len(), "duplicates": report.duplicates,
            "failures": report.failures.len()
        }))?,
    )?;
    fs::remove_file(options.output.join("progress.json"))?;
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

fn cook_file(
    options: &Options<'_>,
    extracted: &Path,
    file: &File,
    audio: &Result<audio::Cooker>,
    report: &mut Report,
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
        Some(fs::read(&source)?)
    };
    let role = file
        .role
        .or_else(|| bytes.as_deref().and_then(audio::structural_role));
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
        return embedded::cook_banner(&bytes, boot.get(3) == Some(&b'J'), &name, options.output);
    }
    let credits = match file.role {
        Some(Role::Credits) => Some(credits::decode(&bytes)?),
        None => credits::detect(&bytes),
        _ => None,
    };
    if let Some(credits) = credits {
        let path = format!("{name}/credits.json");
        write_atomic(&options.output.join(&path), &serde_json::to_vec(&credits)?)?;
        return Ok(vec![path]);
    }
    let recognized = match file.role {
        Some(Role::Victory) => {
            victory::cook(&bytes, &name, options.output, &mut |member, result| {
                report.record(&format!("{}/{member}", file.relative), result)
            })?;
            true
        }
        Some(Role::BattleSkit) => {
            cook_battle_skit(&bytes, &name, options.output, &mut |member, result| {
                report.record(&format!("{}/{member}", file.relative), result)
            })?;
            true
        }
        Some(Role::Font) => anyhow::bail!("font resource reached the ordinary asset queue"),
        Some(Role::Credits) => unreachable!("credits were decoded above"),
        role => geometry::cook(
            &bytes,
            &name,
            options.output,
            audio.as_ref().ok(),
            if role == Some(Role::Field) {
                geometry::Input::Field
            } else {
                geometry::Input::File
            },
            &mut |member, result| report.record(&format!("{}/{member}", file.relative), result),
        ),
    };
    ensure!(recognized, "no decoder for this asset format");
    Ok(if options.output.join(&name).exists() {
        vec![name]
    } else {
        Vec::new()
    })
}

/// fn_1_B78 reads group << 11, length 2048. Include every physical record,
/// including slot zero and those beyond the gameplay-selected group table.
fn cook_battle_skit(
    bytes: &[u8],
    name: &str,
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<()> {
    const RECORD_BYTES: usize = 2048;
    ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(RECORD_BYTES),
        "invalid fixed battle skit archive extent"
    );
    let mut ranges = Vec::new();
    for (index, record) in bytes.chunks_exact(RECORD_BYTES).enumerate() {
        let child = format!("{name}/{index}");
        let result = crate::battle::physical_bundle(record).and_then(|bundle| {
            write_atomic(
                &output.join(format!("{child}/actions.json")),
                &serde_json::to_vec(&bundle)?,
            )
        });
        report(&child, result);
        ranges.push(Some(index * RECORD_BYTES..(index + 1) * RECORD_BYTES));
    }
    write_atomic(
        &output.join(format!("{name}/archive.json")),
        &serde_json::to_vec(&archive::Directory::new(&ranges))?,
    )
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

/// Keep validated legacy bytecode and decoded messages without native-call admission.
pub(crate) fn cook_script(bytes: &[u8], name: &str, output: &Path) -> Result<bool> {
    use symphonia_script::{Program, message};
    let wrapped = bytes.get(..4) == Some(&2u32.to_be_bytes())
        && bytes.get(4..8) == Some(&32u32.to_be_bytes());
    let container = bytes;
    let bytes = if wrapped {
        let Some(payload) = bytes.get(32..) else {
            return Ok(false);
        };
        payload
    } else {
        bytes
    };
    let Some(header) = script_header(bytes) else {
        return Ok(false);
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
    write_atomic(&output.join(name).join("script.ssb"), bytes)?;
    write_atomic(
        &output.join(name).join("messages.json"),
        &serde_json::to_vec(&messages)?,
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
            let paths = cook_file(&options, &root, &file, &audio, &mut report)?;
            assert!(report.failures.is_empty());
            assert_eq!(paths.len(), 1);
            assert!(output.join(&paths[0]).join("0/actions.json").is_file());
            fs::write(root.join("files").join(&file.relative), &bytes[..2047])?;
            assert!(
                cook_file(&options, &root, &file, &audio, &mut report)
                    .unwrap_err()
                    .to_string()
                    .contains("invalid fixed battle skit archive extent")
            );

            file.relative = "renamed/field.txt".into();
            file.role = Some(Role::Field);
            file.hash = crate::digest(&[0; 8]);
            fs::write(root.join("files").join(&file.relative), [0; 8])?;
            assert!(cook_file(&options, &root, &file, &audio, &mut report)?.is_empty());
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
                let paths = cook_file(&options, &root, &file, &audio, &mut report)?;
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
                    cook_file(&options, &root, &file, &audio, &mut report)
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
