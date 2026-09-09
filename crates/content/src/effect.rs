//! Cooked camera-facing sprites; no original draw commands at runtime.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldEffects {
    pub version: u32,
    pub emote_texture: String,
    pub dust_texture: String,
    pub dust_uv: [f32; 4],
    pub emotes: BTreeMap<u16, EmoteTrack>,
    pub mouth_cycle: Vec<u8>,
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
            self.version == 1 && self.emotes.len() <= 256,
            "invalid field effects"
        );
        crate::validate_asset_path(&self.emote_texture)?;
        crate::validate_asset_path(&self.dust_texture)?;
        ensure!(
            !self.mouth_cycle.is_empty()
                && self.mouth_cycle.len() <= 1024
                && self.mouth_cycle.iter().all(|f| *f < 8),
            "invalid mouth animation"
        );
        ensure!(
            self.dust_uv
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "invalid dust UVs"
        );
        for track in self.emotes.values() {
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
