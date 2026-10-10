use super::*;
use crate::{Activity, BattleInput, ButtonInput, Control, ControlInput, ProtectionMode};
const OWNER: ActorId = ActorId(0);

fn battle() -> Battle {
    crate::mobility::tests::battle()
}
fn attack(held: bool, pressed: bool) -> ControlInput {
    ControlInput {
        attack: ButtonInput {
            held,
            pressed,
            ..Default::default()
        },
        ..ControlInput::neutral(OWNER)
    }
}

#[test]
fn charge_uses_held_input_once_and_preserves_charge_through_recovery() {
    let mut battle = battle();
    battle.actors[0].equipment.control_ex.charge = true;
    battle.sample_cast_inputs(&[attack(true, false)]);
    let mut cues = vec![];
    for _ in 0..89 {
        battle.advance_attack_charge(OWNER, &mut cues).unwrap();
    }
    assert_eq!(battle.actors[0].control_ex_state.charge, ChargeLevel::None);
    assert!(cues.is_empty());
    battle.advance_attack_charge(OWNER, &mut cues).unwrap();
    assert_eq!(
        battle.actors[0].control_ex_state.charge,
        ChargeLevel::Normal
    );
    assert_eq!(battle.actors[0].control_ex_state.charge_remaining, 300);
    assert_eq!(
        cues.iter()
            .filter(|cue| matches!(cue, Cue::Charged { .. }))
            .count(),
        1
    );
    for _ in 0..200 {
        battle.advance_attack_charge(OWNER, &mut cues).unwrap();
    }
    assert_eq!(cues.len(), 1);
    battle.enter_idle(OWNER);
    assert_eq!(
        battle.actors[0].control_ex_state.charge,
        ChargeLevel::Normal
    );
    assert_eq!(battle.actors[0].control_ex_state.charge_remaining, 300);
    for _ in 0..299 {
        battle.actors[0].control_ex_state.advance_charge();
    }
    assert!(battle.actors[0].control_ex_state.charge.charged());
    battle.actors[0].control_ex_state.advance_charge();
    assert!(!battle.actors[0].control_ex_state.charge.charged());
    battle.sample_cast_inputs(&[attack(false, true)]);
    battle.advance_attack_charge(OWNER, &mut cues).unwrap();
    assert_eq!(battle.actors[0].control_ex_state.charge_hold, 0);
}

#[test]
fn lucky_charge_clears_hold_and_reports_the_result() {
    let mut battle = battle();
    battle.actors[0].equipment.control_ex.charge = true;
    battle.actors[0].equipment.control_ex.lucky_charge = true;
    battle.actors[0].control_ex_state.charge_hold = 89;
    battle.sample_cast_inputs(&[attack(true, false)]);
    let mut cues = vec![];
    battle.advance_attack_charge(OWNER, &mut cues).unwrap();
    let state = &battle.actors[0].control_ex_state;
    assert_eq!(state.charge_hold, 0);
    assert!(matches!(
        state.charge,
        ChargeLevel::None | ChargeLevel::Strong
    ));
    assert_eq!(
        state.charge_remaining,
        if state.charge.charged() { 300 } else { 0 }
    );
    assert!(matches!(cues.first(), Some(Cue::Charged { level, .. }) if *level == state.charge));
}

#[test]
fn timed_guard_advances_under_hitstop_but_menu_holds_and_ordinary_recovery_clears_low_flags() {
    let mut battle = battle();
    battle.actors[0].control = Control::Manual;
    battle.actors[0].equipment.control_ex.timed_guard = true;
    battle.enter_player_guard(0);
    battle.actors[0].control_ex_state.guard_hold = 179;
    battle.actors[0].hit_stop = 4;
    battle
        .step(BattleInput {
            paused: true,
            ..Default::default()
        })
        .unwrap();
    assert!(!battle.actors[0].control_ex_state.guard_ready);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(battle.actors[0].control_ex_state.guard_ready);
    assert_eq!(battle.actors[0].control_ex_state.guard_hold, 180);
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::GuardReady { .. }))
            .count(),
        1
    );
    battle.actors[0].control_ex_state.counter_active = true;
    battle.actors[0].control_ex_state.double_jump_used = true;
    battle.enter_idle(OWNER);
    assert!(!battle.actors[0].control_ex_state.guard_ready);
    assert!(!battle.actors[0].control_ex_state.counter_active);
    assert!(!battle.actors[0].control_ex_state.double_jump_used);
}

#[test]
fn counter_requires_recent_guard_contact_and_holds_other_actors() {
    for (remaining, active) in [(0, false), (20, false), (30, true)] {
        let mut battle = battle();
        battle.actors[0].equipment.control_ex.counter = true;
        battle.actors[0].guard.recovery = remaining;
        let mut cues = vec![];
        battle.activate_counter(OWNER, &mut cues).unwrap();
        assert_eq!(battle.actors[0].control_ex_state.counter_active, active);
        if active {
            assert_eq!(
                battle.actors[0].reaction.protection.mode,
                ProtectionMode::Escape
            );
            assert_eq!(battle.actors[0].reaction.protection.remaining, 15);
            assert_eq!(battle.timed_hold.unwrap().remaining, 10);
            assert!(battle.is_paused());
        } else {
            assert!(cues.is_empty());
        }
    }
}

#[test]
fn airborne_guard_combines_commands_and_double_jump_precedes_falling_attempt() {
    let mut battle = battle();
    battle.actors[0].control = Control::Manual;
    battle.actors[0].equipment.control_ex.aerial_guard = true;
    battle.actors[0].equipment.control_ex.double_jump = true;
    battle.actors[0].position[1] = 50.;
    battle.actors[0].movement.vertical = -2.;
    battle.set_task(
        0,
        crate::state::ActorTask::Mobility(crate::mobility::Mobility::Jump { launched: true }),
    );
    let input = ControlInput {
        vertical_pressed: 1,
        stick: [0, 60],
        guard: ButtonInput {
            held: true,
            ..Default::default()
        },
        ..attack(true, true)
    };
    battle
        .step(BattleInput {
            controllers: vec![input],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    assert!(battle.actors[0].movement.vertical > 0.);
    assert!(battle.actors[0].control_ex_state.double_jump_used);
    battle.actors[0].movement.vertical = -2.;
    battle
        .step(BattleInput {
            controllers: vec![input],
            ..Default::default()
        })
        .unwrap();
    assert!(battle.actors[0].movement.vertical < 0.);
}

#[test]
fn taunt_recovery_stacks_traits_caps_vitals_and_does_not_roll_lucky_healing() {
    let mut battle = battle();

    battle.actors[0].equipment.control_ex.taunt_vitals = true;
    battle.actors[0].equipment.control_ex.taunt_hp = true;
    battle.actors[0].equipment.recovery.lucky = true;
    battle.actors[0].equipment.max_hp = 1000;
    battle.actors[0].hp = 950;
    battle.actors[0].equipment.max_tp = 50;
    battle.actors[0].tp = 49;
    let mut cues = vec![];
    battle.complete_taunt_recovery(OWNER, &mut cues).unwrap();
    assert_eq!(battle.actors[0].hp, 1000);
    assert_eq!(battle.actors[0].tp, 50); //1% truncateszero thenminimumone.
    for (kind, nominal, applied) in [
        (crate::RecoveryKind::Hp, 20, 20),
        (crate::RecoveryKind::Tp, 1, 1),
        (crate::RecoveryKind::Hp, 80, 30),
    ] {
        assert!(cues.contains(&Cue::Recovered {
            actor: OWNER,
            kind,
            nominal,
            applied
        }));
    }
}

#[test]
fn equipment_rebuild_changes_traits_and_preserves_every_retained_control_operand() {
    let mut battle = battle();
    battle.actors[0].control_ex_state = ControlExState {
        charge: ChargeLevel::Strong,
        charge_remaining: 111,
        charge_hold: 7,
        guard_hold: 179,
        guard_ready: false,
        counter_active: true,
        double_jump_used: true,
    };
    let state = battle.actors[0].control_ex_state;
    let mut replacement = battle.actors[0].clone();
    replacement.equipment.control_ex.charge = true;
    replacement.equipment.control_ex.rebound = true;
    replacement.control_ex_state = Default::default();
    crate::tests::equip(&mut battle, OWNER, replacement).unwrap();
    assert_eq!(battle.actors[0].control_ex_state, state);
    assert!(
        battle.actors[0].equipment.control_ex.charge
            && battle.actors[0].equipment.control_ex.rebound
    );
}
