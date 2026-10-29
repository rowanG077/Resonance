//! Battle playback settings from the native battle module and voice headers.
use crate::{audio_speech, rel::Rel};
use anyhow::{Result, ensure};
use resonance_content::{
    battle_audio::document::{Definition, PATH},
    field_audio::document::Speech,
};
use std::{collections::BTreeSet, fs, path::Path};

/// Publish the one battle audio configuration. Voice IDs and clocks come from
/// native archive metadata, without decoding or writing any recordings.
pub fn cook_battle_audio(root: &Path, library: &Path) -> Result<()> {
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let spatial = |offset| -> Result<[f32; 5]> {
        let bytes = module.slice(4, offset, 20)?;
        let mut values = [0.0; 5];
        for (index, value) in values.iter_mut().enumerate() {
            *value = f32::from_be_bytes(bytes[index * 4..index * 4 + 4].try_into()?);
        }
        Ok(values)
    };
    let rates = audio_speech::speech_playback_rates(root, audio_speech::speech_ids(root)?)?;
    ensure!(
        rates.values().all(|&rate| rate == 32_028),
        "unexpected native battle speech clock"
    );
    let ids = rates.into_keys().collect::<BTreeSet<_>>();
    let definition = Definition {
        speech_rates: if ids.is_empty() {
            Vec::new()
        } else {
            vec![Speech {
                ids,
                sample_rate: Some(32_028),
            }]
        },
        reverbs: None,
        effect_spatial: spatial(0x4d8)?,
        voice_spatial: spatial(0x4668)?,
    };
    let target = library.join(PATH);
    ensure!(!target.exists(), "battle audio definition already exists");
    crate::output::json(&target, &definition)?;
    Ok(())
}
