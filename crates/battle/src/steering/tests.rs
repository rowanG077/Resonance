use super::*;
use crate::tests::{actor, prepared};

fn battle(mut actors: Vec<Actor>) -> Battle {
    actors[0].control = Control::Auto;
    let mut prepared = prepared(actors, 1);
    prepared.resources.arena_boundary = true;
    prepared.finish().unwrap()
}

fn at(side: Side, x: f32, z: f32) -> Actor {
    let mut actor = actor(side);
    actor.position = [x, 0., z];

    actor.body.collider = Some(crate::Collider::sphere(25.));
    actor
}

#[test]
fn avoidance_revisits_blocking_bodies_in_roster_order_and_excludes_the_target() -> Result<()> {
    let mut battle = battle(vec![
        at(Side::Party, -600., 0.),
        at(Side::Party, -200., 0.),
        at(Side::Party, -400., 0.),
        at(Side::Enemy, 300., 0.),
    ]);
    let destination = battle.actors[3].position;
    let waypoint = battle.steer_approach(ActorId(0), ActorId(3), destination)?;
    assert!((waypoint[0] + 621.14624).abs() < 0.001);
    assert!((waypoint[2] - 202.71739).abs() < 0.001);
    battle.actors.swap(1, 2);
    let reordered = battle.steer_approach(ActorId(0), ActorId(3), destination)?;
    assert!((reordered[0] + 649.6151).abs() < 0.001);
    assert!((reordered[2] - 166.41006).abs() < 0.001);
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    battle.actors[2].movement.fixed_height = true;
    assert_eq!(
        battle.steer_approach(ActorId(0), ActorId(3), destination)?,
        destination
    );
    Ok(())
}

#[test]
fn central_retreat_routes_away_from_an_obstacle_with_fixed_lateral_clearance() -> Result<()> {
    let mut battle = battle(vec![
        at(Side::Party, 200., -150.),
        at(Side::Enemy, 300., -200.),
        at(Side::Enemy, 400., -225.),
    ]);
    let owner = ActorId(1);
    battle.actors[1].control = Control::Enemy;
    battle.begin_approach_steering(owner, ActorId(0))?;
    assert_eq!(
        battle.actors[1].movement.steering.side,
        DetourSide::Automatic
    );
    let direction = distance::planar_direction(
        battle.actors[1].position,
        battle.actors[0].position,
        [1., 0., 0.],
    );
    let destination =
        std::array::from_fn(|axis| battle.actors[1].position[axis] + direction[axis] * 200.);
    let waypoint = battle.steer_approach(owner, ActorId(0), destination)?;
    assert!((waypoint[0] - 265.83594).abs() < 0.001);
    assert!((waypoint[2] + 493.32816).abs() < 0.001);
    // Navigation uses its own corridor, independent of contact-sphere size.
    battle.actors[1].body.collider = Some(crate::Collider::sphere(200.));
    battle.actors[2].body.collider = Some(crate::Collider::sphere(1.));
    assert_eq!(
        battle.steer_approach(owner, ActorId(0), destination)?,
        waypoint
    );
    Ok(())
}

#[test]
fn control_modes_and_mutual_allied_passage_change_avoidance() -> Result<()> {
    let mut battle = battle(vec![
        at(Side::Party, -600., 0.),
        at(Side::Party, -300., 0.),
        at(Side::Enemy, 300., 0.),
    ]);
    let destination = battle.actors[2].position;
    assert_ne!(
        battle.steer_approach(ActorId(0), ActorId(2), destination)?,
        destination
    );
    battle.actors[0].movement.steering.passes_allied_obstacles = true;
    battle.actors[1].movement.steering.passable_for_allies = true;
    assert_eq!(
        battle.steer_approach(ActorId(0), ActorId(2), destination)?,
        destination
    );
    battle.actors[1].movement.steering.passable_for_allies = false;
    for control in [Control::Manual, Control::SemiAuto] {
        battle.actors[0].control = control;
        assert_eq!(
            battle.steer_approach(ActorId(0), ActorId(2), destination)?,
            destination
        );
    }
    Ok(())
}

#[test]
fn coincident_destination_needs_no_detour_and_edge_routes_choose_inward() -> Result<()> {
    let mut battle = battle(vec![
        at(Side::Party, 700., 50.),
        at(Side::Party, 600., 50.),
        at(Side::Enemy, 700., 50.),
    ]);
    battle.begin_approach_steering(ActorId(0), ActorId(2))?;
    assert_eq!(
        battle.steer_approach(ActorId(0), ActorId(2), [700., 0., 50.])?,
        [700., 0., 50.]
    );
    battle.actors[2].position = [0., 0., 50.];
    battle.begin_approach_steering(ActorId(0), ActorId(2))?;
    let waypoint = battle.steer_approach(ActorId(0), ActorId(2), battle.actors[2].position)?;
    assert!(waypoint[2] < 50.);
    Ok(())
}

#[test]
fn arena_boundary_rejects_or_slides_steps_and_preserves_vertical_motion() {
    for side in [Side::Party, Side::Enemy] {
        for height in [0., 0.1, 50.] {
            let mut battle = battle(vec![
                at(Side::Party, 0., 0.),
                at(side, 900., 0.),
                at(Side::Enemy, 100., 0.),
            ]);
            battle.actors[0].control = Control::Manual;
            battle.runtime[0].target = ActorId(2);
            battle.actors[1].position[1] = height;
            battle.actors[1].movement.vertical = 7.;
            let boundary = if side == Side::Party { 850. } else { 885. };
            let before = [800., 12., 0.];
            for forced in [false, true] {
                battle.actors[1].position[0] = 900.;
                battle.constrain_actor(1, (!forced).then_some(before));
                let actor = &battle.actors[1];
                let blocked = height == 50. && !forced;
                assert_eq!(
                    actor.movement.steering.arena_contact(),
                    if blocked {
                        ArenaContact::Blocked
                    } else {
                        ArenaContact::Slid
                    }
                );
                assert!(
                    (actor.position[0] - if blocked { before[0] } else { boundary }).abs() < 0.001
                );
                assert_eq!(actor.position[1], height);
                assert_eq!(actor.movement.vertical, 7.);
            }
            // The selected opponent keeps its position even on the ground.
            battle.runtime[0].target = ActorId(1);
            battle.actors[1].position[0] = 900.;
            battle.constrain_actor(1, Some(before));
            assert_eq!(battle.actors[1].position, [before[0], height, before[2]]);
            assert_eq!(
                battle.actors[1].movement.steering.arena_contact(),
                ArenaContact::Blocked
            );
        }
    }
    // A central target line blocks the leader; an outer line permits sliding.
    for line_z in [0., 552.5, 600.] {
        let mut battle = battle(vec![
            at(Side::Party, 700., line_z),
            at(Side::Enemy, -100., line_z),
        ]);
        battle.runtime[0].target = ActorId(1);
        let before = [500., 0., line_z];
        battle.actors[0].position[0] = 900.;
        battle.constrain_actor(0, Some(before));
        if line_z == 0. {
            assert_eq!(battle.actors[0].position, before);
            assert_eq!(
                battle.actors[0].movement.steering.arena_contact(),
                ArenaContact::Blocked
            );
        } else {
            assert!((planar_length(battle.actors[0].position) - 850.).abs() < 0.001);
            assert_eq!(
                battle.actors[0].movement.steering.arena_contact(),
                ArenaContact::Slid
            );
        }
    }
}

#[test]
fn escape_and_explicit_exemptions_skip_arena_clamping_but_keep_floor() {
    let mut battle = battle(vec![at(Side::Party, 900., 0.), at(Side::Enemy, 900., 0.)]);
    battle.actors[0].movement.steering.unrestricted_arena = true;
    battle.actors[0].position[1] = -1.;
    battle.constrain_actor(0, Some([800., 0., 0.]));
    assert_eq!(battle.actors[0].position, [900., 0., 0.]);
    battle.actors[1].movement.steering.leaving_arena = true;
    battle.constrain_actor(1, Some([800., 0., 0.]));
    assert_eq!(battle.actors[1].position[0], 900.);
    battle.actors[1].movement.steering.leaving_arena = false;
    battle.terminal.result = Some(BattleResult::Escaped);
    battle.constrain_actor(1, Some([800., 0., 0.]));
    assert_eq!(battle.actors[1].position[0], 900.);
}
