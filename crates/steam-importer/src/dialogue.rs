//! Dialogue drawing roles and authored story subtitles in the shared UI format.
use crate::{output, package::Package, pe, source::Source, text::Bank, ui_images};
use anyhow::{Context, Result};
use resonance_content::font::{
    BitmapFont, DialogueArt, MovieSubtitles, SelectionArt, SubtitleCue, SubtitleLine,
};
use std::path::Path;

pub(crate) fn cook(source: &Source, output: &Path, font: &BitmapFont) -> Result<()> {
    let bytes = source.require("SYSTEM.PAC")?;
    let package = Package::parse(&bytes)?;
    let names = [
        "ui/dialogue/symbols",
        "ui/dialogue/window-shading",
        "ui/windows/cracked-stone",
        "ui/windows/rough-stone",
        "ui/windows/fine-grain",
        "ui/windows/speckled",
        "ui/windows/small-stones",
        "ui/dialogue/lined-fill",
        "ui/windows/white-fill",
    ];
    let mut textures = Vec::new();
    for (index, name) in names.iter().enumerate() {
        textures.push(ui_images::cook(
            source,
            output,
            package
                .reference(index)?
                .context("missing dialogue image")?,
            &format!("{name}.ktx2"),
            false,
        )?);
    }
    let art = DialogueArt {
        font: "fonts/dialogue.json".into(),
        textures,
        cursor: ui_images::cook(
            source,
            output,
            "MENUSTYLE2TEX_PIC013.TOTEXB_D",
            "ui/cursors/gold-blade.ktx2",
            false,
        )?,
        selection: SelectionArt {
            mode: 2,
            color: [255; 4],
            row_offsets: [0; 9],
            bob_amplitude: 2.,
            bob_step: 0.1,
        },
    };
    art.validate()?;
    output::json(&output.join("ui/dialogue.json"), &art)?;
    subtitles(source, output, font)
}

fn subtitles(source: &Source, output: &Path, font: &BitmapFont) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let table = pe::word(image.slice(0x8b4f84, 4)?, 0)?;
    let text = pe::word(image.slice(0x8b4800 + source.language.slot() * 4, 4)?, 0)?;
    let mut cues = Vec::new();
    for index in 0..16 {
        let row = image.slice(table + index * 16, 16)?;
        let frame = pe::word(row, 0)?;
        if frame == u32::MAX {
            break;
        }
        let mut lines = Vec::new();
        for line in 0..3 {
            let reference = pe::word(row, 4 + line * 4)?;
            let id = pe::word(image.slice(text + reference * 4, 4)?, 0)?;
            let text = bank.menu(id, encoding_rs::WINDOWS_1252)?;
            if text.is_empty() {
                continue;
            }
            font.validate_text(&text)?;
            let mut width = 0;
            for character in text.chars() {
                let glyph = font
                    .glyphs
                    .get(&character)
                    .context("missing subtitle glyph")?;
                let scale = if resonance_content::font::is_single_byte(character) {
                    18. / 17.
                } else {
                    1.
                };
                width += (glyph.advance as f32 * scale) as i32 - 1;
            }
            lines.push(SubtitleLine {
                position: [320. - (width / 2) as f32, 240. + line as f32 * 29.],
                text,
            });
        }
        cues.push(SubtitleCue { frame, lines });
    }
    let track = MovieSubtitles { cues };
    track.validate()?;
    output::json(&output.join("ui/story-subtitles.json"), &track)
}
