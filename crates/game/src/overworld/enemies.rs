//! Resident world enemy symbols. Native spawning uses three slots, a 60-update
//! grace period, and the scene's libc random stream (never formation RNG).
use super::{Position, World, collision, landmarks::Locations, travel::Mount};
use anyhow::Result;
use resonance_content::overworld::{Interaction, MovementParameters};
use std::f32::consts::{PI, TAU};

pub const SLOTS: usize = 3;
const CONTACT_RADIUS: f32 = 100.;
const SPAWN_RADIUS: f32 = 1050.;
const RETIRE_RADIUS: f32 = 1400.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behavior {
    Patrol,
    Guard,
    Wander,
    Watch,
    Avoid,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub position: Position,
    pub variant: u8,
    pub heading: f32,
    pub speed: f32,
    pub animation: u16,
    home: Position,
    target: Position,
    behavior: Behavior,
    pause: u8,
    returning: bool,
    blocked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Symbols {
    slots: [Option<Symbol>; SLOTS],
    grace: u8,
    spawned: u16,
}

pub(super) struct Context<'a> {
    pub world: World,
    pub position: Position,
    pub mount: Mount,
    pub speed: f32,
    pub modifier: u8,
    pub terrain: &'a collision::Terrain,
    pub locations: &'a Locations,
    pub parameters: &'a MovementParameters,
}

impl Symbols {
    pub fn get(&self, slot: usize) -> Option<&Symbol> {
        self.slots.get(slot)?.as_ref()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.slots.iter().flatten()
    }
    pub(super) fn clear(&mut self) {
        self.slots = Default::default();
        self.grace = 0;
    }
    pub(super) fn contact(&self, position: Position) -> Option<(Position, u8)> {
        self.iter()
            .find(|symbol| symbol.position.distance_to(position) < CONTACT_RADIUS)
            .map(|symbol| (symbol.position, symbol.variant))
    }

    /// A missing neighbor cannot leave half the symbols moved or consume RNG.
    pub(super) fn step(&mut self, context: Context<'_>, seed: &mut u32) -> Result<()> {
        let mut next = self.clone();
        let mut random = Random(*seed);
        next.advance(&context, &mut random)?;
        *self = next;
        *seed = random.0;
        Ok(())
    }
    fn advance(&mut self, c: &Context<'_>, random: &mut Random) -> Result<()> {
        if !matches!(c.mount, Mount::Foot | Mount::Noishe) {
            self.clear();
            return Ok(());
        }
        for slot in 0..SLOTS {
            let Some(mut symbol) = self.slots[slot].take() else {
                continue;
            };
            if symbol.position.distance_to(c.position) > RETIRE_RADIUS {
                continue;
            }
            let previous = symbol.position;
            symbol.advance(c, random)?;
            if self.clear_position(symbol.position, c) {
                self.slots[slot] = Some(symbol);
            } else {
                // Actor/landmark overlap rejects horizontal movement; terrain
                // resolution and the next wander target still remain valid.
                symbol.position = previous;
                symbol.blocked = true;
                self.slots[slot] = Some(symbol);
            }
        }
        if self.grace < 60 {
            self.grace += 1;
            return Ok(());
        }
        // Native scans up to 31 actor slots each update; rejected candidates
        // must not consume formation selection or create an invisible symbol.
        for _ in 0..31 {
            let Some(slot) = self.slots.iter().position(Option::is_none) else {
                break;
            };
            let position = random.around(c.position, SPAWN_RADIUS)?;
            if !self.clear_position(position, c) {
                continue;
            }
            let query = c.terrain.query(position, c.parameters.collision_radius)?;
            let Some(surface) = query.surface(collision::Mode::Ground) else {
                continue;
            };
            if surface.height == 0. || !query.has_clearance(collision::Mode::Ground) {
                continue;
            }
            let position =
                Position::from_map([position.map()[0], position.map()[1], surface.height])?;
            let variant = (random.draw() & 1) as u8;
            self.spawned = self.spawned.saturating_add(1).min(1000);
            let mut behavior = random.draw() % 5 + 1;
            if c.modifier == 0 {
                if behavior <= 2 && self.spawned < 10 {
                    behavior = random.draw() % 3 + 3;
                } else if behavior >= 3 && self.spawned > 20 {
                    behavior = (random.draw() & 1) + 1;
                }
            }
            let behavior = match behavior {
                1 => Behavior::Patrol,
                2 => Behavior::Guard,
                3 => Behavior::Wander,
                4 => Behavior::Watch,
                _ => Behavior::Avoid,
            };
            self.slots[slot] = Some(Symbol {
                position,
                variant,
                heading: heading(position, c.position),
                speed: 0.,
                animation: 0,
                home: position,
                target: position,
                behavior: modified(behavior, c.modifier),
                pause: 0,
                returning: false,
                blocked: false,
            });
        }
        Ok(())
    }
    fn clear_position(&self, position: Position, c: &Context<'_>) -> bool {
        !self
            .iter()
            .any(|symbol| symbol.position.distance_to(position) <= CONTACT_RADIUS)
            && !c.locations.visible(c.world).any(|(landmark, appearance)| {
                appearance.interaction != Interaction::Disabled
                    && Position::from_map([landmark.position[0], landmark.position[1], 0.])
                        .is_ok_and(|p| p.distance_to(position) <= landmark.radius + 50.)
            })
    }
}
impl Symbol {
    fn advance(&mut self, c: &Context<'_>, random: &mut Random) -> Result<()> {
        let behavior = modified(self.behavior, c.modifier);
        if behavior != self.behavior {
            self.behavior = behavior;
            self.speed = 0.;
            self.animation = 0;
            return Ok(());
        }
        let distance = self.position.distance_to(c.position);
        self.animation = 0;
        let divisor = if c.mount == Mount::Noishe {
            c.parameters.mounted_stick_divisor
        } else {
            c.parameters.foot_stick_divisor
        };
        let base = c.parameters.stick_maximum / divisor;
        let mut speed = 4.;
        let mut moving = true;
        match self.behavior {
            Behavior::Patrol => {
                if self.pause > 0 {
                    self.pause -= 1;
                    self.heading = heading(self.position, c.position);
                    self.animation = 3;
                    moving = false;
                } else if distance > 968. || c.position.distance_to(self.home) > 968. {
                    if !self.returning && distance <= 968. {
                        self.returning = true;
                        self.pause = 120;
                        self.animation = 3;
                        moving = false;
                    } else {
                        self.wander(500., random)?;
                    }
                } else {
                    self.returning = false;
                    self.heading = heading(self.position, c.position);
                    speed = if c.mount == Mount::Noishe {
                        if distance < 726. { 0.5 * base } else { 4. }
                    } else {
                        2. * base
                    };
                }
            }
            Behavior::Guard => {
                self.heading = heading(self.position, c.position);
                moving = distance <= 300. && c.position.distance_to(self.home) <= 300.;
                speed = if c.mount == Mount::Noishe {
                    if distance < 225. { 0.5 * base } else { 4. }
                } else {
                    2. * base
                };
            }
            Behavior::Wander => self.wander(800., random)?,
            Behavior::Watch => {
                self.heading = heading(self.position, c.position);
                moving = false;
            }
            Behavior::Avoid => {
                if distance > 500. {
                    if self.returning {
                        self.target = random.around(self.home, 800.)?;
                    }
                    self.returning = false;
                    self.wander(800., random)?;
                } else {
                    self.returning = true;
                    if c.position.distance_to(self.home) > 500. {
                        self.heading = heading(self.position, self.home);
                        moving = self.position.distance_to(self.home) >= speed;
                    } else {
                        self.heading = (heading(self.position, c.position) + PI).rem_euclid(TAU);
                    }
                }
            }
        }
        if c.mount == Mount::Noishe && c.speed == 0. {
            moving = false;
        }
        if moving {
            if matches!(self.behavior, Behavior::Patrol | Behavior::Guard) {
                speed = speed.min(self.speed + 0.75);
            }
            self.animation = if speed > 0.5 * base { 2 } else { 1 };
        } else {
            speed = 0.;
        }
        self.speed = speed;
        let motion = c
            .terrain
            .query(self.position, c.parameters.collision_radius)?
            .motion(speed, self.heading, collision::Mode::Ground)?;
        self.blocked = motion.response.is_none();
        self.position = self.position.translated(motion.delta)?;
        Ok(())
    }
    fn wander(&mut self, radius: f32, random: &mut Random) -> Result<()> {
        if self.blocked || self.position.distance_to(self.target) < 4. {
            self.target = random.around(self.home, radius)?;
            self.blocked = false;
        }
        self.heading = heading(self.position, self.target);
        Ok(())
    }
}
fn modified(behavior: Behavior, modifier: u8) -> Behavior {
    match (modifier, behavior) {
        (1, Behavior::Patrol | Behavior::Guard) => Behavior::Avoid,
        (2, Behavior::Wander | Behavior::Watch | Behavior::Avoid) => Behavior::Patrol,
        _ => behavior,
    }
}
fn heading(from: Position, to: Position) -> f32 {
    let [x, z] = from.displacement_to(to);
    x.atan2(-z).rem_euclid(TAU)
}
struct Random(u32);
impl Random {
    fn draw(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
        (self.0 >> 16) & 0x7fff
    }
    fn around(&mut self, origin: Position, radius: f32) -> Result<Position> {
        let angle = TAU * (self.draw() & 4095) as f32 / 4096.;
        origin.translated([
            radius * collision::sine(angle),
            radius * collision::cosine(angle),
            0.,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::{
        TileCoordinate,
        collision::{
            Mesh, Terrain,
            tests::{rectangle, tables},
        },
        landmarks::tests::definitions,
        travel::tests::parameters,
    };
    use std::sync::Arc;

    fn terrain(surface: u32, neighbors: bool) -> Terrain {
        let groups: Vec<_> = (0..16)
            .flat_map(|x| {
                (0..16).map(move |z| {
                    let min = [x as f32 * 400. - 3200., z as f32 * 400. - 3200.];
                    rectangle(surface, min, min.map(|v| v + 400.), 80.)
                })
            })
            .collect();
        let center = TileCoordinate::new(0, 0).unwrap();
        Terrain::new(
            (-1..=1)
                .flat_map(|x| (-1..=1).map(move |z| [x, z]))
                .filter(|&offset| neighbors || offset == [0, 0])
                .map(|offset| (center.neighbor(offset), Mesh::new(&groups).unwrap())),
            tables(),
        )
        .unwrap()
    }
    fn locations() -> Locations {
        Locations::new(
            Arc::new(definitions()),
            Arc::default(),
            crate::overworld::scripts::fixture(),
        )
        .unwrap()
    }
    fn context<'a>(
        terrain: &'a Terrain,
        locations: &'a Locations,
        parameters: &'a MovementParameters,
    ) -> Context<'a> {
        Context {
            world: World::Sylvarant,
            position: Position::from_map([3200., 3200., 80.]).unwrap(),
            mount: Mount::Foot,
            speed: 0.,
            modifier: 0,
            terrain,
            locations,
            parameters,
        }
    }

    #[test]
    fn spawn_grace_resident_limit_ground_and_rng_replay() -> Result<()> {
        let ground = terrain(1, true);
        let locations = locations();
        let parameters = parameters();
        let mut symbols = Symbols::default();
        let mut seed = 1234;
        for _ in 0..60 {
            symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
        }
        assert_eq!(seed, 1234);
        assert_eq!(symbols.iter().count(), 0);
        symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
        assert_eq!(symbols.iter().count(), SLOTS);
        for symbol in symbols.iter() {
            assert!(
                (symbol
                    .position
                    .distance_to(Position::from_map([3200., 3200., 80.])?)
                    - SPAWN_RADIUS)
                    .abs()
                    < 3.
            );
            assert_eq!(symbol.position.map()[2], 80.);
            assert!(matches!(
                symbol.behavior,
                Behavior::Wander | Behavior::Watch | Behavior::Avoid
            ));
        }
        let mut replay = symbols.clone();
        let mut replay_seed = seed;
        for _ in 0..120 {
            symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
            replay.step(context(&ground, &locations, &parameters), &mut replay_seed)?;
            assert_eq!(
                symbols
                    .iter()
                    .map(|s| (s.position, s.animation))
                    .collect::<Vec<_>>(),
                replay
                    .iter()
                    .map(|s| (s.position, s.animation))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(seed, replay_seed);
        let sea = terrain(11, true);
        symbols.clear();
        for _ in 0..65 {
            symbols.step(context(&sea, &locations, &parameters), &mut seed)?;
        }
        assert_eq!(symbols.iter().count(), 0);
        Ok(())
    }

    #[test]
    fn missing_collision_rolls_back_spawn_rng_and_vehicle_travel_clears_symbols() -> Result<()> {
        let ground = terrain(1, false);
        let locations = locations();
        let parameters = parameters();
        let mut symbols = Symbols {
            grace: 60,
            ..Default::default()
        };
        let mut seed = 15;
        let mut c = context(&ground, &locations, &parameters);
        c.position = Position::from_map([100., 100., 80.])?;
        assert!(symbols.step(c, &mut seed).is_err());
        assert_eq!(seed, 15);
        assert_eq!(symbols.iter().count(), 0);
        let ground = terrain(1, true);
        for mount in [Mount::Rheairds, Mount::Ship] {
            symbols.grace = 60;
            symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
            assert_eq!(symbols.iter().count(), SLOTS);
            symbols.step(
                Context {
                    mount,
                    ..context(&ground, &locations, &parameters)
                },
                &mut seed,
            )?;
            assert_eq!(symbols.iter().count(), 0);
            assert_eq!(symbols.grace, 0);
        }
        Ok(())
    }

    #[test]
    fn bottles_change_behavior_noishe_stops_pursuit_and_contacts_wrap() -> Result<()> {
        let ground = terrain(1, true);
        let locations = locations();
        let parameters = parameters();
        let mut symbols = Symbols {
            grace: 60,
            ..Default::default()
        };
        let mut seed = 99;
        symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
        let mut symbol = symbols.get(0).unwrap().clone();
        symbol.behavior = Behavior::Patrol;
        symbol.position = Position::from_map([3400., 3200., 80.])?;
        symbol.home = symbol.position;
        let mut random = Random(seed);
        symbol.advance(
            &Context {
                modifier: 1,
                ..context(&ground, &locations, &parameters)
            },
            &mut random,
        )?;
        assert_eq!(symbol.behavior, Behavior::Avoid);
        symbol.advance(
            &Context {
                modifier: 2,
                ..context(&ground, &locations, &parameters)
            },
            &mut random,
        )?;
        assert_eq!(symbol.behavior, Behavior::Patrol);
        let before = symbol.position;
        symbol.advance(
            &Context {
                mount: Mount::Noishe,
                ..context(&ground, &locations, &parameters)
            },
            &mut random,
        )?;
        assert_eq!(symbol.position, before);
        assert_eq!(symbol.speed, 0.);
        symbol.advance(&context(&ground, &locations, &parameters), &mut random)?;
        assert!(
            symbol
                .position
                .distance_to(context(&ground, &locations, &parameters).position)
                < 200.
        );
        symbol.position = Position::from_map([76790., 10., 80.])?;
        symbols.slots = [Some(symbol), None, None];
        assert!(
            symbols
                .contact(Position::from_map([10., 57590., 80.])?)
                .is_some()
        );
        assert!(
            symbols
                .contact(Position::from_map([110., 10., 80.])?)
                .is_none()
        );
        Ok(())
    }
}
