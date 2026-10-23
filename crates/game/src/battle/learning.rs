//! Immutable learning operands from the saved party, independent of executable
//! repertoire. Construct all rows before returning; no runtime owner is installed.
use super::party::Character;
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Actor, ActorId, Side, TechniqueId,
    learning::{LearningCatalogue, LearningEntry, TechniqueLearningMember},
};
use resonance_content::arte::Catalogue;
use resonance_events::party::Party;
use std::{collections::BTreeSet, sync::Arc};

/// Read saved learning prerequisites independently of executable repertoire.
pub fn read_inputs(
    catalogue: Arc<Catalogue>,
    party: &Party,
    prepared: &[Actor],
    roster: &[(ActorId, Character)],
    colette_learning_unlocked: bool,
) -> Result<Vec<TechniqueLearningMember>> {
    let policy = LearningCatalogue::new(catalogue.clone())
        .with_elemental_learning(party.settings.preferences.learn_elemental_artes)
        .with_both_arte_paths(party.settings.preferences.learn_both_arte_paths);
    ensure!(
        (1..=resonance_battle::PARTY_CAPACITY).contains(&roster.len()),
        "learning roster is empty or exceeds party bank"
    );
    ensure!(
        roster
            .iter()
            .map(|&(_, character)| character as u8)
            .eq(party.formation.iter().take(4).copied()),
        "learning roster differs from formation"
    );
    let mut actors = BTreeSet::new();
    let mut characters = BTreeSet::new();
    let mut rows = Vec::with_capacity(roster.len());
    for &(actor, character) in roster {
        ensure!(
            actors.insert(actor) && characters.insert(character),
            "invalid learning roster identity"
        );
        let saved = party
            .members
            .get(character.index())
            .context("missing learning member")?;
        ensure!(
            saved.disabled_techniques.is_subset(&saved.techniques)
                && saved
                    .shortcuts
                    .iter()
                    .all(|id| *id == 0 || saved.techniques.contains(id)),
            "invalid current technique assignment"
        );
        let admitted = prepared
            .get(actor.index())
            .context("missing prepared learning actor")?;
        ensure!(
            admitted.side == Side::Party,
            "learning actor is not a party member"
        );
        let member = policy.prepare_member(LearningEntry {
            character,
            level: saved.level,
            balance: admitted.equipment.contact.technique_balance,
            story_unlocked: colette_learning_unlocked,
            current: saved.techniques.iter().copied().map(TechniqueId).collect(),
            counts: saved
                .technique_uses
                .iter()
                .map(|(&id, &count)| (TechniqueId(id), count))
                .collect(),
        })?;
        rows.push(TechniqueLearningMember { actor, member });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogue() -> Arc<Catalogue> {
        let mut data = Catalogue {
            definitions: vec![Default::default(); 4],
            learning: vec![vec![1, 2, 3], vec![], vec![], vec![]],
        };
        data.definitions[1].learning.technical_successor = Some(2);
        data.definitions[1].learning.strike_successor = Some(3);
        for row in &mut data.definitions[2..] {
            row.required_level = 1;
            row.learning.parent = Some(1);
            row.learning.parent_uses = 50;
        }
        Arc::new(data)
    }

    fn party() -> Party {
        let (_, _, _, mut party) = crate::battle::rewards::tests::reward_fixture();
        party.formation = vec![1, 2, 3, 4];
        for member in &mut party.members {
            member.level = 50;
            member.experience = 0;
            member.base_stats = [100, 20, 1, 1, 1, 1, 1];
        }
        party
    }

    fn roster() -> Vec<(ActorId, Character)> {
        Character::ALL[..4]
            .iter()
            .copied()
            .enumerate()
            .map(|(index, character)| (ActorId::from_index(index).unwrap(), character))
            .collect()
    }

    fn actors() -> Vec<Actor> {
        let (menus, member) = crate::battle::party::projection_tests::fixture().unwrap();
        Character::ALL[..4]
            .iter()
            .copied()
            .map(|character| {
                let loadout = crate::battle::party::loadout(&menus, &member, character).unwrap();
                crate::battle::party::actor(
                    &loadout,
                    &member,
                    resonance_battle::Control::Manual,
                    [0.; 3],
                    0.,
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn preparation_preserves_forgotten_technique_use_counts() {
        let mut party = party();
        party.members[0].techniques.insert(1);
        party.members[0].shortcuts = [1, 0, 0, 0];
        party.members[0].technique_uses.insert(2, 999);
        let roster = roster();
        let rows = read_inputs(catalogue(), &party, &actors(), &roster, false).unwrap();
        assert_eq!(rows[0].member.counts()[&TechniqueId(2)], 999);
        assert!(!rows[0].member.current().contains(&TechniqueId(2)));
        assert_eq!(party.members[0].techniques, [1].into());
    }

    #[test]
    fn learning_uses_admitted_balance_when_drift_changes_route_and_probability() -> Result<()> {
        use resonance_battle::learning::{LearningAttempt, LearningMode};
        let mut party = party();
        party.formation = vec![1];
        party.members[0].technique_balance = -1;
        party.members[0].techniques.insert(1);
        party.members[0].technique_uses.insert(1, 50);
        let mut admitted = actors();
        let attempt = LearningAttempt {
            mode: LearningMode::Martial,
            current: Some(TechniqueId(1)),
            airborne: false,
        };
        admitted[0].equipment.contact.technique_balance = 20;
        let rows = read_inputs(catalogue(), &party, &admitted, &roster()[..1], true)?;
        assert_eq!(
            rows[0].member.select_after_count(attempt, |_| true, || 9)?,
            Some(TechniqueId(3))
        );
        assert_eq!(party.members[0].technique_balance, -1);
        admitted[0].equipment.contact.technique_balance = -20;
        let rows = read_inputs(catalogue(), &party, &admitted, &roster()[..1], true)?;
        assert_eq!(
            rows[0].member.select_after_count(attempt, |_| true, || 9)?,
            Some(TechniqueId(2))
        );
        party.settings.preferences.learn_both_arte_paths = true;
        party.members[0].techniques.insert(2);
        let rows = read_inputs(catalogue(), &party, &admitted, &roster()[..1], true)?;
        assert_eq!(
            rows[0]
                .member
                .select_after_count(attempt, |_| true, || panic!("random roll"))?,
            Some(TechniqueId(3))
        );
        assert_eq!(party.members[0].techniques, [1, 2].into());
        assert_eq!(party.members[0].technique_uses[&1], 50);
        Ok(())
    }
}
