use super::*;
use crate::CastingTraits;
use crate::conditions::{Condition, ConditionSet};

#[test]
fn casting_modifiers_apply_once_at_entry() {
    let speed_cast = CastingTraits {
        speed_cast: true,
        ..Default::default()
    };
    let angel_song = CastingTraits {
        angel_song: true,
        ..Default::default()
    };
    let reprise = CastingTraits {
        reprise: true,
        ..Default::default()
    };
    for (side, traits, previous, overlimit, speed, expected) in [
        (Side::Party, speed_cast, None, false, false, 90),
        (Side::Party, angel_song, None, false, false, 60),
        (Side::Party, reprise, Some(CAST), false, false, 60),
        (Side::Party, reprise, Some(ALTERNATE), false, false, 120),
        (Side::Party, CastingTraits::default(), None, true, false, 60),
        (Side::Enemy, CastingTraits::default(), None, true, false, 60),
        (Side::Enemy, CastingTraits::default(), None, false, true, 90),
    ] {
        let mut prepared = prepared_cast(Control::Manual, 120, CASTER_TP);
        prepared.actors[0].side = side;
        if side == Side::Enemy {
            prepared.actors[0].control = Control::Enemy;
            prepared.actors[1].side = Side::Party;
            prepared.actors[1].control = Control::Auto;
            prepared.resources.actor_setup[0].techniques.clear();
            crate::tests::assign_action(&mut prepared, 0, CAST);
        }
        prepared.resources.actor_setup[0].overlimit_gain = 1;
        let owner = &mut prepared.actors[0];
        owner.equipment.casting = traits;
        owner.casting_state.previous_spell = previous;
        owner.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
            intrinsic: if speed {
                ConditionSet::of(&[Condition::CastingSpeed])
            } else {
                ConditionSet::EMPTY
            },
            ..Default::default()
        });
        if overlimit {
            owner.overlimit = crate::OverLimit::active(100).unwrap();
        }
        let mut battle = prepared.finish().unwrap();
        battle.step(request()).unwrap();
        assert_eq!(cast_remaining(&battle), expected);
        let owner = &mut battle.actors[0];
        owner.equipment.casting = Default::default();
        owner.overlimit = crate::OverLimit::new(0).unwrap();
        owner.conditions.reload_layers(Default::default());
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(cast_remaining(&battle), expected - 1);
    }
}

#[test]
fn duration_accepts_full_tick_range_and_has_a_one_tick_minimum() {
    for duration in [0, 32780, u32::MAX] {
        let mut prepared = prepared_cast(Control::Manual, 0, CASTER_TP);
        casting_definition(&mut prepared).duration = duration;
        let mut battle = prepared.finish().unwrap();
        let expected = duration.max(1);
        start_cast(&mut battle);
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(cast_remaining(&battle), expected);
    }
}

#[test]
fn rhythm_uses_the_casters_physical_input_channel() {
    let mut battle = delay::controlled(Control::Manual, 20);
    battle.actors[0].equipment.casting.rhythm = true;
    battle.actors[0].control_slot = 1;
    start_cast(&mut battle);
    battle.step(BattleInput::default()).unwrap();
    let mut wrong_channel = crate::ControlInput::neutral(ActorId(2));
    wrong_channel.attack.pressed = true;
    battle
        .step(BattleInput {
            controllers: vec![wrong_channel],
            ..Default::default()
        })
        .unwrap();
    let mut own = crate::ControlInput::neutral(ActorId(0));
    own.attack.pressed = true;
    let before = cast_remaining(&battle);
    for _ in 0..5 {
        battle
            .step(BattleInput {
                controllers: vec![own],
                ..Default::default()
            })
            .unwrap();
    }
    assert_eq!(cast_remaining(&battle), before - 6);
}

#[test]
fn leaving_combat_cancels_chanting_without_release_or_payment() {
    {
        let mut battle = delay::controlled(Control::Manual, 100);
        start_cast(&mut battle);
        battle.step(BattleInput::default()).unwrap();
        let tp = battle.actors[0].tp;
        battle.terminal.result = Some(crate::BattleResult::Victory);
        let cues = battle.step(BattleInput::default()).unwrap().cues;
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
        assert_eq!(battle.actors[0].tp, tp);
        assert!(!cues.iter().any(|cue| matches!(cue, Cue::Released { .. })));
    }
}

#[test]
fn spell_save_resumes_interrupted_progress_without_early_payment() {
    let mut battle = delay::controlled(Control::Manual, 20);
    battle.actors[0].equipment.casting.spell_save = true;
    let id = start_cast(&mut battle);
    for _ in 0..4 {
        battle.step(BattleInput::default()).unwrap();
    }
    let remaining = battle.casting_remaining(ActorId(0)).unwrap();
    let tp = battle.actors[0].tp;
    let frame = battle
        .step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })
        .unwrap();
    assert!(frame.cues.contains(&Cue::Interrupted { action: id }));
    assert_eq!(battle.casting_remaining(ActorId(0)), None);
    assert_eq!(battle.actors[0].tp, tp);
    battle.actors[0].equipment.casting.quick = true;
    battle.actors[0].equipment.casting.random = true;
    let random = battle.random_state();
    start_cast(&mut battle);
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.casting_remaining(ActorId(0)), Some(remaining));
    assert_eq!(
        battle.random_state(),
        random,
        "resuming preserves the committed clock"
    );
}
