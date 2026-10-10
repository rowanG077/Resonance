use super::*;
use crate::PreparedBattle;
use crate::{ActorAvailability, BattleInput, Side, tests::actor};
use crate::{ActorId, Battle, Cue};
use Condition::*;
use anyhow::Result;
use resonance_content::battle_action::Condition as HitAilment;

fn battle() -> Battle {
    let mut owner = actor(Side::Party);
    owner.hp = 20;
    owner.equipment.max_hp = 1000;
    owner.tp = 5;
    owner.equipment.max_tp = 200;
    owner.conditions = Conditions::new(Layers {
        intrinsic: ConditionSet::of(&[RegenerateHp, RegenerateTp]),
        ..Default::default()
    });
    owner.conditions.reload_gear_regeneration(GearRegeneration {
        hp_percent: 3,
        tp_percent: 1,
    });
    PreparedBattle::new(
        vec![
            (owner, Default::default()),
            (actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        17,
    )
    .unwrap()
    .finish()
    .unwrap()
}

fn pulse(battle: &mut Battle) -> Vec<Cue> {
    let mut cues = vec![];
    for _ in 0..REGENERATION_INTERVAL {
        battle.advance_condition_callbacks(0, true, &mut cues);
    }
    cues
}

#[test]
fn regeneration_ticks_every_period_and_emits_recovery_feedback_once() {
    let mut battle = battle();
    let seed = battle.random_state();
    for expected_hp in [50, 80] {
        let cues = pulse(&mut battle);
        for (kind, amount) in [(crate::RecoveryKind::Hp, 30), (crate::RecoveryKind::Tp, 2)] {
            assert!(
                cues.contains(&Cue::Recovered {
                    actor: ActorId(0),
                    kind,
                    nominal: amount,
                    applied: amount
                }),
                "{cues:?}"
            );
        }
        assert_eq!(battle.actors[0].hp, expected_hp);
        assert!(
            battle.actors[0]
                .conditions
                .periodic_effects()
                .iter()
                .all(|effect| effect.remaining == effect.period)
        );
    }
    assert_eq!(battle.actors[0].tp, 9);
    assert_eq!(battle.random_state(), seed);
}

#[test]
fn regeneration_respects_recovery_boost_weak_caps_and_tp_maximum() {
    for (hp, percent, boost, weak, expected) in [
        (20, 5, true, false, 80),
        (490, 3, false, true, 500),
        (800, 3, false, true, 800),
    ] {
        let mut battle = battle();
        let owner = &mut battle.actors[0];
        owner.hp = hp;
        owner.tp = 195;
        owner.equipment.recovery.boost = boost;
        owner.equipment.recovery.lucky = true;
        owner.conditions.reload_gear_regeneration(GearRegeneration {
            hp_percent: percent,
            tp_percent: 5,
        });
        if weak {
            owner.conditions.apply_hit(crate::HitCondition {
                condition: HitAilment::Weak,
                chance: 100,
                value: 0,
            });
        }
        let seed = battle.random_state();
        pulse(&mut battle);
        assert_eq!(battle.actors[0].hp, expected);
        assert_eq!(battle.actors[0].tp, 200);
        assert_eq!(battle.random_state(), seed);
    }
}

#[test]
fn cure_and_equipment_refresh_preserve_live_grants_but_unequipping_removes_them() {
    let mut battle = battle();
    for _ in 0..17 {
        battle.advance_condition_callbacks(0, true, &mut vec![]);
    }
    let conditions = &mut battle.actors[0].conditions;
    let periodic = conditions.periodic_effects().to_vec();
    for cure in [Cure::Physical, Cure::AntiMagic, Cure::All] {
        conditions.apply_hit(crate::HitCondition {
            condition: HitAilment::Paralysis,
            chance: 100,
            value: 0,
        });
        conditions.cure(cure);
        assert_eq!(conditions.periodic_effects(), periodic);
        assert!(
            !conditions
                .base()
                .intersects(ConditionSet::of(&[RegenerateHp, RegenerateTp]))
        );
    }
    conditions.reload_layers(conditions.layers());
    assert_eq!(conditions.periodic_effects(), periodic);
    conditions.reload_layers(Layers::default());
    assert!(conditions.periodic_effects().is_empty());
    assert!(pulse(&mut battle).is_empty());
    assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (20, 5));
    battle.actors[0].conditions.reload_layers(Layers {
        intrinsic: RegenerateHp.into(),
        ..Default::default()
    });
    assert_eq!(
        battle.actors[0].conditions.periodic_effects()[0].remaining,
        REGENERATION_INTERVAL
    );
}

#[test]
fn regeneration_holds_for_global_pauses_phase_and_unavailability_not_hit_stop() -> Result<()> {
    let mut battle = battle();
    let before = battle.actors[0].conditions.clone();
    battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    assert_eq!(battle.actors[0].conditions, before);
    battle.advance_condition_callbacks(0, false, &mut vec![]);
    for availability in [
        ActorAvailability::Dead,
        ActorAvailability::Absent,
        ActorAvailability::Petrified,
    ] {
        battle.actors[0].availability = availability;
        battle.advance_condition_callbacks(0, true, &mut vec![]);
        assert_eq!(battle.actors[0].conditions, before);
    }
    battle.actors[0].availability = ActorAvailability::Active;
    battle.actors[0].hit_stop = 9;
    battle.advance_condition_callbacks(0, true, &mut vec![]);
    assert!(
        battle.actors[0]
            .conditions
            .periodic_effects()
            .iter()
            .all(|effect| effect.remaining == REGENERATION_INTERVAL - 1)
    );
    Ok(())
}

#[test]
fn poison_and_regeneration_share_the_update() -> Result<()> {
    let mut owner = battle().actors[0].clone();
    owner.conditions.reload_layers(Layers {
        base: ConditionSet::of(&[PoisonMild, AttackUp]),
        intrinsic: ConditionSet::of(&[RegenerateHp, RegenerateTp]),
        ..Default::default()
    });
    let mut battle = crate::tests::prepared(vec![owner], 17).finish()?;
    for _ in 0..REGENERATION_INTERVAL {
        battle.step(BattleInput::default())?;
    }
    assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (31, 7));
    assert_eq!(
        battle.actors[0].conditions.remaining(AttackUp),
        Some(DEFAULT_STAT_CONDITION_DURATION - REGENERATION_INTERVAL)
    );
    assert!(!battle.is_diagnostic());
    battle.step(BattleInput::default())?;
    assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (31, 7));
    Ok(())
}
