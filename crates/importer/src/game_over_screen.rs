//! Original defeat text and title artwork directly into the editable screen.
use crate::{dol, native_archive, text, world_texture};
use anyhow::Result;
use resonance_content::{font::UiTexture, game_over::Art};
use std::{fs, path::Path};

pub fn cook_game_over_screen(extracted: &Path, library: &Path) -> Result<()> {
    let image_path = library.join("ui/game-over/background.ktx2");
    let document_path = library.join("ui/game-over/game-over.json");

    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let filename = text::string(&exe, 0x8017_d870)?;
    let tpl = native_archive::read_named(&extracted.join("disc1/files"), &filename)?;
    let descriptor_table = dol::u32(&tpl, 8)? as usize;
    let descriptor = dol::u32(&tpl, descriptor_table + 14 * 8)? as usize;
    let width = u32::from(dol::u16(&tpl, descriptor + 2)?);
    let height = u32::from(dol::u16(&tpl, descriptor)?);
    let (image, _) = world_texture::tpl_texture(&tpl, 14)?;
    let art = Art {
        background: UiTexture {
            path: "ui/game-over/background.ktx2".into(),
            width,
            height,
        },
        font: "fonts/dialogue.json".into(),
        caption: text::string(&exe, 0x8017_d87c)?,
        choices: [
            text::string(&exe, 0x801f_9c48)?,
            text::string(&exe, 0x801f_9c54)?,
        ],
    };
    art.validate()?;

    for (path, bytes) in [
        (image_path, image),
        (document_path, resonance_asset_writer::json::encode(&art)?),
    ] {
        crate::output::write(&path, &bytes)?;
    }
    Ok(())
}
