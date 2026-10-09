//! Camera-facing sprite content.
pub const FIELD_PALETTE_COLORS: usize = 110;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shared field artwork, addressed by scenario sprite IDs.
pub mod sprite {
    pub const GLOW_SPRITE: u16 = 0;
    pub const SMOKE_SPRITE: u16 = 1;
    pub const STATION_GLOW_SPRITE: u16 = 4;
    pub const CAMERA_DISC_SPRITE: u16 = 5;
    pub const WORLD_GLOW_SPRITE: u16 = 6;
    pub const STAR_SPRITE: u16 = 7;
    pub const SPINNING_STAR_SPRITE: u16 = 8;
    pub const ORB_SPRITE: u16 = 10;
    pub const FLAME_SPRITE: u16 = 11;
    pub const TRAIL_GLOW_SPRITE: u16 = 12;
    pub const BURST_SPRITE: u16 = 13;
    pub const ELECTRIC_ARC_SPRITE: u16 = 14;
    pub const SWIRL_SPRITE: u16 = 18;
    pub const SPARKLE_CLUSTER_SPRITE: u16 = 21;
    pub const STATION_HALO_SPRITE: u16 = 22;
    pub const STREAK_SPRITE: u16 = 23;
    pub const RING_SPRITE: u16 = 41;
    pub const ELECTRIC_SPARK_SPRITE: u16 = 42;
    pub const RISING_LIGHT_SPRITE: u16 = 44;
    pub const DEBRIS_SPRITES: [u16; 3] = [52, 53, 54];
    pub const SEAL_STAR_SPRITE: u16 = 68;
    pub const SEAL_SPARK_SPRITE: u16 = 69;
    pub const SQUARE_GLOW_SPRITE: u16 = 70;
    pub const LIGHTNING_BOLT_SPRITE: u16 = 74;
    pub const PLANE_LIGHT_SPRITES: [u16; 4] = [75, 76, 77, 78];
    pub const FLAME_PLUME_SPRITE: u16 = 80;
    pub const COOKING_CLOUDS: std::ops::RangeInclusive<u16> = 501..=505;

    pub const ALL: [u16; 31] = [
        GLOW_SPRITE,
        SMOKE_SPRITE,
        STATION_GLOW_SPRITE,
        CAMERA_DISC_SPRITE,
        WORLD_GLOW_SPRITE,
        STAR_SPRITE,
        SPINNING_STAR_SPRITE,
        ORB_SPRITE,
        FLAME_SPRITE,
        TRAIL_GLOW_SPRITE,
        BURST_SPRITE,
        ELECTRIC_ARC_SPRITE,
        SWIRL_SPRITE,
        SPARKLE_CLUSTER_SPRITE,
        STATION_HALO_SPRITE,
        STREAK_SPRITE,
        RING_SPRITE,
        ELECTRIC_SPARK_SPRITE,
        RISING_LIGHT_SPRITE,
        DEBRIS_SPRITES[0],
        DEBRIS_SPRITES[1],
        DEBRIS_SPRITES[2],
        SEAL_STAR_SPRITE,
        SEAL_SPARK_SPRITE,
        SQUARE_GLOW_SPRITE,
        LIGHTNING_BOLT_SPRITE,
        PLANE_LIGHT_SPRITES[0],
        PLANE_LIGHT_SPRITES[1],
        PLANE_LIGHT_SPRITES[2],
        PLANE_LIGHT_SPRITES[3],
        FLAME_PLUME_SPRITE,
    ];
}

/// Eye atlas frames at the fixed update rate, including the open-eye rest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlinkCycle {
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

/// All images in one script texture resource, in authored index order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayArt {
    pub textures: Vec<OverlayTexture>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayTexture {
    /// Palette variants share dimensions and sampling. Ordinary overlays use
    /// the first palette; keeping the complete bank avoids losing authored data.
    pub images: Vec<crate::font::UiTexture>,
    pub sampler: crate::texture::Sampler,
}

impl OverlayArt {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.textures.is_empty(), "empty overlay texture bank");
        for texture in &self.textures {
            ensure!(!texture.images.is_empty(), "empty overlay palette bank");
            texture.sampler.validate()?;
            let first = &texture.images[0];
            for image in &texture.images {
                crate::validate_asset_path(&image.path)?;
                ensure!(
                    (1..=4096).contains(&image.width)
                        && (1..=4096).contains(&image.height)
                        && (image.width, image.height) == (first.width, first.height),
                    "invalid overlay image dimensions"
                );
            }
        }
        Ok(())
    }
}

/// A textured leaf tumbling in world space, with wind expressed per gameplay tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlutterRecipe<Image = String> {
    pub texture: Image,
    pub uv: [f32; 4],
    pub aspect_ratio: f32,
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
    pub version: u32,
    pub emote_texture: Image,
    pub status_texture: Image,
    pub sprites: BTreeMap<u16, SpriteRecipe<Image>>,
    #[serde(default)]
    pub palette: Vec<[u8; 4]>,
    pub refraction: RefractionRecipe<Image>,
    pub air_refraction: SpriteRecipe<Image>,
    pub mouth_cycle: Vec<u8>,
    pub rising_light_destination: [f32; 3],
}
pub const FIELD_EFFECTS_VERSION: u32 = 10;
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
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == FIELD_EFFECTS_VERSION
                && self.sprites.len() <= 256
                && sprite::ALL.iter().all(|id| self.sprites.contains_key(id))
                && self.palette.len() == FIELD_PALETTE_COLORS,
            "invalid or outdated field effects; run cook-all"
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
