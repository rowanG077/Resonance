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

/// Screen-space location lettering, baked once into an opening sprite track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationCaption {
    pub textures: Vec<crate::font::UiTexture>,
    pub frames: Vec<Vec<CaptionSprite>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionSprite {
    pub texture: usize,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub alpha: u8,
}
impl LocationCaption {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (4..=13).contains(&self.textures.len()) && (2..=512).contains(&self.frames.len()),
            "invalid location caption track"
        );
        for texture in &self.textures {
            crate::validate_asset_path(&texture.path)?;
            ensure!(
                [texture.width, texture.height]
                    .iter()
                    .all(|v| (1..=4096).contains(v)),
                "invalid location caption texture size"
            );
        }
        for frame in &self.frames {
            ensure!(frame.len() <= 16, "location caption exceeds sprite limit");
            for sprite in frame {
                ensure!(
                    sprite.texture < self.textures.len()
                        && sprite.rect.iter().all(|v| v.is_finite())
                        && sprite.rect[0] <= sprite.rect[2]
                        && sprite.rect[1] <= sprite.rect[3]
                        && sprite
                            .uv
                            .iter()
                            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                        && sprite.uv[0] < sprite.uv[2]
                        && sprite.uv[1] < sprite.uv[3],
                    "invalid location caption sprite"
                );
            }
        }
        Ok(())
    }
    pub fn frame(&self, age: usize) -> &[CaptionSprite] {
        &self.frames[age.min(self.frames.len() - 1)]
    }
}

/// A textured leaf tumbling in world space, with wind expressed per gameplay tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlutterRecipe {
    pub texture: String,
    pub uv: [f32; 4],
    pub aspect_ratio: f32,
    pub palette: Vec<[u8; 4]>,
    pub fall_speed: f32,
    pub fall_variation: f32,
    pub spin: f32,
}
impl FlutterRecipe {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(&self.texture)?;
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
pub struct FieldEffects {
    pub version: u32,
    pub emote_texture: String,
    pub status_texture: String,
    pub paralysis: EmoteTrack,
    pub sprites: BTreeMap<u16, SpriteRecipe>,
    pub refraction: RefractionRecipe,
    pub emotes: BTreeMap<u16, EmoteTrack>,
    pub mouth_cycle: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteRecipe {
    pub texture: String,
    pub uv: [f32; 4],
    pub additive: bool,
}
impl SpriteRecipe {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(&self.texture)?;
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
pub struct RefractionRecipe {
    pub sprite: SpriteRecipe,
    /// Signed displacements in authored scene texels.
    pub displacement: [f32; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmoteTrack {
    pub anchor: String,
    pub intro: Vec<Vec<Sprite>>,
    pub cycle: Vec<Vec<Sprite>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sprite {
    pub offset: [f32; 3],
    pub size: [f32; 2],
    pub uv: [f32; 4],
    pub rotation: f32,
}
impl FieldEffects {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 3
                && self.emotes.len() <= 256
                && self.sprites.len() <= 256
                && self.sprites.contains_key(&10),
            "invalid or outdated field effects; run cook-effects"
        );
        crate::validate_asset_path(&self.emote_texture)?;
        crate::validate_asset_path(&self.status_texture)?;
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
                    && !track.cycle.is_empty()
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
    pub fn frame(&self, age: usize) -> &[Sprite] {
        if age < self.intro.len() {
            &self.intro[age]
        } else {
            &self.cycle[(age - self.intro.len()) % self.cycle.len()]
        }
    }
}
