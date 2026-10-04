//! Pedestal lights follow curved paths to the player, then leave a short afterglow.
use super::{BillboardEffect, Fade, NEUTRAL_TINT};
use crate::{GameWorld, Operation};

const FLIGHT_TICKS: u32 = 60;
const AFTERGLOW_TICKS: u32 = 30;
const LIGHTS: usize = 7;
const STATION_HEIGHT: f32 = 150.;
const PLAYER_HEIGHT: f32 = 100.;
const ARC_HEIGHT: f32 = 80.;

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
        let color = world.actors[&station].station_color();
        let mut target = world.actors[&player].position;
        target[2] += PLAYER_HEIGHT;
        if age == 0 || age == FLIGHT_TICKS {
            let mut glow = self.sprite(
                world,
                if age == 0 { self.source } else { target },
                super::STATION_GLOW_SPRITE,
                AFTERGLOW_TICKS,
                12.,
            );
            glow.palette = None;
            glow.rgba = [color[0], color[1], color[2], 192];
            glow.size_delta = 12.;
            world.emit_billboard(glow)?;
        }
        if age < FLIGHT_TICKS {
            let t = age as f32 / FLIGHT_TICKS as f32;
            let arc = (std::f32::consts::PI * t).sin() * ARC_HEIGHT;
            for index in 0..LIGHTS {
                let (sin, cos) = (index as f32 * std::f32::consts::TAU / LIGHTS as f32).sin_cos();
                let mut position =
                    std::array::from_fn(|i| self.source[i] + (target[i] - self.source[i]) * t);
                position[0] += cos * arc;
                position[1] += sin * arc;
                position[2] += arc;
                for (image, size) in [(super::ORB_SPRITE, 32.), (super::STAR_SPRITE, 20.)] {
                    let mut sprite = self.sprite(world, position, image, 1, size);
                    sprite.rotation[2] = age as f32 * 4.;
                    world.emit_billboard(sprite)?;
                }
                if age.is_multiple_of(4) {
                    let mut trail = self.sprite(world, position, super::ORB_SPRITE, 12, 24.);
                    trail.palette = None;
                    trail.rgba[3] = 48;
                    trail.fade = Fade::Proportional {
                        after: 0,
                        lifetime: trail.lifetime,
                    };
                    trail.rgba[..3].copy_from_slice(&color);
                    world.emit_billboard(trail)?;
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
