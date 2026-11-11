//! Initial engine input icons are exported beside the other editable UI assets.
use anyhow::Result;
use resonance_content::input_prompts as prompts;
use std::path::Path;

const DEFAULT_KTX2: &[u8] = include_bytes!("../../../resources/input-prompts/atlas.ktx2");
const DEFAULT_JSON: &[u8] = include_bytes!("../../../resources/input-prompts/atlas.json");
const LICENSE: &[u8] = include_bytes!("../../../resources/input-prompts/License.txt");

pub fn cook_input_prompts(library: &Path) -> Result<()> {
    let pixels = resonance_asset_writer::texture::ktx2(DEFAULT_KTX2)?;
    crate::output::write(
        &library.join(prompts::IMAGE_PATH),
        &resonance_asset_writer::gpu::encode(&pixels)?,
    )?;
    crate::output::write(&library.join(prompts::MANIFEST_PATH), DEFAULT_JSON)?;
    crate::output::write(&library.join(prompts::LICENSE_PATH), LICENSE)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooking_exports_the_entire_atlas_and_manifest_and_overwrites_existing_files() -> Result<()> {
        let library = tempfile::tempdir()?;
        cook_input_prompts(library.path())?;
        let image = std::fs::read(library.path().join(prompts::IMAGE_PATH))?;
        assert!(resonance_asset_writer::gpu::is_native_compressed(&image)?);
        let atlas: prompts::Atlas =
            serde_json::from_slice(&std::fs::read(library.path().join(prompts::MANIFEST_PATH))?)?;
        let defaults: prompts::Atlas = serde_json::from_slice(DEFAULT_JSON)?;
        assert_eq!(atlas.icons.len(), defaults.icons.len());
        assert_eq!(atlas.pressed, defaults.pressed);
        assert!(atlas.icons.len() > 100);
        assert_eq!(
            resonance_asset_writer::texture::dimensions(&image)?,
            atlas.size
        );
        std::fs::write(library.path().join(prompts::IMAGE_PATH), b"old output")?;
        cook_input_prompts(library.path())?;
        assert_eq!(
            std::fs::read(library.path().join(prompts::IMAGE_PATH))?,
            image
        );
        Ok(())
    }
}
