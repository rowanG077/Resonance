//! Native name editor labels and keyboard to the final editable JSON file.
use crate::text;
use anyhow::Result;
use resonance_content::menu_data::RenameData;
use std::{fs, path::Path};

pub fn cook_rename(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let [heading, delete, default, confirm, restore, cancel] = text::table(&exe, 0x8019d3e8)?;
    let document = RenameData {
        defaults: text::table(&exe, 0x8019d400)?,
        keyboard: text::pointer(&exe, 0x8035cf68)?,
        heading,
        delete,
        default,
        commands: [confirm, restore, cancel],
    };
    document.validate()?;
    let path = library.join(resonance_content::menu_data::RENAME_PATH);
    crate::output::json(&path, &document)?;
    Ok(())
}
