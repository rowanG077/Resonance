//! Generated known-ID definitions and authored runtime behaviors.
use anyhow::Result;
use std::path::Path;

pub fn cook_scripts(library: &Path) -> Result<()> {
    for (relative, source) in resonance_script_content::files() {
        let path = format!("scripts/{relative}");
        resonance_content::validate_asset_path(&path)?;
        crate::output::write(&library.join(path), source.as_bytes())?;
    }
    Ok(())
}
