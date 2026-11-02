//! Shared menu cue bindings and editable acoustics for recorded source audio.
use crate::output;
use anyhow::{Context, Result};
use resonance_content::{
    field_audio::{MusicReverbs, mix::Mix},
    library::Mappings,
    startup,
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn cook(library: &Path) -> Result<()> {
    let mappings: Mappings =
        serde_json::from_slice(&fs::read(library.join("audio/mappings.json"))?)?;
    let cues = [("navigate", 1), ("confirm", 2), ("back", 3), ("error", 4)]
        .into_iter()
        .map(|(name, id)| Ok((name.to_owned(), format!("audio/{}", mappings.target(id)?))))
        .collect::<Result<BTreeMap<_, _>>>()?;
    output::json(
        &library.join("audio/menu.json"),
        &startup::Cues {
            version: startup::VERSION,
            reverbs: None,
            cues,
        },
    )?;
    let music: Mappings =
        serde_json::from_slice(&fs::read(library.join("audio/music/mappings.json"))?)?;
    let count = music
        .iter()
        .map(|(id, _)| id as usize)
        .max()
        .context("empty music mappings")?
        + 1;
    // These recordings already include the port's authored mix. Keep the shared
    // effect buses dry until native acoustic presets have an explicit owner.
    let dry = [0., 0., 0.01, 0., 0.];
    let mix = Mix {
        music_gains: (0..128).map(|level| level as f32 / 127.).collect(),
        music_reverbs: MusicReverbs {
            presets: vec![dry],
            selectors: vec![0; count],
        },
        voice_gains: (0..128).map(|level| level as f32 / 127.).collect(),
        field_reverbs: [dry; 2],
        battle_reverbs: [dry; 2],
    };
    mix.validate()?;
    output::json(&library.join("audio/mix.json"), &mix)
}
