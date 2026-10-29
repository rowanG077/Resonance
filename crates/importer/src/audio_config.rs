//! Small shared audio settings kept apart from individual recordings and cues.
use crate::{audio_music, dol};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field_audio::{MusicReverbs, mix::Mix},
    startup,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::EffectId;

/// Menu actions bind directly to the four named resident sound cues.
pub fn cook_menu_audio(_root: &Path, library: &Path) -> Result<()> {
    let cues = [
        ("back", EffectId::Cancel),
        ("confirm", EffectId::Confirm),
        ("error", EffectId::Error),
        ("navigate", EffectId::Navigate),
    ]
    .into_iter()
    .map(|(action, id)| {
        (
            action.to_owned(),
            format!("audio/effects/{}.json", id.resource_name().unwrap()),
        )
    })
    .collect::<BTreeMap<_, _>>();
    let document = startup::Cues {
        version: startup::VERSION,
        reverbs: None,
        cues,
    };
    let target = library.join("audio/menu.json");
    ensure!(!target.exists(), "menu audio definition already exists");
    crate::output::json(&target, &document)?;
    Ok(())
}

/// Publish the shared mix from the executable's reverb and volume tables.
pub fn cook_mix_audio(root: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let field_reverbs = audio_music::reverbs(&exe, 1)?;
    let battle_reverbs = audio_music::reverbs(&exe, 2)?;
    let selectors = dol::slice(&exe, 0x8021_08b0, 112)?.to_vec();
    let music_reverbs = MusicReverbs {
        presets: vec![field_reverbs[0], battle_reverbs[0]],
        selectors,
    };
    let volume = audio_music::tables(&exe)?.mix.volume;
    let settings = dol::slice(&exe, 0x801f_9bc4, 128)?;
    let attenuation = dol::slice(&exe, 0x802b_4680, 1000)?;
    let voice_gains = settings
        .iter()
        .map(|&setting| {
            let index = (usize::from(setting) * 10).min(999);
            let control = usize::from(attenuation[index]);
            volume
                .get(control)
                .copied()
                .with_context(|| format!("invalid native voice control {control}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mix = Mix {
        music_reverbs,
        voice_gains,
        field_reverbs,
        battle_reverbs,
    };
    mix.validate()?;
    let target = library.join("audio/mix.json");
    ensure!(!target.exists(), "shared audio mix already exists");
    crate::output::json(&target, &mix)?;
    Ok(())
}

/// One synthesis table shared by all editable effect and battle cue recipes.
pub fn cook_synthesis_audio(root: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let tables = audio_music::tables(&exe)?;
    let target = library.join("audio/synthesis.json");
    ensure!(!target.exists(), "shared audio synthesis already exists");
    crate::output::json(&target, &tables)?;
    Ok(())
}
