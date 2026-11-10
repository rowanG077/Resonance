//! Generated known-ID definitions and authored runtime behaviors.
use anyhow::Result;
use std::path::Path;

fn definitions() -> &'static symphonia_script_compiler::scenario::Definitions {
    static DEFINITIONS: std::sync::OnceLock<symphonia_script_compiler::scenario::Definitions> =
        std::sync::OnceLock::new();
    DEFINITIONS.get_or_init(|| {
        symphonia_script_compiler::scenario::Definitions::from_sources(
            &resonance_script_content::modules(),
        )
        .expect("generated standard definitions are valid")
    })
}

/// Publish editable source only after its compiled bytes match the input.
pub(crate) fn cook_scenario(library: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    let result = symphonia_script_compiler::scenario::decompile_with_context(
        bytes,
        definitions(),
        symphonia_script_compiler::scenario::Context::from_path(path.strip_prefix(library)?),
    )?;
    crate::output::write(path, result.source.as_bytes())
}

pub(crate) fn read_scenario(path: &Path) -> Result<Vec<u8>> {
    Ok(symphonia_script_compiler::scenario::compile(
        &path.display().to_string(),
        &std::fs::read_to_string(path)?,
        definitions(),
    )?)
}

pub fn cook_scripts(library: &Path) -> Result<()> {
    for (relative, source) in resonance_script_content::files() {
        let path = format!("scripts/{relative}");
        resonance_content::validate_asset_path(&path)?;
        crate::output::write(&library.join(path), source.as_bytes())?;
    }
    Ok(())
}
