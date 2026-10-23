//! Automatic field atmosphere and scenery lighting, independent of scenario emitters.
use super::RenderValue;
use crate::effect::SpriteRecipe;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldEnvironment {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<AmbientParticles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind: Option<FieldWind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat_haze: Option<HeatHaze>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lights: Option<SceneryLighting>,
}
impl FieldEnvironment {
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
            && self.wind.is_none()
            && self.heat_haze.is_none()
            && self.lights.is_none()
    }
    pub fn textures(&self) -> impl Iterator<Item = &String> {
        self.particles
            .iter()
            .map(|p| &p.sprite.texture)
            .chain(self.heat_haze.iter().map(|h| &h.sprite.texture))
    }
    pub fn validate(&self) -> Result<()> {
        for particle in &self.particles {
            particle.gate.validate()?;
            particle.sprite.validate()?;
            match &particle.emission {
                AmbientEmission::Drifting {
                    interval,
                    lifetime,
                    position,
                    position_step,
                    position_steps,
                    velocity,
                    half_size,
                    ..
                } => ensure!(
                    *interval > 0
                        && *lifetime > 0
                        && position_steps.iter().all(|n| *n > 0)
                        && position
                            .iter()
                            .chain(position_step)
                            .chain(velocity)
                            .chain(half_size)
                            .all(|v| v.is_finite())
                        && half_size.iter().all(|v| *v > 0.),
                    "invalid drifting field particles"
                ),
                AmbientEmission::Fire { position, size } => ensure!(
                    position.iter().all(|v| v.is_finite()) && size.is_finite() && *size > 0.,
                    "invalid ambient fire"
                ),
                AmbientEmission::Snow {
                    count,
                    spread,
                    height,
                    fall_speed,
                    half_size,
                    fog_range,
                    ..
                } => ensure!(
                    *count > 0
                        && *count <= 4096
                        && *spread > 0
                        && height.is_finite()
                        && *height > 0.
                        && fall_speed[0] > 0
                        && fall_speed[0] <= fall_speed[1]
                        && half_size.is_finite()
                        && *half_size > 0.
                        && fog_range.iter().all(|v| v.is_finite())
                        && fog_range[0] < fog_range[1],
                    "invalid field snow"
                ),
            }
        }
        if let Some(wind) = &self.wind {
            wind.gate.validate()?;
            ensure!(
                wind.steps > 0 && wind.base.iter().chain(&wind.step).all(|v| v.is_finite()),
                "invalid field wind"
            );
        }
        if let Some(haze) = &self.heat_haze {
            haze.gate.validate()?;
            haze.sprite.validate()?;
            ensure!(
                haze.scroll_per_tick.is_finite()
                    && haze.rotation_degrees.is_finite()
                    && haze.scale.is_finite()
                    && haze.scale > 0.,
                "invalid field heat haze"
            );
        }
        if let Some(lights) = &self.lights {
            lights.gate.validate()?;
            lights.target.validate()?;
            ensure!(
                !lights.lights.is_empty()
                    && lights.lights.len() <= 2
                    && lights.attenuation.iter().all(|v| v.is_finite() && *v >= 0.)
                    && lights.attenuation.iter().any(|v| *v > 0.),
                "invalid scenery lighting"
            );
            for light in &lights.lights {
                light.actor.validate()?;
                ensure!(
                    light.height.is_finite() && light.intensity.is_finite() && light.intensity > 0.,
                    "invalid scenery light"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum EnvironmentGate {
    #[default]
    Always,
    RenderingEnabled,
    SettingNonzero(u8),
    ActorExists(RenderValue),
}
impl EnvironmentGate {
    pub fn active(
        &self,
        enabled: bool,
        settings: &BTreeMap<i32, i32>,
        actor_exists: impl Fn(i32) -> bool,
    ) -> bool {
        match self {
            Self::Always => true,
            Self::RenderingEnabled => enabled,
            Self::SettingNonzero(slot) => {
                settings.get(&i32::from(*slot)).copied().unwrap_or(0) != 0
            }
            Self::ActorExists(actor) => actor_exists(actor.resolve(settings)),
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::SettingNonzero(slot) => ensure!(*slot < 8, "invalid environment setting slot"),
            Self::ActorExists(actor) => actor.validate()?,
            _ => {}
        }
        Ok(())
    }
}
impl RenderValue {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Fixed(_) => {}
            Self::Setting(slot) | Self::SettingOffset { slot, .. } => {
                ensure!(*slot < 8, "invalid rendering setting slot")
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbientParticles {
    pub gate: EnvironmentGate,
    pub sprite: SpriteRecipe,
    pub emission: AmbientEmission,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AmbientEmission {
    Drifting {
        interval: u16,
        lifetime: u16,
        position: [f32; 3],
        /// Independent integer samples in 0..position_steps multiply these offsets.
        position_step: [f32; 3],
        position_steps: [u16; 3],
        velocity: [f32; 3],
        half_size: [f32; 2],
        color: [u8; 4],
    },
    Fire {
        position: [f32; 3],
        size: f32,
    },
    Snow {
        count: u16,
        spread: u16,
        height: f32,
        fall_speed: [u8; 2],
        half_size: f32,
        color: [u8; 4],
        fog_color: [u8; 4],
        fog_range: [f32; 2],
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldWind {
    pub gate: EnvironmentGate,
    pub base: [f32; 3],
    pub step: [f32; 3],
    pub steps: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatHaze {
    pub gate: EnvironmentGate,
    pub sprite: SpriteRecipe,
    pub scroll_per_tick: f32,
    pub rotation_degrees: f32,
    pub scale: f32,
    pub alpha: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneryLighting {
    pub gate: EnvironmentGate,
    pub target: RenderValue,
    pub ambient: [u8; 3],
    pub color: [u8; 3],
    pub attenuation: [f32; 3],
    pub lights: Vec<SceneryLight>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneryLight {
    pub actor: RenderValue,
    pub height: f32,
    pub intensity: f32,
}
