//! Original fatal-defeat artwork and text; behavior belongs to the game controller.
use crate::font::UiTexture;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub mod document;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Art {
    pub version: u32,
    pub background: UiTexture,
    pub font: String,
    pub caption: String,
    pub choices: [String; 2],
}
impl Art {
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let definition: document::Definition = files.json(document::PATH)?;
        definition.art()
    }

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
        Ok(())
    }
}
