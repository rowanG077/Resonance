//! Sampling metadata shared by physical textures and prepared artwork.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TextureLod {
    pub bias: f32,
    pub min: u8,
    pub max: u8,
    pub edge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sampler {
    pub wrap: [crate::TextureWrap; 2],
    pub min_filter: Filter,
    pub mag_filter: Filter,
    pub lod: TextureLod,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapLinear,
}

impl Sampler {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.mag_filter, Filter::Nearest | Filter::Linear)
                && self.lod.bias.is_finite()
                && self.lod.min <= self.lod.max,
            "invalid texture sampler"
        );
        Ok(())
    }
}

/// Ordinary PNGs are sufficient for one linear level. This descriptor adds only
/// color interpretation or a mip policy; dimensions always come from the PNGs.
pub const VERSION: u32 = 1;
pub const PNG_LIMIT: usize = 64 * 1024 * 1024;
pub const TEXTURE_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[default]
    Linear,
    Srgb,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mips {
    #[default]
    None,
    Authored {
        levels: Vec<String>,
    },
    Generate,
}
impl Mips {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub image: String,
    #[serde(default)]
    pub color_space: ColorSpace,
    #[serde(default, skip_serializing_if = "Mips::is_none")]
    pub mips: Mips,
}
impl Definition {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION,
            "unsupported texture definition version"
        );
        if let Mips::Authored { levels } = &self.mips {
            ensure!(
                !levels.is_empty() && levels.len() <= 14,
                "invalid authored texture mip count"
            );
        }
        for path in self.images() {
            crate::library::validate_path(path)?;
            ensure!(
                path.ends_with(".png"),
                "texture levels must be PNG files: {path}"
            );
        }
        Ok(())
    }
    pub fn images(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.image.as_str()).chain(
            match &self.mips {
                Mips::Authored { levels } => levels.as_slice(),
                _ => &[],
            }
            .iter()
            .map(String::as_str),
        )
    }

    /// Prepare actual pixels from the selected resource files. Authored levels
    /// remain unchanged; generation is explicitly requested by the author.
    pub fn decode<B: AsRef<[u8]>>(
        &self,
        mut read: impl FnMut(&str) -> Result<B>,
    ) -> Result<Vec<resonance_asset_writer::texture::Pixels>> {
        use anyhow::Context;
        use resonance_asset_writer::texture;
        self.validate()?;
        let mut encoded = 0usize;
        let mut decoded = 0usize;
        let mut levels = Vec::new();
        for path in self.images() {
            let bytes = read(path)?;
            let bytes = bytes.as_ref();
            encoded = encoded
                .checked_add(bytes.len())
                .context("texture byte count overflow")?;
            ensure!(
                bytes.len() <= PNG_LIMIT && encoded <= TEXTURE_LIMIT,
                "texture exceeds its read budget: {path}"
            );
            let pixels = texture::png(bytes).with_context(|| format!("texture image {path}"))?;
            decoded += pixels.rgba.len();
            ensure!(
                decoded <= TEXTURE_LIMIT,
                "texture exceeds decoded budget: {path}"
            );
            levels.push(pixels);
        }
        if matches!(self.mips, Mips::Generate) {
            levels = texture::generate(levels.remove(0), self.color_space == ColorSpace::Srgb)?;
        }
        texture::validate_levels(&levels)?;
        Ok(levels)
    }
}

/// Called by the current snapshot for this explicit resource extension. The
/// dependency set comes from authoritative texture settings, not an inventory.
pub(crate) fn capture(
    snapshot: &mut crate::library::Snapshot,
    path: &str,
    bytes: &[u8],
) -> Result<()> {
    use anyhow::Context;
    let definition: Definition =
        serde_json::from_slice(bytes).with_context(|| format!("texture definition {path}"))?;
    definition.validate().with_context(|| path.to_owned())?;
    let mut total = 0usize;
    for image in definition.images() {
        let bytes = snapshot.read(image, PNG_LIMIT)?;
        total = total
            .checked_add(bytes.len())
            .context("texture byte count overflow")?;
        ensure!(
            total <= TEXTURE_LIMIT,
            "texture exceeds its complete read budget: {path}"
        );
    }
    Ok(())
}
