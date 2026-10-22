//! Resident world enemy symbols. Native spawning uses three slots, a 60-update
//! grace period, and the scene's libc random stream (never formation RNG).
use super::{Position, World, collision, landmarks::Locations, travel::Mount};
use anyhow::Result;
use resonance_content::overworld::MovementParameters;
use std::f32::consts::{PI, TAU};

pub const SLOTS: usize = 3;
const CONTACT_RADIUS: f32 = 100.;
const SPAWN_RADIUS: f32 = 1050.;
const RETIRE_RADIUS: f32 = 1400.;
const ROAM_SPEED: f32 = 4.;
const BLOCKED_PURSUIT_LIMIT: u8 = 180;
const TURN_DEGREES: f32 = 22.;

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
    display_heading: f32,
    pub speed: f32,
    pub animation: u16,
    home: Position,
    target: Position,
    behavior: Behavior,
    pause: u8,
    returning: bool,
    blocked: u8,
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
        self.spawn(c, random)?;
        for slot in 0..SLOTS {
            let Some(mut symbol) = self.slots[slot].take() else {
                continue;
            };
            if symbol.position.distance_to(c.position) > RETIRE_RADIUS {
                continue;
            }
            let previous = symbol.position;
            let terrain_clear = symbol.advance(c, random)?;
            if terrain_clear && self.clear_position(symbol.position, c) {
                symbol.blocked = 0;
            } else {
                // Actor/landmark overlap rejects horizontal movement; terrain
                // resolution and the next wander target still remain valid.
                symbol.position = Position::from_map([
                    previous.map()[0],
                    previous.map()[1],
                    symbol.position.map()[2],
                ])?;
                symbol.blocked = symbol.blocked.saturating_add(1).min(BLOCKED_PURSUIT_LIMIT);
                if symbol.blocked == BLOCKED_PURSUIT_LIMIT
                    && matches!(symbol.behavior, Behavior::Patrol | Behavior::Guard)
                {
                    symbol.behavior = Behavior::Wander;
                }
            }
            self.slots[slot] = Some(symbol);
        }
        Ok(())
    }
    fn spawn(&mut self, c: &Context<'_>, random: &mut Random) -> Result<()> {
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
                display_heading: heading(position, c.position),
                speed: 0.,
                animation: 0,
                home: position,
                target: position,
                behavior: modified(behavior, c.modifier),
                pause: 0,
                returning: false,
                blocked: 0,
            });
        }
        Ok(())
    }
    fn clear_position(&self, position: Position, c: &Context<'_>) -> bool {
        !self
            .iter()
            .any(|symbol| symbol.position.distance_to(position) <= CONTACT_RADIUS)
            && !c.locations.enemy_obstacles(c.world).any(|landmark| {
                Position::from_map([landmark.position[0], landmark.position[1], 0.])
                    .is_ok_and(|p| p.distance_to(position) <= landmark.radius + 50.)
            })
    }
}
impl Symbol {
    pub fn display_heading(&self) -> f32 {
        self.display_heading
    }

    fn advance(&mut self, c: &Context<'_>, random: &mut Random) -> Result<bool> {
        let behavior = modified(self.behavior, c.modifier);
        if behavior != self.behavior {
            self.behavior = behavior;
            self.speed = 0.;
            self.animation = 0;
            return Ok(true);
        }
        let distance = self.position.distance_to(c.position);
        self.animation = 0;
        let divisor = if c.mount == Mount::Noishe {
            c.parameters.mounted_stick_divisor
        } else {
            c.parameters.foot_stick_divisor
        };
        let base = c.parameters.stick_maximum / divisor;
        let mut speed = ROAM_SPEED;
        let mut moving = true;
        match self.behavior {
            Behavior::Patrol => {
                if self.pause > 0 {
                    self.pause -= 1;
                    if self.pause == 0 {
                        moving = self.wander(500., random)?;
                    } else {
                        self.heading = heading(self.position, c.position);
                        self.animation = 3;
                        moving = false;
                    }
                } else if distance > 968. || c.position.distance_to(self.home) > 968. {
                    if !self.returning && distance <= 968. {
                        self.returning = true;
                        self.pause = 120;
                        self.heading = heading(self.position, c.position);
                        self.animation = 3;
                        moving = false;
                    } else {
                        moving = self.wander(500., random)?;
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
            Behavior::Wander => moving = self.wander(800., random)?,
            Behavior::Watch => {
                self.heading = heading(self.position, c.position);
                moving = false;
            }
            Behavior::Avoid => {
                if distance > 500. {
                    if self.returning {
                        self.target = random.around(self.home, 800.)?;
                        self.blocked = 0;
                    }
                    self.returning = false;
                    moving = self.wander(800., random)?;
                } else {
                    self.returning = true;
                    if c.position.distance_to(self.home) > 500. {
                        moving = self.position.distance_to(self.home) >= speed;
                        if moving {
                            self.heading = heading(self.position, self.home);
                        } else {
                            self.position = self.home;
                            self.heading = heading(self.position, c.position);
                        }
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
            self.animation = if speed > 0.5 * base { 2 } else { 1 };
            if matches!(self.behavior, Behavior::Patrol | Behavior::Guard) {
                speed = speed.min(self.speed + 0.75);
            }
        } else {
            speed = 0.;
        }
        self.speed = speed;
        let motion = c
            .terrain
            .query(self.position, c.parameters.collision_radius)?
            .motion(speed, self.heading, collision::Mode::Ground)?;
        self.position = self.position.translated(motion.delta)?;
        let current = self.display_heading.to_degrees().trunc();
        let target = self.heading.to_degrees().trunc();
        let turn = (target - current).rem_euclid(360.);
        self.display_heading = if turn <= TURN_DEGREES || turn >= 360. - TURN_DEGREES {
            self.heading
        } else {
            (current
                + if turn > 180. {
                    -TURN_DEGREES
                } else {
                    TURN_DEGREES
                })
            .rem_euclid(360.)
            .to_radians()
        };
        Ok(motion.response.is_some())
    }
    fn wander(&mut self, radius: f32, random: &mut Random) -> Result<bool> {
        let distance = self.position.distance_to(self.target);
        self.heading = heading(self.position, self.target);
        if self.blocked != 0 || distance < ROAM_SPEED {
            self.target = random.around(self.home, radius)?;
            self.blocked = 0;
        }
        Ok(distance != 0.)
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

    fn symbol(position: Position, behavior: Behavior) -> Symbol {
        Symbol {
            position,
            variant: 0,
            heading: 0.,
            display_heading: 0.,
            speed: 0.,
            animation: 0,
            home: position,
            target: position,
            behavior,
            pause: 0,
            returning: false,
            blocked: 0,
        }
    }

    #[test]
    fn pursuit_turns_the_model_without_delaying_movement_or_run_animation() -> Result<()> {
        let ground = terrain(1, true);
        let locations = locations();
        let parameters = parameters();
        let c = context(&ground, &locations, &parameters);
        let mut enemy = symbol(c.position.translated([-200., 0., 0.])?, Behavior::Patrol);
        let start = enemy.position;
        let mut random = Random(7);
        enemy.advance(&c, &mut random)?;
        assert!(enemy.position.distance_to(c.position) < start.distance_to(c.position));
        assert_eq!(enemy.speed, 0.75);
        assert_eq!(enemy.animation, 2);
        assert!((enemy.heading.to_degrees() - 90.).abs() < 0.001);
        assert!((enemy.display_heading().to_degrees() - 22.).abs() < 0.001);
        for _ in 0..4 {
            enemy.advance(&c, &mut random)?;
        }
        assert!((enemy.display_heading() - enemy.heading).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn wandering_retargets_after_steering_and_patrol_resumes_on_pause_completion() -> Result<()> {
        let ground = terrain(1, true);
        let locations = locations();
        let parameters = parameters();
        let c = context(&ground, &locations, &parameters);
        let mut enemy = symbol(c.position.translated([600., 0., 0.])?, Behavior::Wander);
        let start = enemy.position;
        let mut random = Random(7);
        enemy.advance(&c, &mut random)?;
        assert_eq!(enemy.position, start);
        assert_ne!(enemy.target, start);
        assert_eq!(enemy.animation, 0);
        enemy.target = start.translated([3., 0., 0.])?;
        enemy.advance(&c, &mut random)?;
        assert!((start.displacement_to(enemy.position)[0] - ROAM_SPEED).abs() < 0.01);
        assert!(start.displacement_to(enemy.position)[1].abs() < 0.01);

        enemy.behavior = Behavior::Patrol;
        enemy.pause = 1;
        enemy.target = enemy.position.translated([100., 0., 0.])?;
        let start = enemy.position;
        enemy.advance(&c, &mut random)?;
        assert_eq!(enemy.pause, 0);
        assert!(enemy.position.map()[0] > start.map()[0]);
        assert_eq!(enemy.animation, 1);
        Ok(())
    }

    #[test]
    fn blocked_pursuers_give_up_after_three_seconds_and_keep_ground_height() -> Result<()> {
        let ground = terrain(1, true);
        let locations = locations();
        let parameters = parameters();
        let mut seed = 7;
        for behavior in [Behavior::Patrol, Behavior::Guard] {
            let player = context(&ground, &locations, &parameters).position;
            let position = player.translated([-200., 0., -20.])?;
            let mut enemies = Symbols {
                slots: [
                    Some(symbol(position, behavior)),
                    Some(symbol(
                        position.translated([100., 0., 20.])?,
                        Behavior::Watch,
                    )),
                    Some(symbol(
                        position.translated([0., 500., 20.])?,
                        Behavior::Watch,
                    )),
                ],
                ..Default::default()
            };
            for _ in 0..BLOCKED_PURSUIT_LIMIT - 1 {
                enemies.step(context(&ground, &locations, &parameters), &mut seed)?;
            }
            let blocked = enemies.get(0).unwrap();
            assert_eq!(blocked.behavior, behavior);
            assert_eq!(&blocked.position.map()[..2], &position.map()[..2]);
            assert_eq!(blocked.position.map()[2], 80.);
            enemies.step(context(&ground, &locations, &parameters), &mut seed)?;
            assert_eq!(enemies.get(0).unwrap().behavior, Behavior::Wander);
            enemies.step(context(&ground, &locations, &parameters), &mut seed)?;
            assert_eq!(enemies.get(0).unwrap().blocked, 1);
        }
        Ok(())
    }

    #[test]
    fn disabled_landmarks_still_block_enemy_spawning_and_movement() -> Result<()> {
        use resonance_content::overworld::Marker;

        let ground = terrain(1, true);
        let parameters = parameters();
        let mut data = definitions();
        data.worlds[0][0].position = [3200., 3200.];
        data.worlds[0][0].radius = 100.;
        let mut locations = Locations::new(
            Arc::new(data),
            Arc::default(),
            crate::overworld::scripts::fixture(),
        )?;
        let mut seed = 7;
        for marker in [Marker::FieldPoint, Marker::Unmodeled] {
            locations.appearances.get_mut(&1).unwrap().marker = marker;
            let player = context(&ground, &locations, &parameters).position;
            let start = player.translated([150., 0., 0.])?;
            let mut symbols = Symbols {
                slots: [Some(symbol(start, Behavior::Guard)), None, None],
                ..Default::default()
            };
            assert!(!symbols.clear_position(player, &context(&ground, &locations, &parameters)));
            symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
            assert_eq!(symbols.get(0).unwrap().position, start);

            locations.appearances.get_mut(&1).unwrap().marker = Marker::None;
            assert!(symbols.clear_position(player, &context(&ground, &locations, &parameters)));
            symbols.step(context(&ground, &locations, &parameters), &mut seed)?;
            assert!(symbols.get(0).unwrap().position.distance_to(player) < 150.);
        }
        Ok(())
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
