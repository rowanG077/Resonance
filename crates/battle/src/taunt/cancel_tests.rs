use super::*;
use crate::{
    Control, DamageKind, GuardResult, GuardRule, HitElement, HitProtection, HitRule, Power,
    Protection, ReactionRule,
};

fn prepared_controlled(enabled: bool, mode: Control) -> PreparedBattle {
    let mut prepared = super::prepared(0, true);
    prepared.actors[0].equipment.taunt_enabled = true;
    prepared.actors[0].equipment.taunt_cancel = enabled;
    prepared.actors[0].control = mode;
    prepared.resources.unison_available = true;
    prepared.resources.actor_setup[0].companion = Some(crate::CompanionDefinition {
        initial_policy: [1; 3],
        defaults: [1; 3],
        limits: [crate::PolicyLimits::default(); 9],
        level: 1,
        level_difference: 0,
    });
    prepared
}

fn controlled(enabled: bool, mode: Control) -> Battle {
    prepared_controlled(enabled, mode).finish().unwrap()
}

pub(super) fn guard() -> BattleInput {
    BattleInput {
        controllers: vec![ControlInput {
            guard: ButtonInput {
                held: true,
                ..Default::default()
            },
            ..ControlInput::neutral(ActorId(0))
        }],
        ..Default::default()
    }
}

fn enter(battle: &mut Battle) {
    battle.step(input(true, true)).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Taunting);
}

fn no_award(battle: &Battle, cues: &[Cue]) {
    assert_eq!(battle.unison_gauge(), 0);
    assert!(!cues.iter().any(|cue| matches!(cue, Cue::UnisonReady)));
}

#[test]
fn taunt_cancel_activates_guard_and_advances_movement_once() {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = controlled(true, mode);
        battle.actors[0].equipment.taunt_guard = true;
        enter(&mut battle);
        let owner = &mut battle.actors[0];
        owner.movement.direction = [0., 0., 1.];
        owner.movement.forward = 4.;
        let before = owner.position;
        let frame = battle.step(guard()).unwrap();
        no_award(&battle, &frame.cues);
        let owner = &battle.actors[0];
        assert_eq!(battle.activity(ActorId(0)), Activity::Guarding);
        assert!(owner.guard.active);
        assert_eq!(owner.reaction.protection, Protection::default());

        assert_eq!(owner.position, [before[0], before[1], before[2] + 4.]);
        assert!(owner.movement.forward < 4.);
        battle.step(guard()).unwrap();
        assert!(battle.actors[0].guard.active);
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    }
}

#[test]
fn taunt_cancel_uses_guard_hold_only_and_requires_live_trait() {
    for enabled in [false, true] {
        for held in [false, true] {
            let mut battle = controlled(enabled, Control::Manual);
            enter(&mut battle);
            let tp = battle.actors[0].tp;
            let mut buttons = guard();
            let input = &mut buttons.controllers[0];
            input.guard.held = held;
            input.guard.pressed = true;
            input.attack.pressed = true;
            input.technique.pressed = true;
            input.taunt.pressed = true;
            input.horizontal_pressed = -1;
            input.stick = [-70, 70];
            let frame = battle.step(buttons).unwrap();
            no_award(&battle, &frame.cues);
            assert_eq!(
                battle.activity(ActorId(0)),
                if enabled && held {
                    Activity::Guarding
                } else {
                    Activity::Taunting
                }
            );
            assert_eq!(battle.actors[0].guard.active, enabled && held);
            assert_eq!(battle.actors[0].tp, tp);
        }
    }
}

#[test]
fn taunt_cancel_blocks_contact_immediately_without_preventing_lethal_damage() {
    for hp in [50, 1] {
        let mut battle = controlled(true, Control::Manual);
        battle.actors[0].equipment.taunt_guard = true;
        enter(&mut battle);
        battle.step(guard()).unwrap();
        assert!(battle.actors[0].guard.active);
        battle.actors[0].hp = hp;
        battle.actors[0].guard.reduction = 50;
        battle.actors[0].guard.break_pressure = 31;
        battle.actors[0].facing_direction = [1., 0., 0.];
        battle.actors[1].facing_direction = [-1., 0., 0.];
        let (result, _) = contact(
            &mut battle,
            HitRule {
                kind: DamageKind::Slash,
                arte: false,
                overlimit_pause: false,
                power: Power::Fixed(9),
                element: HitElement::Neutral,
                prevents_defeat: false,
                guard: GuardRule::default(),
                reaction: ReactionRule {
                    hitstun: 5,
                    ..Default::default()
                },
                condition: None,
            },
        );
        assert_eq!(result.protection, HitProtection::None);
        assert_eq!(result.amount, 4);
        if hp > result.amount {
            assert!(matches!(result.guard, GuardResult::Blocked { .. }));
        }
        assert_eq!(result.hp_change, -hp.min(4));
        assert_eq!(
            battle.activity(ActorId(0)),
            if hp == 1 {
                Activity::Defeated
            } else {
                Activity::Guarding
            }
        );
        assert_eq!(battle.unison_gauge(), 0);
    }
}

#[test]
fn escape_cancels_taunt_without_awarding_gauge_or_entering_guard() {
    let mut battle = controlled(true, Control::Manual);
    enter(&mut battle);
    battle.recognize_escape(true).unwrap();
    assert_eq!(
        battle.recognize_result(),
        Some(crate::BattleResult::Escaped)
    );
    let frame = battle.step(guard()).unwrap();
    no_award(&battle, &frame.cues);
    assert_eq!(battle.activity(ActorId(0)), Activity::Escaping);
    assert!(!battle.actors[0].guard.active);
}
