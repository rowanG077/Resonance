//! Physical geometry and texture recipes retained independently of renderer
//! support. Paths name owned files; material table labels are authored symbols.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
mod draws;
#[cfg(test)]
mod tests;
mod texture;

/// Start with vertex color and apply the operations in order, clamping to [0, 1].
/// Inherited state is explicit; native GPU stage counts are not authoring data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialRecipe {
    Inherit,
    #[serde(untagged)]
    Operations(Vec<MaterialOperation>),
}

impl Default for MaterialRecipe {
    fn default() -> Self {
        Self::Operations(Vec::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialOperation {
    MultiplyTexture(u8),
    /// Interpolate RGB toward the texture using its alpha; preserve previous alpha.
    DecalTexture(u8),
    /// Interpolate RGB toward white using texture RGB; multiply alpha by texture alpha.
    BlendTexture(u8),
    ReplaceTexture(u8),
    /// Add texture RGB, clamp to [0, 1], and preserve previous alpha.
    AddTexture(u8),
    /// Subtract texture RGB, clamp to [0, 1], and preserve previous alpha.
    SubtractTexture(u8),
    /// Requires the draw matrix for its UV transform, prior alpha to weight the
    /// added texture RGB, and the caller's constant alpha for its output alpha.
    DefaultViewTexture(u8),
    MultiplyVertexColor,
}

impl MaterialRecipe {
    pub fn operations(&self) -> &[MaterialOperation] {
        match self {
            Self::Inherit => &[],
            Self::Operations(operations) => operations,
        }
    }

    pub fn textures(&self) -> impl Iterator<Item = u8> + '_ {
        self.operations()
            .iter()
            .filter_map(|operation| match operation {
                MaterialOperation::MultiplyTexture(texture)
                | MaterialOperation::DecalTexture(texture)
                | MaterialOperation::BlendTexture(texture)
                | MaterialOperation::ReplaceTexture(texture)
                | MaterialOperation::AddTexture(texture)
                | MaterialOperation::SubtractTexture(texture)
                | MaterialOperation::DefaultViewTexture(texture) => Some(*texture),
                MaterialOperation::MultiplyVertexColor => None,
            })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub mesh: String,
    /// Prepared from the GLB, never duplicated in the material document.
    #[serde(skip)]
    pub bone_names: Vec<String>,
    #[serde(default, skip_serializing_if = "TextureSource::is_empty")]
    pub textures: TextureSource,
    #[serde(with = "draws")]
    pub draws: Vec<Draw>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureSource {
    Caller,
    #[serde(untagged)]
    Local(TextureCatalogue),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Draw {
    /// Empty authored draws retain their metadata and hierarchy without a mesh.
    pub mesh: Option<usize>,
    pub node: Option<String>,
    /// Resolved from the current GLB; native import prepares the same index.
    #[serde(skip)]
    pub model_node: Option<usize>,
    pub draw_order: u32,
    /// Importer-only alpha classification used when preparing field materials.
    /// It is derived from the current pixels and vertex colors, never authored.
    #[serde(skip)]
    pub preview_blend: bool,
    pub recipe: MaterialRecipe,
    pub textures: Vec<Texture>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Texture {
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub stage: u8,
    /// Authored table label, never a filesystem path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    pub image: u16,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub wrap: [crate::TextureWrap; 2],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub min_filter: crate::texture::Filter,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub mag_filter: crate::texture::Filter,
}

/// Alternate images for one texture slot. KTX2 owns dimensions and pixel format.
#[derive(Debug, Clone)]
pub struct TextureImage {
    pub sampler: crate::texture::Sampler,
    pub images: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TextureCatalogue {
    pub textures: Vec<Option<TextureImage>>,
}

impl Scene {
    pub fn prepare_bones(&mut self, mesh: &[u8]) -> Result<()> {
        self.bone_names = crate::animation::joint_names_from_glb(mesh)?;
        for draw in &mut self.draws {
            draw.model_node = draw
                .node
                .as_ref()
                .map(|name| {
                    self.bone_names
                        .iter()
                        .position(|bone| bone == name)
                        .with_context(|| format!("unknown physical draw joint {name}"))
                })
                .transpose()?;
        }
        Ok(())
    }

    /// This validates the physical recipe, without claiming the current renderer
    /// can project its operations or inventing textures supplied by a caller.
    pub fn validate(&self) -> Result<()> {
        crate::library::validate_path(&self.mesh)?;
        if let TextureSource::Local(catalogue) = &self.textures {
            catalogue.validate()?;
        }
        let mut meshes = std::collections::BTreeSet::new();
        for draw in &self.draws {
            ensure!(
                draw.node.as_ref().is_none_or(|node| self
                    .bone_names
                    .iter()
                    .filter(|bone| *bone == node)
                    .count()
                    == 1),
                "unknown or ambiguous physical draw joint {:?}",
                draw.node
            );
            if let Some(mesh) = draw.mesh {
                ensure!(
                    meshes.insert(mesh),
                    "duplicate physical mesh assignment {mesh}"
                );
            }
            ensure!(
                draw.recipe.operations().len() <= 16
                    && draw.recipe.textures().all(|texture| texture < 16),
                "invalid material recipe for mesh {:?}",
                draw.mesh
            );
            let mut stages = std::collections::BTreeSet::new();
            for texture in &draw.textures {
                ensure!(
                    texture.stage < 8 && stages.insert(texture.stage),
                    "duplicate or invalid texture stage for mesh {:?}",
                    draw.mesh
                );
                ensure!(
                    matches!(
                        texture.mag_filter,
                        crate::texture::Filter::Nearest | crate::texture::Filter::Linear
                    ),
                    "invalid magnification filter for mesh {:?}",
                    draw.mesh
                );
            }
        }
        Ok(())
    }
}

impl TextureCatalogue {
    /// A single ordinary image needs no bank document. Image headers and bytes
    /// are captured by the caller's current library view, just like JSON banks.
    pub fn load<B: AsRef<[u8]>>(
        path: &str,
        mut read: impl FnMut(&str) -> Result<B>,
    ) -> Result<Self> {
        crate::library::validate_path(path)?;
        let bytes = read(path)?;
        let bank = if path.ends_with(".ktx2") {
            resonance_asset_writer::texture::dimensions(bytes.as_ref())?;
            Self {
                textures: vec![Some(TextureImage {
                    sampler: Self::default_sampler(),
                    images: vec![path.into()],
                })],
            }
        } else {
            ensure!(
                path.ends_with(".json"),
                "image bank must be KTX2 or JSON: {path}"
            );
            serde_json::from_slice(bytes.as_ref())?
        };
        bank.validate()?;
        Ok(bank)
    }

    pub fn default_sampler() -> crate::texture::Sampler {
        crate::texture::Sampler {
            wrap: [crate::TextureWrap::Clamp; 2],
            ..Default::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        for texture in self.textures.iter().flatten() {
            ensure!(!texture.images.is_empty(), "empty physical texture");
            texture.sampler.validate()?;
            for path in &texture.images {
                crate::library::validate_path(path)?;
            }
        }
        Ok(())
    }

    pub fn references(&self) -> impl Iterator<Item = &str> {
        self.textures
            .iter()
            .flatten()
            .flat_map(|texture| texture.images.iter().map(String::as_str))
    }
}

/// Follow current physical-scene references, including edited local palettes.
/// Caller texture tables are symbols, so they must never be opened as filenames.
pub fn capture(
    snapshot: &mut crate::library::Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    use crate::preparation::{ASSET_LIMIT, JSON_LIMIT};
    ensure!(!cancelled(), "physical scene preparation cancelled");
    let mut scene: Scene = snapshot.json(path, JSON_LIMIT)?;
    let mesh = snapshot
        .read(&scene.mesh, ASSET_LIMIT)
        .with_context(|| format!("{path} mesh {}", scene.mesh))?;
    scene
        .prepare_bones(&mesh)
        .with_context(|| path.to_owned())?;
    scene.validate().with_context(|| path.to_owned())?;
    if let TextureSource::Local(catalogue) = &scene.textures {
        for image in catalogue.references() {
            ensure!(!cancelled(), "physical scene preparation cancelled");
            snapshot
                .read(image, ASSET_LIMIT)
                .with_context(|| format!("{path} image {image}"))?;
        }
    }
    ensure!(!cancelled(), "physical scene preparation cancelled");
    Ok(())
}
