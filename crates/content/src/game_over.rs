//! Original fatal-defeat artwork and text; behavior belongs to the game controller.
use crate::{field_preload::File, font::UiTexture};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "game/game-over.json";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Art {
    pub version: u32,
    pub source_sha256: String,
    pub background: UiTexture,
    pub font: String,
    pub caption: String,
    pub choices: [String; 2],
    pub files: BTreeMap<String, File>,
}
impl Art {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported game-over artwork");
        self.background.validate()?;
        crate::validate_asset_path(&self.font)?;
        ensure!(
            self.background.width == 640 && self.background.height == 480,
            "invalid game-over background size"
        );
        for text in std::iter::once(&self.caption).chain(&self.choices) {
            ensure!(
                text.len() <= 1024 && text.chars().all(|c| !c.is_control()),
                "invalid game-over text"
            );
        }
        for path in [&self.background.path, &self.font] {
            ensure!(
                self.files.contains_key(path),
                "unlisted game-over dependency {path}"
            );
        }
        Ok(())
    }
}
