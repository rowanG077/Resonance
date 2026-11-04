//! Native world rules, terrain and travel models use the GC world documents.
use crate::{output, package::Package, pe, preview_model, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Mappings,
    overworld::{self as world, document},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod actors;
mod cinematics;
mod encounters;
mod movement;
mod terrain;
mod visuals;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    output::json(
        &library.join("world/movement.json"),
        &movement::read(&image)?,
    )?;
    output::json(
        &library.join("world/encounters.json"),
        &encounters::read(&image)?,
    )?;
    movement::bind_landmarks(library, &image)?;
    cook_script(source, library, &image)?;
    terrain::cook(source, library, &image)?;
    actors::cook(source, library, &image)?;
    visuals::cook(source, library, &image)?;
    cinematics::cook(source, library)?;
    assemble(source, library)?;
    Ok(())
}

fn float(image: &pe::Image<'_>, address: u32) -> Result<f32> {
    let value = f32::from_bits(pe::word(image.slice(address, 4)?, 0)?);
    ensure!(value.is_finite(), "nonfinite world parameter");
    Ok(value)
}
fn double(image: &pe::Image<'_>, address: u32) -> Result<f32> {
    let value = f64::from_le_bytes(image.slice(address, 8)?.try_into()?) as f32;
    ensure!(value.is_finite(), "nonfinite world parameter");
    Ok(value)
}
fn surface(native: u32) -> Result<world::TerrainSurface> {
    use world::TerrainSurface::*;
    Ok(match native {
        0 | 8 | 9 | 12 | 14 => Barrier,
        1 => Grassland,
        2 => Road,
        3 => Wasteland,
        4 => Snowfield,
        5 => Desert,
        6 => Forest,
        7 => DeepForest,
        10 => Beach,
        11 => Water,
        13 => Bridge,
        15 => Pier,
        _ => anyhow::bail!("unknown world collision surface {native}"),
    })
}
fn stack_array(
    image: &pe::Image<'_>,
    origin: i32,
    runs: &[(u32, usize, usize)],
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for &(start, count, stride) in runs {
        ensure!(matches!(stride, 7 | 10), "invalid world stack-array stride");
        for index in 0..count {
            let instruction = image.slice(start + (index * stride) as u32, stride)?;
            let displacement = if stride == 10 {
                ensure!(instruction[..2] == [0xc7, 0x85], "expected EBP disp32 MOV");
                pe::word(instruction, 2)? as i32
            } else {
                ensure!(instruction[..2] == [0xc7, 0x45], "expected EBP disp8 MOV");
                i32::from(instruction[2] as i8)
            };
            ensure!(
                displacement == origin + bytes.len() as i32,
                "unordered world stack array"
            );
            bytes.extend_from_slice(&instruction[stride - 4..]);
        }
    }
    Ok(bytes)
}
fn cook_script(source: &Source, library: &Path, image: &pe::Image<'_>) -> Result<()> {
    let native = image.text(0x870980)?;
    let (bytes, encoding) = source
        .read_encoded(native)?
        .context("missing world scenario")?;
    let script = crate::script::decode(&bytes, encoding)?;
    let bytes = crate::script_input::normalize(&script.bytes)?;
    output::write(&library.join("world/scripts/scenario.ssb"), &bytes)?;
    if !script.messages.is_empty() {
        output::json(&library.join("world/messages.json"), &script.messages)?;
    }
    Ok(())
}

/// Publish the world entry point only after all referenced owners exist.
pub(crate) fn assemble(source: &Source, library: &Path) -> Result<()> {
    use resonance_content::field_audio::{ServiceCue, document as audio, speech};
    use symphonia_script::{
        NativeCall, Program,
        message::{Message, Token},
        scenario,
    };
    let mappings = |path: &str| -> Result<Mappings> {
        Ok(serde_json::from_slice(&fs::read(library.join(path))?)?)
    };
    let music = mappings(audio::MUSIC)?
        .iter()
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    let mut effects = mappings(audio::EFFECTS)?
        .iter()
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    let speech: speech::Mappings = serde_json::from_slice(&fs::read(library.join(audio::SPEECH))?)?;
    let voices = speech.keys().copied().collect::<BTreeSet<_>>();
    let bytes = fs::read(library.join("world/scripts/scenario.ssb"))?;
    let program = Program::decode(&bytes)?;
    let analysis = scenario::analyze(&bytes)?;
    let arguments = |call, arity, position| -> Result<Vec<Option<i32>>> {
        Ok(
            scenario::calls::analyzed_arguments(&program, &analysis, call, arity)?
                .into_values()
                .map(|args| args[position])
                .collect(),
        )
    };
    let select = |values: Vec<Option<i32>>,
                  available: &BTreeSet<u32>,
                  selected: &mut BTreeSet<u32>|
     -> Result<()> {
        for value in values {
            match value {
                Some(id) if id >= 0 => {
                    ensure!(
                        available.contains(&(id as u32)),
                        "unmapped world audio ID {id}"
                    );
                    selected.insert(id as u32);
                }
                Some(_) => (),
                None => selected.extend(available),
            }
        }
        Ok(())
    };
    let mut selected = audio::Definition {
        music: [2, 3, 4, 5].into(),
        effects: ServiceCue::ALL.iter().map(|cue| *cue as u32).collect(),
        ..Default::default()
    };
    // Travel services emit these fixed aliases outside scenario/cinematic calls:
    // Rheairds loop, ship loop, and world-portal departure respectively.
    select(
        vec![Some(24), Some(25), Some(26)],
        &effects,
        &mut selected.effects,
    )?;
    select(
        arguments(NativeCall::AudioCommand, 1, 0)?,
        &music,
        &mut selected.music,
    )?;
    let mut selected_voices = BTreeSet::new();
    select(
        arguments(NativeCall::PlayVoice, 1, 0)?
            .into_iter()
            .filter(|value| value.is_none_or(|id| id < 0 || (id as u32) >> 16 != 0))
            .collect(),
        &voices,
        &mut selected_voices,
    )?;
    for (call, arity, position) in crate::script_audio::CALLS {
        let values = arguments(call, arity, position)?;
        for &value in values.iter().flatten().filter(|value| **value >= 0) {
            if !effects.contains(&(value as u32)) {
                crate::audio_effects::bind_unused(source, library, value as u32)?;
                effects.insert(value as u32);
            }
        }
        select(values, &effects, &mut selected.effects)?;
    }
    let message_path = library.join("world/messages.json");
    if message_path.is_file() {
        let messages: Vec<Message> = serde_json::from_slice(&fs::read(&message_path)?)?;
        for token in messages.iter().flat_map(|message| &message.tokens) {
            if let Token::Control {
                opcode: 9,
                expression,
            } = token
            {
                select(vec![expression.constant()], &voices, &mut selected_voices)?;
            }
        }
    }
    let named = resonance_content::library::Library::open(library, &[])?;
    let mut snapshot = named.snapshot();
    for (id, _) in mappings(document::CINEMATICS)?.iter() {
        let path = snapshot.resolve(document::CINEMATICS, id, Some("cinematic.json"))?;
        let scene: document::Presentation = serde_json::from_slice(&fs::read(library.join(path))?)?;
        selected_voices.extend(scene.dialogue.iter().map(|line| line.voice));
        for id in scene
            .presentation
            .actions()
            .filter_map(world::CinematicAudio::sound)
        {
            let id = u32::try_from(id).context("negative cinematic sound ID")?;
            if !effects.contains(&id) {
                crate::audio_effects::bind_unused(source, library, id)?;
                effects.insert(id);
            }
            selected.effects.insert(id);
        }
    }
    ensure!(
        selected_voices.is_subset(&voices),
        "world cinematic speech has no catalogue binding"
    );
    if !selected_voices.is_empty() {
        selected.speech.push(audio::Speech {
            ids: selected_voices,
            sample_rate: None,
        });
    }
    let definition = document::Definition {
        script: "world/scripts/scenario.ssb".into(),
        messages: message_path.is_file().then(|| "world/messages.json".into()),
        audio: selected,
        movement: "world/movement.json".into(),
        encounters: "world/encounters.json".into(),
        landmarks: "world/landmarks.json".into(),
        continents: document::MAPPINGS.into(),
        actors: document::ACTORS.into(),
        markers: document::MARKERS.into(),
        cinematics: document::CINEMATICS.into(),
        dust: std::array::from_fn(|index| format!("world/effects/dust-{index}.ktx2")),
        skies: [
            "world/skies/normal/model.json".into(),
            "world/skies/late/model.json".into(),
        ],
    };
    definition.validate()?;
    output::json(&library.join(document::PATH), &definition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM, RESONANCE_TEST_LIBRARY and new/empty RESONANCE_SCRIPT_OUTPUT; publishes source script categories privately"]
    fn installed_source_script_categories_publish_privately() -> Result<()> {
        use sha2::{Digest, Sha256};
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let baseline = fs::canonicalize(PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        ))?;
        let requested = PathBuf::from(
            std::env::var_os("RESONANCE_SCRIPT_OUTPUT")
                .context("set RESONANCE_SCRIPT_OUTPUT to a new/empty private directory")?,
        );
        let absolute = if requested.is_absolute() {
            requested
        } else {
            std::env::current_dir()?.join(requested)
        };
        let parent = fs::canonicalize(absolute.parent().context("script output has no parent")?)
            .context("private script output parent must already exist")?;
        let output = parent.join(
            absolute
                .file_name()
                .context("script output has no directory name")?,
        );
        ensure!(
            !output.starts_with(&baseline),
            "private script output must be outside the read-only baseline"
        );
        if output.exists() {
            ensure!(
                fs::symlink_metadata(&output)?.file_type().is_dir()
                    && fs::read_dir(&output)?.next().is_none(),
                "private script output must be an empty directory"
            );
        } else {
            fs::create_dir(&output)?;
        }

        // Only this existing metadata is needed by silence_skits. Its muted
        // banks already use clock-only Silence entries; no recordings, portraits
        // or other media are copied or recooked. The baseline remains read-only.
        let speech = "audio/speech/english/mappings.json";
        let baseline_speech = fs::read(baseline.join(speech))?;
        output::write(&output.join(speech), &baseline_speech)?;
        crate::field_directory::cook(&source, &output)?;
        crate::field_core::cook(&source, &output, None)?;
        crate::skits::cook(&source, &output)?;
        let image = pe::Image::parse(&source.executable)?;
        cook_script(&source, &output, &image)?;
        ensure!(
            fs::read(baseline.join(speech))? == baseline_speech,
            "read-only baseline speech metadata changed"
        );

        fn files(root: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
            for entry in fs::read_dir(root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    files(&entry.path(), result)?;
                } else if entry.file_type()?.is_file() {
                    result.push(entry.path());
                }
            }
            Ok(())
        }
        let hash = |bytes: &[u8]| {
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let mut paths = Vec::new();
        files(&output, &mut paths)?;
        paths.sort();
        let mut scripts = Vec::new();
        let mut messages = Vec::new();
        let mut counts = [0usize; 3];
        for path in paths {
            let relative = path.strip_prefix(&output)?;
            if path.extension().is_some_and(|extension| extension == "ssb") {
                let bytes = fs::read(&path)?;
                symphonia_script::Program::decode(&bytes)
                    .with_context(|| format!("source-produced script {}", relative.display()))?;
                let category = if relative.starts_with("fields") {
                    0
                } else if relative.starts_with("skits") {
                    1
                } else if relative == Path::new("world/scripts/scenario.ssb") {
                    2
                } else {
                    anyhow::bail!("unexpected script producer path {}", relative.display())
                };
                counts[category] += 1;
                let previous = fs::read(baseline.join(relative))
                    .with_context(|| format!("baseline script {}", relative.display()))?;
                scripts.push(serde_json::json!({"path":relative,"bytes":bytes.len(),
                    "sha256":hash(&bytes),"baseline_sha256":hash(&previous),"changed":bytes!=previous}));
            } else if path.file_name().is_some_and(|name| name == "messages.json") {
                let bytes = fs::read(&path)?;
                let _: Vec<symphonia_script::message::Message> = serde_json::from_slice(&bytes)?;
                let previous = fs::read(baseline.join(relative))
                    .with_context(|| format!("baseline messages {}", relative.display()))?;
                ensure!(
                    bytes == previous,
                    "source publication changes messages {}",
                    relative.display()
                );
                messages.push(serde_json::json!({"path":relative,"sha256":hash(&bytes),
                    "baseline_sha256":hash(&previous),"unchanged":true}));
            }
        }
        ensure!(
            counts[0] > 0 && counts[1] > 0 && counts[2] == 1,
            "incomplete source script categories {counts:?}"
        );
        let report = serde_json::json!({"schema":1,"status":"ALL_SOURCE_SCRIPT_CATEGORIES_COOKED_AND_DECODED",
            "source_executable_sha256":hash(&source.executable),"read_only_baseline":baseline,"private_output":output,
            "producers":{"fields":"field_directory::cook + field_core::cook(all), original Source::scenario",
                "skits":"skits::cook, original source SKP programs", "world":"world::cook_script, original executable-named source program"},
            "normalization":"Production raw-source decode then script_input::normalize exactly once; no cooked script is an input",
            "counts":{"fields":counts[0],"skits":counts[1],"world":counts[2],"scripts":scripts.len(),"unchanged_messages":messages.len()},
            "scripts":scripts,"messages":messages,
            "publication_scope":"Root selects only produced .ssb files; collision, mappings/catalogue and other helper outputs are not canonical publication inputs"});
        output::json(
            &output.join("source-script-publication-report.json"),
            &report,
        )?;
        eprintln!(
            "Private source scripts: fields={}, skits={}, world={}, unchanged messages={}",
            counts[0], counts[1], counts[2], report["counts"]["unchanged_messages"]
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; stages private world assembly"]
    fn installed_world_travel_cues_are_prepared_by_assembler() -> Result<()> {
        use resonance_content::field_audio::document as audio;
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let private = tempfile::tempdir()?;
        let copy = |path: &str| -> Result<()> {
            let target = private.path().join(path);
            fs::create_dir_all(target.parent().context("staged file has no parent")?)?;
            fs::copy(root.join(path), target).with_context(|| format!("stage {path}"))?;
            Ok(())
        };
        for path in [
            resonance_content::library::DESCRIPTOR,
            document::PATH,
            audio::MUSIC,
            audio::EFFECTS,
            audio::SPEECH,
            "world/scripts/scenario.ssb",
            document::CINEMATICS,
        ] {
            copy(path)?;
        }
        if root.join("world/messages.json").is_file() {
            copy("world/messages.json")?;
        }
        let library = resonance_content::library::Library::open(&root, &[])?;
        let mut snapshot = library.snapshot();
        let cinematics: Mappings =
            serde_json::from_slice(&fs::read(root.join(document::CINEMATICS))?)?;
        for (id, _) in cinematics.iter() {
            copy(&snapshot.resolve(document::CINEMATICS, id, Some("cinematic.json"))?)?;
        }
        let before: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(document::PATH))?)?;
        assemble(&source, private.path())?;
        let after_bytes = fs::read(private.path().join(document::PATH))?;
        let after: serde_json::Value = serde_json::from_slice(&after_bytes)?;
        let mut expected = before.clone();
        let selected = expected["audio"]["effects"]
            .as_array_mut()
            .context("world effects are not an array")?;
        for id in [24, 25, 26] {
            if !selected.contains(&id.into()) {
                selected.push(id.into());
            }
        }
        selected.sort_by_key(|value| value.as_u64());
        assert_eq!(
            after, expected,
            "world assembly changed unrelated definitions"
        );
        let cues: Mappings = serde_json::from_slice(&fs::read(root.join(audio::EFFECTS))?)?;
        for (service, native) in [(24, 1), (25, 0), (26, 2)] {
            assert_eq!(cues.target(service)?, cues.target(512 + native)?);
            let cue: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("audio").join(cues.target(service)?))?)?;
            ensure!(
                cue["kind"] == "wave",
                "world travel cue is not recorded PCM"
            );
            let pcm = root.join(cue["audio"].as_str().context("cue has no PCM")?);
            let wave = hound::WavReader::open(pcm)?;
            ensure!(wave.duration() > 0, "empty world travel recording");
        }
        if let Some(path) = std::env::var_os("RESONANCE_TEST_WORLD_CANDIDATE") {
            output::write(&PathBuf::from(path), &after_bytes)?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; checks native travel and encounter tables"]
    fn installed_world_rules_use_the_shared_schema() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        let movement = movement::read(&image)?;
        movement.validate()?;
        let encounters = encounters::read(&image)?;
        encounters.validate()?;
        let bytes = serde_json::to_vec(&encounters)?;
        let round_trip: world::EncounterTables = serde_json::from_slice(&bytes)?;
        round_trip.validate()?;
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks terrain and travel media"]
    fn installed_world_media_cooks() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &library)
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_LIBRARY with cooked world owners; loads the complete world package"]
    fn cooked_world_owners_prepare_through_shared_reader() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let library = resonance_content::library::Library::open(&root, &[])?;
        let mut snapshot = library.snapshot();
        let (definition, world) = document::capture(&mut snapshot, &|| false)?;
        world.validate_content(|_| true)?;
        assert_eq!(
            world.worlds.iter().map(Vec::len).collect::<Vec<_>>(),
            [108, 108]
        );
        assert_eq!(
            world
                .worlds
                .iter()
                .flatten()
                .filter(|tile| tile.alternate.is_some())
                .count(),
            12
        );
        assert_eq!(world.visuals.actors.len(), 29);
        assert_eq!(world.visuals.cinematics.len(), 14);
        definition.audio.resolve(&mut snapshot)?;
        Ok(())
    }
}
