//! Frozen effect inputs for paired renderer tests; no gameplay or random emission.
use anyhow::{Result, ensure};
use resonance_events::{
    GameWorld,
    effect::{
        BillboardEffect, Blend, Fade, Flutter, RefractionImage, RefractionPulse, SpriteOrientation,
    },
    model_particle::ModelParticle,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectProbe {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub size: [f32; 2],
    pub rgba: [u8; 4],
    pub world_space: bool,
    pub blend: u8,
    #[serde(default)]
    pub age: u32,
    pub shape: Shape,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Sprite { recipe: u16, uv: Option<[f32; 4]> },
    Model { resource: u32, scale: [f32; 3] },
    Refraction { air: bool },
    Leaf { recipe: i32, motion: Flutter },
}

impl EffectProbe {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.position
                .iter()
                .chain(&self.rotation)
                .chain(&self.size)
                .all(|v| v.is_finite())
                && self.size.iter().all(|v| *v > 0.)
                && self.blend <= 2
                && self.age <= u32::MAX - 2,
            "invalid effect probe placement or blend"
        );
        match &self.shape {
            Shape::Sprite { uv: Some(uv), .. } => ensure!(
                uv.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "invalid probe atlas rectangle"
            ),
            Shape::Model { scale, .. } => ensure!(
                scale.iter().all(|v| v.is_finite() && *v > 0.),
                "invalid probe model scale"
            ),
            _ => {}
        }
        Ok(())
    }

    pub(super) fn apply(&self, world: &mut GameWorld, id: i32) {
        let orientation = if self.world_space {
            SpriteOrientation::World
        } else {
            SpriteOrientation::Camera
        };
        match &self.shape {
            Shape::Sprite { recipe, uv } => {
                let mut sprite = BillboardEffect::default();
                sprite.recipe = *recipe;
                sprite.uv = *uv;
                sprite.born = world.tick.saturating_sub(self.age);
                sprite.lifetime = self.age + 2;
                sprite.position = self.position;
                sprite.rotation = self.rotation;
                sprite.size = self.size;
                sprite.rgba = self.rgba;
                sprite.orientation = orientation;
                sprite.blend = Some(
                    i32::from(self.blend)
                        .try_into()
                        .expect("validated probe blend"),
                );
                sprite.field_fog = false;
                world.billboards.insert(id, sprite);
            }
            Shape::Model { resource, scale } => {
                let mut model = ModelParticle::new(*resource);
                model.position = self.position;
                model.rotation = self.rotation;
                model.scale = *scale;
                model.rgba = self.rgba;
                model.orientation = orientation;
                model.blend = match self.blend {
                    0 => Blend::Alpha,
                    1 => Blend::Additive,
                    _ => Blend::Subtractive,
                };
                world.model_particles.insert(id, model);
            }
            Shape::Refraction { air } => {
                world.refractions.insert(
                    id,
                    RefractionPulse {
                        operation: None,
                        owner: None,
                        image: if *air {
                            RefractionImage::Air
                        } else {
                            RefractionImage::Ripple
                        },
                        palette: 0,
                        orientation,
                        rotation: self.rotation,
                        position: self.position,
                        born: world.tick,
                        lifetime: 2,
                        size: self.size[0],
                        growth: 0.,
                        alpha: self.rgba[3] as f32,
                        fade: Fade::Linear(0.),
                    },
                );
            }
            Shape::Leaf { recipe, motion } => {
                world.particles.push(resonance_events::Particle {
                    kind: *recipe,
                    handle: id,
                    born: world.tick,
                    lifetime: 100,
                    position: self.position,
                    velocity: [0.; 3],
                    size: self.size[0],
                    size_delta: 0.,
                    rgba: self.rgba.map(f32::from),
                    alpha_delta: 0.,
                    flutter: Some(motion.clone()),
                });
            }
        }
    }
}
