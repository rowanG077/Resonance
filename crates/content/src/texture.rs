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
