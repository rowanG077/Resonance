//! Catalogue entries bind shared decoded packages; no field has its own cook recipe.
use super::{Report, pool};
use crate::{field::MapArchive, scene::decoded::Package};
use anyhow::{Context, Result, ensure};
#[cfg(test)]
use std::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

pub(super) struct Field<'a> {
    pub id: u32,
    pub extracted: &'a Path,
    pub hash: String,
    pub archive: Arc<MapArchive>,
    pub declarations: crate::field_resources::Declarations,
    pub dependencies: BTreeSet<String>,
}

pub(super) struct PreparedField(pub u32, pub BTreeSet<String>);

pub(super) struct PreparedParty {
    pub files: BTreeSet<String>,
    pub outputs: BTreeMap<String, Vec<String>>,
}

pub(super) fn party<'a>(
    dag: &mut pool::Dag<'a, ()>,
    sources: &'a [crate::battle_model::party::Source],
    packages: &BTreeMap<String, pool::Output<super::CookedJob>>,
    output: &'a Path,
) -> Result<pool::Output<PreparedParty>> {
    let inputs = sources
        .iter()
        .flat_map(|source| &source.keys)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| {
            packages
                .get(key)
                .copied()
                .context("unqueued party battle source")
        })
        .collect::<Result<Vec<_>>>()?;
    let task = dag.add(
        "party battle models",
        inputs
            .iter()
            .map(|input| input.dependency())
            .collect::<Vec<_>>(),
        move |_, resolver| {
            let mut decoded = Package::default();
            for input in &inputs {
                let package = resolver.get(*input)?;
                package.require_success()?;
                decoded.extend(&package.decoded);
            }
            let mut result = PreparedParty {
                files: BTreeSet::new(),
                outputs: BTreeMap::new(),
            };
            for source in sources {
                let path = crate::battle_model::party::publish(source, &decoded, output)?;
                result.files.insert(path.clone());
                for key in &source.keys {
                    result
                        .outputs
                        .entry(key.clone())
                        .or_default()
                        .push(path.clone());
                }
            }
            Ok(result)
        },
    );
    dag.estimate(task, 4096, 128 * 1024 * 1024)?;
    Ok(task)
}

pub(super) fn discover<'a>(
    discs: &BTreeMap<u8, &'a Path>,
    documents: &BTreeMap<u8, Arc<super::Document>>,
    sources: &mut BTreeMap<String, String>,
    report: &mut Report,
) -> Result<Vec<Field<'a>>> {
    let mut fields = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let source_hash = |sources: &mut BTreeMap<String, String>,
                       disc,
                       files: &Path,
                       path: &str|
     -> Result<String> {
        let label = format!("disc{disc}/{path}");
        if !sources.contains_key(&label) {
            sources.insert(label.clone(), crate::media::hash_file(&files.join(path))?);
        }
        Ok(sources[&label].clone())
    };
    let mut archives = BTreeMap::<String, Arc<MapArchive>>::new();
    for (&disc, &extracted) in discs {
        let document = &documents[&disc];
        let catalogue = &document.catalogues.resources;
        let files = extracted.join("files");
        for name in catalogue
            .standalone
            .iter()
            .chain(catalogue.groups.iter().map(|group| &group.path))
            .flatten()
        {
            if let Some(path) = crate::field_resources::find_path(&files, name)? {
                source_hash(sources, disc, &files, &path)?;
            }
        }
        let mut party = BTreeSet::new();
        let wings = crate::field_resources::resolve_path(&files, &catalogue.colette_wings)?;
        party.insert(source_hash(sources, disc, &files, &wings)?);
        for id in 1..=catalogue.party_bodies.len() as u8 {
            for name in [
                catalogue.party(crate::resource::PartyResource::Body, id, 0)?,
                catalogue.field_motion(id)?,
                catalogue.field_service(id)?,
            ] {
                let path = crate::field_resources::resolve_path(&files, name)?;
                party.insert(source_hash(sources, disc, &files, &path)?);
            }
        }
        for name in catalogue.party_bodies.iter().flatten().flatten() {
            if let Some(path) = crate::field_resources::find_path(&files, name)? {
                party.insert(source_hash(sources, disc, &files, &path)?);
            }
        }
        for phase in &document.catalogues.field_phases().records {
            if fields.contains_key(&phase.id) {
                continue;
            }
            let Some(name) = &phase.resource else {
                continue;
            };
            let Some(path) = crate::field_resources::find_path(&files, &format!("MAP/{name}"))?
            else {
                continue;
            };
            let result = (|| -> Result<Option<Field<'a>>> {
                let hash = source_hash(sources, disc, &files, &path)?;
                if !visited.insert((phase.id, hash.clone())) {
                    return Ok(None);
                }
                let archive = match archives.get(&hash) {
                    Some(archive) => Arc::clone(archive),
                    None => {
                        let archive = Arc::new(MapArchive::open(&files.join(&path))?);
                        archives.insert(hash.clone(), Arc::clone(&archive));
                        archive
                    }
                };
                // Native phases have no VM program. Their physical assets and
                // original phase catalogue remain available to their native consumers.
                let Some(script) = archive.optional_section(6) else {
                    return Ok(None);
                };
                let mut dependencies = party.clone();
                dependencies.insert(hash.clone());
                let declarations = crate::field_resources::declarations(script)?;
                for &id in &declarations.resources {
                    let path = crate::field_resources::resolve_path(&files, catalogue.source(id)?)?;
                    dependencies.insert(source_hash(sources, disc, &files, &path)?);
                }
                if declarations.save_point {
                    let path = crate::field_resources::resolve_path(&files, &catalogue.save_point)?;
                    dependencies.insert(source_hash(sources, disc, &files, &path)?);
                }
                if declarations.treasures {
                    for treasure in &catalogue.treasures {
                        let path = crate::field_resources::resolve_path(&files, treasure)?;
                        dependencies.insert(source_hash(sources, disc, &files, &path)?);
                    }
                }
                Ok(Some(Field {
                    id: phase.id.try_into()?,
                    extracted,
                    hash: hash.clone(),
                    archive,
                    declarations,
                    dependencies,
                }))
            })();
            match result {
                Ok(Some(field)) => {
                    fields.insert(phase.id, field);
                }
                Ok(None) => (),
                Err(error) => {
                    report.record(&format!("field {}/dependencies", phase.id), Err(error))
                }
            }
        }
    }
    Ok(fields.into_values().collect())
}

pub(super) fn add<'a>(
    dag: &mut pool::Dag<'a, ()>,
    fields: &'a [Field<'a>],
    packages: &BTreeMap<String, pool::Output<super::CookedJob>>,
    shared: pool::Output<crate::shared::Prepared>,
    party: pool::Output<PreparedParty>,
    output: &'a Path,
) -> Result<()> {
    for field in fields {
        let inputs = field
            .dependencies
            .iter()
            .map(|hash| {
                packages.get(hash).copied().with_context(|| {
                    format!("field {} depends on unqueued source {hash}", field.id)
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let task = dag.add(
            format!("field {}/prepare", field.id),
            inputs
                .iter()
                .map(|input| input.dependency())
                .chain([shared.dependency(), party.dependency()])
                .collect::<Vec<_>>(),
            move |_, resolver| {
                let mut decoded = Package::default();
                for input in &inputs {
                    let package = resolver.get(*input)?;
                    package.require_success()?;
                    decoded.extend(&package.decoded);
                }
                let map = crate::scene::binding::Map::from_archive(
                    output,
                    Arc::clone(&field.archive),
                    &decoded,
                )?;
                let shared = resolver.get(shared)?;
                resolver.get(party)?;
                let (assets, paths) = crate::field::prepare(
                    field.id,
                    output,
                    &map,
                    &shared,
                    &decoded,
                    &field.declarations,
                )?;
                crate::field::publish(output, &assets)?;
                Ok(PreparedField(field.id, paths))
            },
        );
        dag.estimate(task, 0, 64 * 1024 * 1024)?;
    }
    Ok(())
}

pub(super) fn finish(
    options: &super::Options<'_>,
    fields: &[Field<'_>],
    prepared: &BTreeMap<u32, BTreeSet<String>>,
    shared_paths: &BTreeSet<String>,
    session: &Arc<crate::media::OutputSession>,
    battle_audio: &crate::battle_audio::Selection,
    sources: &mut BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let discs = super::source_discs(options.discs)?;
    let primary = *discs.first_key_value().context("no source disc")?.1;
    crate::cook_boot(primary, options.output)?;
    crate::cook_title(primary, options.output)?;
    crate::media::bind_all_movies(options.discs, options.output, sources)?;
    // Audio caches are owned by each source environment. All fields select
    // references into the same library; no output device is opened.
    for &extracted in discs.values() {
        let other = discs
            .values()
            .find(|&&path| path != extracted)
            .map(|path| path.to_path_buf());
        let mut audio = crate::media::FieldAudioCooker::new(session.workspace(extracted)?, other)?;
        for field in fields
            .iter()
            .filter(|field| field.extracted == extracted && prepared.contains_key(&field.id))
        {
            audio
                .cook(field.id, &field.archive)
                .with_context(|| format!("prepare field {} audio", field.id))?;
        }
    }
    crate::media::prepare_title_audio(session.workspace(primary)?)?;
    crate::media::prepare_title_sounds(session.workspace(primary)?)?;
    let audio = crate::battle_audio::publish_in(
        &session.workspace(primary)?,
        options.output,
        battle_audio,
    )?;
    let disc = crate::disc_number(primary)?;
    for source in audio.inputs.keys() {
        let source = source.strip_prefix("files/").unwrap_or(source);
        let aliases = sources.entry(format!("disc{disc}/{source}")).or_default();
        aliases.push(resonance_content::battle_audio::PATH.into());
        aliases.sort();
        aliases.dedup();
    }
    crate::field::finish(options.output, prepared, shared_paths)?;
    ensure!(
        !fields.is_empty(),
        "field catalogue produced no scripted fields"
    );
    Ok(())
}

#[test]
#[ignore = "requires original disc 1 and current party publications; no field/media recook"]
fn original_party_bindings_use_the_production_graph() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let extracted = local.join("extracted/disc1");
    let output = tempfile::tempdir()?;
    let discs = [extracted.clone()];
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let mut identities = BTreeMap::new();
    let sources = crate::battle_model::party::discover(&extracted, 1, &catalogue, &mut identities)?;
    let mut aliases = BTreeMap::<String, Vec<String>>::new();
    for (label, hash) in identities {
        aliases.entry(hash).or_default().push(
            label
                .strip_prefix("disc1/")
                .context("invalid source identity")?
                .into(),
        );
    }
    let options = super::Options {
        discs: &discs,
        output: output.path(),
        jobs: 3,
    };
    let mut dag = pool::Dag::new();
    let mut packages = BTreeMap::new();
    for (key, names) in aliases {
        let file = super::File {
            hash: key.clone(),
            relative: names[0].clone(),
            role: None,
        };
        let options = &options;
        let extracted = &extracted;
        let package = dag.add(file.relative.clone(), [], move |_, _| {
            let mut decoded = Package::default();
            let mut report = Report::default();
            let paths = super::cook_file(
                options,
                extracted,
                &file,
                &Err(anyhow::anyhow!("model job requested audio")),
                &mut report,
                super::PackageInput {
                    map: None,
                    models: &mut decoded,
                    aliases: &names,
                },
            )?;
            Ok(super::CookedJob {
                key: file.hash.clone(),
                paths,
                report,
                decoded: Arc::new(decoded),
            })
        });
        packages.insert(key, package);
    }
    party(&mut dag, &sources, &packages, output.path())?;
    let mut complete = false;
    for result in dag.run(
        3,
        || (),
        |completion| {
            if let Ok(party) = completion.result::<PreparedParty>() {
                assert_eq!(party.files.len(), 9);
                assert!(sources.iter().all(|source| source.keys.iter().all(|key| {
                    party.outputs[key].contains(&resonance_content::battle_model::party_path(
                        source.character,
                    ))
                })));
                complete = true;
            }
        },
    )? {
        result?;
    }
    assert!(complete);
    for character in 1..=9 {
        let path = resonance_content::battle_model::party_path(character);
        assert_eq!(
            fs::read(output.path().join(&path))?,
            fs::read(local.join("all-assets").join(path))?
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs and read-only RESONANCE_COOKED media; writes a private temporary library"]
fn original_catalogue_fields_share_the_production_graph() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let paths = [root.join("disc1"), root.join("disc2")];
    let media = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_COOKED")
            .context("set RESONANCE_COOKED to the read-only shared media library")?,
    )
    .canonicalize()?;
    let temporary = tempfile::tempdir()?;
    let output = temporary.path();
    let options = super::Options {
        discs: &paths,
        output,
        jobs: 3,
    };
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let discs = super::source_discs(&paths)?;
    let documents = super::read_documents(&discs)?;
    let mut sources = BTreeMap::new();
    let mut report = Report::default();
    let fields = discover(&discs, &documents, &mut sources, &mut report)?;
    ensure!(
        report.failures.is_empty(),
        "field discovery failed: {:?}",
        report.failures
    );
    let expected: BTreeSet<_> = fields.iter().map(|field| field.id).collect();
    ensure!(
        !expected.is_empty(),
        "field discovery produced no scripted maps"
    );

    // Only retained media is input. Geometry, metadata and dependency inventories
    // must all be produced by this graph in independently owned temporary files.
    let mut retained = vec![
        std::path::PathBuf::from("audio"),
        "sources.json".into(),
        resonance_content::battle_audio::PATH.into(),
    ];
    let mut movies = BTreeSet::new();
    for field in &fields {
        retained.push(crate::field::audio_path(field.id).into());
        for args in crate::field_resources::literal_arguments(
            field.archive.section(6)?,
            symphonia_script::NativeCall::PlayMovieBlocking,
            1,
        )? {
            let id = args[0].context("unresolved movie dependency")?;
            ensure!(id >= -1, "invalid movie dependency {id}");
            if id >= 0 {
                movies.insert(format!("movies/{id}.json"));
            }
        }
    }
    for path in movies {
        let movie: resonance_content::MovieAsset =
            serde_json::from_slice(&fs::read(media.join(&path))?)?;
        movie.validate()?;
        retained.extend([path.into(), movie.path.into()]);
    }
    while let Some(path) = retained.pop() {
        let source = media.join(&path);
        let metadata = fs::metadata(&source)?;
        if metadata.is_dir() {
            // Directory aliases are not traversed; linked files still copy into
            // independent destinations without introducing recursive cycles.
            if fs::symlink_metadata(&source)?.file_type().is_symlink() {
                continue;
            }
            for entry in fs::read_dir(&source)? {
                retained.push(path.join(entry?.file_name()));
            }
        } else {
            ensure!(
                metadata.is_file(),
                "non-file media input: {}",
                source.display()
            );
            let target = output.join(&path);
            fs::create_dir_all(target.parent().context("media output parent")?)?;
            fs::copy(source, target)?;
        }
    }
    let party_sources = crate::battle_model::party::discover(
        &paths[0],
        1,
        &documents[&1].catalogues.resources,
        &mut sources,
    )?;
    eprintln!(
        "Preparing {} catalogue fields; {} dependency failures",
        fields.len(),
        report.failures.len()
    );
    let needed = fields
        .iter()
        .flat_map(|field| field.dependencies.iter().cloned())
        .chain(
            party_sources
                .iter()
                .flat_map(|source| source.keys.iter().cloned()),
        )
        .collect::<BTreeSet<_>>();
    let maps = fields
        .iter()
        .map(|field| (field.hash.clone(), Arc::clone(&field.archive)))
        .collect::<BTreeMap<_, _>>();
    let mut dag = pool::Dag::new();
    let shared = dag.add("shared presentation", [], |_, _| {
        let document = &documents[&1];
        let battle_sources =
            crate::source_assets::Sources::read_with(&paths[0], &document.executable)?;
        let usual = fs::read(paths[0].join("files").join(&battle_sources.usual))?;
        crate::battle_formation::publish(
            &usual,
            &output.join(resonance_content::battle_formation::PATH),
        )?;
        crate::battle_effect::publish(&usual, output, "battle")?;
        crate::battle_projectile::publish(&usual, output, "battle")?;
        let module = paths[0].join("files").join(&battle_sources.module);
        crate::battle_recoil::publish(&module, output, "battle")?;
        crate::battle_profile::publish_party(&module, &usual, output, "battle")?;
        crate::battle_effect::publish_tints(&module, output, "battle")?;
        crate::battle_scene::publish(&paths[0], 237, output)?;
        crate::battle_stage::publish(&paths[0], 13, output)?;
        crate::battle_ui::publish(&paths[0], output)?;
        crate::shared::prepare(
            &paths[0],
            output,
            &sources,
            &document.executable,
            &document.catalogues,
            &battle_sources,
            &usual,
            &crate::skit::Source::read(&paths[0], &document.executable)?,
        )
    });
    let mut packages = BTreeMap::new();
    for hash in needed {
        let labels = sources
            .iter()
            .filter(|(_, candidate)| *candidate == &hash)
            .map(|(source, _)| source.as_str())
            .collect::<Vec<_>>();
        let (disc, relative) = labels[0].split_once('/').context("invalid source label")?;
        let disc = disc
            .strip_prefix("disc")
            .context("invalid source disc")?
            .parse::<u8>()?;
        let extracted = discs[&disc];
        let aliases = labels
            .iter()
            .map(|label| label.split_once('/').unwrap().1.to_owned())
            .collect::<Vec<_>>();
        let map = maps.get(&hash).cloned();
        let file = super::File {
            relative: relative.into(),
            hash: hash.clone(),
            role: map.as_ref().map(|_| super::Role::Field),
        };
        let options = &options;
        let package = dag.add(relative, [shared.dependency()], move |_, _| {
            let mut decoded = Package::default();
            let mut report = Report::default();
            let paths = super::cook_file(
                options,
                extracted,
                &file,
                &Err(anyhow::anyhow!("field geometry must not decode audio")),
                &mut report,
                super::PackageInput {
                    map: map.as_deref(),
                    models: &mut decoded,
                    aliases: &aliases,
                },
            )?;
            ensure!(
                report.failures.is_empty(),
                "physical field dependencies failed: {:?}",
                report.failures
            );
            Ok(super::CookedJob {
                key: file.hash.clone(),
                paths,
                report,
                decoded: Arc::new(decoded),
            })
        });
        packages.insert(hash, package);
    }
    let party = party(&mut dag, &party_sources, &packages, output)?;
    add(&mut dag, &fields, &packages, shared, party, output)?;
    let mut prepared = BTreeMap::new();
    let mut shared_paths = BTreeSet::new();
    let mut failures = dag
        .run(
            3,
            || (),
            |completion| {
                if let Ok(field) = completion.result::<PreparedField>() {
                    prepared.insert(field.0, field.1.clone());
                } else if let Ok(party) = completion.result::<PreparedParty>() {
                    shared_paths.extend(party.files.iter().cloned());
                } else if let Ok(shared) = completion.result::<crate::shared::Prepared>() {
                    shared_paths.extend(shared.files.iter().cloned());
                }
            },
        )?
        .into_iter()
        .filter_map(Result::err)
        .map(|error| format!("{error:#}"))
        .collect::<Vec<_>>();
    failures.extend(
        report
            .failures
            .into_iter()
            .map(|failure| format!("{}: {}", failure.path, failure.error)),
    );
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(prepared.keys().copied().collect::<BTreeSet<_>>(), expected);
    crate::field::finish(output, &prepared, &shared_paths)?;
    let manifests: Vec<_> = expected
        .iter()
        .map(|id| format!("fields/map-{id}.preload.json"))
        .collect();
    let files = resonance_content::prepared::Files::load(
        output,
        &manifests.iter().map(String::as_str).collect::<Vec<_>>(),
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    assert_eq!(
        files.manifests.keys().copied().collect::<BTreeSet<_>>(),
        expected
    );
    for &id in &expected {
        let assets: resonance_content::field::FieldAssets =
            files.json(&resonance_content::field::metadata_path(id))?;
        assert_eq!(assets.map_id, id);
        assets.validate()?;
    }
    drop(files);

    drop(_publications);
    let (&id, paths) = prepared.first_key_value().context("no prepared fields")?;
    let missing = crate::field::audio_path(id);
    fs::remove_file(output.join(&missing))?;
    assert!(crate::field::finish(output, &[(id, paths.clone())].into(), &shared_paths).is_err());
    let rejected: resonance_content::field_preload::Manifest = serde_json::from_slice(&fs::read(
        output.join(format!("fields/map-{id}.preload.json")),
    )?)?;
    assert_eq!(rejected.missing_inputs, [missing].into());
    Ok(())
}
