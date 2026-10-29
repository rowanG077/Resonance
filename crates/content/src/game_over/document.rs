use super::Art;
use crate::{library::Snapshot, preparation};
use anyhow::{Context, Result, ensure};

pub const PATH: &str = "ui/game-over/game-over.json";

impl Art {
    pub fn prepare<B: AsRef<[u8]>>(
        mut self,
        read: impl FnMut(&str) -> Result<B>,
        diagnostics: &crate::diagnostics::Diagnostics,
    ) -> Result<Self> {
        if diagnostics
            .attempt(
                "game-over background",
                crate::ui_art::prepare_image(&mut self.background, read),
            )?
            .is_none()
        {
            // The existing lenient policy permits an absent optional backdrop.
            self.background.width = 640;
            self.background.height = 480;
        }
        self.validate().with_context(|| PATH.to_owned())?;
        Ok(self)
    }
}

pub fn capture(
    snapshot: &mut Snapshot,
    diagnostics: &crate::diagnostics::Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    ensure!(!cancelled(), "game-over preparation cancelled");
    let art: Art = snapshot.json(PATH, preparation::JSON_LIMIT)?;
    let art = art.prepare(
        |path| snapshot.read(path, preparation::ASSET_LIMIT),
        diagnostics,
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
