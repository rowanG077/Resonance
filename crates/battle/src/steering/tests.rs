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
fn avoidance_uses_nearest_blocking_body_and_excludes_the_target() -> Result<()> {
    let mut battle = battle(vec![
        at(Side::Party, -600., 0.),
        at(Side::Party, -200., 0.),
        at(Side::Party, -400., 0.),
        at(Side::Enemy, 300., 0.),
    ]);
    let destination = battle.actors[3].position;
    let waypoint = battle.steer_approach(ActorId(0), ActorId(3), destination)?;
    assert!(waypoint[0] < -400. && waypoint[2].abs() >= 50.);
    battle.actors.swap(1, 2);
    assert_eq!(
        battle.steer_approach(ActorId(0), ActorId(3), destination)?,
        waypoint
    );
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    battle.actors[2].movement.fixed_height = true;
    assert_eq!(
        battle.steer_approach(ActorId(0), ActorId(3), destination)?,
        destination
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
fn arena_clamp_preserves_height_and_velocity_for_both_sides() {
    for side in [Side::Party, Side::Enemy] {
        for height in [0., 50.] {
            let mut actor = at(side, 900., 0.);
            actor.position[1] = height;
            actor.movement.vertical = 7.;
            constrain(&mut actor);
            assert_eq!(actor.movement.steering.arena_contact(), ArenaContact::Slid);
            assert!((actor.position[0] + actor.body_radius() - ARENA_RADIUS).abs() < 0.001);
            assert_eq!(actor.position[1], height);
            assert_eq!(actor.movement.vertical, 7.);
        }
    }
}

#[test]
fn escape_and_explicit_exemptions_skip_arena_clamping_but_keep_floor() {
    let mut battle = battle(vec![at(Side::Party, 900., 0.), at(Side::Enemy, 900., 0.)]);
    battle.actors[0].movement.steering.unrestricted_arena = true;
    battle.actors[0].position[1] = -1.;
    battle.constrain_actor(0);
    assert_eq!(battle.actors[0].position, [900., 0., 0.]);
    battle.actors[1].movement.steering.leaving_arena = true;
    battle.constrain_actor(1);
    assert_eq!(battle.actors[1].position[0], 900.);
    battle.actors[1].movement.steering.leaving_arena = false;
    battle.terminal.result = Some(BattleResult::Escaped);
    battle.constrain_actor(1);
    assert_eq!(battle.actors[1].position[0], 900.);
}
