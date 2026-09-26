//! A8A8/A730 pending title slots and the ordinary 53914 level checks.
//! Combat eligibility is sampled by the ledger, not reconstructed at victory.
use super::ResultNotice;
use resonance_battle::{Actor, ActorId, TitleEvent};
use resonance_events::party::Party;
use std::collections::BTreeSet;

struct Recipient<'a> {
    character: u8,
    actor: Option<usize>,
    available: bool,
    level: u8,
    conditions: u32,
    learned: &'a BTreeSet<u8>,
}

/// The collector invokes these in order, after encounter/contact title attempts.
/// IDs are the low byte of the original packed character/title argument.
const LEVEL_TITLES: &[(u8, u8, u8)] = &[
    (1, 20, 15),
    (1, 40, 16),
    (1, 100, 17),
    (2, 20, 13),
    (2, 40, 14),
    (2, 100, 15),
    (3, 20, 14),
    (3, 40, 15),
    (4, 20, 11),
    (4, 40, 12),
    (4, 100, 13),
    (5, 40, 12),
    (5, 100, 13),
    (6, 40, 10),
    (6, 100, 11),
    (7, 40, 10),
    (7, 100, 11),
    (8, 40, 10),
    (8, 100, 11),
    (9, 20, 7),
    (9, 40, 8),
    (9, 100, 9),
];

fn pending(recipients: &[Recipient<'_>], events: &[TitleEvent]) -> Vec<Option<u8>> {
    let mut pending = vec![None; recipients.len()];
    for event in events {
        let Some((slot, recipient)) = recipients.iter().enumerate().find(|(_, recipient)| {
            recipient.character == event.character
                && recipient
                    .actor
                    .is_some_and(|actor| event.eligible_actors & (1 << actor) != 0)
        }) else {
            continue;
        };
        if pending[slot].is_none() && !recipient.learned.contains(&event.title) {
            pending[slot] = Some(event.title);
        }
    }
    for &(character, level, title) in LEVEL_TITLES {
        if let Some((slot, recipient)) = recipients
            .iter()
            .enumerate()
            .find(|(_, recipient)| recipient.character == character && recipient.level >= level)
        {
            // A730 tries the available active actor first, then the persistent
            // formation slot with neither KO nor petrification set.
            if pending[slot].is_none()
                && (recipient.available || recipient.conditions & 0x8000_0100 == 0)
                && !recipient.learned.contains(&title)
            {
                pending[slot] = Some(title);
            }
        }
    }
    pending
}

fn selected(recipients: &[Recipient<'_>], events: &[TitleEvent]) -> Vec<(u8, u8)> {
    recipients
        .iter()
        .zip(pending(recipients, events))
        .filter_map(|(recipient, title)| {
            let eligible = if recipient.actor.is_some() {
                recipient.available
            } else {
                recipient.conditions & 0x8000_0100 == 0
            };
            title
                .filter(|_| eligible)
                .map(|title| (recipient.character, title))
        })
        .collect()
}

pub(super) fn award(
    party: &mut Party,
    roster: &[(ActorId, u8)],
    actors: &[Actor],
    events: &[TitleEvent],
) -> Vec<ResultNotice> {
    let recipients: Vec<_> = party
        .formation
        .iter()
        .map(|&character| {
            let member = &party.members[usize::from(character - 1)];
            let actor = roster
                .iter()
                .find(|&&(_, id)| id == character)
                .map(|&(id, _)| id.index());
            Recipient {
                character,
                actor,
                available: actor.is_some_and(|id| actors[id].available()),
                level: member.level,
                conditions: member.conditions,
                learned: &member.titles,
            }
        })
        .collect();
    // 57718 applies active slots before reserves. This occurs before result
    // posture construction changes a KO actor's presentation availability.
    let awards = selected(&recipients, events);
    awards
        .into_iter()
        .map(|(character, title)| {
            party.members[usize::from(character - 1)]
                .titles
                .insert(title);
            ResultNotice::Title { character, title }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lloyd(learned: &BTreeSet<u8>) -> Recipient<'_> {
        Recipient {
            character: 1,
            actor: Some(2),
            available: true,
            level: 3,
            conditions: 0,
            learned,
        }
    }
    fn event(title: u8, eligible_actors: u16) -> TitleEvent {
        TitleEvent {
            character: 1,
            title,
            eligible_actors,
        }
    }

    #[test]
    fn first_unowned_eligible_attempt_wins_in_contact_order() {
        let learned = [1, 18].into();
        let roster = [lloyd(&learned)];
        assert_eq!(
            pending(&roster, &[event(18, 4), event(22, 4), event(19, 4)]),
            [Some(22)]
        );
        assert_eq!(pending(&roster, &[event(19, 4), event(22, 4)]), [Some(19)]);
        // The first Tetra attempt belonged to another actor; later eligibility
        // must not move ahead of Lloyd's earlier eligible combo title.
        assert_eq!(
            pending(&roster, &[event(22, 1), event(19, 4), event(22, 4)]),
            [Some(19)]
        );
    }

    #[test]
    fn level_collector_keeps_pending_awards_and_skips_owned_lower_titles() {
        let learned = [1, 15].into();
        let mut member = lloyd(&learned);
        member.level = 100;
        assert_eq!(pending(&[member], &[]), [Some(16)]);
        let mut member = lloyd(&learned);
        member.level = 100;
        assert_eq!(pending(&[member], &[event(22, 4)]), [Some(22)]);
    }

    #[test]
    fn reserve_level_title_uses_persistent_ko_and_petrification_bits() {
        let learned = [1].into();
        for conditions in [0x8000_0000, 0x100] {
            let mut member = lloyd(&learned);
            member.actor = None;
            member.available = false;
            member.level = 20;
            member.conditions = conditions;
            assert_eq!(pending(&[member], &[]), [None]);
        }
        let mut member = lloyd(&learned);
        member.actor = None;
        member.available = false;
        member.level = 20;
        assert_eq!(pending(&[member], &[]), [Some(15)]);
    }

    #[test]
    fn pending_combat_award_requires_availability_again_before_result_poses() {
        let learned = [1].into();
        let mut member = lloyd(&learned);
        member.available = false;
        member.conditions = 0x8000_0000;
        assert!(selected(&[member], &[event(22, 4)]).is_empty());
        assert_eq!(selected(&[lloyd(&learned)], &[event(22, 4)]), [(1, 22)]);
        assert!(selected(&[lloyd(&learned)], &[event(22, 1)]).is_empty());
    }
}
