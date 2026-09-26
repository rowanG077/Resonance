use super::*;
use crate::{
    BattleInput, Side,
    tests::{actor, prepared},
};
use std::sync::Arc;

fn sine() -> Vec<f32> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-hover.json")).unwrap();
    fixture["sine_bits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
        .collect()
}

fn hovering_battle() -> Battle {
    let mut ghost = actor(Side::Enemy);
    ghost.movement.flying = true;
    ghost.movement.hover_height = 50.;
    let prepared = Arc::try_unwrap(prepared(
        "pub task run() {}",
        vec![actor(Side::Party), ghost],
        1,
    ))
    .unwrap();
    Battle::new(Arc::new(
        prepared
            .with_hover_bobbing(vec![ActorId(1)], &sine())
            .unwrap(),
    ))
}

#[test]
fn original_ghost_settling_and_bobbing_observations_match_bit_for_bit() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-movement.json")).unwrap();
    let rows: Vec<_> = fixture["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["flying"] == true && (227..=266).contains(&row["combat_tick"].as_u64().unwrap())
        })
        .collect();
    assert_eq!(rows.len(), 40);
    let f = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
    let mut battle = hovering_battle();
    let ghost = &mut battle.actors[1];
    ghost.position[1] = f(&rows[0]["before"]["position_bits"][1]);
    ghost.movement.vertical = f(&rows[0]["before"]["velocity_bits"][3]);
    ghost.movement.gravity = f(&rows[0]["before"]["velocity_bits"][5]);
    for row in rows {
        let before = &row["before"];
        let ghost = &battle.actors[1];
        assert_eq!(
            [
                ghost.position[1].to_bits(),
                ghost.movement.vertical.to_bits(),
                ghost.movement.gravity.to_bits()
            ],
            [
                before["position_bits"][1].as_u64().unwrap() as u32,
                before["velocity_bits"][3].as_u64().unwrap() as u32,
                before["velocity_bits"][5].as_u64().unwrap() as u32
            ],
            "original combat tick {}",
            row["combat_tick"]
        );
        battle.step(BattleInput::default()).unwrap();
    }
    assert!(battle.actors[1].movement.hover_ready());
    assert_eq!(battle.actors[1].movement.hover.phase, 9);
}

#[test]
fn capture_is_strict_uses_updated_velocity_and_preserves_profile_on_reset() {
    let mut movement = Movement {
        flying: true,
        hover_height: 50.,
        vertical: 2.,
        gravity: 0.5,
        ..Default::default()
    };
    let mut position = [0., 46., 0.];
    movement.integrate(&mut position, [0.; 2]);
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
    assert_eq!(
        battle.actors[1].position[1],
        0.5_f32.mul_add(sine()[92], 50.)
    );
}

#[test]
fn landing_emits_once_at_pre_correction_position_and_clears_only_below_zero() {
    let mut prepared = Arc::try_unwrap(prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        1,
    ))
    .unwrap();
    prepared
        .effects
        .insert(4, crate::tests::effect_binding(4, [17]));
    let mut battle = Battle::new(Arc::new(
        prepared
            .with_landing_effect(EffectAppearance {
                resource: 4,
                member: 17,
            })
            .unwrap(),
    ));
    let actor = &mut battle.actors[0];
    actor.position[1] = 5.;
    floor(actor);
    assert!(!actor.movement.floor.armed);
    actor.position[1] = 5.01;
    floor(actor);
    assert!(actor.movement.floor.armed);
    actor.position = [2., 0., 3.];
    actor.movement.vertical = -1.;
    actor.movement.airborne_action = true;
    floor(actor);
    assert_eq!(actor.movement.vertical, -1.);
    assert!(actor.movement.airborne_action);
    let mut cues = vec![];
    battle.landing_effect(0, &mut cues).unwrap();
    assert!(matches!(
        cues.as_slice(),
        [Cue::Effect {
            member: 17,
            position: [2., 0., 3.],
            ..
        }]
    ));
    cues.clear();
    let actor = &mut battle.actors[0];
    actor.position[1] = 6.;
    floor(actor);
    actor.position[1] = -2.;
    floor(actor);
    floor(actor); // The central tail may repeat a callback's floor correction.
    assert_eq!(actor.position[1], 0.);
    assert_eq!(actor.movement.vertical, 0.);
    assert!(!actor.movement.airborne_action);
    battle.landing_effect(0, &mut cues).unwrap();
    battle.landing_effect(0, &mut cues).unwrap();
    assert!(matches!(
        cues.as_slice(),
        [Cue::Effect {
            member: 17,
            position: [2., -2., 3.],
            ..
        }]
    ));
}
