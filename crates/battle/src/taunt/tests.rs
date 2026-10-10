use super::*;
use crate::Activity;
use crate::Side;
use crate::{
    ActionId, BattleInput, ButtonInput, ControlInput, Cue, HitResult, HitRule, MeleeDefinition,
    contact::Contacts,
};

#[path = "cancel_tests.rs"]
mod cancel_tests;
#[path = "guard_tests.rs"]
mod guard_tests;

fn prepared(gauge: i16, unlocked: bool) -> PreparedBattle {
    let mut prepared = crate::mobility::tests::prepared();
    prepared.resources.unison_available = unlocked;
    prepared.actors[0].equipment.taunt_enabled = true;
    prepared.resources.unison_gauge = gauge;
    prepared
}

fn battle(gauge: i16, unlocked: bool) -> Battle {
    prepared(gauge, unlocked).finish().unwrap()
}

fn input(pressed: bool, held: bool) -> BattleInput {
    BattleInput {
        controllers: vec![ControlInput {
            taunt: ButtonInput {
                pressed,
                held,
                released: false,
            },
            ..ControlInput::neutral(ActorId(0))
        }],
        ..Default::default()
    }
}

fn contact(battle: &mut Battle, rule: HitRule) -> (HitResult, Vec<Cue>) {
    battle.actors[1].position = battle.actors[0].position;
    battle.actors[0].body.collider = Some(crate::Collider::sphere(2.));

    let mut contacts = Contacts::default();
    contacts
        .melee(
            ActorId(1),
            ActionId(1),
            &MeleeDefinition {
                hit: rule,
                trail: None,
                volume: crate::MeleeVolume {
                    offset: [0.; 3],
                    radius: 20.,
                    half_height: 20.,
                },
            },
            &[],
        )
        .unwrap();
    let mut cues = Vec::new();
    contacts.resolve(battle, &mut cues).unwrap();
    assert!(!battle.is_diagnostic());
    let result = cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Hit { actor, result, .. } if *actor == ActorId(0) => Some(*result),
            _ => None,
        })
        .expect("native Taunt must receive the submitted contact");
    (result, cues)
}

#[test]
fn taunt_completes_once_without_waiting_for_artwork() {
    {
        let prepared = prepared(0, true);
        let mut battle = prepared.finish().unwrap();
        battle.set_diagnostics(Default::default());
        battle.step(input(false, true)).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        battle.step(input(true, true)).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Taunting);
        for _ in 1..TAUNT_DURATION_TICKS - 1 {
            battle.step(input(false, true)).unwrap();
        }
        assert_eq!(battle.unison_gauge(), 0);
        battle.step(input(false, true)).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(battle.unison_gauge(), TAUNT_GAUGE_GAIN);
        battle.step(input(false, true)).unwrap();
        assert_eq!(battle.unison_gauge(), TAUNT_GAUGE_GAIN);
        assert_eq!(battle.saved_unison_gauge(), 3);
        assert!(!battle.is_diagnostic());
    }
}

#[test]
fn taunt_respects_gauge_cap_unlock_and_interruption() {
    let mut battle = battle(3190, true);
    battle.step(input(true, true)).unwrap();
    for _ in 1..TAUNT_DURATION_TICKS - 1 {
        battle.step(BattleInput::default()).unwrap();
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.unison_gauge(), 3200);
    assert_eq!(
        frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::UnisonReady))
            .count(),
        1
    );
    battle.step(input(true, true)).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);

    let mut locked = self::battle(100, false);
    locked.step(input(true, true)).unwrap();
    for _ in 0..TAUNT_DURATION_TICKS {
        locked.step(BattleInput::default()).unwrap();
    }
    assert_eq!(locked.unison_gauge(), 100);
    battle.add_unison_gauge(i16::MIN, &mut vec![]);
    assert_eq!(battle.unison_gauge(), 0);
}

fn startup(saved: u8, unlocked: bool, initial_full: bool) -> Battle {
    let prepared = PreparedBattle::new(
        vec![
            (crate::tests::actor(Side::Party), Default::default()),
            (crate::tests::actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        0x13572468,
    )
    .unwrap()
    .with_unison_gauge(saved, unlocked, initial_full)
    .unwrap();
    prepared.finish().unwrap()
}

#[test]
fn unison_startup_restores_gauge_and_applies_override_once() {
    for (saved, gauge) in [(0, 0), (199, 3184), (200, 3200)] {
        for initial_full in [false, true] {
            for unlocked in [false, true] {
                let mut battle = startup(saved, unlocked, initial_full);
                let expected = if initial_full {
                    (3200, 200)
                } else {
                    (gauge, saved)
                };
                let initial = battle.snapshot();
                assert_eq!(
                    (initial.unison_gauge, battle.saved_unison_gauge()),
                    expected
                );
                assert_eq!(initial.unison_available, unlocked);
                assert!(initial.cues.is_empty());
                let frame = battle.step(BattleInput::default()).unwrap();
                assert_eq!(frame.unison_gauge, expected.0);
                assert!(!frame.cues.iter().any(|cue| matches!(cue, Cue::UnisonReady)));
            }
        }
    }
    let mut battle = startup(0, true, true);
    battle.unison_gauge = 160;
    assert_eq!(
        battle.step(BattleInput::default()).unwrap().unison_gauge,
        160
    );
    let saved = battle.saved_unison_gauge();
    assert_eq!(saved, 10);
    assert_eq!(startup(saved, true, true).unison_gauge(), 3200);
    assert_eq!(startup(saved, true, false).unison_gauge(), 160);
}

#[test]
fn taunt_healing_requires_completion() {
    for cancelled in [false, true] {
        let mut battle = battle(0, true);
        let owner = &mut battle.actors[0];
        owner.equipment.taunt_cancel = true;
        owner.equipment.control_ex.taunt_hp = true;
        owner.equipment.max_hp = 100;
        owner.hp = 10;
        battle.step(input(true, true)).unwrap();
        assert_eq!(battle.activity(ActorId(0)), Activity::Taunting);
        if cancelled {
            battle.step(cancel_tests::guard()).unwrap();
        }
        for _ in 0..TAUNT_DURATION_TICKS {
            battle.step(BattleInput::default()).unwrap();
        }
        assert_eq!(battle.actors[0].hp, if cancelled { 10 } else { 18 });
    }
}
