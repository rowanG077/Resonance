//! Field entry points select the same recorded audio resources as the GC cook.
use crate::{
    output,
    script_audio::{self, Script, select},
    source::Source,
};
use anyhow::{Context, Result};
use resonance_content::{
    field::document::Definition,
    field_audio::{ServiceCue, document as audio, speech},
    library::{FIELDS, Mappings},
};
use std::{collections::BTreeSet, fs, path::Path};
use symphonia_script::{NativeCall, message::Message};

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
        let script = Script::read(&bytes)?;
        let mut selected = audio::Definition {
            reverbs: field.audio.reverbs,
            ..Default::default()
        };
        selected.music = select(script.arguments(NativeCall::AudioCommand, 1, 0)?, &music)
            .with_context(|| format!("field {id} music"))?;
        selected
            .effects
            .extend(ServiceCue::ALL.iter().map(|cue| *cue as u32));
        let dynamic_effects = script
            .effects(source, library, &mut effects, &mut selected.effects)
            .with_context(|| format!("field {id} effects"))?;
        let mut requested = script.arguments(NativeCall::PlayVoice, 1, 0)?;
        if let Some(messages) = &field.messages {
            let messages: Vec<Message> =
                serde_json::from_slice(&fs::read(library.join(messages))?)?;
            requested.extend(script_audio::message_voices(&messages));
        }
        let requested = select(requested, &voices).with_context(|| format!("field {id} speech"))?;
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
