//! Camera-facing sprite content.
pub const FIELD_PALETTE_COLORS: usize = 110;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod frame_cycle;
/// Shared field artwork, addressed by scenario sprite IDs.
pub mod sprite;

/// Eye atlas frames at the fixed update rate, including the open-eye rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlinkCycle {
    #[serde(with = "frame_cycle")]
    pub frames: Vec<u8>,
}
impl BlinkCycle {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.frames.is_empty()
                && self.frames.len() <= 1024
                && self.frames.iter().all(|frame| *frame < 16),
            "invalid eye blink animation"
        );
        Ok(())
    }
}

/// Image banks share one editable representation with model textures.
pub type OverlayArt = crate::physical_scene::TextureCatalogue;

/// A textured leaf tumbling in world space, with wind expressed per gameplay tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlutterRecipe<Image = String> {
    pub texture: Image,
    pub uv: [f32; 4],
    pub aspect_ratio: f32,
    /// Omission in a field document uses its shared settings palette.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<[u8; 4]>,
    pub fall_speed: f32,
    pub fall_variation: f32,
    pub spin: f32,
}
impl<Image: AsRef<str>> FlutterRecipe<Image> {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(self.texture.as_ref())?;
        ensure!(
            self.uv
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                && self.uv[0] < self.uv[2]
                && self.uv[1] < self.uv[3]
                && self.aspect_ratio.is_finite()
                && self.aspect_ratio > 0.
                && (1..=256).contains(&self.palette.len())
                && [self.fall_speed, self.fall_variation, self.spin]
                    .iter()
                    .all(|v| v.is_finite()),
            "invalid fluttering particle recipe"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldEffects<Image = String> {
    pub emote_texture: Image,
    pub status_texture: Image,
    #[serde(with = "sprite::named")]
    pub sprites: BTreeMap<u16, SpriteRecipe<Image>>,
    #[serde(default)]
    pub palette: Vec<[u8; 4]>,
    pub refraction: RefractionRecipe<Image>,
    pub air_refraction: SpriteRecipe<Image>,
    #[serde(with = "frame_cycle")]
    pub mouth_cycle: Vec<u8>,
    pub rising_light_destination: [f32; 3],
}
pub const SMOKE_UPDATES: u32 = 56;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteFrame {
    pub uv: [f32; 4],
    pub ticks: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteRecipe<Image = String> {
    pub texture: Image,
    pub uv: [f32; 4],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub additive: bool,
    /// Frame durations include the native timer-zero pose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<SpriteFrame>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repeat: bool,
}
impl<Image: AsRef<str>> SpriteRecipe<Image> {
    pub fn uv_at(&self, mut age: u32) -> [f32; 4] {
        if self.repeat {
            let period = self.frames.iter().map(|f| u32::from(f.ticks)).sum::<u32>();
            if period > 0 {
                age %= period;
            }
        }
        for frame in &self.frames {
            if age < u32::from(frame.ticks) {
                return frame.uv;
            }
            age -= u32::from(frame.ticks);
        }
        self.frames.last().map_or(self.uv, |frame| frame.uv)
    }

    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(self.texture.as_ref())?;
        ensure!(
            self.frames.len() <= 126
                && self.frames.iter().all(|frame| {
                    frame.ticks > 0
                        && frame.uv[0] < frame.uv[2]
                        && frame.uv[1] < frame.uv[3]
                        && frame
                            .uv
                            .iter()
                            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                }),
            "invalid sprite animation"
        );
        ensure!(
            self.uv
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                && self.uv[0] < self.uv[2]
                && self.uv[1] < self.uv[3],
            "invalid effect UVs"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefractionRecipe<Image = String> {
    pub sprite: SpriteRecipe<Image>,
    /// Signed displacements in authored scene texels.
    pub displacement: [f32; 2],
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAnchor {
    #[default]
    Center,
    Bottom,
    Top,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sprite {
    pub offset: [f32; 3],
    pub size: [f32; 2],
    pub uv: [f32; 4],
    pub rotation: f32,
    #[serde(default)]
    pub vertical_anchor: VerticalAnchor,
    #[serde(default = "opaque")]
    pub alpha: u8,
}
fn opaque() -> u8 {
    255
}
impl<Image: AsRef<str>> FieldEffects<Image> {
    pub fn references(&self) -> impl Iterator<Item = &str> {
        [self.emote_texture.as_ref(), self.status_texture.as_ref()]
            .into_iter()
            .chain(
                self.sprites
                    .values()
                    .chain([&self.refraction.sprite, &self.air_refraction])
                    .map(|sprite| sprite.texture.as_ref()),
            )
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.sprites.len() <= 256
                && sprite::all().all(|id| self.sprites.contains_key(&id))
                && self.palette.len() == FIELD_PALETTE_COLORS,
            "invalid field effects"
        );
        ensure!(
            self.rising_light_destination.iter().all(|v| v.is_finite()),
            "invalid light destination"
        );
        crate::validate_asset_path(self.emote_texture.as_ref())?;
        crate::validate_asset_path(self.status_texture.as_ref())?;
        for sprite in self
            .sprites
            .values()
            .chain([&self.refraction.sprite, &self.air_refraction])
        {
            sprite.validate()?;
        }
        ensure!(
            self.refraction
                .displacement
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 256.),
            "invalid refraction displacement"
        );
        ensure!(
            !self.mouth_cycle.is_empty()
                && self.mouth_cycle.len() <= 1024
                && self.mouth_cycle.iter().all(|f| *f < 8),
            "invalid mouth animation"
        );
        Ok(())
    }
}
