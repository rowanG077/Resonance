//! Original fatal-defeat artwork and text; behavior belongs to the game controller.
use crate::font::UiTexture;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub mod document;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Art {
    #[serde(with = "crate::ui_art::image_path")]
    pub background: UiTexture,
    pub font: String,
    pub caption: String,
    #[serde(with = "crate::ui_art::game_over_choices")]
    pub choices: [String; 2],
}
impl Art {
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let art: Self = files.json(document::PATH)?;
        art.prepare(|path| files.read(path), files.diagnostics())
    }

    pub fn validate(&self) -> Result<()> {
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
