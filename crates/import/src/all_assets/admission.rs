//! The publication boundary follows current documents in the clean candidate.
//! It cannot reach converter scratch or an old cooked-content reader.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    self as content,
    diagnostics::Diagnostics,
    library::{Library, Snapshot},
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use std::{collections::BTreeSet, path::Path};

pub(super) fn validate(root: &Path) -> Result<()> {
    let library = Library::open(root, &[])?;
    let strict = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    content::preparation::gameplay(&mut snapshot, &strict, &|| false)?;
    content::preparation::ui(&mut snapshot, &strict, &|| false)?;
    let gameplay = content::prepared::Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        strict.clone(),
    )?;
    content::menu_data::MenuData::load(&gameplay)?;
    drop(gameplay);
    content::battle_rules::capture(&mut library.snapshot(), &|| false)?;
    content::battle_ui::document::capture(&mut library.snapshot(), &strict, &|| false)?;
    content::credits::document::capture(&mut library.snapshot(), &|| false)?;
    content::game_over::document::capture(&mut library.snapshot(), &strict, &|| false)?;
    content::skit::document::capture(&mut library.snapshot(), &|| false)?;
    content::resource::capture(&mut library.snapshot(), content::resource::PATH, &|| false)?;
    let (title, _, _) = content::startup::capture(&mut library.snapshot(), true, &|| false)?;
    let mut snapshot = library.snapshot();
    snapshot.field(title.day_field)?;
    snapshot.field(title.night_field)?;
    let cues: content::startup::Cues = snapshot.json(&title.cues, JSON_LIMIT)?;
    ensure!(
        cues.version == content::startup::VERSION,
        "unsupported title cue version"
    );
    for path in cues.cues.values() {
        snapshot.read(path, JSON_LIMIT)?;
    }
    music(&library, &title.music)?;
    drop(snapshot);

    for id in ids(&library, content::library::FIELDS)? {
        let mut snapshot = library.snapshot();
        let (definition, _) = content::field::document::capture_scene(&mut snapshot, id, || false)
            .with_context(|| format!("publish field {id}"))?;
        selection(&mut snapshot, &definition.audio)?;
        for movie in definition.movies {
            content::movie::capture(&mut snapshot, movie)?;
        }
    }
    let mut snapshot = library.snapshot();
    let (world, _) = content::overworld::document::capture(&mut snapshot, &|| false)?;
    selection(&mut snapshot, &world.audio)?;
    drop(snapshot);
    for id in ids(&library, content::library::MOVIES)? {
        content::movie::capture(&mut library.snapshot(), id)?;
    }
    for id in ids(&library, content::battle_stage::document::MAPPINGS)? {
        content::battle_stage::document::capture(
            &mut library.snapshot(),
            id.try_into()?,
            &strict,
            &|| false,
        )?;
    }
    for id in ids(&library, content::battle_enemy::document::MAPPINGS)? {
        content::battle_enemy::document::capture(
            &mut library.snapshot(),
            id.try_into()?,
            &strict,
            &|| false,
        )?;
    }
    for id in ids(&library, content::battle_model::document::EQUIPMENT)? {
        content::battle_model::document::capture_equipment(
            &mut library.snapshot(),
            id.try_into()?,
            &strict,
            &|| false,
        )?;
    }
    for character in ids(&library, content::battle_model::document::PARTY)? {
        let mut snapshot = library.snapshot();
        let mapping = snapshot.resolve(
            content::battle_model::document::PARTY,
            character,
            Some("mappings.json"),
        )?;
        for costume in ids(&library, &mapping)? {
            let mut snapshot = library.snapshot();
            let (party, _) = content::battle_model::document::capture_party(
                &mut snapshot,
                character.try_into()?,
                costume.try_into()?,
                &strict,
                &|| false,
            )?;
            content::battle_victory::document::capture(
                &mut snapshot,
                [(
                    character.try_into()?,
                    costume.try_into()?,
                    party.victory.as_str(),
                )],
                &strict,
                &|| false,
            )?;
        }
    }
    for kind in [
        content::battle_scene::document::Kind::Technique,
        content::battle_scene::document::Kind::Skill,
    ] {
        for id in ids(&library, kind.mapping())? {
            content::battle_scene::document::capture(
                &mut library.snapshot(),
                kind,
                id.try_into()?,
                &strict,
                &|| false,
            )?;
        }
    }
    for id in ids(&library, content::battle_event::document::MAPPINGS)? {
        content::battle_event::document::capture(
            &mut library.snapshot(),
            id.try_into()?,
            &strict,
            &|| false,
        )?;
    }
    for path in [
        content::battle_effect::document::COMMON,
        content::battle_effect::document::TECHNIQUES,
    ] {
        content::battle_effect::document::capture(&mut library.snapshot(), path, &strict, &|| {
            false
        })?;
    }
    content::battle_audio::document::Catalogue::capture(&mut library.snapshot(), &|| false)?;
    catalogue_models(&library, "game/menu-data.json", true)?;
    catalogue_models(&library, content::menu_data::FIGURINES_PATH, false)?;

    // Include unused physical resources and independent audio programs, not just
    // the subset selected by scenario bytecode. Every mapping target must exist.
    let paths = files(root)?;
    let mut cache = resonance_audio::package::SampleCache::default();
    let environment: content::battle_audio::document::Definition = library
        .snapshot()
        .json(content::battle_audio::document::PATH, JSON_LIMIT)?;
    for path in &paths {
        if path.ends_with(".texture.json") {
            let mut snapshot = library.snapshot();
            let definition: content::texture::Definition = snapshot.json(path, JSON_LIMIT)?;
            definition
                .decode(|image| snapshot.read(image, content::texture::PNG_LIMIT))
                .with_context(|| format!("publish texture {path}"))?;
        } else if path.ends_with(".png") {
            resonance_asset_writer::texture::png(
                &library.snapshot().read(path, content::texture::PNG_LIMIT)?,
            )
            .with_context(|| format!("publish image {path}"))?;
        }
        if path.ends_with("/mappings.json") {
            let mut snapshot = library.snapshot();
            let mappings = snapshot.mappings(path)?.clone();
            for (id, _) in mappings.iter() {
                let target = mappings.resolve(path, id, None)?;
                ensure!(
                    root.join(&target).try_exists()?,
                    "mapping {path} ID {id} refers to missing {target}"
                );
            }
        }
        if path.starts_with("audio/") && path.ends_with("/music.json") {
            music(&library, path)?;
        }
        if path.starts_with("audio/") && path.ends_with("/sound.json") {
            let mut snapshot = library.snapshot();
            let program: resonance_audio::effect::Definition = snapshot.json(path, JSON_LIMIT)?;
            let inherited = program.reverbs().is_none().then_some(environment.reverbs);
            program
                .prepare_in(
                    inherited,
                    &mut |path, limit| snapshot.read(path, limit),
                    &mut cache,
                )
                .with_context(|| format!("publish audio program {path}"))?;
        }
        if path.starts_with("resources/physical/") && path.ends_with("/scene.json") {
            content::physical_scene::capture(&mut library.snapshot(), path, &|| false)?;
        }
        if path.starts_with("audio/speech/") && path.ends_with(".wav") {
            let wave = hound::WavReader::new(library.snapshot().open_stream(path)?)?;
            ensure!(
                wave.duration() > 0 && wave.spec().channels > 0,
                "speech WAV is empty: {path}"
            );
        }
    }
    Ok(())
}

fn ids(library: &Library, mapping: &str) -> Result<Vec<u32>> {
    Ok(library
        .snapshot()
        .mappings(mapping)?
        .iter()
        .map(|(id, _)| id)
        .collect())
}

fn selection(snapshot: &mut Snapshot, path: &str) -> Result<()> {
    let definition: content::field_audio::document::Definition = snapshot.json(path, JSON_LIMIT)?;
    let resolved = definition
        .resolve(snapshot)
        .with_context(|| path.to_owned())?;
    for path in resolved.music.values().chain(resolved.effects.values()) {
        snapshot.read(path, JSON_LIMIT)?;
    }
    for voice in resolved.voices.values() {
        snapshot.open_stream(&voice.path)?;
    }
    Ok(())
}

fn music(library: &Library, path: &str) -> Result<()> {
    let captured = content::music::capture(&mut library.snapshot(), path, &|| false)?;
    let repeat = captured
        .definition
        .repeat
        .map(|repeat| repeat.start_frame..repeat.end_frame);
    resonance_audio::recording::Prepared::new(captured.audio, repeat)?;
    if let Some(file) = captured.battle {
        let variant = captured
            .definition
            .battle
            .context("battle recording has no definition")?;
        let repeat = variant
            .repeat
            .map(|repeat| repeat.start_frame..repeat.end_frame);
        resonance_audio::recording::Prepared::new(file, repeat)?;
    }
    Ok(())
}

fn catalogue_models(library: &Library, path: &str, menu: bool) -> Result<()> {
    let mut snapshot = library.snapshot();
    let value: serde_json::Value = snapshot.json(path, JSON_LIMIT)?;
    let catalogue = if menu {
        &value["presentation"]["monsters"]
    } else {
        &value
    };
    for record in catalogue["records"]
        .as_array()
        .context("model catalogue needs records")?
    {
        let path = record["preview"]
            .as_str()
            .context("model catalogue needs a model path")?;
        let mut snapshot = library.snapshot();
        let mut model =
            content::model_preview::document::read(path, |path| snapshot.read(path, JSON_LIMIT))?;
        content::model_preview::document::prepare(&mut model, |path| {
            snapshot.read(path, ASSET_LIMIT)
        })?;
    }
    Ok(())
}

fn files(root: &Path) -> Result<BTreeSet<String>> {
    let mut pending = vec![root.to_owned()];
    let mut files = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(
                !kind.is_symlink(),
                "candidate contains a symlink: {}",
                entry.path().display()
            );
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                ensure!(kind.is_file(), "candidate contains a non-file entry");
                files.insert(crate::relative_source_path(root, &entry.path())?);
            }
        }
    }
    Ok(files)
}
