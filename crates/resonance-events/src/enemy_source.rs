//! Reusable enemy sources from native 0xC6. Removing the source actor retires its
//! controller; the already materialized enemy has an independent lifetime.
use crate::{Actor, GameWorld};

const MATERIALIZE_TICKS: u32 = 120;
const GROW_TICKS: u32 = 90;
const OPACITY: i32 = 8;
const SCALE: [i32; 3] = [30, 31, 32];
const TINT: [i32; 3] = [42, 43, 44];
const FULL_SCALE: u32 = 100;
const INITIAL_WIDTH: u32 = 10;
const INITIAL_HEIGHT: u32 = 300;
const FULL_TINT: u32 = 255;
const COLOR_STEP: u32 = 2;

#[derive(Debug, Clone)]
pub(crate) struct EnemySource {
    enemy_id: i32,
    prototype: Box<Actor>,
    delay: u32,
    remaining: Limit,
    phase: Phase,
}

#[derive(Debug, Clone)]
enum Limit {
    Unlimited,
    Remaining(u32),
}

#[derive(Debug, Clone)]
enum Phase {
    Waiting(u32),
    Materializing(u32),
}

impl EnemySource {
    pub fn new(enemy_id: i32, prototype: Actor, delay: i32, count: i32) -> Result<Self, String> {
        Ok(Self {
            enemy_id,
            prototype: Box::new(prototype),
            delay: u32::try_from(delay).map_err(|_| "negative enemy respawn delay")?,
            remaining: if count <= 0 {
                Limit::Unlimited
            } else {
                Limit::Remaining(count as u32)
            },
            // Spawn immediately; delay applies to respawns.
            phase: Phase::Waiting(0),
        })
    }
}

impl GameWorld {
    pub(crate) fn step_enemy_sources(&mut self) {
        // Creation order matches other actor controllers. New enemies start updating
        // on the following tick, without borrowing or cloning the whole actor pool.
        for id in self.actor_order.clone() {
            let Some(source) = self.actors.get(&id).and_then(|a| a.enemy_source.as_ref()) else {
                continue;
            };
            let occupied = self.actors.contains_key(&source.enemy_id);
            let actor = self.actors.get_mut(&id).unwrap();
            let source = actor.enemy_source.as_mut().unwrap();
            match &mut source.phase {
                Phase::Waiting(timer) => {
                    actor.visible = false;
                    if occupied {
                        *timer = source.delay;
                    } else if *timer > 0 {
                        *timer -= 1;
                    } else {
                        source.phase = Phase::Materializing(0);
                    }
                }
                Phase::Materializing(elapsed) => {
                    let grown = (*elapsed + 1).min(GROW_TICKS);
                    actor.visible = true;
                    actor
                        .properties
                        .insert(OPACITY, (grown * COLOR_STEP) as i32);
                    let width = INITIAL_WIDTH + grown;
                    let height =
                        INITIAL_HEIGHT - grown * (INITIAL_HEIGHT - FULL_SCALE) / GROW_TICKS;
                    for (axis, scale) in SCALE.into_iter().zip([width, width, height]) {
                        actor.properties.insert(axis, scale as i32);
                    }
                    let tint = FULL_TINT - grown * COLOR_STEP;
                    for channel in TINT {
                        actor.properties.insert(channel, tint as i32);
                    }
                    if *elapsed < MATERIALIZE_TICKS {
                        *elapsed += 1;
                        continue;
                    }
                    source.phase = Phase::Waiting(source.delay);
                    actor.visible = false;
                    // An event can replace the child during materialization. Never
                    // overwrite that actor or charge it against the spawn allowance.
                    if occupied {
                        continue;
                    }
                    let mut enemy = (*source.prototype).clone();
                    enemy.position = actor.position;
                    enemy.face(actor.heading);
                    if let Some(autonomy) = &mut enemy.autonomy {
                        autonomy.home = actor.position;
                    }
                    if let Some(animation) = &mut enemy.animation {
                        animation.seek(0., self.tick);
                    }
                    let enemy_id = source.enemy_id;
                    let exhausted = match &mut source.remaining {
                        Limit::Unlimited => false,
                        Limit::Remaining(count) => {
                            *count -= 1;
                            *count == 0
                        }
                    };
                    self.insert_actor(enemy_id, enemy);
                    if exhausted {
                        self.actors.remove(&id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: i32 = 100;
    const ENEMY: i32 = 101;
    const RESPAWN_DELAY: u32 = 5;

    fn world(count: i32) -> GameWorld {
        let mut world = GameWorld::default();
        let mut source = Actor::new(0, [0.; 3]);
        source.enemy_source = Some(
            EnemySource::new(ENEMY, Actor::new(7, [0.; 3]), RESPAWN_DELAY as i32, count).unwrap(),
        );
        world.insert_actor(SOURCE, source);
        world
    }
    fn step(world: &mut GameWorld, ticks: u32) {
        for _ in 0..ticks {
            world.step_enemy_sources();
        }
    }
    #[test]
    fn respawns_after_removal_and_delay_then_retires_at_limit() {
        let mut world = world(2);
        step(&mut world, MATERIALIZE_TICKS + 2);
        let first = world.actors[&ENEMY].instance;
        step(&mut world, MATERIALIZE_TICKS * 2);
        assert_eq!(world.actors[&ENEMY].instance, first);
        world.actors.remove(&ENEMY);
        world.actors.get_mut(&SOURCE).unwrap().position = [20., 30., 0.];
        step(&mut world, RESPAWN_DELAY + MATERIALIZE_TICKS + 1);
        assert!(!world.actors.contains_key(&ENEMY));
        step(&mut world, 1);
        assert_eq!(world.actors[&ENEMY].position, [20., 30., 0.]);
        assert!(!world.actors.contains_key(&SOURCE));
    }
    #[test]
    fn replacement_during_materialization_preserves_allowance_and_source_despawn_cancels() {
        let mut world = world(1);
        step(&mut world, 2);
        world.insert_actor(ENEMY, Actor::new(9, [0.; 3]));
        step(&mut world, MATERIALIZE_TICKS + 1);
        assert_eq!(world.actors[&ENEMY].resource, 9);
        assert!(world.actors.contains_key(&SOURCE));
        world.actors.remove(&ENEMY);
        step(&mut world, RESPAWN_DELAY + 2);
        world.actors.remove(&SOURCE);
        step(&mut world, MATERIALIZE_TICKS * 2);
        assert!(!world.actors.contains_key(&ENEMY));
    }
}
