//! Credits text, pictures and an ordinary recording. Runtime timing is derived
//! from the captured WAV, preserving an explicit playback-rate override only.
use super::{Credits, Manifest, Operation};
use crate::{field_audio::Voice, font::UiTexture, library::Snapshot, preparation};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const PATH: &str = "ui/credits/credits.json";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub program: Credits,
    pub pictures: Vec<UiTexture>,
    pub music: Recording,
    pub final_hold_ticks: u32,
}

impl Definition {
    pub fn prepare(&self, mut read: impl FnMut(&str) -> Result<Arc<[u8]>>) -> Result<Manifest> {
        ensure!(self.version == VERSION, "unsupported named credits version");
        crate::library::validate_path(&self.music.path)?;
        let bytes =
            read(&self.music.path).with_context(|| format!("{PATH} music {}", self.music.path))?;
        let music = Voice::from_wave(&self.music.path, self.music.sample_rate, &bytes)
            .with_context(|| format!("{PATH} music {}", self.music.path))?;
        let manifest = Manifest {
            program: self.program.clone(),
            pictures: self.pictures.clone(),
            music,
            final_hold_ticks: self.final_hold_ticks,
        };
        manifest.validate().with_context(|| PATH.to_owned())?;
        for picture in &manifest.pictures {
            read(&picture.path).with_context(|| format!("{PATH} picture {}", picture.path))?;
        }
        Ok(manifest)
    }
}

pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<()> {
    ensure!(!cancelled(), "credits preparation cancelled");
    let definition: Definition = snapshot.json(PATH, preparation::JSON_LIMIT)?;
    let manifest = definition.prepare(|path| {
        ensure!(!cancelled(), "credits preparation cancelled");
        snapshot.read(path, preparation::ASSET_LIMIT)
    })?;
    let dialogue: crate::font::DialogueArt =
        snapshot.json("ui/dialogue.json", preparation::JSON_LIMIT)?;
    dialogue.validate()?;
    let font = preparation::font(snapshot, &dialogue.font)?;
    for operation in &manifest.program.operations {
        if let Operation::Text { text } = operation {
            for c in text.chars() {
                ensure!(
                    font.glyphs.contains_key(&c),
                    "credits text needs missing glyph {c:?} in {}",
                    dialogue.font
                );
            }
        }
    }
    ensure!(!cancelled(), "credits preparation cancelled");
    Ok(())
}
