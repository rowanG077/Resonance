//! Physical geometry and texture recipes retained independently of renderer
//! support. Paths name owned files; material table labels are authored symbols.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// For a nonzero stage count, start with vertex color and apply the operations
/// in order, clamping to [0, 1]. Texture indices identify authored UV/sampler slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialRecipe {
    pub context: MaterialContext,
    pub source_mode: u32,
    pub source_texture_count: u8,
    /// Requested stage count, including pass-through stages. Zero requires prior
    /// graphics state and has no standalone ordinary-material projection.
    pub stage_count: u8,
    pub operations: Vec<MaterialOperation>,
}

/// Ordinary material pass. Lighting overlays and explicit render-mode overrides
/// require separate recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialContext {
    DefaultGplOrdinary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterialOperation {
    MultiplyTexture {
        texture: u8,
    },
    /// Interpolate RGB toward the texture using its alpha; preserve previous alpha.
    DecalTexture {
        texture: u8,
    },
    /// Interpolate RGB toward white using texture RGB; multiply alpha by texture alpha.
    BlendTexture {
        texture: u8,
    },
    ReplaceTexture {
        texture: u8,
    },
    /// Add texture RGB, clamp to [0, 1], and preserve previous alpha.
    AddTexture {
        texture: u8,
    },
    /// Subtract texture RGB, clamp to [0, 1], and preserve previous alpha.
    SubtractTexture {
        texture: u8,
    },
    /// Requires the draw matrix for its UV transform, prior alpha to weight the
    /// added texture RGB, and the caller's constant alpha for its output alpha.
    DefaultViewTexture {
        texture: u8,
    },
    MultiplyVertexColor,
}

impl MaterialRecipe {
    pub fn textures(&self) -> impl Iterator<Item = u8> + '_ {
        self.operations
            .iter()
            .filter_map(|operation| match operation {
                MaterialOperation::MultiplyTexture { texture }
                | MaterialOperation::DecalTexture { texture }
                | MaterialOperation::BlendTexture { texture }
                | MaterialOperation::ReplaceTexture { texture }
                | MaterialOperation::AddTexture { texture }
                | MaterialOperation::SubtractTexture { texture }
                | MaterialOperation::DefaultViewTexture { texture } => Some(*texture),
                MaterialOperation::MultiplyVertexColor => None,
            })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub mesh: String,
    pub bone_names: Vec<String>,
    pub textures: TextureSource,
    pub draws: Vec<Draw>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextureSource {
    Local { catalogue: String },
    Caller,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Draw {
    pub source_index: usize,
    /// Empty authored draws retain their metadata and hierarchy without a mesh.
    pub mesh: Option<usize>,
    pub model_node: Option<usize>,
    pub draw_order: u32,
    pub name: String,
    pub index_count: usize,
    pub color: [f32; 4],
    /// Exporter alpha classification, not an authored blend-state command.
    /// Runtime binding must recalculate alpha when caller textures become known.
    pub preview_blend: bool,
    pub recipe: MaterialRecipe,
    pub textures: Vec<Texture>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Texture {
    pub stage: u8,
    /// Authored table label, never a filesystem path.
    pub table: Option<String>,
    pub image: u16,
    pub wrap: [crate::TextureWrap; 2],
    pub min_filter: crate::texture::Filter,
    pub mag_filter: crate::texture::Filter,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureFormat {
    I4,
    I8,
    Ia4,
    Ia8,
    Rgb565,
    Rgb5a3,
    Rgba8,
    Ci4,
    Ci8,
    Ci14,
    Cmpr,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteFormat {
    Ia8,
    Rgb565,
    Rgb5a3,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Palette {
    pub format: PaletteFormat,
    pub colors: Vec<[u8; 4]>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TextureImage {
    pub dimensions: [u16; 2],
    pub format: TextureFormat,
    pub sampler: crate::texture::Sampler,
    pub palette: Option<Palette>,
    pub images: Vec<String>,
}
impl TextureImage {
    pub fn image(&self, palette: usize) -> Result<crate::font::UiTexture> {
        Ok(crate::font::UiTexture {
            path: self
                .images
                .get(palette)
                .context("missing cooked texture palette")?
                .clone(),
            width: u32::from(self.dimensions[0]),
            height: u32::from(self.dimensions[1]),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TextureCatalogue {
    pub textures: Vec<Option<TextureImage>>,
}

impl Scene {
    /// This validates the physical recipe, without claiming the current renderer
    /// can project its operations or inventing textures supplied by a caller.
    pub fn validate(&self) -> Result<()> {
        crate::library::validate_path(&self.mesh)?;
        if let TextureSource::Local { catalogue } = &self.textures {
            crate::library::validate_path(catalogue)?;
        }
        for draw in &self.draws {
            ensure!(
                (draw.index_count == 0) == draw.mesh.is_none()
                    && draw
                        .model_node
                        .is_none_or(|node| node < self.bone_names.len())
                    && draw.color.iter().all(|value| value.is_finite()),
                "invalid physical draw {}",
                draw.name
            );
            // Native recipes can require more stages than their source texture
            // count. Preserve that requirement, including zero-stage recipes.
            ensure!(
                draw.recipe.source_texture_count <= 8
                    && draw.recipe.stage_count <= 16
                    && draw.recipe.textures().all(|texture| texture < 16),
                "invalid material recipe in {}",
                draw.name
            );
            let mut stages = std::collections::BTreeSet::new();
            for texture in &draw.textures {
                ensure!(
                    texture.stage < 8 && stages.insert(texture.stage),
                    "duplicate or invalid texture stage in {}",
                    draw.name
                );
                ensure!(
                    matches!(
                        texture.mag_filter,
                        crate::texture::Filter::Nearest | crate::texture::Filter::Linear
                    ),
                    "invalid magnification filter in {}",
                    draw.name
                );
            }
        }
        Ok(())
    }
}

impl TextureCatalogue {
    pub fn validate(&self) -> Result<()> {
        for texture in self.textures.iter().flatten() {
            ensure!(
                texture.dimensions.iter().all(|&size| size > 0) && !texture.images.is_empty(),
                "empty physical texture"
            );
            texture.sampler.validate()?;
            if let Some(palette) = &texture.palette {
                ensure!(
                    !palette.colors.is_empty() && palette.colors.len() <= 16384,
                    "invalid physical texture palette"
                );
            }
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
    let scene: Scene = snapshot.json(path, JSON_LIMIT)?;
    scene.validate().with_context(|| path.to_owned())?;
    snapshot
        .read(&scene.mesh, ASSET_LIMIT)
        .with_context(|| format!("{path} mesh {}", scene.mesh))?;
    if let TextureSource::Local { catalogue } = &scene.textures {
        let textures: TextureCatalogue = snapshot
            .json(catalogue, JSON_LIMIT)
            .with_context(|| format!("{path} textures {catalogue}"))?;
        textures.validate().with_context(|| catalogue.clone())?;
        for image in textures.references() {
            ensure!(!cancelled(), "physical scene preparation cancelled");
            snapshot
                .read(image, ASSET_LIMIT)
                .with_context(|| format!("{catalogue} image {image} (scene {path})"))?;
        }
    }
    ensure!(!cancelled(), "physical scene preparation cancelled");
    Ok(())
}
