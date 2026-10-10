//! Immutable learning operands from the saved party, independent of executable
//! repertoire. Construct all rows before returning; no runtime owner is installed.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Actor, ActorId, Side,
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
    roster: &[(ActorId, u8)],
    story3: bool,
) -> Result<Vec<TechniqueLearningMember>> {
    let policy = LearningCatalogue::new(catalogue.clone());
    ensure!(
        (1..=resonance_battle::PARTY_CAPACITY).contains(&roster.len()),
        "learning roster is empty or exceeds party bank"
    );
    ensure!(
        roster
            .iter()
            .map(|&(_, character)| character)
            .eq(party.formation.iter().take(4).copied()),
        "learning roster differs from formation"
    );
    let mut actors = BTreeSet::new();
    let mut characters = BTreeSet::new();
    let mut rows = Vec::with_capacity(roster.len());
    for &(actor, character) in roster {
        ensure!(
            (1..=9).contains(&character) && actors.insert(actor) && characters.insert(character),
            "invalid learning roster identity"
        );
        let saved = party
            .members
            .get(usize::from(character - 1))
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
            story_unlocked: story3,
            current: saved.techniques.clone(),
            counts: saved.technique_uses.clone(),
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

    fn roster() -> Vec<(ActorId, u8)> {
        (0..4)
            .map(|index| (ActorId::from_index(index).unwrap(), index as u8 + 1))
            .collect()
    }

    fn actors() -> Vec<Actor> {
        let (menus, member) = crate::battle::party::projection_tests::fixture().unwrap();
        (0..4)
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
        assert_eq!(rows[0].member.counts()[&2], 999);
        assert!(!rows[0].member.current().contains(&2));
        assert_eq!(party.members[0].techniques, [1].into());
    }

    #[test]
    fn invalid_usage_or_duplicate_actor_is_rejected() {
        let mut party = party();
        let roster = roster();
        party.members[3].technique_uses.insert(1, 50);
        assert!(read_inputs(catalogue(), &party, &actors(), &roster, true).is_err());
        party.members[3].technique_uses.clear();
        party.members[0].technique_uses.insert(1, 1000);
        assert!(read_inputs(catalogue(), &party, &actors(), &roster, true).is_err());
        party.members[0].technique_uses.clear();
        let mut duplicate = roster.clone();
        duplicate[3].0 = duplicate[2].0;
        assert!(read_inputs(catalogue(), &party, &actors(), &duplicate, true).is_err());
    }

    #[test]
    fn current_membership_and_assignments_are_validated_before_return() {
        let original = party();
        let roster = roster();
        let mut invalid = original.clone();
        invalid.members[0].disabled_techniques.insert(1);
        assert!(read_inputs(catalogue(), &invalid, &actors(), &roster, true).is_err());
        let mut invalid = original.clone();
        invalid.members[0].shortcuts[3] = 1;
        assert!(read_inputs(catalogue(), &invalid, &actors(), &roster, true).is_err());
        let mut invalid = original.clone();
        invalid.members[3].techniques.insert(1);
        assert!(read_inputs(catalogue(), &invalid, &actors(), &roster, true).is_err());
        let mut invalid = original.clone();
        invalid.members.truncate(3);
        assert!(read_inputs(catalogue(), &invalid, &actors(), &roster, true).is_err());
        let mut wrong_roster = roster.clone();
        wrong_roster.swap(0, 1);
        assert!(read_inputs(catalogue(), &original, &actors(), &wrong_roster, true).is_err());
        assert!(read_inputs(catalogue(), &original, &actors(), &[], true).is_err());
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
            current: Some(1),
            airborne: false,
        };
        admitted[0].equipment.contact.technique_balance = 20;
        let rows = read_inputs(catalogue(), &party, &admitted, &roster()[..1], true)?;
        assert_eq!(
            rows[0].member.select_after_count(attempt, |_| true, || 9)?,
            Some(3)
        );
        assert_eq!(party.members[0].technique_balance, -1);
        admitted[0].equipment.contact.technique_balance = -20;
        let rows = read_inputs(catalogue(), &party, &admitted, &roster()[..1], true)?;
        assert_eq!(
            rows[0].member.select_after_count(attempt, |_| true, || 9)?,
            Some(2)
        );
        Ok(())
    }
}
