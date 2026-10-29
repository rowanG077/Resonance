//! The GameCube controller pictures from the field that presents the help screen.
use crate::{compression, dol, field_core, menu_sprites};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

const FIELD: usize = 77;
const NAMES: [&str; 32] = [
    "stick-neutral",
    "stick-up",
    "stick-down",
    "stick-right",
    "stick-left",
    "a-raised",
    "a-pressed",
    "b-raised",
    "b-pressed",
    "x-side",
    "x-front",
    "y-side",
    "y-front",
    "l-raised",
    "l-pressed",
    "r-raised",
    "r-pressed",
    "round-button-raised",
    "round-button-pressed",
    "window-left",
    "window-middle",
    "window-right",
    "z-raised",
    "z-pressed",
    "c-stick-neutral",
    "c-stick-up",
    "c-stick-down",
    "d-pad-neutral",
    "d-pad-up",
    "d-pad-down",
    "d-pad-right",
    "d-pad-left",
];

pub(crate) fn field_archive(extracted: &Path, field: usize) -> Result<Vec<u8>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let row = dol::slice(&exe, field_core::PHASES + field as u32 * 24, 24)?;
    field_core::native_archive(extracted, &exe, row, field)?
        .with_context(|| format!("field {field} is absent"))
}

pub(crate) fn local_texture(archive: &[u8], section: usize) -> Result<Vec<u8>> {
    let bytes = field_core::optional_section(archive, section)?
        .with_context(|| format!("local image section {section} is absent"))?;
    let bytes = if bytes.len() >= 9
        && matches!(bytes[0], 0 | 1 | 3)
        && usize::try_from(u32::from_le_bytes(bytes[1..5].try_into()?))? + 9 == bytes.len()
    {
        compression::decode(bytes)?
    } else {
        bytes.to_vec()
    };
    ensure!(
        dol::u32(&bytes, 0)? == 0x0020_af30,
        "local image is not TPL"
    );
    Ok(bytes)
}

pub fn cook_ui_controls(extracted: &Path, library: &Path) -> Result<()> {
    let archive = field_archive(extracted, FIELD)?;
    let atlas = local_texture(&archive, 16)?;
    ensure!(
        dol::u32(&atlas, 4)? as usize == NAMES.len(),
        "controller image count differs"
    );
    let mut paths = Vec::with_capacity(NAMES.len());
    for (index, name) in NAMES.iter().enumerate() {
        let (width, height, rgba) = menu_sprites::decode_image(&atlas, index)
            .with_context(|| format!("controller-help image {name}"))?;
        let path = format!("ui/controls/gamecube/{name}.ktx2");
        crate::output::write(
            &library.join(&path),
            &resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?,
        )?;
        paths.push(path);
    }
    crate::output::json(&library.join("ui/controls/gamecube.json"), &paths)
}
