use super::Art;
use crate::{font::UiTexture, library::Snapshot, preparation};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "ui/game-over/game-over.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub background: UiTexture,
    pub font: String,
    pub caption: String,
    pub choices: [String; 2],
}

impl Definition {
    pub fn art(&self) -> Result<Art> {
        let art = Art {
            version: self.version,
            background: self.background.clone(),
            font: self.font.clone(),
            caption: self.caption.clone(),
            choices: self.choices.clone(),
        };
        art.validate().with_context(|| PATH.to_owned())?;
        Ok(art)
    }
}

pub fn capture(
    snapshot: &mut Snapshot,
    diagnostics: &crate::diagnostics::Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    ensure!(!cancelled(), "game-over preparation cancelled");
    let definition: Definition = snapshot.json(PATH, preparation::JSON_LIMIT)?;
    let art = definition.art()?;
    diagnostics.attempt(
        "game-over background",
        snapshot
            .read(&art.background.path, preparation::ASSET_LIMIT)
            .with_context(|| format!("{PATH} background {}", art.background.path)),
    )?;
    ensure!(!cancelled(), "game-over preparation cancelled");
    let font = preparation::font(snapshot, &art.font)?;
    ensure!(
        std::iter::once(&art.caption)
            .chain(&art.choices)
            .flat_map(|text| text.chars())
            .all(|c| font.glyphs.contains_key(&c)),
        "game-over text needs a missing glyph in {}",
        art.font
    );
    ensure!(!cancelled(), "game-over preparation cancelled");
    Ok(())
}
