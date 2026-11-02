//! Field entry points select the same recorded audio resources as the GC cook.
use crate::{output, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::document::Definition,
    field_audio::{ServiceCue, document as audio, speech},
    library::{FIELDS, Mappings},
};
use std::{collections::BTreeSet, fs, path::Path};
use symphonia_script::{
    NativeCall, Program,
    message::{Message, Token},
    scenario,
};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    crate::audio_effects::bind_services(source, library)?;
    let mappings =
        |path| -> Result<Mappings> { Ok(serde_json::from_slice(&fs::read(library.join(path))?)?) };
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
    let mut definitions = Vec::new();
    for (id, name) in mappings(FIELDS)?.iter() {
        let path = library.join(format!("fields/{name}/field.json"));
        let mut field: Definition = serde_json::from_slice(&fs::read(&path)?)?;
        let bytes = fs::read(library.join(&field.script))?;
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
        let mut selected = audio::Definition {
            reverbs: field.audio.reverbs,
            ..Default::default()
        };
        select(
            arguments(NativeCall::AudioCommand, 1, 0)?,
            &music,
            &mut selected.music,
        )
        .with_context(|| format!("field {id} music"))?;
        selected
            .effects
            .extend(ServiceCue::ALL.iter().map(|cue| *cue as u32));
        let mut dynamic_effects = false;
        for (call, arity, position) in crate::script_audio::CALLS {
            let values = arguments(call, arity, position)?;
            dynamic_effects |= values.iter().any(Option::is_none);
            for &value in values.iter().flatten().filter(|value| **value >= 0) {
                if !effects.contains(&(value as u32)) {
                    crate::audio_effects::bind_unused(source, library, value as u32)
                        .with_context(|| format!("field {id} {call:?}"))?;
                    effects.insert(value as u32);
                }
            }
            select(values, &effects, &mut selected.effects)
                .with_context(|| format!("field {id} {call:?}"))?;
        }
        let mut requested = BTreeSet::new();
        let mut voice = |value: Option<i32>| {
            match value {
                Some(value) if value >= 0 => {
                    requested.insert(value as u32);
                }
                Some(_) => (),
                // Dynamic selectors use the published speech catalogue, just
                // as dynamic music/effects select their existing catalogues.
                None => requested.extend(&voices),
            }
        };
        for value in arguments(NativeCall::PlayVoice, 1, 0)? {
            voice(value);
        }
        if let Some(messages) = &field.messages {
            let messages: Vec<Message> =
                serde_json::from_slice(&fs::read(library.join(messages))?)?;
            for message in messages {
                for token in message.tokens {
                    if let Token::Control {
                        opcode: 9,
                        expression,
                    } = token
                    {
                        voice(expression.constant());
                    }
                }
            }
        }
        ensure!(
            requested.is_subset(&voices),
            "field {id} has missing voices: {:?}",
            requested.difference(&voices).collect::<Vec<_>>()
        );
        if !requested.is_empty() {
            selected.speech.push(audio::Speech {
                ids: requested,
                sample_rate: None,
            });
        }
        field.audio = selected;
        definitions.push((path, field, dynamic_effects));
    }
    // Native rejected selectors can add silence aliases while discovering
    // fields. Bind dynamic choices against the complete catalogue, independent
    // of field order or an earlier interrupted cook.
    for (path, mut field, dynamic_effects) in definitions {
        if dynamic_effects {
            field.audio.effects.extend(&effects);
        }
        output::json(&path, &field)?;
    }
    Ok(())
}

fn select(
    values: Vec<Option<i32>>,
    available: &BTreeSet<u32>,
    selected: &mut BTreeSet<u32>,
) -> Result<()> {
    for value in values {
        match value {
            Some(id) if id >= 0 => {
                ensure!(available.contains(&(id as u32)), "unmapped audio ID {id}");
                selected.insert(id as u32);
            }
            Some(_) => (), // Stop/pause/resume sentinels do not select recordings.
            None => selected.extend(available),
        }
    }
    Ok(())
}
