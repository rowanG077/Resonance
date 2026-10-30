//! Initial engine input icons are exported beside the other editable UI assets.
use anyhow::Result;
use resonance_content::input_prompts as prompts;
use std::path::Path;

const DEFAULT_KTX2: &[u8] = include_bytes!("../../../resources/input-prompts/atlas.ktx2");
const DEFAULT_JSON: &[u8] = include_bytes!("../../../resources/input-prompts/atlas.json");
const LICENSE: &[u8] = include_bytes!("../../../resources/input-prompts/License.txt");

pub fn cook_input_prompts(library: &Path) -> Result<()> {
    crate::output::write(&library.join(prompts::IMAGE_PATH), DEFAULT_KTX2)?;
    crate::output::write(&library.join(prompts::MANIFEST_PATH), DEFAULT_JSON)?;
    crate::output::write(&library.join(prompts::LICENSE_PATH), LICENSE)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooking_exports_the_entire_atlas_and_manifest_and_preserves_existing_files() -> Result<()> {
        let library = tempfile::tempdir()?;
        cook_input_prompts(library.path())?;
        assert_eq!(
            std::fs::read(library.path().join(prompts::IMAGE_PATH))?,
            DEFAULT_KTX2
        );
        let atlas: prompts::Atlas =
            serde_json::from_slice(&std::fs::read(library.path().join(prompts::MANIFEST_PATH))?)?;
        let defaults: prompts::Atlas = serde_json::from_slice(DEFAULT_JSON)?;
        assert_eq!(atlas.icons.len(), defaults.icons.len());
        assert_eq!(atlas.pressed, defaults.pressed);
        assert!(atlas.icons.len() > 100);
        assert!(cook_input_prompts(library.path()).is_err());
        Ok(())
    }
}
