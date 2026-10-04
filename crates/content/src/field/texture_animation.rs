//! UV motion with an explicit scene or effect clock.
use super::RenderValue;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTextureAnimation {
    pub actor: RenderValue,
    pub texture: RenderValue,
    pub clock: TextureClock,
    pub motion: TextureMotion,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureClock {
    Field,
    Effect,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextureMotion {
    Scroll {
        velocity: [f32; 2],
        vertical_wave: Option<FieldTextureWave>,
    },
    Atlas {
        frames: u32,
        interval: u32,
        step: [f32; 2],
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTextureWave {
    pub degrees_per_tick: f32,
    pub amplitude: f32,
}
impl FieldTextureAnimation {
    pub fn validate(&self) -> Result<()> {
        for target in [&self.actor, &self.texture] {
            ensure!(
                !matches!(target, RenderValue::Setting(slot) | RenderValue::SettingOffset { slot, .. } if *slot > 7),
                "invalid texture target slot"
            );
        }
        match &self.motion {
            TextureMotion::Scroll {
                velocity,
                vertical_wave,
            } => {
                ensure!(
                    velocity.iter().all(|v| v.is_finite()),
                    "invalid texture velocity"
                );
                if let Some(wave) = vertical_wave {
                    ensure!(
                        wave.degrees_per_tick.is_finite() && wave.amplitude.is_finite(),
                        "invalid texture wave"
                    );
                }
            }
            TextureMotion::Atlas {
                frames,
                interval,
                step,
            } => ensure!(
                *frames > 0 && *interval > 0 && step.iter().all(|v| v.is_finite()),
                "invalid texture atlas"
            ),
        }
        Ok(())
    }
    pub fn offset(&self, field_tick: u64, effect_tick: u32) -> [f32; 2] {
        let tick = match self.clock {
            TextureClock::Field => field_tick,
            TextureClock::Effect => u64::from(effect_tick),
        };
        match &self.motion {
            TextureMotion::Scroll {
                velocity,
                vertical_wave,
            } => {
                let mut offset = velocity.map(|v| v * tick as f32);
                if let Some(wave) = vertical_wave {
                    offset[1] +=
                        (tick as f32 * wave.degrees_per_tick).to_radians().sin() * wave.amplitude;
                }
                offset
            }
            TextureMotion::Atlas {
                frames,
                interval,
                step,
            } => step.map(|v| v * ((tick / u64::from(*interval)) % u64::from(*frames)) as f32),
        }
    }
}
