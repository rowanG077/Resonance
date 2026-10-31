//! Preserve the world's native script bytecode and extract any message table.
use anyhow::{Context, Result};
use std::{fs, path::Path};
use symphonia_script::{Program, message, scenario};

pub fn cook_world_script(root: &Path, library: &Path) -> Result<()> {
    let script_path = library.join("world/scripts/scenario.ssb");
    let messages_path = library.join("world/messages.json");
    let bytes = fs::read(root.join("disc1/files/field_sil.so"))?;
    let header = scenario::parse_header(&bytes)?;
    Program::decode(&bytes)?;
    let messages = message::parse(
        bytes
            .get(header.auxiliary_offset()..)
            .context("world script message offset exceeds file")?,
    )?;
    crate::output::write(&script_path, &bytes)?;
    if !messages.is_empty() {
        crate::output::json(&messages_path, &messages)?;
    }
    Ok(())
}
