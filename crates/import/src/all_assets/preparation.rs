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

pub(super) struct Field {
    pub id: u32,
    pub hash: String,
    pub archive: Arc<MapArchive>,
    pub declarations: crate::field_resources::Declarations,
    pub dependencies: BTreeSet<String>,
}

pub(super) use crate::field::Prepared as PreparedField;

pub(super) struct PreparedParty {
    pub records: BTreeMap<(u8, u8), resonance_content::battle_model::Party>,
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
                records: BTreeMap::new(),
                outputs: BTreeMap::new(),
            };
            for source in sources {
                let record = crate::battle_model::party::prepare(source, &decoded, output)?;
                let path = crate::named::party_path(source.character, source.costume)?;
                result
                    .records
                    .insert((source.character, source.costume), record);
                for key in source.source_keys() {
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
) -> Result<Vec<Field>> {
    let mut fields = BTreeMap::<_, Field>::new();
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
    let shared_catalogue = &documents
        .first_key_value()
        .context("no source catalogue")?
        .1
        .catalogues
        .resources;
    for (&disc, &extracted) in discs {
        let document = &documents[&disc];
        let catalogue = &document.catalogues.resources;
        shared_catalogue.ensure_shared_bindings(catalogue)?;
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
            let Some(name) = &phase.resource else {
                continue;
            };
            let Some(path) = crate::field_resources::find_path(&files, &format!("MAP/{name}"))?
            else {
                continue;
            };
            let result = (|| -> Result<Option<Field>> {
                let hash = source_hash(sources, disc, &files, &path)?;
                if let Some(previous) = fields.get(&phase.id) {
                    ensure!(
                        previous.hash == hash,
                        "conflicting source definitions for field {}",
                        phase.id
                    );
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
                if let Some(previous) = fields.get(&phase.id) {
                    ensure!(
                        previous.dependencies == dependencies,
                        "conflicting source dependencies for field {}",
                        phase.id
                    );
                    return Ok(None);
                }
                Ok(Some(Field {
                    id: phase.id.try_into()?,
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
    fields: &'a [Field],
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
                crate::field::prepare(
                    field.id,
                    output,
                    &map,
                    &shared,
                    &decoded,
                    &field.declarations,
                )
            },
        );
        dag.estimate(task, 0, 64 * 1024 * 1024)?;
    }
    Ok(())
}

pub(super) fn finish(
    options: &super::Options<'_>,
    fields: &[Field],
    prepared: &BTreeMap<u32, Arc<PreparedField>>,
    world: Option<&resonance_content::overworld::Package>,
    party: Option<&PreparedParty>,
    sources: &mut BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let discs = super::source_discs(options.discs)?;
    let primary = *discs.first_key_value().context("no source disc")?.1;
    crate::named::Publisher::new(primary, options.output)?.movie_catalogue(options.discs)?;
    let mut publisher = crate::named::Publisher::new(options.output, options.output)?
        .with_audio_source(crate::named::AudioSource::read(
            primary,
            discs.values().copied().find(|path| *path != primary),
        )?);
    publisher.startup()?;
    publisher.world(world.context("world preparation did not complete")?.clone())?;
    publisher.battle_audio()?;
    let party = party.context("party preparation did not complete")?;
    publisher.battle_party(party.records.clone())?;
    let names = crate::naming::fields(prepared.keys().copied())?;
    for (&id, field) in prepared {
        publisher.field(
            names.target(id)?,
            field.assets.clone(),
            field.art.clone(),
            field.movies.clone(),
        )?;
    }
    publisher.finish_fields()?;
    for (source, paths) in publisher.audio_source_outputs() {
        let outputs = sources.entry(source.clone()).or_default();
        outputs.extend(paths.iter().cloned());
        outputs.sort();
        outputs.dedup();
    }
    let disc = crate::disc_number(primary)?;
    let events = crate::battle_event::prepare(primary, &party.records, options.output)?;
    let paths = publisher.battle_events(events.scenes)?;
    for input in events.inputs {
        let source = input.strip_prefix("files/").unwrap_or(&input);
        let aliases = sources.entry(format!("disc{disc}/{source}")).or_default();
        aliases.extend(paths.iter().cloned());
        aliases.sort();
        aliases.dedup();
    }
    ensure!(
        !fields.is_empty(),
        "field catalogue produced no scripted fields"
    );
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; binds native party records through the production graph"]
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
        memory_budget: 2 * 1024 * 1024 * 1024,
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
    let mut complete = None;
    for result in dag.run(
        3,
        || (),
        |completion| {
            if let Ok(party) = completion.result::<PreparedParty>() {
                assert_eq!(party.records.len(), sources.len());
                assert!(sources.iter().all(|source| {
                    source.keys.iter().all(|key| {
                        party.outputs[key].contains(
                            &crate::named::party_path(source.character, source.costume).unwrap(),
                        )
                    })
                }));
                complete = Some(party);
            }
        },
    )? {
        result?;
    }
    let complete = complete.context("party task did not complete")?;
    let mut publisher = crate::named::Publisher::new(output.path(), output.path())?;
    publisher.battle_party(complete.records.clone())?;
    let victories = crate::battle_victory::prepare(&extracted, output.path())?;
    publisher.battle_victories(&victories, "battle")?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = resonance_content::library::Library::open(output.path(), &[])?.snapshot();
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
    for (&(character, costume), native) in &complete.records {
        let (_, current) = resonance_content::battle_model::document::capture_party(
            &mut snapshot,
            character,
            costume,
            &diagnostics,
            &|| false,
        )?;
        assert_eq!(
            serde_json::to_value(&native.body)?,
            serde_json::to_value(&current.body)?
        );
        assert_eq!(native.null_motions, current.null_motions);
        for (native, current) in native.parts.iter().zip(&current.parts) {
            assert_eq!(native.bone_names, current.bone_names);
            assert_eq!(
                native
                    .clips
                    .iter()
                    .map(|clip| clip.resource_slot)
                    .collect::<Vec<_>>(),
                current
                    .clips
                    .iter()
                    .map(|clip| clip.resource_slot)
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs; binds native fields through the production graph and named writer"]
fn original_representative_fields_share_the_production_graph() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let paths = [root.join("disc1"), root.join("disc2")];
    let temporary = tempfile::tempdir()?;
    let output = temporary.path();
    let options = super::Options {
        discs: &paths,
        output,
        jobs: 3,
        memory_budget: 2 * 1024 * 1024 * 1024,
    };
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let discs = super::source_discs(&paths)?;
    let documents = super::read_documents(&discs)?;
    let mut sources = BTreeMap::new();
    let mut report = Report::default();
    let mut fields = discover(&discs, &documents, &mut sources, &mut report)?;
    ensure!(
        report.failures.is_empty(),
        "field discovery failed: {:?}",
        report.failures
    );
    // Autoplay scenery, declared animation plus a movie, and an overlay each
    // require different inputs to reach the same production field job.
    let expected: BTreeSet<_> = [330, 340, 372].into();
    fields.retain(|field| expected.contains(&field.id));
    assert_eq!(
        fields.iter().map(|field| field.id).collect::<BTreeSet<_>>(),
        expected
    );
    assert!(fields.iter().all(|field| !field.declarations.save_point));

    let document = &documents[&1];
    let primary = discs[&1];
    let mut texture_animations = crate::texture_animation::field::profiles();
    texture_animations.extend(crate::texture_animation::title_profiles(
        &document.executable,
    )?);
    let shared_value = Arc::new(crate::shared::Prepared {
        catalogue: document.catalogues.resources.clone(),
        texture_animations,
        resource_catalogue: resonance_content::resource::PATH.into(),
        font: crate::font::prepare(primary, output)?.font,
        effects: crate::field_effects::cook(primary, output)?,
        toon_ramp: crate::field_lighting::cook(primary, output)?,
        save_point_tutorial: Vec::new(),
        save_point_unlock: Vec::new(),
        save_point_no_gem: Vec::new(),
        files: BTreeSet::new(),
        enemy_paths: Vec::new(),
        equipment_paths: Vec::new(),
    });
    let party_value = Arc::new(PreparedParty {
        records: BTreeMap::new(),
        outputs: BTreeMap::new(),
    });
    let needed = fields
        .iter()
        .flat_map(|field| field.dependencies.iter().cloned())
        .collect::<BTreeSet<_>>();
    let maps = fields
        .iter()
        .map(|field| (field.hash.clone(), Arc::clone(&field.archive)))
        .collect::<BTreeMap<_, _>>();
    let mut dag = pool::Dag::new();
    let shared = dag.add_shared("native shared presentation", [], |_, _| {
        Ok(Arc::clone(&shared_value))
    });
    let party = dag.add_shared("field-only party dependency", [], |_, _| {
        Ok(Arc::clone(&party_value))
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
    add(&mut dag, &fields, &packages, shared, party, output)?;
    let mut prepared = BTreeMap::new();
    for result in dag.run(
        3,
        || (),
        |completion| {
            if let Ok(field) = completion.result::<PreparedField>() {
                prepared.insert(field.assets.map_id, field);
            }
        },
    )? {
        result?;
    }
    assert_eq!(prepared.keys().copied().collect::<BTreeSet<_>>(), expected);
    drop(_publications);
    let named = tempfile::tempdir()?;
    // Raw converters and the named writer use separate trees in this fixture,
    // making accidental dependencies on old semantic publications observable.
    let mut publisher = crate::named::Publisher::new(output, named.path())?
        .with_audio_source(crate::named::AudioSource::read(primary, Some(discs[&2]))?);
    let names = crate::naming::fields(expected.iter().copied())?;
    for (&id, field) in &prepared {
        assert_eq!(
            field.movies,
            crate::field::movie_requirements(
                fields
                    .iter()
                    .find(|field| field.id == id)
                    .unwrap()
                    .archive
                    .section(6)?
            )?
        );
        publisher.field(
            names.target(id)?,
            field.assets.clone(),
            field.art.clone(),
            field.movies.clone(),
        )?;
    }
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = resonance_content::library::Library::open(named.path(), &[])?;
    let mut snapshot = library.snapshot();
    let mut missing = None;
    for &id in &expected {
        let (definition, assets) =
            resonance_content::field::document::capture_scene(&mut snapshot, id, || false)?;
        assert_eq!(assets.map_id, id);
        assert_eq!(
            snapshot
                .read(&definition.script, 64 * 1024 * 1024)?
                .as_ref(),
            fs::read(output.join(&prepared[&id].assets.script))?
        );
        assert!(!named.path().join(format!("fields/map-{id}.json")).exists());
        if missing.is_none() {
            missing = assets.parts.first().map(|part| (id, part.mesh.clone()));
        }
    }
    let (id, path) = missing.context("fixture did not publish scenery")?;
    fs::remove_file(named.path().join(path))?;
    assert!(
        resonance_content::field::document::capture_scene(&mut library.snapshot(), id, || false)
            .is_err()
    );
    Ok(())
}
