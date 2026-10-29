//! Sampling metadata shared by physical textures and prepared artwork.
pub(crate) mod images;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TextureLod {
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub bias: f32,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub min: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub max: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub edge: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sampler {
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub wrap: [crate::TextureWrap; 2],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub min_filter: Filter,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub mag_filter: Filter,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub lod: TextureLod,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Nearest,
    #[default]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_defaults_and_exceptions_preserve_filtering_and_lod() -> Result<()> {
        let default: Sampler = serde_json::from_str("{}")?;
        assert_eq!(default.wrap, [crate::TextureWrap::Repeat; 2]);
        assert_eq!(default.min_filter, Filter::Linear);
        assert_eq!(default.mag_filter, Filter::Linear);
        assert_eq!(default.lod, TextureLod::default());
        assert_eq!(serde_json::to_value(&default)?, serde_json::json!({}));
        let value = serde_json::json!({"wrap":["mirror","clamp"],
            "min_filter":"linear_mipmap_linear","mag_filter":"nearest",
            "lod":{"bias":-0.5,"min":1,"max":5,"edge":true}});
        let selected: Sampler = serde_json::from_value(value.clone())?;
        selected.validate()?;
        assert_eq!(serde_json::to_value(&selected)?, value);
        let mut invalid = selected;
        invalid.lod.max = 0;
        assert!(invalid.validate().is_err());
        Ok(())
    }

    // Installed palette and draw coverage lives in physical_scene::tests,
    // alongside the grouped material reader. Historical JSON pointers no longer
    // describe the editable files.
}

/// Color interpretation carried by a KTX2 data format descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[default]
    Linear,
    Srgb,
}
