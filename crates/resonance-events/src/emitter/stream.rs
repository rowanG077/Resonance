//! Continuous emission uses normalized settings prepared at the script boundary.
use super::{State, inherit, normalized, palette};
use crate::effect::{BillboardController, BillboardEffect};
use crate::world::random_unit;

#[derive(Debug, Clone, Default)]
pub(crate) struct Stream {
    pub(super) sprite: BillboardEffect,
    pub(super) palette: Option<i32>,
    pub(super) images: &'static [u16],
    pub(super) interval: u32,
    pub(super) count: u32,
    pub(super) radius: [f32; 2],
    pub(super) filled: bool,
    pub(super) converge: bool,
    pub(super) size_variation: [f32; 2],
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
impl Stream {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn particles(
        &self,
        state: &mut State,
        owner: i32,
        center: [f32; 3],
        camera: [f32; 3],
        actor: &crate::Actor,
        born: u32,
        tick: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        let s = self;
        let center = if s.converge {
            let direction = normalized([camera[0], camera[1], 0.]);
            std::array::from_fn(|i| center[i] + direction[i] * s.camera_offset.unwrap_or(0.))
        } else {
            center
        };
        let actor_speed = actor.movement_speed();
        if s.limit.is_some_and(|limit| state.emitted >= limit) {
            return;
        }
        state.angle = (state.angle + actor_speed * s.orbit_speed_scale) % 360.;
        if !tick.is_multiple_of(s.interval) {
            return;
        }
        let speed = actor_speed * s.speed_scale;
        if state.emitted == 0
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
            .map_or(s.count, |limit| s.count.min(limit - state.emitted));
        for spoke in 0..count {
            let mut p = s.sprite.clone();
            p.born = born;
            for (position, origin) in p.position.iter_mut().zip(center) {
                *position += origin;
            }
            p.recipe = s.images[crate::world::random(random) as usize % s.images.len()];
            p.palette = s.palette.map(|color| palette(color, random));
            let variation = random_unit(random);
            for (size, spread) in p.size.iter_mut().zip(s.size_variation) {
                *size += variation * spread;
            }
            let angle = if s.count > 1 {
                state.angle + spoke as f32 * 360. / s.count as f32
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
            if s.converge {
                let orbit = super::Orbit::new(center, normalized(camera), spread, 0., angle, 0.);
                p.position = orbit.position(0);
                p.velocity =
                    std::array::from_fn(|i| (center[i] - p.position[i]) / p.lifetime as f32);
                let [x, y, z] = p.velocity;
                p.rotation = [
                    z.atan2(x.hypot(y)).to_degrees(),
                    0.,
                    (-x).atan2(y).to_degrees(),
                ];
            }
            if s.owned {
                p.owner = Some(owner);
            }
            if let Some(distance) = s.camera_offset.filter(|_| !s.converge) {
                p.controller = Some(BillboardController::CameraOffset {
                    emitter: owner,
                    center,
                    distance,
                });
            }
            if s.inherit_appearance {
                inherit(&mut p, actor);
            }
            out.push(p);
        }
        state.emitted = state.emitted.saturating_add(count);
    }
}
