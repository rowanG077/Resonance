use super::*;
use crate::{
    Side,
    tests::{actor, prepared},
};

fn hovering_battle() -> Battle {
    let mut ghost = actor(Side::Enemy);
    ghost.movement.flying = true;
    ghost.movement.hover_height = 50.;
    let prepared = prepared(vec![actor(Side::Party), ghost], 1);
    prepared
        .with_hover_bobbing(vec![ActorId(1)])
        .unwrap()
        .finish()
        .unwrap()
}

#[test]
fn settling_uses_velocity_and_preserves_hover_profile_on_reset() {
    let mut movement = Movement {
        flying: true,
        hover_height: 50.,
        vertical: 2.,
        gravity: 0.5,
        ..Default::default()
    };
    let mut position = [0., 46., 0.];
    movement.integrate(&mut position);
    movement.settle_hover(&mut position[1]);
    assert_eq!(position[1], 50.); // delta2 is smaller than new speed2.5.
    assert!(movement.hover_ready());
    movement.hover.bobbing = true;
    movement.hover.phase = 359;
    movement.advance_hover_phase();
    assert_eq!(movement.hover.phase, 0);
    movement.reset_hover();
    assert!(!movement.hover_ready());
    assert!(movement.hover.bobbing);
    movement.settle_hover(&mut position[1]);
    assert!(!movement.hover_ready()); // exact target with zero speed is not captured.
    assert_eq!(movement.gravity, 0.01);
    movement.vertical = 2.;
    position[1] = 48.;
    movement.settle_hover(&mut position[1]);
    assert!(!movement.hover_ready()); // abs(delta) == abs(speed).
    movement.vertical = 9.;
    position[1] = 20.;
    movement.settle_hover(&mut position[1]);
    assert_eq!(movement.vertical, 6.);
    position[1] = 100.;
    movement.settle_hover(&mut position[1]);
    assert_eq!((movement.vertical, movement.gravity), (-2.5, 0.));
}

#[test]
fn stopping_settles_without_bobbing_but_common_phase_keeps_running() {
    let mut battle = hovering_battle();
    battle.actors[1].movement.hover.settled = true;
    battle.actors[1].movement.hover.phase = 22;
    battle.actors[1].position[1] = 50.;
    battle.advance_hover(1, false).unwrap();
    assert_eq!(battle.actors[1].position[1], 50.);
    battle.advance_actor_common(1, &mut vec![]).unwrap();
    assert_eq!(battle.actors[1].movement.hover.phase, 23);
    battle.advance_hover(1, true).unwrap();
    let height = battle.actors[1].position[1];
    assert!(height > 50. && height <= 50.5);
    for _ in 0..360 {
        battle.advance_actor_common(1, &mut vec![]).unwrap();
        battle.advance_hover(1, true).unwrap();
        assert!((battle.actors[1].position[1] - 50.).abs() <= 0.5);
    }
}
