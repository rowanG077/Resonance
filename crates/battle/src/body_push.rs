//! Bounded planar separation of overlapping actor bodies.
use crate::{ActorId, Battle};

/// Extra clearance after a moving body remains deeply overlapped.
const MOVING_BODY_CLEARANCE: f32 = 6.;

struct CollisionBody {
    points: Vec<([f32; 3], f32)>,
    half_height: f32,
    step: f32,
}

impl Battle {
    pub(crate) fn bypasses_body_collision(&self, actor: ActorId) -> bool {
        self.is_captured(actor) || self.runtime[actor.index()].body_bypass
    }

    pub(crate) fn push_bodies(
        &mut self,
        previous_positions: &[[f32; 3]],
        steps: &[f32],
        placements: &[Option<resonance_content::animation::Matrix>],
    ) {
        let mut bodies: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                let id = ActorId(index as u8);
                if !actor.available() || self.bypasses_body_collision(id) {
                    return None;
                }
                let before = previous_positions[index];
                let prepared = self.models.as_ref().and_then(|models| {
                    let placement = *placements.get(index)?.as_ref()?;
                    models.body_collision_points(id, placement)
                });
                let (points, half_height) = match prepared {
                    Some(points) => (points, 0.),
                    None => {
                        let collider = actor.body.collider?;
                        let center = [
                            before[0],
                            before[1] + collider.center_height * actor.body.scale,
                            before[2],
                        ];
                        (
                            vec![(center, actor.body_radius())],
                            collider.half_height * actor.body.scale,
                        )
                    }
                };
                Some(CollisionBody {
                    points,
                    half_height,
                    step: steps[index],
                })
            })
            .collect();
        // Each body marker resolves against the first opposing marker it touches.
        // Corrections affect the remaining marker visits in this same update.
        for moving in 0..self.actors.len() {
            self.push_body(moving, true, &mut bodies, previous_positions);
        }
    }

    fn push_body(
        &mut self,
        moving: usize,
        retry: bool,
        bodies: &mut [Option<CollisionBody>],
        previous_positions: &[[f32; 3]],
    ) {
        let leader = self.preferred_party_leader();
        let selected = leader.and_then(|actor| self.target(actor));
        let Some(body) = &bodies[moving] else {
            return;
        };
        let step = body.step;
        if step.abs() <= f32::EPSILON {
            return;
        }
        for other in 0..self.actors.len() {
            if moving == other || bodies[other].is_none() {
                continue;
            }
            let a = &self.actors[moving];
            let b = &self.actors[other];
            let sa = &a.movement.steering;
            let sb = &b.movement.steering;
            if sb.push_obstacle_disabled
                || a.side == b.side
                    && ((sa.passes_allied_obstacles && sb.passable_for_allies)
                        || (sb.passes_allied_obstacles && sa.passable_for_allies))
            {
                continue;
            }
            let priority = a.side == b.side
                && ((leader == Some(ActorId(moving as u8)) && a.control != crate::Control::Auto)
                    || selected == Some(ActorId(moving as u8)));
            let recipient = if priority { other } else { moving };
            if self.actors[recipient].movement.steering.push_immovable {
                continue;
            }
            let direction = planar_direction(
                self.actor_start_centers[moving],
                self.actor_start_centers[other],
            );
            for marker in 0..bodies[moving].as_ref().unwrap().points.len() {
                let body = bodies[moving].as_ref().unwrap();
                let Some(depth) = overlap(
                    body.points[marker],
                    body.half_height,
                    bodies[other].as_ref().unwrap(),
                ) else {
                    continue;
                };
                let mut correction = if step < 0.5 {
                    step
                } else if depth > f64::from(step) {
                    step + MOVING_BODY_CLEARANCE
                } else {
                    depth as f32
                };
                if self.actors[moving].movement.forward < 0. {
                    correction = -correction;
                }
                if recipient == moving {
                    correction = -correction;
                }
                let displacement = direction.map(|axis| axis * correction);
                self.actors[recipient].position[0] += displacement[0];
                self.actors[recipient].position[2] += displacement[1];
                // Keep separation geometry independent of arena-constrained roots.
                for (point, _) in &mut bodies[recipient].as_mut().unwrap().points {
                    point[0] += displacement[0];
                    point[2] += displacement[1];
                }
                self.constrain_actor(recipient, Some(previous_positions[recipient]));
                if retry
                    && self.actors[recipient].movement.steering.arena_contact()
                        != crate::ArenaContact::None
                {
                    // Give the other body one chance to resolve a blocked separation.
                    let opposite = if recipient == moving { other } else { moving };
                    self.push_body(opposite, false, bodies, previous_positions);
                }
            }
        }
    }
}

fn overlap(
    (point, radius): ([f32; 3], f32),
    half_height: f32,
    other: &CollisionBody,
) -> Option<f64> {
    other.points.iter().find_map(|&(center, other_radius)| {
        let reach = f64::from(radius) + f64::from(other_radius);
        let vertical = ((point[1] - center[1]).abs() - half_height - other.half_height).max(0.);
        // Preserve depth until the movement-clearance branch has been selected.
        let distance = (f64::from(center[0]) - f64::from(point[0]))
            .hypot(f64::from(center[2]) - f64::from(point[2]));
        (f64::from(vertical) <= reach && distance <= reach).then_some(reach - distance)
    })
}

fn planar_direction(a: [f32; 3], b: [f32; 3]) -> [f32; 2] {
    let delta = [b[0] - a[0], b[2] - a[2]];
    let distance = delta[0].hypot(delta[1]);
    if distance > f32::EPSILON {
        delta.map(|v| v / distance)
    } else {
        [1., 0.]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, ActorAvailability, BattleInput, Body, PreparedBattle, Side};

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
    fn completed_body_markers_correct_only_the_moving_actor() {
        use std::sync::Arc;
        for (step, root_lateral, marker_gap, marker_lateral, correction) in [
            (0_f32, 0_f32, 50., 0., 0.),
            (5., 0., 50., 0., 22.),
            (5., 4., 50., 0., 22.),
            (5., 0., -50., 0., 22.),
            (5., 0., 50., 30., 22.),
            (15., 0., 50., 0., 42.),
            (5., 0., 119., 0., 11.),
            (5., 0., 129., 0., 1.),
            (-5., 0., 50., 0., 10.),
        ] {
            let other = [250. + marker_gap, 0., 0.];
            let mut battle = battle(vec![
                body_actor(Side::Party, [0.; 3]),
                body_actor(Side::Enemy, other),
            ]);
            let mut moving = crate::tests::model_definition(1, [(0, 10.)]);
            moving.skeleton.bones[0].bind.translation = [250., marker_lateral, 90.];
            for radius in [60., 50.] {
                moving
                    .body_spheres
                    .push(crate::BodySphere { bone: 0, radius });
            }
            let mut fixed = crate::tests::model_definition(2, [(0, 10.)]);
            fixed.body_spheres.push(crate::BodySphere {
                bone: 0,
                radius: 70.,
            });
            battle.set_models(
                crate::Models::new(
                    battle.actors(),
                    vec![Some(Arc::new(moving)), Some(Arc::new(fixed))],
                    Default::default(),
                    resonance_content::diagnostics::Diagnostics::new(true),
                )
                .unwrap(),
            );
            let placements = battle.models.as_ref().unwrap().body_placements();
            let root_x = -(step * step - root_lateral * root_lateral).sqrt();
            battle.actors[0].position = [root_x, 0., root_lateral];
            battle.actors[0].movement.forward = step;
            battle.push_bodies(&[[0.; 3], other], &[step, 0.], &placements);
            assert_eq!(
                battle.actors[0].position,
                [root_x - correction, 0., root_lateral]
            );
            assert_eq!(battle.actors[1].position, other);
        }
        // An animated marker reaches a shallow overlap this update. Its owner
        // also moves, so using either the old pose or the new root gives a
        // different correction.
        let mut moving = crate::tests::model_definition(1, [(0, 10.)]);
        moving.initial.rate = 0.;
        for parent in [0, 1] {
            moving
                .skeleton
                .bones
                .push(resonance_content::animation::Bone {
                    name: format!("chain-{parent}"),
                    parent: Some(parent),
                    bind_channels: resonance_content::animation::TransformChannels(8),
                    bind: resonance_content::animation::Transform {
                        translation: [0., 0., 10.],
                        ..Default::default()
                    },
                });
        }
        moving
            .secondary_motion
            .push(resonance_content::secondary_motion::Chain {
                joints: [1, 2]
                    .into_iter()
                    .map(|node| resonance_content::secondary_motion::Joint {
                        node,
                        gravity: 0.,
                        damping: 0.,
                    })
                    .collect(),
                attraction: 1.,
                preserve_rotation: true,
                rotation_locks: [false; 2],
                collision_plane: None,
            });
        moving.body_spheres.push(crate::BodySphere {
            bone: 0,
            radius: 60.,
        });
        moving
            .motions
            .get_mut(&0)
            .unwrap()
            .tracks
            .push(resonance_content::animation::Track {
                bone: 0,
                bind_channels: resonance_content::animation::TransformChannels(8),
                period_frames: 10.,
                times: vec![0., 10.],
                translation: Some(resonance_content::animation::VectorCurve {
                    interpolation: resonance_content::animation::VectorInterpolation::Linear,
                    values: vec![[0.; 3], [140., 0., 0.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            });
        let mut fixed = crate::tests::model_definition(2, [(0, 10.)]);
        fixed.body_spheres.push(crate::BodySphere {
            bone: 0,
            radius: 70.,
        });
        let mut battle = battle(vec![
            body_actor(Side::Party, [0.; 3]),
            body_actor(Side::Enemy, [140., 0., 0.]),
        ]);
        battle.set_models(
            crate::Models::new(
                battle.actors(),
                vec![Some(Arc::new(moving)), Some(Arc::new(fixed))],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )
            .unwrap(),
        );
        battle.actors[0].movement.forward = 5.;
        battle.actors[0].movement.direction = [1., 0., 0.];
        battle
            .model_requests
            .push(crate::ModelRequest::AnimationRate {
                actor: ActorId(0),
                rate: 1.,
            });
        let mut frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[0].position, [1., 0., 0.]);
        battle
            .advance_models(&mut frame, crate::BattleClock::Running, true)
            .unwrap();
        assert_eq!(frame.models[0].frame, 1.);
        assert_eq!(frame.models[0].world[3][0], 1.);
        let chain_root = resonance_content::animation::transform_point(
            frame.models[0].world,
            resonance_content::animation::transform_point(frame.models[0].bones[1], [0.; 3]),
        );
        assert!((chain_root[0] - 15.).abs() < 0.0001);
    }

    #[test]
    fn separated_or_vertically_clear_bodies_stay_put() {
        for position in [[150., 0., 0.], [0., 150., 0.]] {
            let mut battle = battle(vec![
                body_actor(Side::Party, [0.; 3]),
                body_actor(Side::Enemy, position),
            ]);
            battle.push_bodies(&[[5., 0., 0.], position], &[5., 0.], &[]);
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
            battle.push_bodies(&[[5., 0., 0.], [45., 0., 0.]], &[5., 5.], &[]);
            assert_eq!(gap(&battle), 61.);
            if immovable {
                assert_eq!(battle.actors[0].position, [0.; 3]);
            } else {
                assert_eq!(battle.actors[1].position, [50., 0., 0.]);
            }
        }
        // Controlled fighters and their targets keep their line through allied bodies.
        for (side, player, selected, displaced) in [
            (Side::Party, 1, 0, 2),
            (Side::Enemy, 0, 1, 2),
            (Side::Party, 0, 0, 1),
        ] {
            for immovable in [false, true] {
                let positions = [[-1000., 0., 0.], [0.; 3], [50., 0., 0.]];
                let mut battle = battle(vec![
                    body_actor(Side::Party, positions[0]),
                    body_actor(side, positions[1]),
                    body_actor(side, positions[2]),
                ]);
                for actor in &mut battle.actors {
                    actor.control = crate::Control::Auto;
                }
                battle.actors[player].control = crate::Control::Manual;
                battle.runtime[player].target = ActorId(selected);
                battle.actors[1].movement.forward = 5.;
                battle.actors[displaced].movement.steering.push_immovable = immovable;
                battle.push_bodies(&positions, &[0., 5., 0.], &[]);
                let mut expected = positions;
                if !immovable {
                    expected[displaced][0] += if displaced == 2 { 11. } else { -11. };
                }
                assert_eq!(
                    battle
                        .actors
                        .iter()
                        .map(|actor| actor.position)
                        .collect::<Vec<_>>(),
                    expected
                );
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
            battle.push_bodies(&[[5., 0., 0.], [45., 0., 0.]], &[5., 5.], &[]);
            assert_eq!(battle.actors[0].position, [0.; 3]);
            assert_eq!(battle.actors[1].position, [50., 0., 0.]);
        }
    }

    #[test]
    fn moving_overlap_correction_respects_arena_boundary() {
        let mut battle = battle(vec![
            body_actor(Side::Party, [840., 0., 0.]),
            body_actor(Side::Enemy, [880., 0., 0.]),
        ]);
        battle.runtime[0].target = ActorId(1);
        battle.push_bodies(&[[845., 0., 0.], [875., 0., 0.]], &[5., 5.], &[]);
        assert!(battle.actors[0].position[0] < 840.);
        assert_eq!(battle.actors[1].position, [875., 0., 0.]);
        assert_eq!(
            battle.actors[1].movement.steering.arena_contact(),
            crate::ArenaContact::Blocked
        );
        for actor in &battle.actors {
            let boundary = if actor.side == Side::Party {
                850.
            } else {
                885.
            };
            assert!(actor.position[0].hypot(actor.position[2]) <= boundary);
        }
        // An arena-blocked correction still separates the collision markers.
        // Deep overlaps also give the other body one bounded follow-up visit.
        for (enemy_x, resolved_x) in [(-742., -742.), (-810., -790.)] {
            let positions = [[-860., 0., 0.], [enemy_x, 0., 0.]];
            let mut blocked = self::battle(vec![
                body_actor(Side::Party, positions[0]),
                body_actor(Side::Enemy, positions[1]),
            ]);
            blocked.runtime[0].target = ActorId(1);
            blocked.actors[0].movement.forward = 8.;
            blocked.actors[1].movement.forward = 4.;
            blocked.push_bodies(&positions, &[8., 4.], &[]);
            assert_eq!(blocked.actors[0].position, positions[0]);
            assert_eq!(blocked.actors[1].position, [resolved_x, 0., 0.]);
        }
    }

    #[test]
    fn body_pass_through_survives_recovery_until_expiry_or_interruption() {
        for (duration, interrupt) in [(0, false), (4, false), (4, true)] {
            let mut prepared = crate::PreparedBattle::new(
                vec![
                    (body_actor(Side::Party, [0.; 3]), Default::default()),
                    (body_actor(Side::Enemy, [50., 0., 0.]), Default::default()),
                ],
                (vec![crate::ActionDefinition {
                    normal: None,
                    tp_cost: 0,
                    execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                        events: vec![(
                            0,
                            crate::AttackEvent::PassThrough {
                                enabled: true,
                                duration,
                            },
                        )],
                        ..crate::PreparedAttack::new(1)
                    }),
                }])
                .into(),
                1,
            )
            .unwrap();
            crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
            let mut battle = prepared.finish().unwrap();
            battle
                .step(BattleInput::from(crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: crate::ActionKey(0),
                }))
                .unwrap();
            assert_eq!(gap(&battle), 50.);
            assert!(battle.bypasses_body_collision(ActorId(0)));
            if interrupt {
                battle.interrupt_actor(ActorId(0), &mut Vec::new());
                assert!(!battle.bypasses_body_collision(ActorId(0)));
                continue;
            }
            // The admission update already consumes the first actor timer tick.
            for _ in 0..2 {
                battle.step(BattleInput::default()).unwrap();
            }
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
            assert!(battle.bypasses_body_collision(ActorId(0)));
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
            assert_eq!(battle.bypasses_body_collision(ActorId(0)), duration == 0);
            if duration == 0 {
                battle.step(BattleInput::default()).unwrap();
                assert!(battle.bypasses_body_collision(ActorId(0)));
                battle.interrupt_actor(ActorId(0), &mut Vec::new());
                assert!(!battle.bypasses_body_collision(ActorId(0)));
            }
            let previous = [battle.actors[0].position, battle.actors[1].position];
            battle.actors[0].position[0] -= 5.;
            battle.push_bodies(&previous, &[5., 0.], &[]);
            assert_eq!(gap(&battle), 66.);
        }
    }

    #[test]
    fn entry_and_stationary_combat_preserve_formation() {
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
        // Combat begins while entry settling still holds ordinary actor visits.
        for _ in 0..60 {
            if battle.clock == crate::BattleClock::Running {
                break;
            }
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(gap(&battle), 50.);
        }
        assert_eq!(battle.clock, crate::BattleClock::Running);

        let moving = ActorId(0);
        battle.begin_hurt(moving, 100, &mut Vec::new());
        battle.actors[0].reaction.direction = [-1., 0., 0.];
        battle.actors[0].movement.forward = 2.;
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(gap(&battle), 60.);
        assert_eq!(battle.actors[0].movement.forward_step, 2.);

        // A new recoil delays integration while separation retains the previous step.
        let owner = &mut battle.actors[0];
        owner.reaction.recoil.start(
            &mut owner.movement,
            crate::RecoilRule {
                impulse: [4., 0.],
                delay: 3,
                ..Default::default()
            },
            Default::default(),
            false,
            false,
        );
        battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(gap(&battle), 60.);
        assert_eq!(battle.actors[0].reaction.recoil.delay, 3);
        assert_eq!(battle.actors[0].movement.forward_step, 2.);
        for (delay, distance) in [(2, 68.), (1, 76.)] {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(moving), crate::Activity::Hurt);
            assert_eq!(battle.actors[0].reaction.recoil.delay, delay);
            assert_eq!(battle.actors[0].movement.forward, 0.);
            assert_eq!(battle.actors[0].movement.forward_step, 2.);
            assert_eq!(gap(&battle), distance);
        }
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[0].reaction.recoil.delay, 0);
        assert_eq!(battle.actors[0].movement.forward_step, 4.);
        assert_eq!(gap(&battle), 90.);
        assert_eq!(battle.actors[1].position, [50., 0., 0.]);

        // Stationary integration replaces the retained step and stops separation.
        battle.enter_idle(moving);
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.activity(moving), crate::Activity::Idle);
        assert_eq!(battle.actors[0].movement.forward_step, 0.);
        assert_eq!(gap(&battle), 90.);
    }
}
