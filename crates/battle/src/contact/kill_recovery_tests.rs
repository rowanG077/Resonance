use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::conditions::{Conditions, Layers};
use crate::{Actor, ActorAvailability, RecoveryKind};

const BOTH: ConditionSet =
    ConditionSet::of(&[Condition::KillHpRecovery, Condition::KillTpRecovery]);

fn target(side: Side, hp: i32) -> Actor {
    let mut actor = crate::tests::actor(side);
    actor.hp = hp;
    actor.equipment.max_hp = 1000;
    actor.body.collider = Some(crate::Collider::sphere(2.));
    actor
}

fn killer(side: Side, conditions: ConditionSet) -> Actor {
    let mut actor = crate::tests::actor(side);
    actor.hp = 100;
    actor.equipment.max_hp = 1000;
    actor.tp = 20;
    actor.equipment.max_tp = 100;
    actor.conditions = Conditions::new(Layers {
        intrinsic: conditions,
        ..Default::default()
    });
    actor
}

fn battle(owner: Actor, victim: Actor) -> Result<Battle> {
    let prepared = crate::PreparedBattle::new(
        vec![(owner, Default::default()), (victim, Default::default())],
        Default::default(),
        1,
    )?;
    prepared.finish()
}

fn resolve(battle: &mut Battle, power: u16, prevents_defeat: bool) -> Result<Vec<Cue>> {
    strike(
        battle,
        HitRule {
            prevents_defeat,
            ..rule(power)
        },
    )
}

#[test]
fn contact_kills_recover_the_owners_equipped_vitals() -> Result<()> {
    for (side, conditions, expected) in [
        (Side::Party, BOTH, (200, 25)),
        (Side::Enemy, Condition::KillHpRecovery.into(), (200, 20)),
        (Side::Party, Condition::KillTpRecovery.into(), (100, 25)),
        (Side::Enemy, ConditionSet::EMPTY, (100, 20)),
    ] {
        let victim_side = if side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        };
        let mut battle = battle(killer(side, conditions), target(victim_side, 1))?;
        battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(false));

        let cues = resolve(&mut battle, 100, false)?;
        assert_eq!((battle.actors[0].hp, battle.actors[0].tp), expected);
        assert_eq!(battle.actors[1].availability, ActorAvailability::Dead);
        assert!(!battle.is_diagnostic());
        for (kind, condition) in [
            (RecoveryKind::Hp, Condition::KillHpRecovery),
            (RecoveryKind::Tp, Condition::KillTpRecovery),
        ] {
            assert_eq!(
                cues.iter().any(|cue| matches!(cue,
                Cue::Recovered { actor: ActorId(0), kind: actual, .. } if *actual == kind)),
                conditions.contains(condition)
            );
        }
    }
    Ok(())
}

#[test]
fn contact_kill_recovery_skips_unavailable_killer_nonlethal_and_prevented_defeat() -> Result<()> {
    for availability in [
        ActorAvailability::Absent,
        ActorAvailability::Dead,
        ActorAvailability::Petrified,
    ] {
        let mut owner = killer(Side::Party, BOTH);
        owner.availability = availability;
        if availability == ActorAvailability::Dead {
            owner.hp = 0;
        }
        let original = (owner.hp, owner.tp);
        let mut battle = battle(owner, target(Side::Enemy, 1))?;
        let cues = resolve(&mut battle, 100, false)?;
        assert_eq!(battle.actors[1].availability, ActorAvailability::Dead);
        assert_eq!((battle.actors[0].hp, battle.actors[0].tp), original);
        assert!(!cues.iter().any(|cue| matches!(cue, Cue::Recovered { .. })));
    }
    for (hp, prevents_defeat) in [(1000, false), (1, true)] {
        let mut battle = battle(killer(Side::Party, BOTH), target(Side::Enemy, hp))?;
        resolve(&mut battle, 100, prevents_defeat)?;
        assert!(battle.actors[1].available());
        assert_eq!((battle.actors[0].hp, battle.actors[0].tp), (100, 20));
        assert_eq!(battle.ledger.kills, [0, 0]);
    }
    Ok(())
}

#[test]
fn contact_kill_recovery_is_not_owned_by_victim_or_generic_death() -> Result<()> {
    let mut victim = target(Side::Enemy, 1);
    victim.conditions = Conditions::new(Layers {
        intrinsic: BOTH,
        ..Default::default()
    });
    let mut actual = battle(killer(Side::Party, ConditionSet::EMPTY), victim)?;
    resolve(&mut actual, 100, false)?;
    assert_eq!((actual.actors[0].hp, actual.actors[0].tp), (100, 20));
    assert_eq!(actual.actors[1].hp, 0);
    let mut generic = battle(killer(Side::Party, BOTH), target(Side::Enemy, 0))?;
    generic.enter_death(ActorId(1), &mut vec![]);
    assert_eq!((generic.actors[0].hp, generic.actors[0].tp), (100, 20));
    assert_eq!(generic.ledger.kills, [0, 0]);
    Ok(())
}

#[test]
fn last_hit_recovery_tracks_damage_but_ignores_absorption_and_contacts_after_combat() -> Result<()>
{
    for (affinity, ended) in [
        (Affinity::Normal, false),
        (Affinity::Resistant, false),
        (Affinity::Absorb, false),
        (Affinity::Immune, false),
        (Affinity::Normal, true),
    ] {
        let mut victim = target(Side::Enemy, 400);
        victim.equipment.affinities[0] = affinity;
        let mut actual = battle(killer(Side::Party, ConditionSet::EMPTY), victim)?;
        actual.runtime[1].recovery.last_hit_damage = 149;
        actual.runtime[1].recovery.last_hit_recovery = 73;
        if ended {
            actual.recognize_escape(false)?;
            actual.recognize_result();
        }
        let cues = resolve(&mut actual, 101, false)?;
        let amount = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit {
                    actor: ActorId(1),
                    result,
                    ..
                } => Some(result.amount),
                _ => None,
            })
            .unwrap();
        let expected = if ended || matches!(affinity, Affinity::Absorb | Affinity::Immune) {
            73
        } else {
            amount >> 1
        };
        assert_eq!(actual.runtime[1].recovery.last_hit_recovery, expected);
        assert_eq!(
            actual.runtime[1].recovery.last_hit_damage,
            if ended || matches!(affinity, Affinity::Absorb | Affinity::Immune) {
                149
            } else {
                amount
            }
        );
    }
    Ok(())
}

#[test]
fn contact_kill_recovery_uses_released_projectile_owner_and_skips_owner_dead_after_emission()
-> Result<()> {
    for dead_after_emission in [false, true] {
        // An ally keeps combat live after the projectile owner dies.
        let mut actual = crate::PreparedBattle::new(
            vec![
                (killer(Side::Party, BOTH), Default::default()),
                (target(Side::Enemy, 1), Default::default()),
                (crate::tests::actor(Side::Party), Default::default()),
            ],
            Default::default(),
            1,
        )?
        .finish()?;
        let mut projectile = crate::tests::contact_projectile(false, false);
        projectile.velocity = [0.; 3];
        projectile.acceleration = [0.; 3];
        projectile.offset = [0.; 3];
        projectile.active = None;
        projectile.contact.as_mut().unwrap().hit = rule(100);
        actual.emit(
            Arc::new(projectile),
            ActionId(99),
            ActorId(0),
            ActorId(1),
            [0.; 3],
        )?;
        if dead_after_emission {
            actual.actors[0].hp = 0;
            actual.enter_death(ActorId(0), &mut vec![]);
        }
        actual.step(Default::default())?;
        let cues = actual.step(Default::default())?.cues;
        assert_eq!(actual.actors[1].availability, ActorAvailability::Dead);
        assert_eq!(
            (actual.actors[0].hp, actual.actors[0].tp),
            if dead_after_emission {
                (0, 20)
            } else {
                (200, 25)
            }
        );
        assert_eq!(actual.ledger.kills, [1, 0, 0]);
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Hit {
                source: ContactSource::Projectile(_),
                actor: ActorId(1),
                ..
            }
        )));
    }
    Ok(())
}

#[test]
fn contact_half_damage_budget_updates_guard_and_skips_avoided_contact() -> Result<()> {
    for avoided in [false, true] {
        let mut victim = target(Side::Enemy, 1000);
        if avoided {
            // Party mode4 avoids; enemy mode4 instead takes reduced damage.
            victim.side = Side::Party;
            victim.reaction.protection.mode = crate::ProtectionMode::Recovery;
        } else {
            victim.guard.active = true;
            victim.guard.break_pressure = 10;
            victim.heading = 180.;
            victim.facing_direction = crate::direction_from_heading(180.);
        }
        let owner_side = if avoided { Side::Enemy } else { Side::Party };
        let mut actual = battle(killer(owner_side, ConditionSet::EMPTY), victim)?;
        actual.runtime[1].recovery.last_hit_damage = 149;
        actual.runtime[1].recovery.last_hit_recovery = 73;
        let cues = resolve(&mut actual, 101, false)?;
        let result = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit { result, .. } => Some(result),
                _ => None,
            })
            .unwrap();
        if avoided {
            assert_eq!(result.protection, crate::HitProtection::Avoided);
            assert_eq!(
                (
                    actual.runtime[1].recovery.last_hit_damage,
                    actual.runtime[1].recovery.last_hit_recovery,
                ),
                (149, 73)
            );
        } else {
            assert_eq!(
                result.guard,
                GuardResult::Blocked {
                    first: true,
                    special: false,
                }
            );
            assert_eq!(result.amount, 101);
            assert_eq!(
                (
                    actual.runtime[1].recovery.last_hit_damage,
                    actual.runtime[1].recovery.last_hit_recovery,
                ),
                (101, 50)
            );
        }
    }
    Ok(())
}
