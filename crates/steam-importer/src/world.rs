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
    use crate::script_audio::{self, Script, select};
    use resonance_content::field_audio::{ServiceCue, document as audio, speech};
    use symphonia_script::{NativeCall, message::Message};
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
    let script = Script::read(&bytes)?;
    let mut selected = audio::Definition {
        music: [2, 3, 4, 5].into(),
        effects: ServiceCue::ALL.iter().map(|cue| *cue as u32).collect(),
        ..Default::default()
    };
    // Travel services emit these fixed aliases outside scenario/cinematic calls:
    // Rheairds loop, ship loop, and world-portal departure respectively.
    selected
        .effects
        .extend(select([Some(24), Some(25), Some(26)], &effects)?);
    selected.music.extend(select(
        script.arguments(NativeCall::AudioCommand, 1, 0)?,
        &music,
    )?);
    let mut selected_voices = select(
        script
            .arguments(NativeCall::PlayVoice, 1, 0)?
            .into_iter()
            .filter(|value| value.is_none_or(|id| id < 0 || (id as u32) >> 16 != 0)),
        &voices,
    )?;
    let dynamic_effects = script.effects(source, library, &mut effects, &mut selected.effects)?;
    let message_path = library.join("world/messages.json");
    if message_path.is_file() {
        let messages: Vec<Message> = serde_json::from_slice(&fs::read(&message_path)?)?;
        selected_voices.extend(select(script_audio::message_voices(&messages), &voices)?);
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
    if dynamic_effects {
        selected.effects.extend(&effects);
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
    use std::path::PathBuf;

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
