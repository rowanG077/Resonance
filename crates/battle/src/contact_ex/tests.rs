use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::{Activity, Actor, Affinity, GuardResult, HitProtection, HitResult, PreparedBattle};

fn hit(amount: i32) -> HitResult {
    HitResult {
        amount,
        hp_change: -amount,
        critical: false,
        boosted: false,
        affinity: Affinity::Normal,
        guard: GuardResult::None,
        protection: HitProtection::None,
    }
}

fn candidate(actors: Vec<Actor>) -> PreparedBattle {
    PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )
    .unwrap()
}

fn battle(actors: Vec<Actor>) -> Battle {
    candidate(actors).finish().unwrap()
}

fn rule() -> crate::HitRule {
    crate::HitRule {
        kind: crate::DamageKind::Slash,
        arte: true,
        overlimit_pause: false,
        power: crate::Power::Fixed(7),
        element: crate::HitElement::Neutral,
        prevents_defeat: false,
        reaction: Default::default(),
        guard: crate::GuardRule {
            enabled: false,
            pressure: 0,
            breaks: false,
            unbreakable: false,
        },
        condition: None,
    }
}

#[test]
fn melee_contact_counts_once_and_applies_retaliation_only_to_survivors() -> Result<()> {
    for (initial, hp) in [(7_u8, 50), (u8::MAX, 50), (7, 1)] {
        let owner = crate::tests::actor(Side::Party);

        let mut victim = crate::tests::actor(Side::Enemy);
        victim.body.collider = Some(crate::Collider::sphere(2.));
        victim.reaction.contributor_hits[0] = initial;
        victim.hp = hp;
        victim.equipment.luck = u16::MAX;
        victim.equipment.reaction_ex.aid_revenge = true;
        victim.equipment.reaction_ex.reflect_damage = true;
        let mut battle = battle(vec![owner, victim]);
        let mut contacts = crate::contact::Contacts::default();
        contacts.melee(
            ActorId(0),
            crate::ActionId(1),
            &crate::MeleeDefinition {
                hit: rule(),
                trail: None,
                volume: crate::MeleeVolume {
                    offset: [0.; 3],
                    radius: 20.,
                    half_height: 20.,
                },
            },
            &[],
        )?;
        let mut cues = vec![];
        contacts.resolve(&mut battle, &mut cues)?;
        assert_eq!(
            battle.actors[1].reaction.contributor_hits[0],
            initial.saturating_add(1)
        );
        if hp == 1 {
            assert_eq!(battle.activity(ActorId(1)), Activity::Defeated);
            assert_eq!(battle.actors[0].hp, 50);
            assert!(
                !cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::ExSkillLabel { .. }))
            );
        } else {
            assert_eq!(battle.actors[1].reaction.combo_hits, 1);
            assert_eq!(battle.actors[1].hp, hp - 7 + 20);
            assert_eq!(battle.actors[0].hp, 43);
        }
        assert_eq!(
            cues.iter().filter(|c| matches!(c, Cue::Hit { .. })).count(),
            1
        );
    }
    Ok(())
}

#[test]
fn reflection_uses_actual_damage_and_never_defeats_or_revives_the_attacker() -> Result<()> {
    for (hp, received, expected) in [
        (0, 20, 0),
        (1, 20, 1),
        (10, 20, 1),
        (100_000, 20, 99_980),
        (100_000, 65_535, 34_465),
    ] {
        let mut attacker = crate::tests::actor(Side::Enemy);
        attacker.hp = hp;
        attacker.equipment.max_hp = 100_000;
        if hp == 0 {
            attacker.availability = crate::ActorAvailability::Dead;
        }
        let mut defender = crate::tests::actor(Side::Party);
        defender.equipment.reaction_ex.reflect_damage = true;
        defender.equipment.luck = u16::MAX;
        let mut battle = battle(vec![attacker, defender]);
        let mut result = hit(65_535);
        result.hp_change = -received;
        let mut cues = vec![];
        battle.contact_retaliation(ActorId(0), ActorId(1), result, &mut cues)?;
        assert_eq!(battle.actors[0].hp, expected);
        if expected == hp {
            assert!(cues.is_empty());
            assert_ne!(battle.activity(ActorId(0)), Activity::Hurt);
        } else {
            assert!(cues.contains(&Cue::IncidentalDamage {
                actor: ActorId(0),
                amount: hp - expected
            }));
            assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
        }
    }
    Ok(())
}

#[test]
fn retaliation_heals_immediately_and_its_projectile_survives_interruption() -> Result<()> {
    let mut defender = crate::tests::actor(Side::Party);
    defender.equipment.luck = u16::MAX;
    defender.equipment.reaction_ex = ContactEx {
        hammer_revenge: true,
        aid_revenge: true,
        reflect_damage: true,
        ..Default::default()
    };
    defender.attack_power = 185;
    defender.tp = 7;
    let mut attacker = crate::tests::actor(Side::Enemy);
    attacker.position = [90., 0., 0.];

    attacker.body.collider = Some(crate::Collider::sphere(2.));
    // Gameplay needs no effect bank or actor-specific projectile definition.
    let mut battle = battle(vec![attacker, defender, crate::tests::actor(Side::Enemy)]);
    battle.runtime[1].target = ActorId(2);
    let mut cues = vec![];
    battle.contact_retaliation(ActorId(0), ActorId(1), hit(10), &mut cues)?;
    assert_eq!(battle.actors[0].hp, 40);
    assert_eq!(battle.activity(ActorId(0)), Activity::Hurt);
    assert_eq!(battle.actors[1].hp, 70);
    assert_eq!(battle.actors[1].tp, 7);
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Recovered {
            actor: ActorId(1),
            applied: 20,
            ..
        }
    )));
    assert!(
        cues.iter()
            .any(|cue| matches!(cue, Cue::HammerRevenge { projectile }
        if battle.projectiles.contains_key(projectile)))
    );
    battle.begin_hurt(ActorId(1), 3, &mut cues);
    let frame = battle.step(Default::default())?;
    assert_eq!(frame.projectiles.len(), 1);
    let projectile = battle.projectiles.values().next().unwrap();
    assert_eq!(projectile.frame.owner, ActorId(1));
    assert_eq!(projectile.frame.target, ActorId(0));
    assert_eq!(projectile.attack_power, 185);
    let id = projectile.frame.id;
    assert!(projectile.frame.position[1] > battle.actors[0].effect_origin()[1]);
    let mut contacts = vec![];
    for _ in 0..40 {
        let frame = battle.step(Default::default())?;
        for cue in frame.cues {
            if let Cue::Hit {
                source: crate::ContactSource::Projectile(projectile),
                actor,
                ..
            } = cue
                && projectile == id
            {
                contacts.push(actor);
            }
        }
    }
    assert_eq!(contacts, [ActorId(0)]);
    assert!(battle.projectiles.is_empty());
    assert!(battle.actors[0].hp < 40);
    assert_eq!(battle.actors[1].hp, 70);
    Ok(())
}

#[test]
fn hurt_exit_recovers_attributed_vitals_and_clears_contributions() -> Result<()> {
    let mut party = crate::tests::actor(Side::Party);
    party.hp = 50;
    party.tp = 0;
    party.equipment.max_hp = 1000;
    party.equipment.max_tp = 1000;
    party.equipment.recovery.boost = true;
    party.equipment.recovery.lucky = true; // Lucky does not alter combo recovery.
    party.equipment.reaction_ex.combo_hp = true;
    party.equipment.reaction_ex.combo_tp = true;
    let mut victim = crate::tests::actor(Side::Enemy);
    victim.tp = 0;
    victim.equipment.max_tp = 1000;
    victim.equipment.reaction_ex.damage_tp = true;
    victim.reaction.combo_damage = 333;
    victim.reaction.combo_hits = 10;
    victim.reaction.contributor_hits[0] = 10; // The boost raises HP recovery from 5% to 6%.
    let mut battle = battle(vec![party, victim]);
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(false));

    battle.begin_hurt(ActorId(1), 1, &mut vec![]);
    let cues = battle.step(crate::BattleInput::default())?.cues;
    assert_eq!(battle.actors[0].hp, 110);
    assert_eq!(battle.actors[0].tp, 50);
    assert_eq!(battle.actors[1].tp, 9);
    assert_eq!(battle.actors[1].reaction.contributor_hits, [0; 12]);
    assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
    let mut labels: Vec<_> = cues
        .iter()
        .filter_map(|c| match c {
            Cue::ExSkillLabel { actor, .. } => Some(*actor),
            _ => None,
        })
        .collect();
    labels.sort();
    assert_eq!(labels, [ActorId(0), ActorId(1)]);
    battle.step(crate::BattleInput::default())?;
    assert_eq!(battle.actors[0].tp, 50);
    assert_eq!(battle.actors[1].tp, 9);
    assert!(!battle.is_diagnostic());

    Ok(())
}

#[test]
fn knockdown_initialization_bounds_follow_up_and_recovers_tp_once() -> Result<()> {
    let mut actor = crate::tests::actor(Side::Party);
    actor.reaction.combo_hits = 120;
    actor.equipment.reaction_ex.follow_up = true;
    actor.equipment.reaction_ex.down_tp = true;
    actor.equipment.luck = u16::MAX;
    actor.tp = 0;
    actor.equipment.max_tp = 19; //5% zero falls back to1.
    let mut battle = battle(vec![actor, crate::tests::actor(Side::Enemy)]);
    let mut cues = vec![];
    battle.initialize_knockdown_window(0, &mut cues);
    assert_eq!(battle.actors[0].reaction.stagger.window, 6);
    assert_eq!(battle.actors[0].tp, 1);
    battle.initialize_knockdown_window(0, &mut cues);
    assert_eq!(battle.actors[0].tp, 1);
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(c, Cue::ExSkillLabel { .. }))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn held_hurt_defers_recovery_then_large_tp_recovery_caps_at_capacity() -> Result<()> {
    let mut victim = crate::tests::actor(Side::Enemy);
    victim.equipment.reaction_ex.damage_tp = true;
    victim.reaction.combo_damage = 2_000_000;
    victim.tp = 6000;
    victim.equipment.max_tp = 7000;
    let mut battle = battle(vec![crate::tests::actor(Side::Party), victim]);
    battle.begin_hurt(ActorId(1), 1, &mut vec![]);
    battle.actors[1].hit_stop = 1;
    let cues = battle.step(crate::BattleInput::default())?.cues;
    assert_eq!(battle.actors[1].tp, 6000);
    assert!(cues.is_empty());
    battle.step(crate::BattleInput::default())?;
    assert_eq!(battle.actors[1].tp, battle.actors[1].equipment.max_tp);
    assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
    Ok(())
}

#[test]
fn combo_recovery_preserves_weak_above_cap_skips_defeated_and_has_minimum_one() -> Result<()> {
    let mut party = crate::tests::actor(Side::Party);
    party.equipment.max_hp = 1000;
    party.hp = 700;
    party.tp = 0;
    party.equipment.max_tp = 19;
    party.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        base: ConditionSet::of(&[Condition::Weak]),
        ..Default::default()
    });
    party.equipment.reaction_ex.combo_hp = true;
    party.equipment.reaction_ex.combo_tp = true;
    let mut dead = party.clone();
    dead.hp = 0;
    dead.availability = crate::ActorAvailability::Dead;
    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.reaction.contributor_hits[0] = 2;
    enemy.reaction.contributor_hits[1] = 254;
    let mut battle = battle(vec![party, dead, enemy]);
    let mut cues = vec![];
    battle.finish_combo_recovery(ActorId(2), &mut cues);
    assert_eq!(battle.actors[0].hp, 700);
    assert_eq!(battle.actors[0].tp, 1);
    assert_eq!(battle.actors[1].hp, 0);
    assert_eq!(battle.actors[1].tp, 0);
    assert!(cues.iter().any(|c| matches!(
        c,
        Cue::Recovered {
            kind: crate::RecoveryKind::Hp,
            actor: ActorId(0),
            nominal: 10,
            applied: 0
        }
    )));
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(c, Cue::ExSkillLabel { .. }))
            .count(),
        1
    );
    Ok(())
}
