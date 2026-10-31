//! Named party portraits from the native battle UI TPL bank.
use crate::{dol, native_archive, world_texture};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};
use symphonia_constants::PartyCharacterId;

/// Convert the nine battle portraits without touching the unrelated HUD atlas.
pub fn cook_battle_ui_portraits(root: &Path, library: &Path) -> Result<()> {
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let images = native_archive::member(&usual, 4, 14)?.context("battle images are missing")?;
    let portraits =
        native_archive::member(images, 2, 5)?.context("battle portraits are missing")?;
    ensure!(
        dol::u32(portraits, 0)? == 0x0020_af30,
        "battle portraits are not a TPL"
    );
    let count = dol::u32(portraits, 4)? as usize;
    ensure!(
        count == PartyCharacterId::RESOURCE_NAMES.len(),
        "unexpected battle portrait count"
    );
    let mut files = Vec::with_capacity(count);
    for (index, name) in PartyCharacterId::RESOURCE_NAMES.iter().enumerate() {
        let (bytes, _) = world_texture::tpl_texture(portraits, index)?;
        files.push((
            library.join(format!("ui/battle/portraits/{name}.ktx2")),
            bytes,
        ));
    }
    for (path, bytes) in files {
        crate::output::write(&path, &bytes)?;
    }
    Ok(())
}
