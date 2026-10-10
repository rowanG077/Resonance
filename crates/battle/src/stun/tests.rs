use super::*;
use crate::Activity;
use crate::conditions::{Condition, ConditionSet};
use crate::tests::prepared;
use crate::{
    ActionId, ActionRequest, BattleInput, DamageKind, GuardResult, GuardRule, HitElement, HitRule,
    MeleeDefinition, Power, PreparedBattle, ReactionRule, Side,
};
fn actor(side: Side) -> Actor {
    let mut actor = crate::tests::actor(side);
    actor.body.collider = Some(crate::Collider::sphere(1.));

    actor
}

fn battle(mut target: Actor) -> Battle {
    target.side = Side::Enemy;
    let mut prepared = prepared(vec![actor(Side::Party), target], 120);
    prepared.resources.actor_setup[0] = Default::default();
    crate::tests::assign_action(&mut prepared, 1, crate::ActionKey(0));
    prepared.finish().unwrap()
}

fn hit() -> HitResult {
    HitResult {
        amount: 8,
        hp_change: -8,
        critical: false,
        boosted: false,
        affinity: crate::Affinity::Normal,
        guard: GuardResult::None,
        protection: crate::HitProtection::None,
    }
}

fn melee(chance: u8) -> MeleeDefinition {
    MeleeDefinition {
        hit: HitRule {
            overlimit_pause: true,
            condition: None,
            arte: false,
            kind: DamageKind::Slash,
            power: Power::Fixed(8),
            element: HitElement::Neutral,
            prevents_defeat: false,
            guard: GuardRule::default(),
            reaction: ReactionRule {
                stun_chance: chance,
                hitstun: 20,
                ..Default::default()
            },
        },
        trail: None,
        volume: crate::MeleeVolume {
            offset: [0.; 3],
            radius: 2.,
            half_height: 2.,
        },
    }
}

fn contact(battle: &mut Battle, chance: u8) -> Vec<Cue> {
    let mut contacts = crate::contact::Contacts::default();
    contacts
        .melee(ActorId(0), ActionId(999), &melee(chance), &[])
        .unwrap();
    let mut cues = vec![];
    contacts.resolve(battle, &mut cues).unwrap();
    cues
}

#[test]
fn stun_chance_respects_immunity_resistance_bonuses_and_guarding() {
    let mut owner = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    for chance in [0, 100] {
        target.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
            immunity: ConditionSet::of(&[Condition::Stun]),
            ..Default::default()
        });
        let mut random = crate::state::Random::new(1);
        assert!(!roll(&owner, &target, chance, hit(), &mut random));
    }
    target.conditions = Default::default();
    target.reaction.stun.resistance = 255;
    owner.equipment.stun_ex_bonus = true;
    assert_eq!(chance_percent(&owner, &target, 0), 5);
    owner.reaction.stun.chance_bonus = 100;
    target.reaction.stun.resistance = 0;
    assert!(roll(
        &owner,
        &target,
        0,
        hit(),
        &mut crate::state::Random::new(1)
    ));
    for case in 0..4 {
        let mut target = target.clone();
        let mut result = hit();
        match case {
            0 => target.time_stop = u16::MAX,
            1 => result.protection = crate::HitProtection::Armored,
            2 => result.affinity = crate::Affinity::Absorb,
            _ => {
                result.guard = GuardResult::Blocked {
                    first: true,
                    special: false,
                }
            }
        }
        let mut random = crate::state::Random::new(1);
        assert!(!roll(&owner, &target, 100, result, &mut random));
        assert_eq!(random.state(), 1);
    }
    let mut broken = hit();
    broken.guard = GuardResult::Broken;
    assert!(roll(
        &owner,
        &target,
        100,
        broken,
        &mut crate::state::Random::new(1)
    ));
}

#[test]
fn stun_interrupts_work_and_recovers_without_resuming_it() {
    let mut battle = battle(actor(Side::Enemy));
    let start = battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(1),
                target: ActorId(1),
                action: crate::ActionKey(0),
            }],
            ..Default::default()
        })
        .unwrap();
    let action = start.actions[0].0;
    let cues = contact(&mut battle, 100);
    assert_eq!(
        cues.iter()
            .filter(|cue| **cue == Cue::Interrupted { action })
            .count(),
        1
    );
    assert_eq!(battle.activity(ActorId(1)), Activity::Stunned);
    let hp = battle.actors[1].hp;
    for _ in 0..DURATION + 10 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(
            frame.actors[1].hp, hp,
            "interrupted healing must not resume"
        );
        if frame.actors[1].activity == Activity::Idle {
            break;
        }
    }
    assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
}

#[test]
fn stun_recovery_respects_pause_immunity_and_manual_struggle() {
    fn recovery_time(control: Control, short: bool, struggle: bool) -> u32 {
        let mut target = actor(Side::Party);
        target.control = control;
        if short {
            target.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
                immunity: ConditionSet::of(&[Condition::ShortStun]),
                ..Default::default()
            });
        }
        let mut battle =
            PreparedBattle::new(vec![(target, Default::default())], Default::default(), 0)
                .unwrap()
                .finish()
                .unwrap();
        battle.enter_stun(ActorId(0), &mut vec![]);
        for _ in 0..DURATION + 1 {
            battle
                .step(BattleInput {
                    paused: true,
                    stun_struggle: vec![ActorId(0)],
                    ..Default::default()
                })
                .unwrap();
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Stunned);
        for elapsed in 1..=DURATION + 1 {
            battle
                .step(BattleInput {
                    stun_struggle: if struggle { vec![ActorId(0)] } else { vec![] },
                    ..Default::default()
                })
                .unwrap();
            if battle.activity(ActorId(0)) == Activity::Idle {
                return elapsed;
            }
        }
        panic!("stun did not recover");
    }
    let ordinary = recovery_time(Control::Manual, false, false);
    assert!(recovery_time(Control::Manual, true, false) < ordinary);
    assert!(recovery_time(Control::SemiAuto, false, true) < ordinary);
    assert_eq!(recovery_time(Control::Auto, false, true), ordinary);
}

#[test]
fn stun_recovers_without_body_artwork() {
    let mut battle = PreparedBattle::new(
        vec![
            (actor(Side::Party), Default::default()),
            (actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        1,
    )
    .unwrap()
    .finish()
    .unwrap();
    battle.set_diagnostics(Default::default());
    contact(&mut battle, 100);
    assert_eq!(battle.activity(ActorId(1)), Activity::Stunned);
    for _ in 0..DURATION - 1 {
        battle.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.activity(ActorId(1)), Activity::Stunned);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.actors[1].activity, Activity::Idle);
    assert_eq!(frame.actors[1].hp, 42);
    assert!(!battle.is_diagnostic());
    battle.recognize_escape(true).unwrap();
    battle.recognize_result();
    assert!(battle.finish_result().unwrap().outcome.is_some());
}
