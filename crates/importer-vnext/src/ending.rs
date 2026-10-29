//! Ending portraits and FIN lettering from three native texture banks.
use crate::{dol, menu_sprites, native_archive, text};
use anyhow::{Result, ensure};
use std::{fs, path::Path};

pub(crate) const FIN: [&str; 4] = [
    "ui/ending/fin-f.ktx2",
    "ui/ending/fin-i.ktx2",
    "ui/ending/fin-n.ktx2",
    "ui/ending/fin-dot.ktx2",
];

fn source(exe: &[u8], files: &Path, id: usize) -> Result<Vec<u8>> {
    let directory = dol::slice(exe, 0x801f_85e4, 53 * 4)?;
    let address = dol::u32(directory, id * 4)?;
    let filename = text::string(exe, address)?;
    native_archive::read_named(files, &filename)
}

pub fn cook_ending(extracted: &Path, library: &Path) -> Result<()> {
    let disc = extracted.join("disc1");
    let files = disc.join("files");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let mut output = Vec::new();
    for (id, name) in [
        (49, "ui/ending/party-with-kratos.ktx2"),
        (50, "ui/ending/party-with-zelos.ktx2"),
    ] {
        let tpl = source(&exe, &files, id)?;
        ensure!(
            dol::u32(&tpl, 4)? == 1,
            "ending portrait bank is not singular"
        );
        let (width, height, rgba) = menu_sprites::decode_image(&tpl, 0)?;
        output.push((
            name,
            resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?,
        ));
    }
    let tpl = source(&exe, &files, 51)?;
    ensure!(dol::u32(&tpl, 4)? == 4, "FIN bank must have four letters");
    for (index, name) in FIN.into_iter().enumerate() {
        let (width, height, rgba) = menu_sprites::decode_image(&tpl, index)?;
        output.push((
            name,
            resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?,
        ));
    }
    for (relative, bytes) in output {
        let path = library.join(relative);
        crate::output::write(&path, &bytes)?;
    }
    let path = library.join("ui/ending/fin.json");
    crate::output::json(&path, &FIN)?;
    Ok(())
}
