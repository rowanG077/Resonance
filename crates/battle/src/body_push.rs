//! Bounded planar separation of overlapping actor bodies.
use crate::{Actor, ActorId, Battle};

/// Extra clearance after a moving body remains deeply overlapped.
const MOVING_BODY_CLEARANCE: f32 = 6.;

struct CollisionBody {
    points: Vec<([f32; 3], f32)>,
    half_height: f32,
    step: f32,
}

impl Battle {
    pub(crate) fn bypasses_body_collision(&self, actor: ActorId) -> bool {
        self.is_captured(actor)
            || self.runtime[actor.index()].body_bypass_ticks != 0
            || self.runtime[actor.index()]
                .task()
                .action()
                .is_some_and(|(_, sequence)| sequence.collision_bypass)
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
            let Some(body) = &bodies[moving] else {
                continue;
            };
            let step = body.step;
            if step <= f32::EPSILON || self.actors[moving].movement.steering.push_immovable {
                continue;
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
                let direction = planar_direction(a, b);
                for marker in 0..bodies[moving].as_ref().unwrap().points.len() {
                    let body = bodies[moving].as_ref().unwrap();
                    let Some(depth) = overlap(
                        body.points[marker],
                        body.half_height,
                        bodies[other].as_ref().unwrap(),
                    ) else {
                        continue;
                    };
                    let correction = if step < 0.5 {
                        step
                    } else if depth > step {
                        step + MOVING_BODY_CLEARANCE
                    } else {
                        depth
                    };
                    let before = self.actors[moving].position;
                    self.actors[moving].position[0] -= direction[0] * correction;
                    self.actors[moving].position[2] -= direction[1] * correction;
                    self.constrain_actor(moving);
                    let after = self.actors[moving].position;
                    for (point, _) in &mut bodies[moving].as_mut().unwrap().points {
                        point[0] += after[0] - before[0];
                        point[2] += after[2] - before[2];
                    }
                }
            }
        }
    }
}

fn overlap(
    (point, radius): ([f32; 3], f32),
    half_height: f32,
    other: &CollisionBody,
) -> Option<f32> {
    other.points.iter().find_map(|&(center, other_radius)| {
        let reach = radius + other_radius;
        let vertical = ((point[1] - center[1]).abs() - half_height - other.half_height).max(0.);
        let distance = (center[0] - point[0]).hypot(center[2] - point[2]);
        (vertical <= reach && distance <= reach).then_some(reach - distance)
    })
}

fn planar_direction(a: &Actor, b: &Actor) -> [f32; 2] {
    let delta = [b.position[0] - a.position[0], b.position[2] - a.position[2]];
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
    fn completed_body_markers_correct_only_the_moving_actor() {
        use std::sync::Arc;
        for (step, marker_gap, marker_lateral, correction) in [
            (0., 50., 0., 0.),
            (5., 50., 0., 22.),
            (5., -50., 0., 22.),
            (5., 50., 30., 22.),
            (15., 50., 0., 42.),
            (5., 119., 0., 11.),
            (5., 129., 0., 1.),
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
            battle.actors[0].position[0] = -step;
            battle.push_bodies(&[[0.; 3], other], &[step, 0.], &placements);
            assert_eq!(battle.actors[0].position, [-step - correction, 0., 0.]);
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
            body_actor(Side::Party, [750., 0., 0.]),
            body_actor(Side::Enemy, [780., 0., 0.]),
        ]);
        battle.push_bodies(&[[755., 0., 0.], [775., 0., 0.]], &[5., 5.], &[]);
        assert!(battle.actors[0].position[0] < 750.);
        assert!(battle.actors[1].position[0] > 780.);
        for actor in &battle.actors {
            let boundary = if actor.side == Side::Party {
                850.
            } else {
                885.
            };
            assert!(actor.position[0].hypot(actor.position[2]) <= boundary);
        }
    }

    #[test]
    fn body_pass_through_expires_with_action_or_timer() {
        for duration in [0, 4] {
            let mut prepared = crate::PreparedBattle::new(
                vec![
                    (body_actor(Side::Party, [0.; 3]), Default::default()),
                    (body_actor(Side::Enemy, [50., 0., 0.]), Default::default()),
                ],
                (vec![crate::ActionDefinition {
                    normal: None,
                    tp_cost: 0,
                    execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                        contact_recovery: None,
                        opening: None,
                        chain_at: None,
                        end_at: 1,
                        recovery: 0,
                        events: vec![(
                            0,
                            crate::AttackEvent::PassThrough {
                                enabled: true,
                                duration,
                            },
                        )],
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
            // The admission update already consumes the first actor timer tick.
            for _ in 0..2 {
                battle.step(BattleInput::default()).unwrap();
            }
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
            assert_eq!(battle.bypasses_body_collision(ActorId(0)), duration != 0);
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
            assert!(!battle.bypasses_body_collision(ActorId(0)));
            let previous = [battle.actors[0].position, battle.actors[1].position];
            battle.actors[0].position[0] -= 5.;
            battle.push_bodies(&previous, &[5., 0.], &[]);
            assert_eq!(gap(&battle), 66.);
        }
    }

    #[test]
    fn entry_and_stationary_combat_preserve_formation() {
        let mut outside = battle(vec![
            body_actor(Side::Party, [1800., 0., 0.]),
            body_actor(Side::Party, [1800., 0., 0.]),
            body_actor(Side::Enemy, [1800., 0., 0.]),
        ]);
        for _ in 0..2 {
            outside.step(BattleInput::default()).unwrap();
            assert_eq!(outside.actors[0].position, [850., 0., 0.]);
            assert_eq!(outside.actors[1].position, [850., 0., 0.]);
            assert_eq!(outside.actors[2].position, [885., 0., 0.]);
        }
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
        assert_eq!(gap(&battle), 50.);
    }
}
