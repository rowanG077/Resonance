//! The overworld entry point references the final owners and their native audio IDs.
use crate::{rel::Rel, world_cinematic_rules};
use anyhow::{Context, Result};
use resonance_content::{
    field_audio::{ServiceCue, document as audio},
    overworld::{CinematicAudio, document as world},
};
use std::{collections::BTreeSet, fs, path::Path};
use symphonia_constants::MusicId;
use symphonia_script::{NativeCall, Program, message, scenario};

fn ids(
    program: &Program,
    analysis: &scenario::Analysis,
    call: NativeCall,
    arity: usize,
    position: usize,
) -> Result<BTreeSet<u32>> {
    let values = scenario::calls::analyzed_arguments(program, analysis, call, arity)?
        .into_values()
        .map(|args| args[position].with_context(|| format!("dynamic world {call:?} ID")))
        .collect::<Result<Vec<_>>>()?;
    values
        .into_iter()
        .filter(|id| *id >= 0)
        .map(|id| Ok(u32::try_from(id)?))
        .collect()
}

pub fn cook_world_definition(root: &Path, library: &Path) -> Result<()> {
    let script = fs::read(root.join("disc1/files/field_sil.so"))?;
    let program = Program::decode(&script)?;
    let analysis = scenario::analyze(&script)?;
    let messages = message::parse(
        script
            .get(scenario::parse_header(&script)?.auxiliary_offset()..)
            .context("world message table exceeds script")?,
    )?;
    let rel = Rel::read(fs::read(root.join("disc1/files/US_r_Top2field.rel"))?)?;
    let scenes = world_cinematic_rules::read(&rel)?;

    // These four native world travel tracks are selected by world state outside
    // scenario bytecode, so the scenario's explicit music calls are additive.
    let mut music: BTreeSet<u32> = [
        MusicId::TheLandOfSylvarant,
        MusicId::WalkingOnTetheAlla,
        MusicId::TripOnTheShip,
        MusicId::AviatorsRheaBird,
    ]
    .into_iter()
    .map(|id| u32::from(id.raw()))
    .collect();
    music.extend(ids(&program, &analysis, NativeCall::AudioCommand, 1, 0)?);

    let mut effects: BTreeSet<u32> = ServiceCue::ALL.iter().map(|cue| *cue as u32).collect();
    for (call, arity, position) in [
        (NativeCall::PlaySoundSimple, 2, 0),
        (NativeCall::PlaySound, 4, 0),
        (NativeCall::PlayActorSound, 4, 1),
        (NativeCall::SetActorAmbientSound, 4, 1),
    ] {
        effects.extend(ids(&program, &analysis, call, arity, position)?);
    }
    effects.extend(world_cinematic_rules::retained_sounds()?);
    let mut voices = ids(&program, &analysis, NativeCall::PlayVoice, 1, 0)?;
    voices.retain(|id| id >> 16 != 0);
    for text in &messages {
        for token in &text.tokens {
            if let message::Token::Control {
                opcode: 9,
                expression,
            } = token
            {
                let id = expression
                    .constant()
                    .context("dynamic world message voice")?;
                if id >= 0 {
                    voices.insert(u32::try_from(id)?);
                }
            }
        }
    }
    for scene in scenes.values() {
        voices.extend(scene.dialogue.iter().map(|line| line.voice));
        for sound in scene
            .presentation
            .initial_audio
            .iter()
            .chain(scene.presentation.audio.iter().map(|event| &event.action))
            .chain(&scene.presentation.end_audio)
            .filter_map(CinematicAudio::sound)
        {
            effects.insert(u32::try_from(sound)?);
        }
    }
    let definition = world::Definition {
        script: "world/scripts/scenario.ssb".into(),
        messages: (!messages.is_empty()).then(|| "world/messages.json".into()),
        audio: audio::Definition {
            music,
            effects,
            speech: (!voices.is_empty())
                .then_some(audio::Speech {
                    ids: voices,
                    sample_rate: None,
                })
                .into_iter()
                .collect(),
            ..Default::default()
        },
        movement: "world/movement.json".into(),
        encounters: "world/encounters.json".into(),
        landmarks: "world/landmarks.json".into(),
        continents: world::MAPPINGS.into(),
        actors: world::ACTORS.into(),
        markers: world::MARKERS.into(),
        cinematics: world::CINEMATICS.into(),
        dust: std::array::from_fn(|index| format!("world/effects/dust-{index}.ktx2")),
        skies: [
            "world/skies/normal/model.json".into(),
            "world/skies/late/model.json".into(),
        ],
    };
    definition.validate()?;
    crate::output::json(&library.join(world::PATH), &definition)
}
