//! The pedestal's seven-light transfer (fn_8007BF58); scripts own interaction sequencing.
use super::{BillboardEffect, Fade, NEUTRAL_TINT, SpriteOrientation};
use crate::{GameWorld, Operation};

const FLIGHT_TICKS: u32 = 60;
const AFTERGLOW_TICKS: u32 = 30;

#[derive(Debug, Clone, Copy, Default)]
struct Light {
    position: [f32; 3],
    velocity: [f32; 3],
    curve: [f32; 3],
    decay: [f32; 3],
    remaining: u32,
    active: bool,
}
impl Light {
    fn new(
        source: [f32; 3],
        target: [f32; 3],
        heading: f32,
        index: usize,
        speed: f32,
    ) -> Result<Self, String> {
        let fan = (-225. - index as f32 * 45.).to_radians();
        let (sin, cos) = heading.to_radians().sin_cos();
        let x = fan.sin() * 61_f32.to_radians().sin();
        let y = 61_f32.to_radians().cos();
        let z = fan.cos() * 61_f32.to_radians().sin();
        let initial = [x * cos - y * sin, x * sin + y * cos, z];
        let delta: [f32; 3] = std::array::from_fn(|i| target[i] - source[i]);
        let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        let direction = delta.map(|v| v / length.max(f32::EPSILON));
        let half = (length / speed * 0.5).max(1.);
        if !half.is_finite() || half >= (i32::MAX / 2) as f32 {
            return Err("station transfer duration is out of range".into());
        }
        let half = half as u32;
        let curve = std::array::from_fn(|i| speed * (initial[i] - direction[i]) * 8.);
        Ok(Self {
            position: source,
            velocity: direction.map(|v| speed * v),
            curve,
            decay: curve.map(|v| v / half as f32),
            remaining: half * 2,
            active: true,
        })
    }
    fn advance(&mut self) {
        if self.remaining > 0 {
            for i in 0..3 {
                self.curve[i] -= self.decay[i];
                self.position[i] += self.velocity[i] + self.curve[i];
            }
            self.remaining -= 1;
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Transfer {
    station: i32,
    player: i32,
    born: u32,
    lights: [Light; 7],
    pub operation: Operation,
}
impl Transfer {
    pub fn start(
        world: &mut GameWorld,
        station: i32,
        player: i32,
        operation: Operation,
    ) -> Result<Self, String> {
        let station_actor = &world.actors[&world.actor_id(station)?];
        let mut source = station_actor.position;
        source[2] += (world.tick as f32).to_radians().sin() * 10. + 150.;
        let color = station_actor.station_color();
        let mut target = world.actors[&world.actor_id(player)?].position;
        target[2] += 100.;
        let heading = (target[0] - source[0])
            .atan2(-(target[1] - source[1]))
            .to_degrees();
        let mut transfer = Self {
            station,
            player,
            born: world.tick,
            lights: [Light::default(); 7],
            operation,
        };
        transfer.glow(
            world,
            source,
            2,
            [color[0], color[1], color[2], 128],
            12.,
            -4.,
        )?;
        for (index, light) in transfer.lights.iter_mut().enumerate() {
            let speed = 2.33 + (world.random() % 16) as f32 / 64.;
            *light = Light::new(source, target, heading, index, speed)?;
        }
        transfer.poll(world)?;
        Ok(transfer)
    }

    pub fn poll(&mut self, world: &mut GameWorld) -> Result<bool, String> {
        let age = world.tick - self.born;
        if age >= FLIGHT_TICKS + AFTERGLOW_TICKS {
            self.operation.complete(None)?;
            return Ok(true);
        }
        if age > FLIGHT_TICKS {
            return Ok(false);
        }
        let (Ok(station), Ok(player)) = (world.actor_id(self.station), world.actor_id(self.player))
        else {
            self.operation.complete(None)?;
            return Ok(true);
        };
        let mut target = world.actors[&player].position;
        target[2] += 100.;
        let color = world.actors[&station].station_color();
        if age == FLIGHT_TICKS {
            self.glow(
                world,
                target,
                2,
                [color[0], color[1], color[2], 255],
                12.,
                -8.,
            )?;
            self.glow(
                world,
                target,
                24,
                [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 128],
                6.,
                -4.,
            )?;
        } else {
            for index in 0..self.lights.len() {
                let light = &mut self.lights[index];
                if !light.active {
                    continue;
                }
                let dx = light.position[0] - target[0];
                let dy = light.position[1] - target[1];
                if dx * dx + dy * dy < 64. {
                    light.active = false;
                } else {
                    let position = light.position;
                    light.advance();
                    self.draw(world, position, index, color)?;
                }
            }
        }
        Ok(false)
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
            owner: None,
            field_lighting: true,
            field_fog: true,
            recipe,
            orientation: SpriteOrientation::Camera,
            anchor: resonance_content::effect::VerticalAnchor::Center,
            palette: Some(0),
            born: world.tick,
            lifetime,
            position,
            velocity: [0.; 3],
            controller: None,
            acceleration: None,
            gravity: 0.,
            rotation: [0.; 3],
            angular_velocity: [0.; 3],
            size: [size; 2],
            size_delta: 0.,
            rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 128],
            fade: Fade::tail(lifetime),
            blend_mode: None,
        }
    }
    fn glow(
        &self,
        world: &mut GameWorld,
        position: [f32; 3],
        palette: u16,
        rgba: [u8; 4],
        growth: f32,
        fade: f32,
    ) -> Result<(), String> {
        let mut glow = self.sprite(world, position, super::STATION_GLOW_SPRITE, 30, 12.);
        glow.palette = Some(palette);
        glow.rgba = rgba;
        glow.size_delta = growth;
        glow.fade = Fade::Linear(fade);
        world.emit_billboard(glow)?;
        Ok(())
    }
    fn draw(
        &self,
        world: &mut GameWorld,
        position: [f32; 3],
        index: usize,
        color: [u8; 3],
    ) -> Result<(), String> {
        let mut star = self.sprite(world, position, super::STAR_SPRITE, 1, 80.);
        star.rotation[0] = world.tick as f32 * if index.is_multiple_of(2) { -1. } else { 1. } * 4.;
        star.fade = Fade::Linear(-8.);
        world.emit_billboard(star)?;
        let mut disc = self.sprite(world, position, super::CAMERA_DISC_SPRITE, 1, 48.);
        disc.rgba[3] = 64;
        disc.fade = Fade::Linear(-4.);
        world.emit_billboard(disc)?;
        if world.tick.is_multiple_of(4) {
            for palette in [0, 2] {
                let mut trail = self.sprite(world, position, 22, 30, 96.);
                trail.palette = Some(palette);
                trail.rotation[0] = world.random() as f32;
                trail.angular_velocity[0] = -0.02;
                trail.fade = Fade::Linear(0.);
                if palette == 2 {
                    trail.rgba[..3].copy_from_slice(&color);
                    trail.angular_velocity[0] = -3.;
                }
                world.emit_billboard(trail)?;
            }
        }
        Ok(())
    }
}
