//! Pedestal lights follow curved paths to the player, then leave a short afterglow.
use super::{BillboardEffect, Fade, NEUTRAL_TINT};
use crate::{GameWorld, Operation};

const FLIGHT_TICKS: u32 = 44;
const AFTERGLOW_TICKS: u32 = 30;
const LIGHTS: usize = 7;
const STATION_HEIGHT: f32 = 150.;
const PLAYER_HEIGHT: f32 = 100.;
const LAUNCH_SPEED: f32 = 24.5;
const FAN_SPEED: f32 = 17.;
const UPWARD_SPEED: f32 = 7.5;
const TRAIL_RADIUS: f32 = 48.;

#[derive(Debug, Clone)]
pub(crate) struct Transfer {
    station: i32,
    player: i32,
    source: [f32; 3],
    born: u32,
    pub operation: Operation,
}
impl Transfer {
    pub fn start(
        world: &mut GameWorld,
        station: i32,
        player: i32,
        operation: Operation,
    ) -> Result<Self, String> {
        let mut source = world.actors[&world.actor_id(station)?].position;
        source[2] += STATION_HEIGHT;
        let mut transfer = Self {
            station,
            player,
            source,
            born: world.tick,
            operation,
        };
        transfer.poll(world)?;
        Ok(transfer)
    }

    pub fn poll(&mut self, world: &mut GameWorld) -> Result<bool, String> {
        let age = world.tick - self.born;
        let (Ok(station), Ok(player)) = (world.actor_id(self.station), world.actor_id(self.player))
        else {
            self.operation.complete(None)?;
            return Ok(true);
        };
        if age >= FLIGHT_TICKS + AFTERGLOW_TICKS {
            self.operation.complete(None)?;
            return Ok(true);
        }
        let color = world.actors[&station].tint;
        let mut target = world.actors[&player].position;
        target[2] += PLAYER_HEIGHT;
        if age == 0 {
            let mut glow = self.sprite(
                world,
                self.source,
                super::STATION_GLOW_SPRITE,
                AFTERGLOW_TICKS,
                18.,
            );
            glow.palette = None;
            glow.rgba = [color[0], color[1], color[2], 124];
            glow.size_delta = 6.;
            glow.fade = Fade::Linear(-4.);
            world.emit_billboard(glow)?;
        }
        if age < FLIGHT_TICKS {
            let mut dust = Vec::new();
            let t = age as f32 / FLIGHT_TICKS as f32;
            let away = super::emission::normalized([
                self.source[0] - target[0],
                self.source[1] - target[1],
                0.,
            ]);
            for index in 0..LIGHTS {
                let angle = (index as f32 - (LIGHTS - 1) as f32 / 2.) * std::f32::consts::FRAC_PI_4;
                let (sin, cos) = angle.sin_cos();
                let velocity = [
                    away[0] * LAUNCH_SPEED - away[1] * FAN_SPEED * sin,
                    away[1] * LAUNCH_SPEED + away[0] * FAN_SPEED * sin,
                    UPWARD_SPEED + FAN_SPEED * cos,
                ];
                // One quadratic curve per light: launch away, fan out, then meet the player.
                let position = std::array::from_fn(|i| {
                    let control = self.source[i] + velocity[i] * FLIGHT_TICKS as f32 / 2.;
                    (1. - t).powi(2) * self.source[i]
                        + 2. * (1. - t) * t * control
                        + t * t * target[i]
                });
                for (image, size, alpha) in [
                    (super::STAR_SPRITE, 80., 120),
                    (super::CAMERA_DISC_SPRITE, 48., 60),
                ] {
                    let mut sprite = self.sprite(world, position, image, 2, size);
                    sprite.rgba[3] = alpha;
                    if image == super::STAR_SPRITE {
                        let direction = if index % 2 == 0 { -1. } else { 1. };
                        sprite.rotation[2] = direction * world.effect_tick as f32 * 4.;
                    }
                    world.emit_billboard(sprite)?;
                }
                let mut trail = self.sprite(world, position, super::ORB_SPRITE, 27, 2.);
                trail.rgba = [63, 63, 63, 245];
                trail.fade = Fade::Linear(-10.);
                super::emission::Emission {
                    particle: trail,
                    count: 8,
                    spread: TRAIL_RADIUS,
                    speed: 0.,
                    size_variation: 2.,
                }
                .emit(&mut world.random_state, &mut dust);
            }
            for particle in dust {
                world.emit_billboard(particle)?;
            }
        }
        if age == FLIGHT_TICKS {
            let mut halo = self.sprite(
                world,
                target,
                super::STATION_HALO_SPRITE,
                AFTERGLOW_TICKS,
                80.,
            );
            halo.size_delta = 6.;
            halo.fade = Fade::Linear(-4.);
            world.emit_billboard(halo)?;
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
            field_lighting: true,
            recipe,
            palette: Some(0),
            born: world.tick,
            lifetime,
            position,
            size: [size; 2],
            rgba: [NEUTRAL_TINT, NEUTRAL_TINT, NEUTRAL_TINT, 128],
            fade: Fade::tail(lifetime),
            ..Default::default()
        }
    }
}
