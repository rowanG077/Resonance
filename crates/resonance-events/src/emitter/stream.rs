//! Continuous emission uses normalized settings prepared at the script boundary.
use super::{inherit, normalized, palette, particle};
use crate::effect::{BillboardController, BillboardEffect};
use crate::world::random_unit;

#[derive(Debug, Clone)]
pub(crate) struct Stream {
    pub(super) settings: Settings,
    pub(super) angle: f32,
    pub(super) emitted: u32,
}

#[derive(Debug, Clone)]
pub(super) struct Settings {
    pub(super) sprite: BillboardEffect,
    pub(super) palette: Option<i32>,
    pub(super) images: &'static [u16],
    pub(super) interval: u32,
    pub(super) count: u32,
    pub(super) radius: [f32; 2],
    pub(super) filled: bool,
    pub(super) size_variation: f32,
    pub(super) speed_variation: f32,
    pub(super) speed_scale: f32,
    pub(super) radial_speed: f32,
    pub(super) orbit_speed_scale: f32,
    pub(super) radial_speed_scale: f32,
    pub(super) target: Option<[f32; 3]>,
    pub(super) camera_offset: Option<f32>,
    pub(super) inherit_appearance: bool,
    pub(super) owned: bool,
    pub(super) preserve_particles: bool,
    pub(super) limit: Option<u32>,
    pub(super) flash: Option<BillboardEffect>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            sprite: particle([0.; 3], 0, 0, 300),
            palette: Some(0),
            images: &[10],
            interval: 1,
            count: 1,
            radius: [0.; 2],
            filled: false,
            size_variation: 0.,
            speed_variation: 0.,
            speed_scale: 0.,
            radial_speed: 0.,
            orbit_speed_scale: 0.,
            radial_speed_scale: 0.,
            target: None,
            camera_offset: None,
            inherit_appearance: false,
            owned: false,
            preserve_particles: false,
            limit: None,
            flash: None,
        }
    }
}

impl Stream {
    pub(crate) fn camera_offset(&self) -> Option<f32> {
        self.settings.camera_offset
    }
    pub(super) fn preserves_particles(&self) -> bool {
        self.settings.preserve_particles
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn particles(
        &mut self,
        owner: i32,
        center: [f32; 3],
        properties: &std::collections::BTreeMap<i32, i32>,
        blend: Option<crate::model_particle::Blend>,
        born: u32,
        tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) -> Result<(), String> {
        let s = &self.settings;
        let actor_speed = properties.get(&super::SPEED_PROPERTY).copied().unwrap_or(0) as f32;
        if s.limit.is_some_and(|limit| self.emitted >= limit) {
            return Ok(());
        }
        self.angle = (self.angle + actor_speed * s.orbit_speed_scale) % 360.;
        if !tick.is_multiple_of(s.interval) {
            return Ok(());
        }
        let speed = actor_speed * s.speed_scale;
        if self.emitted == 0
            && let Some(flash) = &s.flash
        {
            let mut flash = flash.clone();
            flash.position = center;
            flash.born = born;
            flash.palette = s.palette.map(|color| palette(color, random));
            out.push(flash);
        }
        let count = s
            .limit
            .map_or(s.count, |limit| s.count.min(limit - self.emitted));
        for spoke in 0..count {
            let mut p = s.sprite.clone();
            p.born = born;
            for (position, origin) in p.position.iter_mut().zip(center) {
                *position += origin;
            }
            p.recipe = s.images[crate::world::random(random) as usize % s.images.len()];
            p.palette = s.palette.map(|color| palette(color, random));
            let size = random_unit(random) * s.size_variation;
            p.size = p.size.map(|v| v + size);
            let angle = if s.count > 1 {
                self.angle + spoke as f32 * 360. / s.count as f32
            } else {
                random_unit(random) * 360.
            };
            let (sin, cos) = angle.to_radians().sin_cos();
            let spread = s.radius[0].max(s.radius[1]);
            let radius = if s.filled { random_unit(random) } else { 1. };
            if let Some(target) = s.target {
                let direction = std::array::from_fn(|i| {
                    target[i] - center[i] + (random_unit(random) * 2. - 1.) * spread
                });
                p.velocity = normalized(direction).map(|v| v * speed);
            } else {
                p.position[0] += cos * s.radius[0] * radius;
                p.position[1] += sin * s.radius[1] * radius;
                p.velocity[0] += cos * (s.radial_speed + actor_speed * s.radial_speed_scale);
                p.velocity[1] += sin * (s.radial_speed + actor_speed * s.radial_speed_scale);
                p.velocity[2] += speed + random_unit(random) * s.speed_variation;
            }
            if s.owned {
                p.owner = Some(owner);
            }
            if let Some(distance) = s.camera_offset {
                p.controller = Some(BillboardController::CameraOffset {
                    emitter: owner,
                    center,
                    distance,
                });
            }
            if s.inherit_appearance {
                inherit(&mut p, properties, blend);
            }
            out.push(p);
        }
        self.emitted = self.emitted.saturating_add(count);
        Ok(())
    }
}
