//! Authored meshes and material recipes, independent of renderer capabilities.
use crate::{geometry, geometry::MaterialRecipe};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Scene {
    pub(crate) mesh: String,
    pub(crate) bone_names: Vec<String>,
    pub(crate) textures: TextureSource,
    pub(crate) draws: Vec<Draw>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TextureSource {
    Local { catalogue: String },
    Caller,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Draw {
    pub(crate) source_index: usize,
    /// Empty authored draws retain their metadata and hierarchy without a mesh.
    pub(crate) mesh: Option<usize>,
    pub(crate) model_node: Option<usize>,
    pub(crate) draw_order: u32,
    pub(crate) name: String,
    pub(crate) index_count: usize,
    pub(crate) color: [f32; 4],
    /// Exporter alpha classification, not an authored blend-state command.
    /// Runtime binding must recalculate alpha when caller textures become known.
    pub(crate) preview_blend: bool,
    pub(crate) recipe: MaterialRecipe,
    pub(crate) textures: Vec<Texture>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Texture {
    pub(crate) stage: u8,
    /// Authored table label, never a filesystem path.
    pub(crate) table: Option<String>,
    pub(crate) image: u16,
    pub(crate) wrap: [resonance_content::TextureWrap; 2],
    pub(crate) min_filter: crate::tpl::Filter,
    pub(crate) mag_filter: crate::tpl::Filter,
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
    fn shared_node_modules_are_registered_in_each_package_receipt() -> Result<()> {
        let output = tempfile::tempdir()?;
        let _session = crate::publication::Session::start(output.path())?;
        let first = crate::publication::Capture::start()?;
        let path = publish_nodes(output.path(), &[])?.context("missing empty-skeleton symbols")?;
        let files = first.finish()?;
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            [&output.path().join(&path).canonicalize()?]
        );
        let second = crate::publication::Capture::start()?;
        assert_eq!(publish_nodes(output.path(), &[])?, Some(path.clone()));
        assert_eq!(second.finish()?, files);
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
        let unknown = crate::publication::Capture::start()?;
        assert!(
            publish_nodes(output.path(), &["not_a_shipped_skeleton_test_node".into()])?.is_none()
        );
        assert!(unknown.finish()?.is_empty());
        Ok(())
    }
}
