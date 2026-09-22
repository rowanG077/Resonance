//! Cooked camera-facing sprites; no original draw commands at runtime.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Eye atlas frames at the fixed update rate, including the open-eye rest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlinkCycle {
    pub frames: Vec<u8>,
    pub initial_tick: u16,
    pub initial_spread: u16,
}
impl BlinkCycle {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.frames.is_empty()
                && self.frames.len() <= 1024
                && self.frames.iter().all(|frame| *frame < 16)
                && self.initial_spread > 0
                && usize::from(self.initial_tick) + usize::from(self.initial_spread)
                    <= self.frames.len(),
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
    pub paralysis: EmoteTrack,
    pub sprites: BTreeMap<u16, SpriteRecipe<Image>>,
    pub refraction: RefractionRecipe<Image>,
    pub emotes: BTreeMap<u16, EmoteTrack>,
    pub mouth_cycle: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteRecipe<Image = String> {
    pub texture: Image,
    pub uv: [f32; 4],
    pub additive: bool,
}
impl<Image: AsRef<str>> SpriteRecipe<Image> {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(self.texture.as_ref())?;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmoteTrack {
    pub anchor: String,
    /// Added to logical actor position when the named model node is absent.
    /// Sprite offsets already include their ordinary height above the anchor.
    pub missing_anchor_offset: [f32; 3],
    pub rotation: EmoteRotation,
    /// Frames are interleaved by the controller's initial random phase.
    #[serde(default = "single_phase")]
    pub phase_count: u8,
    pub intro: Vec<Vec<Sprite>>,
    pub cycle: Vec<Vec<Sprite>>,
}

/// Rotation can follow the shared effect clock independently of a sprite's age.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "clock", rename_all = "snake_case")]
pub enum EmoteRotation {
    Fixed,
    GlobalTick { degrees_per_tick: u16 },
}
impl EmoteRotation {
    pub fn angle(self, tick: u32) -> f32 {
        match self {
            Self::Fixed => 0.,
            Self::GlobalTick { degrees_per_tick } => {
                // The authored angle is a signed 16-bit degree value.
                tick.wrapping_mul(u32::from(degrees_per_tick)) as i16 as f32
            }
        }
    }
}
fn single_phase() -> u8 {
    1
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
            self.version == 5
                && self.emotes.len() <= 256
                && self.sprites.len() <= 256
                && self.sprites.contains_key(&10),
            "invalid or outdated field effects; run cook-all"
        );
        crate::validate_asset_path(self.emote_texture.as_ref())?;
        crate::validate_asset_path(self.status_texture.as_ref())?;
        ensure!(
            self.paralysis.intro.is_empty()
                && self.paralysis.cycle.len() == 2
                && self.paralysis.cycle.iter().all(|frame| frame.len() == 1),
            "paralysis requires two visible symbol poses"
        );
        for sprite in self.sprites.values().chain([&self.refraction.sprite]) {
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
        for track in self.emotes.values().chain([&self.paralysis]) {
            ensure!(
                !track.anchor.is_empty()
                    && track.missing_anchor_offset.iter().all(|v| v.is_finite())
                    && (1..=32).contains(&track.phase_count)
                    && !track.cycle.is_empty()
                    && track
                        .intro
                        .len()
                        .is_multiple_of(usize::from(track.phase_count))
                    && track
                        .cycle
                        .len()
                        .is_multiple_of(usize::from(track.phase_count))
                    && track.intro.len() + track.cycle.len() <= 4096,
                "invalid emote track"
            );
            for frame in track.intro.iter().chain(&track.cycle) {
                ensure!(frame.len() <= 64, "emote frame exceeds sprite limit");
                for sprite in frame {
                    ensure!(
                        sprite
                            .offset
                            .iter()
                            .chain(&sprite.size)
                            .chain(&sprite.uv)
                            .all(|v| v.is_finite())
                            && sprite.rotation.is_finite(),
                        "nonfinite emote sprite"
                    );
                }
            }
        }
        Ok(())
    }
}
impl EmoteTrack {
    pub fn frame_with_phase(&self, age: usize, phase: u8) -> &[Sprite] {
        let phases = usize::from(self.phase_count);
        let phase = usize::from(phase) % phases;
        let intro = self.intro.len() / phases;
        if age < intro {
            &self.intro[age * phases + phase]
        } else {
            &self.cycle[((age - intro) % (self.cycle.len() / phases)) * phases + phase]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emote_phase_variants_preserve_age_and_sprite_defaults() {
        let data = serde_json::json!({
            "anchor":"head", "missing_anchor_offset":[0.,0.,128.],
            "rotation":{"clock":"fixed"}, "intro":[[]], "cycle":[[{
                "offset":[0.,0.,0.], "size":[3.,3.], "uv":[0.,0.,1.,1.], "rotation":0.
            }]]
        });
        let mut track: EmoteTrack = serde_json::from_value(data).unwrap();
        assert!(track.frame_with_phase(0, 0).is_empty());
        assert_eq!(track.frame_with_phase(1, 31)[0].alpha, 255);
        assert!(matches!(
            track.frame_with_phase(1, 0)[0].vertical_anchor,
            VerticalAnchor::Center
        ));

        let mut alternate = track.cycle[0].clone();
        alternate[0].alpha = 30;
        track.phase_count = 2;
        track.intro = vec![Vec::new(); 2];
        track
            .cycle
            .extend([alternate.clone(), alternate, track.cycle[0].clone()]);
        assert!(track.frame_with_phase(0, 1).is_empty());
        assert_eq!(track.frame_with_phase(1, 0)[0].alpha, 255);
        assert_eq!(track.frame_with_phase(1, 31)[0].alpha, 30);
        assert_eq!(track.frame_with_phase(2, 0)[0].alpha, 30);
        assert_eq!(track.frame_with_phase(3, 0)[0].alpha, 255);
    }

    #[test]
    fn global_emote_rotation_preserves_signed_angle_wrap() {
        let rotation = EmoteRotation::GlobalTick {
            degrees_per_tick: 4,
        };
        for (tick, angle) in [
            (0, 0.),
            (90, 360.),
            (8191, 32764.),
            (8192, -32768.),
            (16384, 0.),
            (u32::MAX, -4.),
        ] {
            assert_eq!(rotation.angle(tick), angle);
            assert_eq!(EmoteRotation::Fixed.angle(tick), 0.);
        }
    }
}
