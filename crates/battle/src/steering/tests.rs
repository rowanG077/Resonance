use super::*;
use crate::tests::{actor, prepared};
use std::sync::Arc;

fn battle(mut actors: Vec<Actor>) -> Battle {
    actors[0].control = Control::Auto;
    let mut prepared = prepared("pub task run() { battle::finish(); }", actors, 1);
    Arc::get_mut(&mut prepared).unwrap().arena_boundary = true;
    Battle::new(prepared)
}

fn at(side: Side, x: f32, z: f32) -> Actor {
    let mut actor = actor(side);
    actor.position = [x, 0., z];
    actor
}

#[test]
fn avoidance_uses_roster_order_and_excludes_the_actual_target() {
    let mut battle = battle(vec![
        at(Side::Party, -600., 0.),
        at(Side::Party, -300., 0.),
        at(Side::Party, -200., 0.),
        at(Side::Enemy, 300., 0.),
    ]);
    let destination = battle.actors[3].position;
    let detour = battle
        .steering_detour(ActorId(0), ActorId(3), destination)
        .unwrap()
        .unwrap();
    assert_eq!(detour[0], -300.);
    assert!((detour[2] - 300.).abs() < 0.001);
    battle.actors.swap(1, 2);
    assert_eq!(
        battle
            .steering_detour(ActorId(0), ActorId(3), destination)
            .unwrap()
            .unwrap()[0],
        -200.
    );
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    battle.actors[2].movement.fixed_height = true;
    assert_eq!(
        battle
            .steer_approach(ActorId(0), ActorId(3), destination)
            .unwrap(),
        destination
    );
}

#[test]
fn control_modes_and_mutual_allied_passage_change_the_obstacle_scan() {
    let mut battle = battle(vec![
        at(Side::Party, -600., 0.),
        at(Side::Party, -300., 0.),
        at(Side::Enemy, 300., 0.),
        at(Side::Enemy, -100., 0.),
    ]);
    let destination = battle.actors[2].position;
    battle.actors[0].control = Control::SemiAuto;
    let detour = battle
        .steering_detour(ActorId(0), ActorId(2), destination)
        .unwrap()
        .unwrap();
    assert_eq!(detour[0], -100.);
    battle.actors[0].control = Control::Manual;
    assert_eq!(
        battle
            .steer_approach(ActorId(0), ActorId(2), destination)
            .unwrap(),
        destination
    );
    battle.actors[0].control = Control::Auto;
    battle.actors[0].movement.steering.passes_allied_obstacles = true;
    assert_eq!(
        battle
            .steering_detour(ActorId(0), ActorId(2), destination)
            .unwrap()
            .unwrap()[0],
        -300.
    );
    battle.actors[1].movement.steering.passable_for_allies = true;
    assert_eq!(
        battle
            .steering_detour(ActorId(0), ActorId(2), destination)
            .unwrap()
            .unwrap()[0],
        -100.
    );
}

#[test]
fn edge_detour_side_is_retained_and_semi_auto_clears_it() {
    let mut battle = battle(vec![at(Side::Party, 700., 50.), at(Side::Enemy, 0., 50.)]);
    assert_eq!(
        battle.actors[0].movement.steering.side,
        DetourSide::Automatic
    );
    battle
        .begin_approach_steering(ActorId(0), ActorId(1))
        .unwrap();
    assert_eq!(battle.actors[0].movement.steering.side, DetourSide::Left);
    battle.actors[0].position[2] = -50.;
    battle.actors[1].position[2] = -50.;
    assert_eq!(battle.actors[0].movement.steering.side, DetourSide::Left);
    battle
        .begin_approach_steering(ActorId(0), ActorId(1))
        .unwrap();
    assert_eq!(battle.actors[0].movement.steering.side, DetourSide::Right);
    battle.actors[0].control = Control::SemiAuto;
    battle
        .begin_approach_steering(ActorId(0), ActorId(1))
        .unwrap();
    assert_eq!(
        battle.actors[0].movement.steering.side,
        DetourSide::Automatic
    );
}

#[test]
fn coincident_edge_side_is_required_only_by_an_eligible_detour() -> Result<()> {
    let mut battle = battle(vec![
        at(Side::Party, 700., 0.),
        at(Side::Party, 600., 0.),
        at(Side::Enemy, 700., 0.),
    ]);
    battle.begin_approach_steering(ActorId(0), ActorId(2))?;
    assert_eq!(
        battle.actors[0].movement.steering.side,
        DetourSide::Unproved
    );
    assert_eq!(
        battle.steer_approach(ActorId(0), ActorId(2), [700., 0., 0.])?,
        [700., 0., 0.]
    );
    // Moving the destination creates a defined line through the obstacle.
    // Its detour must not use the side retained before coincident admission.
    battle.actors[2].position = [500., 0., 0.];
    let error = battle
        .steering_detour(ActorId(0), ActorId(2), battle.actors[2].position)
        .unwrap_err();
    assert!(error.to_string().contains("coincident approach endpoints"));
    assert!(error.to_string().contains("actor 0, target 2"));
    Ok(())
}

#[test]
fn a_defined_admission_replaces_an_unproved_detour_side() -> Result<()> {
    for semi_auto in [false, true] {
        let mut battle = battle(vec![
            at(Side::Party, 700., 0.),
            at(Side::Party, 600., 0.),
            at(Side::Enemy, 700., 0.),
        ]);
        battle.begin_approach_steering(ActorId(0), ActorId(2))?;
        assert_eq!(
            battle.actors[0].movement.steering.side,
            DetourSide::Unproved
        );
        if semi_auto {
            battle.actors[0].control = Control::SemiAuto;
        } else {
            battle.actors[2].position = [500., 0., 0.];
        }
        battle.begin_approach_steering(ActorId(0), ActorId(2))?;
        assert_eq!(
            battle.actors[0].movement.steering.side,
            if semi_auto {
                DetourSide::Automatic
            } else {
                DetourSide::Right
            }
        );
        battle.steer_approach(ActorId(0), ActorId(2), battle.actors[2].position)?;
    }
    Ok(())
}

#[test]
fn coincident_avoidance_keeps_the_unobserved_stack_branch_explicit() {
    let battle = battle(vec![
        at(Side::Party, 0., 0.),
        at(Side::Party, 20., 0.),
        at(Side::Enemy, 0., 0.),
    ]);
    assert!(
        battle
            .steer_approach(ActorId(0), ActorId(2), [0.; 3])
            .unwrap_err()
            .to_string()
            .contains("original-game validation")
    );
}

#[test]
fn free_grounded_actor_slides_but_airborne_actor_restores_horizontal_position() {
    let mut actor = at(Side::Party, 860., 0.);
    actor.movement.previous_position = [840., 0., 2.];
    constrain(&mut actor, false);
    assert_eq!(actor.movement.steering.arena_contact(), ArenaContact::Slid);
    assert!((actor.position[0] - 850.).abs() < 0.001);
    actor.position = [860., 0.2, 0.];
    actor.movement.vertical = 7.;
    constrain(&mut actor, false);
    assert_eq!(actor.position, [840., 0.2, 2.]);
    assert_eq!(actor.movement.vertical, 7.);
    assert_eq!(
        actor.movement.steering.arena_contact(),
        ArenaContact::Blocked
    );
}

#[test]
fn party_enemy_radii_and_exemptions_are_independent_of_floor_clamping() {
    let mut battle = battle(vec![at(Side::Party, 860., 0.), at(Side::Enemy, 860., 0.)]);
    battle.actors[0].movement.steering.unrestricted_arena = true;
    battle.actors[0].position[1] = -1.;
    battle.actors[0].movement.vertical = -3.;
    battle.constrain_actor(0);
    assert_eq!(battle.actors[0].position, [860., 0., 0.]);
    assert_eq!(battle.actors[0].movement.vertical, 0.);
    battle.constrain_actor(1);
    assert_eq!(battle.actors[1].position, [860., 0., 0.]);
    battle.actors[1].position[0] = 890.;
    battle.actors[1].movement.steering.leaving_arena = true;
    battle.constrain_actor(1);
    assert_eq!(battle.actors[1].position[0], 890.);
}

#[test]
fn leader_line_and_its_target_restore_prior_positions() {
    let mut battle = battle(vec![
        at(Side::Party, 860., 0.),
        at(Side::Party, 860., 30.),
        at(Side::Enemy, -890., 0.),
    ]);
    battle.actors[0].control = Control::SemiAuto;
    for actor in &mut battle.actors {
        actor.movement.previous_position = [actor.position[0] / 2., 0., actor.position[2]];
    }
    battle.constrain_actor(0);
    battle.constrain_actor(2);
    assert_eq!(battle.actors[0].position[0], 430.);
    assert_eq!(battle.actors[2].position[0], -445.);
    battle.constrain_actor(1);
    assert_eq!(
        battle.actors[1].movement.steering.arena_contact(),
        ArenaContact::Slid
    );
}
