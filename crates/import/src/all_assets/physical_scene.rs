//! Authored meshes and material recipes, independent of renderer capabilities.
use crate::geometry;
use anyhow::{Context, Result};
pub(crate) use resonance_content::physical_scene::{Draw, Scene, Texture, TextureSource};
use std::path::Path;

/// Publish maintained symbols when available; unknown skeletons remain cookable.
pub(crate) fn publish_nodes(output: &Path, bones: &[String]) -> Result<Option<String>> {
    let digest = crate::digest(&serde_json::to_vec(bones)?);
    let Ok(index) = resonance_script_content::NODE_MODULES
        .binary_search_by_key(&digest.as_str(), |&(signature, _)| signature)
    else {
        return Ok(None);
    };
    let (_, relative) = resonance_script_content::NODE_MODULES[index];
    let source = resonance_script_content::FILES
        .iter()
        .find_map(|&(path, source)| (path == relative).then_some(source))
        .context("embedded node module is missing")?;
    let path = format!("scripts/{relative}");
    crate::write_atomic(&output.join(&path), source.as_bytes())?;
    Ok(Some(path))
}

#[cfg(test)]
pub(crate) fn cook(bytes: &[u8], output: &Path, textures: TextureSource) -> Result<Scene> {
    Ok(cook_decoded(bytes, output, textures, None)?.0.scene)
}

pub(crate) fn cook_decoded(
    bytes: &[u8],
    output: &Path,
    textures: TextureSource,
    alpha: Option<Vec<bool>>,
) -> Result<(geometry::DecodedGeometry, crate::publication::File)> {
    let mut decoded = match alpha {
        Some(alpha) => geometry::decode_section_with_alpha(bytes, textures, alpha)?,
        None => geometry::decode_section(bytes, textures)?,
    };
    let glb = resonance_asset_writer::gltf::pack_glb(&decoded.gltf, &decoded.binary)?;
    decoded.scene.mesh = format!("meshes/{}.glb", crate::digest(&glb));
    let published = crate::publication::File::write(&output.join(&decoded.scene.mesh), &glb)?;
    Ok((decoded, published))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_node_modules_publish_embedded_sources() -> Result<()> {
        let output = tempfile::tempdir()?;
        let _session = crate::publication::Session::start(output.path())?;
        let path = publish_nodes(output.path(), &[])?.context("missing empty-skeleton symbols")?;
        assert_eq!(publish_nodes(output.path(), &[])?, Some(path.clone()));
        let source = std::fs::read_to_string(output.path().join(&path))?;
        let relative = path.strip_prefix("scripts/").unwrap();
        assert_eq!(
            source,
            resonance_script_content::FILES
                .iter()
                .find(|&&(path, _)| path == relative)
                .unwrap()
                .1
        );
        assert!(
            publish_nodes(output.path(), &["not_a_shipped_skeleton_test_node".into()])?.is_none()
        );
        Ok(())
    }
}
