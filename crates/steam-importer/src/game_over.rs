//! Native defeat artwork and localized captions in the common document.
use crate::{output, pe, source::Source, text, ui_images};
use anyhow::Result;
use resonance_content::font::UiTexture;
use std::path::Path;

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = text::Bank::parse(&bytes)?;
    let slot = source.language.slot();
    let pixels = ui_images::read(source, "TITLE_PIC014.TOTEXB_D")?;
    let pixels = image::imageops::resize(&pixels, 640, 480, image::imageops::FilterType::Triangle);
    let path = "ui/game-over/background.ktx2";
    output::write(
        &output.join(path),
        &resonance_asset_writer::ktx2::encode_levels(640, 480, &[pixels.as_raw()])?,
    )?;
    let text = |address| -> Result<String> {
        bank.menu(
            pe::word(image.slice(address + slot * 4, 4)?, 0)?,
            encoding_rs::WINDOWS_1252,
        )
    };
    let definition = resonance_content::game_over::Art {
        background: UiTexture {
            path: path.into(),
            width: 640,
            height: 480,
        },
        font: "fonts/dialogue.json".into(),
        caption: text(0x8d81a0)?,
        choices: [text(0x8d8140)?, text(0x8d8170)?],
    };
    definition.validate()?;
    output::json(
        &output.join(resonance_content::game_over::document::PATH),
        &definition,
    )
}
