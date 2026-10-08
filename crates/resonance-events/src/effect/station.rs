//! Pedestal lights follow sampled arcs, then the player receives a final glow.
use super::{BillboardEffect, Fade, NEUTRAL_TINT, emission::normalized};
use crate::{GameWorld, Operation};

const LIGHTS: usize = 7;
const TRANSFER_TICKS: u32 = 60;
const AFTERGLOW_TICKS: u32 = 30;
const PLAYER_HEIGHT: f32 = 100.;
const ARRIVAL_RADIUS: f32 = 8.;
const CURVATURE: f32 = 8.;
const FAN_TILT: f32 = 61.;

#[derive(Debug, Clone)]
struct Light {
    velocity: [f32; 3],
    bend: [f32; 3],
    duration: u32,
    position: [f32; 3],
    active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Transfer {
    station: i32,
    player: i32,
    source: [f32; 3],
    born: u32,
    lights: Vec<Light>,
    pub operation: Operation,
}
impl Transfer {
    pub fn new(station: i32, player: i32, operation: Operation, tick: u32) -> Self {
        Self {
            station,
            player,
            source: [0.; 3],
            // Let the completed interaction reach the field before launching.
            born: tick + 2,
            lights: Vec::new(),
            operation,
        }
    }

    pub fn update(&mut self, world: &mut GameWorld) -> Result<(), String> {
        if !self.operation.is_pending() || world.tick < self.born {
            return Ok(());
        }
        let (Ok(station), Ok(player)) = (world.actor_id(self.station), world.actor_id(self.player))
        else {
            return self.operation.complete(None);
        };
        let mut target = world.actors[&player].position;
        target[2] += PLAYER_HEIGHT;
        let color = world.actors[&station].tint;
        let age = world.tick - self.born;
        if age >= TRANSFER_TICKS + AFTERGLOW_TICKS {
            return self.operation.complete(None);
        }
        if age == 0 {
            self.source = world.actors[&station].position;
            self.source[2] += (world.effect_tick as f32).to_radians().sin() * 10. + 150.;
            let delta: [f32; 3] = std::array::from_fn(|i| target[i] - self.source[i]);
            let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            let direction = normalized(delta);
            let away = normalized([-delta[0], -delta[1], 0.]);
            let (fan, forward) = FAN_TILT.to_radians().sin_cos();
            self.lights = (0..LIGHTS)
                .map(|index| {
                    let angle =
                        (index as f32 - (LIGHTS - 1) as f32 / 2.) * std::f32::consts::FRAC_PI_4;
                    let (sin, cos) = angle.sin_cos();
                    let launch = [
                        away[0] * forward - away[1] * fan * sin,
                        away[1] * forward + away[0] * fan * sin,
                        fan * cos,
                    ];
                    let speed = 2.33 + (world.random() & 15) as f32 / 64.;
                    Light {
                        velocity: direction.map(|v| v * speed),
                        bend: std::array::from_fn(|i| {
                            (launch[i] - direction[i]) * speed * CURVATURE
                        }),
                        duration: (distance / (2. * speed)).max(1.) as u32 * 2,
                        position: self.source,
                        active: true,
                    }
                })
                .collect();
            self.glow(
                world,
                self.source,
                [color[0], color[1], color[2], 128],
                12.,
                -4.,
            )?;
        } else {
            for light in self.lights.iter_mut().filter(|light| light.active) {
                world.random();
                if age == 2 {
                    world.random();
                }
                let t = age.min(light.duration) as f32;
                let curve = t * (1. - (t + 1.) / light.duration as f32);
                light.position = std::array::from_fn(|i| {
                    self.source[i] + light.velocity[i] * t + light.bend[i] * curve
                });
                // Even a zero-width trail occupies its place among blended lights.
                world.emit_billboard(BillboardEffect {
                    operation: Some(self.operation.clone()),
                    recipe: super::ORB_SPRITE,
                    born: world.tick,
                    lifetime: TRANSFER_TICKS + 1,
                    position: light.position,
                    fade: Fade::Linear(-10.),
                    ..Default::default()
                })?;
                light.active = age < TRANSFER_TICKS
                    && ((light.position[0] - target[0]).powi(2)
                        + (light.position[1] - target[1]).powi(2))
                    .sqrt()
                        >= ARRIVAL_RADIUS;
            }
        }
        Ok(())
    }

    pub fn draw(&self, world: &mut GameWorld) -> Result<(), String> {
        if !self.operation.is_pending() || self.lights.is_empty() {
            return Ok(());
        }
        let station = world.actor_id(self.station)?;
        let color = world.actors[&station].tint;
        for (index, light) in self
            .lights
            .iter()
            .enumerate()
            .filter(|(_, light)| light.active)
        {
            for (image, size, alpha, fade) in [
                (super::STAR_SPRITE, 80., 128, -8.),
                (super::CAMERA_DISC_SPRITE, 48., 64, -4.),
            ] {
                let mut sprite = self.sprite(world, light.position, image, 2, size);
                sprite.rgba[3] = alpha;
                sprite.fade = Fade::Linear(fade);
                if image == super::STAR_SPRITE {
                    sprite.rotation[2] =
                        if index % 2 == 0 { -4. } else { 4. } * world.effect_tick as f32;
                }
                world.emit_billboard(sprite)?;
            }
            if world.effect_tick.is_multiple_of(4) {
                for (speed, rgba) in [
                    (-0.02, [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 128]),
                    (-3., [color[0], color[1], color[2], 128]),
                ] {
                    let mut halo =
                        self.sprite(world, light.position, super::STATION_HALO_SPRITE, 31, 96.);
                    halo.palette = None;
                    halo.rgba = rgba;
                    halo.rotation[2] = world.random() as f32;
                    halo.velocity[2] = speed;
                    world.emit_billboard(halo)?;
                }
            }
        }
        if world.tick - self.born == TRANSFER_TICKS {
            let player = world.actor_id(self.player)?;
            let mut target = world.actors[&player].position;
            target[2] += PLAYER_HEIGHT;
            self.glow(world, target, [color[0], color[1], color[2], 255], 12., -8.)?;
            self.glow(world, target, [255, 255, 255, 128], 6., -4.)?;
        }
        Ok(())
    }

    fn glow(
        &self,
        world: &mut GameWorld,
        position: [f32; 3],
        rgba: [u8; 4],
        growth: f32,
        fade: f32,
    ) -> Result<(), String> {
        let mut glow = self.sprite(
            world,
            position,
            super::STATION_GLOW_SPRITE,
            AFTERGLOW_TICKS + 1,
            12.,
        );
        glow.palette = None;
        glow.rgba = rgba;
        glow.size_delta = growth;
        glow.fade = Fade::Linear(fade);
        world.emit_billboard(glow)?;
        Ok(())
    }

    fn sprite(
        &self,
        world: &GameWorld,
        position: [f32; 3],
        recipe: u16,
        lifetime: u32,
        size: f32,
    ) -> BillboardEffect {
        BillboardEffect {
            operation: Some(self.operation.clone()),
            recipe,
            palette: Some(0),
            born: world.tick,
            lifetime,
            position,
            size: [size; 2],
            rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 128],
            fade: Fade::Linear(0.),
            ..Default::default()
        }
    }
}
