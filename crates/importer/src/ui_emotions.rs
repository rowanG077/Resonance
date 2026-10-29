//! Izoold's field-local emotion and selection pictures into named textures.
use crate::{dol, menu_sprites, ui_controls};
use anyhow::{Context, Result, ensure};
use std::path::Path;

const PICTURES: [&str; 10] = [
    "ui/izoold/selection-board",
    "ui/emotions/anger",
    "ui/emotions/heart",
    "ui/emotions/idea",
    "ui/emotions/confusion",
    "ui/emotions/music",
    "ui/emotions/question",
    "ui/emotions/ellipsis",
    "ui/emotions/sweat",
    "ui/emotions/sleep",
];

pub(crate) fn picture_path(section: usize) -> Option<String> {
    PICTURES
        .get(section.checked_sub(20)?)
        .map(|name| format!("{name}.ktx2"))
}

pub fn cook_ui_emotions(extracted: &Path, library: &Path) -> Result<()> {
    let archive = ui_controls::field_archive(extracted, 354)?;
    for (offset, name) in PICTURES.iter().enumerate() {
        let atlas = ui_controls::local_texture(&archive, 20 + offset)
            .with_context(|| format!("native image {name}"))?;
        ensure!(
            dol::u32(&atlas, 4)? == 1,
            "{name} has unexpected image count"
        );
        let (width, height, rgba) = menu_sprites::decode_image(&atlas, 0)?;
        crate::output::write(
            &library.join(format!("{name}.ktx2")),
            &resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?,
        )?;
    }
    Ok(())
}
