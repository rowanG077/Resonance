//! Bounded planar separation of overlapping actor bodies.
use crate::{Actor, ActorId, Battle};

// A fixed budget also bounds work when a crowd cannot fit against the arena edge.
const MAX_SEPARATION_PASSES: usize = 8;

impl Battle {
    pub(crate) fn bypasses_body_collision(&self, actor: ActorId) -> bool {
        self.runtime[actor.index()]
            .task()
            .action()
            .is_some_and(|(_, sequence)| sequence.collision_bypass)
    }

    pub(crate) fn push_bodies(&mut self) {
        let collidable: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                actor.available() && !self.bypasses_body_collision(ActorId(index as u8))
            })
            .collect();
        for _ in 0..MAX_SEPARATION_PASSES {
            let mut moved = false;
            for first in 0..self.actors.len() {
                for second in first + 1..self.actors.len() {
                    if !collidable[first] || !collidable[second] {
                        continue;
                    }
                    let a = &self.actors[first];
                    let b = &self.actors[second];
                    let sa = &a.movement.steering;
                    let sb = &b.movement.steering;
                    if a.side == b.side
                        && ((sa.passes_allied_obstacles && sb.passable_for_allies)
                            || (sb.passes_allied_obstacles && sa.passable_for_allies))
                    {
                        continue;
                    }
                    let move_a = u8::from(!sa.push_immovable && !sb.push_obstacle_disabled) as f32;
                    let move_b = u8::from(!sb.push_immovable && !sa.push_obstacle_disabled) as f32;
                    let weight = move_a + move_b;
                    if weight == 0. {
                        continue;
                    }
                    let separation = separation(a, b);
                    if separation == [0.; 2] {
                        continue;
                    }
                    for (index, share) in [(first, -move_a / weight), (second, move_b / weight)] {
                        if share == 0. {
                            continue;
                        }
                        let before = self.actors[index].position;
                        self.actors[index].position[0] += separation[0] * share;
                        self.actors[index].position[2] += separation[1] * share;
                        self.constrain_actor(index);
                        moved |= self.actors[index].position != before;
                    }
                }
            }
            if !moved {
                break;
            }
        }
    }
}

fn separation(a: &Actor, b: &Actor) -> [f32; 2] {
    let (Some(ca), Some(cb)) = (a.body.collider, b.body.collider) else {
        return [0.; 2];
    };
    let reach = a.body_radius() + b.body_radius();
    let center_a = a.position[1] + ca.center_height * a.body.scale;
    let center_b = b.position[1] + cb.center_height * b.body.scale;
    let vertical = ((center_b - center_a).abs()
        - ca.half_height * a.body.scale
        - cb.half_height * b.body.scale)
        .max(0.);
    if vertical >= reach {
        return [0.; 2];
    }
    let delta = [b.position[0] - a.position[0], b.position[2] - a.position[2]];
    let distance = delta[0].hypot(delta[1]);
    let depth = ((reach * reach - vertical * vertical).sqrt() - distance).max(0.);
    let direction = if distance > f32::EPSILON {
        delta.map(|v| v / distance)
    } else {
        [1., 0.]
    };
    direction.map(|v| v * depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorAvailability, BattleInput, Body, PreparedBattle, Side};

    fn body_actor(side: Side, position: [f32; 3]) -> Actor {
        let mut actor = crate::tests::actor(side);
        actor.position = position;
        actor.body = Body {
            collider: Some(crate::Collider::sphere(60.)),
            ..Default::default()
        };
        actor
    }

    fn battle(actors: Vec<Actor>) -> Battle {
        PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            Default::default(),
            1,
        )
        .unwrap()
        .with_arena_boundary()
        .finish()
        .unwrap()
    }

    fn gap(battle: &Battle) -> f32 {
        let a = battle.actors[0].position;
        let b = battle.actors[1].position;
        (b[0] - a[0]).hypot(b[2] - a[2])
    }

    #[test]
    fn stationary_and_coincident_bodies_separate_on_first_combat_update() {
        for offset in [0., 50.] {
            let mut battle = battle(vec![
                body_actor(Side::Party, [0.; 3]),
                body_actor(Side::Enemy, [offset, 0., 0.]),
            ]);
            battle.push_bodies();
            assert!(gap(&battle) >= 119.99);
            assert!(battle.actors.iter().all(|actor| actor.position[1] == 0.));
        }
    }

    #[test]
    fn separated_or_vertically_clear_bodies_stay_put() {
        for position in [[150., 0., 0.], [0., 150., 0.]] {
            let mut battle = battle(vec![
                body_actor(Side::Party, [0.; 3]),
                body_actor(Side::Enemy, position),
            ]);
            battle.push_bodies();
            assert_eq!(battle.actors[0].position, [0.; 3]);
            assert_eq!(battle.actors[1].position, position);
        }
    }

    #[test]
    fn immovable_and_non_obstacle_traits_assign_the_correction() {
        for immovable in [true, false] {
            let mut fixed = body_actor(Side::Party, [0.; 3]);
            fixed.movement.steering.push_immovable = immovable;
            fixed.movement.steering.push_obstacle_disabled = !immovable;
            let mut battle = battle(vec![fixed, body_actor(Side::Enemy, [50., 0., 0.])]);
            battle.push_bodies();
            assert!(gap(&battle) >= 119.99);
            if immovable {
                assert_eq!(battle.actors[0].position, [0.; 3]);
            } else {
                assert_eq!(battle.actors[1].position, [50., 0., 0.]);
            }
        }
    }

    #[test]
    fn passable_allies_and_unavailable_bodies_do_not_separate() {
        for case in 0..4 {
            let mut first = body_actor(Side::Party, [0.; 3]);
            let mut second = body_actor(Side::Party, [50., 0., 0.]);
            match case {
                0 => {
                    first.movement.steering.passes_allied_obstacles = true;
                    second.movement.steering.passable_for_allies = true;
                }
                1 => second.availability = ActorAvailability::Petrified,
                2 => second.availability = ActorAvailability::Dead,
                _ => {
                    first.movement.steering.push_immovable = true;
                    second.movement.steering.push_immovable = true;
                }
            }
            let mut battle = battle(vec![first, second]);
            battle.push_bodies();
            assert_eq!(battle.actors[0].position, [0.; 3]);
            assert_eq!(battle.actors[1].position, [50., 0., 0.]);
        }
    }

    #[test]
    fn arena_clamping_keeps_bodies_inside_while_resolving_overlap() {
        let mut battle = battle(vec![
            body_actor(Side::Party, [750., 0., 0.]),
            body_actor(Side::Enemy, [780., 0., 0.]),
        ]);
        battle.push_bodies();
        assert!(gap(&battle) >= 119.);
        for actor in &battle.actors {
            assert!(actor.position[0].hypot(actor.position[2]) <= 790.);
        }
    }

    #[test]
    fn completing_an_action_restores_collision_without_an_explicit_clear() {
        let mut prepared = crate::PreparedBattle::new(
            vec![
                (body_actor(Side::Party, [0.; 3]), Default::default()),
                (body_actor(Side::Enemy, [50., 0., 0.]), Default::default()),
            ],
            (vec![crate::ActionDefinition {
                normal: None,
                tp_cost: 0,
                execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                    opening: None,
                    chain_at: None,
                    end_at: 2,
                    recovery: 0,
                    events: vec![(0, crate::AttackEvent::PassThrough(true))],
                }),
            }])
            .into(),
            1,
        )
        .unwrap();
        crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
        let mut battle = prepared.finish().unwrap();
        battle
            .step(BattleInput {
                actions: vec![crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: crate::ActionKey(0),
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(gap(&battle), 50.);
        for _ in 0..3 {
            battle.step(BattleInput::default()).unwrap();
        }
        assert!(!battle.bypasses_body_collision(ActorId(0)));
        assert!(gap(&battle) >= 119.99);
    }

    #[test]
    fn entry_preserves_formation_and_waits_for_unpaused_updates_without_a_camera() {
        let mut battle = crate::PreparedBattle::new(
            vec![
                (body_actor(Side::Party, [0.; 3]), Default::default()),
                (body_actor(Side::Enemy, [50., 0., 0.]), Default::default()),
            ],
            Default::default(),
            1,
        )
        .unwrap()
        .with_entry()
        .finish()
        .unwrap();
        assert!(battle.camera.is_none());
        for _ in 0..60 {
            battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(battle.phase(), crate::BattlePhase::Entry);
            assert_eq!(gap(&battle), 50.);
        }
        for _ in 0..60 {
            if battle.phase() == crate::BattlePhase::Combat {
                break;
            }
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(gap(&battle), 50.);
        }
        assert_eq!(battle.phase(), crate::BattlePhase::Combat);
        battle.step(BattleInput::default()).unwrap();
        assert!(gap(&battle) >= 119.99);
    }
}
