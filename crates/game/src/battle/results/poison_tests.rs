//! Poison persistence and reward rules use native state; one content scenario covers item bindings.
use super::item_tests::{
    enter_battle, escape, native_fixture, prepared_fixture_with_party, release,
};
use super::*;
use resonance_battle::{
    PreparedBattle,
    conditions::{
        Condition::{PoisonMild, PoisonSevere},
        Conditions, Layers, POISON,
    },
    item::Release,
};

use resonance_events::party::Poison;

fn poisoned(candidate: &Candidate, battle: &Battle, character: u8) -> Result<bool> {
    Ok(candidate
        .victory_context(battle)?
        .party
        .iter()
        .find(|member| member.character as u8 == character)
        .context("missing poison participant")?
        .poisoned)
}

#[test]
fn saved_poison_roundtrips_through_completion_and_fresh_entry() -> Result<()> {
    let prepare = |previous: Option<Party>| {
        native_fixture(
            &[4, 1],
            |party, _, _| {
                if let Some(previous) = previous {
                    *party = previous;
                } else {
                    party.members[3].ailments.poison = Poison::Both;
                }
                Ok(())
            },
            |actors, _| {
                PreparedBattle::new(
                    (actors)
                        .into_iter()
                        .map(|actor| (actor, Default::default()))
                        .collect(),
                    Default::default(),
                    1,
                )?
                .finish()
            },
        )
    };
    let mut f = prepare(None)?;
    let original = serde_json::to_value(&f.field)?;
    let outcome = escape(&mut f.candidate, &mut f.battle)?;
    let completed = f.candidate.finish(&f.battle, &outcome)?;
    let saved: Party = serde_json::from_slice(&serde_json::to_vec(&completed.party)?)?;
    assert_eq!(saved.members[3].ailments.poison, Poison::Both);
    assert_eq!(serde_json::to_value(&f.field)?, original);
    let next = prepare(Some(saved))?;
    let actor = &next.battle.actors()[0];
    assert_eq!(actor.conditions.base().intersection(POISON), POISON);
    assert_eq!(actor.conditions.periodic_effects().len(), 2);
    assert_eq!(actor.availability, ActorAvailability::Active);
    assert!(poisoned(&next.candidate, &next.battle, 4)?);
    assert!(!next.battle.actors()[1].conditions.base().intersects(POISON));
    Ok(())
}

#[test]
#[ignore = "requires composed Poison/Petrify/Items/profile/script publications; CPU only"]
fn real_panacea_and_miracle_clear_saved_poison_through_candidate_item_and_finish() -> Result<()> {
    // Both inventory policies reach the shared cure and persistent handoff.
    for item in [10, 12] {
        let (user_slot, target_slot) = (0, 1);
        let mut f = prepared_fixture_with_party(&[4, 1], 2, &[], |party, _, _| {
            party.members[0].ailments.poison = Poison::Both;
            Ok(())
        })?;
        enter_battle(&mut f.candidate, &mut f.battle)?;
        let (user, user_character) = f.candidate.setup.actors[user_slot];
        let (target, character) = f.candidate.setup.actors[target_slot];
        f.candidate
            .party
            .change_item(&f.candidate.session, item, 1)
            .map_err(anyhow::Error::msg)?;
        let uses = f.candidate.party.battles.items[usize::from(user_character - 1)];
        let target_before = f.battle.actors()[target.index()].clone();
        assert!(f.battle.item_target_eligible(item, target)?);
        release(
            &mut f.candidate,
            &mut f.battle,
            Release { user, target, item },
        )?;
        assert!(!f.battle.is_diagnostic());
        assert!(
            !f.battle.actors()[target.index()]
                .conditions
                .base()
                .intersects(POISON)
        );
        assert_eq!(f.battle.actors()[target.index()].tp, target_before.tp);
        assert!(f.battle.actors()[target.index()].hp <= target_before.hp);
        assert!(!f.battle.item_target_eligible(item, target)?);
        assert!(!poisoned(&f.candidate, &f.battle, character)?);
        assert!(!f.candidate.party.items.contains_key(&item));
        assert_eq!(f.battle.ledger().items[user.index()], 1);
        assert_eq!(
            f.candidate.party.battles.items[usize::from(user_character - 1)],
            uses
        );
        f.candidate.sync_party(&f.battle)?;
        assert_eq!(
            f.candidate.party.members[usize::from(character - 1)]
                .ailments
                .poison,
            Poison::None
        );
        let outcome = escape(&mut f.candidate, &mut f.battle)?;
        let completed = f.candidate.finish(&f.battle, &outcome)?;
        let saved: Party = serde_json::from_slice(&serde_json::to_vec(&completed.party)?)?;
        assert_eq!(
            saved.battles.items[usize::from(user_character - 1)],
            uses + 1
        );
        assert_eq!(
            saved.members[usize::from(character - 1)].ailments.poison,
            Poison::None
        );
        assert_eq!(
            f.field.members[usize::from(character - 1)].ailments.poison,
            Poison::Both
        );
    }
    Ok(())
}

#[test]
fn results_query_and_reward_grade_read_effective_poison_but_export_reads_base() -> Result<()> {
    for (layers, query, grade, saved) in [
        (Layers::default(), false, 400, Poison::None),
        (
            Layers {
                base: PoisonMild.into(),
                ..Default::default()
            },
            true,
            300,
            Poison::Mild,
        ),
        (
            Layers {
                intrinsic: PoisonSevere.into(),
                ..Default::default()
            },
            true,
            300,
            Poison::None,
        ),
        (
            Layers {
                equipment_overlay: POISON,
                ..Default::default()
            },
            true,
            300,
            Poison::None,
        ),
    ] {
        let mut f = native_fixture(
            &[4, 1],
            |_, _, _| Ok(()),
            |mut actors, _| {
                for actor in &mut actors {
                    actor.hp = if actor.side == resonance_battle::Side::Enemy {
                        0
                    } else {
                        actor.equipment.max_hp
                    };
                    actor.tp = actor.equipment.max_tp;
                }
                PreparedBattle::new(
                    (actors)
                        .into_iter()
                        .map(|actor| (actor, Default::default()))
                        .collect(),
                    Default::default(),
                    1,
                )?
                .finish()
            },
        )?;
        let (id, character) = f.candidate.setup.actors[0];
        let battle = &mut f.battle;
        assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));
        battle.retire_combat()?;
        let actor = battle.actors()[id.index()].clone();
        battle.refresh_result_member_conditions(
            id,
            actor.hp,
            actor.equipment.max_hp,
            actor.tp,
            actor.equipment.max_tp,
            actor.overlimit,
            actor.is_petrified(),
            Conditions::new(layers),
        )?;
        assert_eq!(poisoned(&f.candidate, battle, character)?, query);
        f.candidate.setup.level_difference = 0;
        for enemy in &mut f.candidate.setup.enemies {
            enemy.grade = 0;
        }
        f.candidate.selection = Some(Selection {
            actor: id,
            character,
            pose: Some(0),
            group: None,
        });
        f.candidate.construct_rewards(battle)?;
        assert_eq!(f.candidate.results.as_ref().unwrap().grade, grade);
        assert_eq!(
            f.candidate.party.members[usize::from(character - 1)]
                .ailments
                .poison,
            saved
        );
        assert_eq!(battle.actors()[id.index()].conditions.layers(), layers);
    }
    Ok(())
}
