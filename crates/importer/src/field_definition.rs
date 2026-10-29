//! Final field entry point, assembled from its native script and cooked resources.
use crate::{audio_speech, field_core};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::document::Definition,
    field_audio::{ServiceCue, document as audio},
    library::MOVIES,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_script::{NativeCall, Program, message, scenario};

fn arguments(
    program: &Program,
    analysis: &scenario::Analysis,
    call: NativeCall,
    arity: usize,
    position: usize,
) -> Result<BTreeSet<i32>> {
    let values = scenario::calls::analyzed_arguments(program, analysis, call, arity)?;
    values
        .into_values()
        .map(|args| args[position].with_context(|| format!("dynamic {call:?} resource ID")))
        .collect()
}

fn positive_ids(values: impl IntoIterator<Item = i32>) -> Result<BTreeSet<u32>> {
    values
        .into_iter()
        .filter(|id| *id >= 0)
        .map(|id| u32::try_from(id).map_err(Into::into))
        .collect()
}

fn mapped(library: &Path, mapping: &str, ids: &BTreeSet<u32>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let entries: std::collections::BTreeMap<u32, serde_json::Value> =
        serde_json::from_slice(&fs::read(library.join(mapping))?)?;
    for id in ids {
        let entry = entries
            .get(id)
            .with_context(|| format!("{mapping} lacks field resource {id}"))?;
        if let Some(path) = entry.as_str() {
            let asset = library
                .join(mapping)
                .parent()
                .context("mapping has no parent")?
                .join(path);
            ensure!(
                asset.is_file(),
                "field resource {id} lacks {}",
                asset.display()
            );
        }
    }
    Ok(())
}

/// Publish a field only after its script, scene, collision and actors are present.
pub fn cook_field_definition(extracted: &Path, library: &Path, id: u32) -> Result<()> {
    ensure!(
        (id as usize) < field_core::FIELD_COUNT,
        "invalid field ID {id}"
    );
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let script = format!("{root}/scripts/scenario.ssb");
    let scene = format!("{root}/scene.json");
    let actors = format!("{root}/actors.json");
    let collision = format!("{root}/collision.glb");
    for path in [&script, &scene, &actors] {
        ensure!(library.join(path).is_file(), "field {id} lacks {path}");
    }
    ensure!(
        library.join("fields/settings.json").is_file(),
        "field {id} lacks shared field settings"
    );
    let bytes = fs::read(library.join(&script))?;
    let program = Program::decode(&bytes)?;
    let analysis = scenario::analyze(&bytes)?;
    let messages = message::parse(
        bytes
            .get(scenario::parse_header(&bytes)?.auxiliary_offset()..)
            .context("field message table exceeds script")?,
    )?;
    let messages_path = format!("{root}/messages.json");
    if !messages.is_empty() {
        ensure!(
            library.join(&messages_path).is_file(),
            "field {id} lacks {messages_path}"
        );
    }
    let music = positive_ids(arguments(
        &program,
        &analysis,
        NativeCall::AudioCommand,
        1,
        0,
    )?)?;
    let mut effects: BTreeSet<u32> = ServiceCue::ALL.iter().map(|cue| *cue as u32).collect();
    for (call, arity, position) in [
        (NativeCall::PlaySoundSimple, 2, 0),
        (NativeCall::PlaySound, 4, 0),
        (NativeCall::PlayActorSound, 4, 1),
        (NativeCall::SetActorAmbientSound, 4, 1),
    ] {
        effects.extend(positive_ids(arguments(
            &program, &analysis, call, arity, position,
        )?)?);
    }
    let mut voices = positive_ids(arguments(&program, &analysis, NativeCall::PlayVoice, 1, 0)?)?;
    voices.retain(|voice| voice >> 16 != 0);
    for text in &messages {
        for token in &text.tokens {
            if let message::Token::Control {
                opcode: 9,
                expression,
            } = token
            {
                let voice = expression
                    .constant()
                    .context("dynamic field message voice")?;
                if voice >= 0 {
                    voices.insert(u32::try_from(voice)?);
                }
            }
        }
    }
    let movies = positive_ids(arguments(
        &program,
        &analysis,
        NativeCall::PlayMovieBlocking,
        1,
        0,
    )?)?;
    mapped(library, audio::MUSIC, &music)?;
    mapped(library, audio::EFFECTS, &effects)?;
    if !movies.is_empty() {
        mapped(library, MOVIES, &movies)?;
    }
    if !voices.is_empty() {
        mapped(library, audio::SPEECH, &voices)?;
    }
    let rates = if voices.is_empty() {
        BTreeMap::new()
    } else {
        audio_speech::speech_playback_rates(extracted, voices.iter().copied())?
    };
    let mut speech = BTreeMap::<Option<u32>, BTreeSet<u32>>::new();
    for voice in voices {
        speech
            .entry(rates.get(&voice).copied())
            .or_default()
            .insert(voice);
    }
    let definition = Definition {
        script,
        messages: (!messages.is_empty()).then_some(messages_path),
        scene,
        collision: library.join(&collision).is_file().then_some(collision),
        actors,
        audio: audio::Definition {
            music,
            effects,
            speech: speech
                .into_iter()
                .map(|(sample_rate, ids)| audio::Speech { ids, sample_rate })
                .collect(),
            ..Default::default()
        },
        movies,
    };
    crate::output::json(&library.join(format!("{root}/field.json")), &definition)
}
