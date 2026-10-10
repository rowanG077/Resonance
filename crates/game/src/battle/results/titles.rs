use super::ResultNotice;
use crate::battle::party::Character;
use resonance_battle::{Actor, ActorId, Control, Ledger};
use resonance_events::party::Party;

const WOODEN_BLADE_ENCOUNTER: u16 = 19;
const ETERNAL_APPRENTICE: u8 = 27;
const ASSIST_COMMANDER: u8 = 25;
const TETRA_SLASH: u8 = 22;
const ITEM_KEEPER: u8 = 14;
const ESCAPE_CANCELLER: u8 = 15;
const ESCAPE_VETERAN: u8 = 14;
const GILGAMESH: u8 = 14;
const GILGAMESH_ARMOR: [u16; 3] = [238, 288, 330];
const GILGAMESH_ACCESSORY: u16 = 444;
const GILGAMESH_ARMS: [u16; 2] = [375, 366];
const COMBO_TITLES: &[(u16, u8)] = &[(10, 18), (30, 19), (60, 20), (100, 21)];

// Content title IDs are local to each character.
const LEVEL_TITLES: &[(Character, u8, u8)] = &[
    (Character::Lloyd, 20, 15),
    (Character::Lloyd, 40, 16),
    (Character::Lloyd, 100, 17),
    (Character::Colette, 20, 13),
    (Character::Colette, 40, 14),
    (Character::Colette, 100, 15),
    (Character::Genis, 20, 14),
    (Character::Genis, 40, 15),
    (Character::Raine, 20, 11),
    (Character::Raine, 40, 12),
    (Character::Raine, 100, 13),
    (Character::Sheena, 40, 12),
    (Character::Sheena, 100, 13),
    (Character::Zelos, 40, 10),
    (Character::Zelos, 100, 11),
    (Character::Presea, 40, 10),
    (Character::Presea, 100, 11),
    (Character::Regal, 40, 10),
    (Character::Regal, 100, 11),
    (Character::Kratos, 20, 7),
    (Character::Kratos, 40, 8),
    (Character::Kratos, 100, 9),
];

fn gilgamesh_equipment(equipment: [u16; 6]) -> bool {
    equipment[..3] == GILGAMESH_ARMOR
        && equipment[3..5].contains(&GILGAMESH_ACCESSORY)
        && GILGAMESH_ARMS.contains(&equipment[5])
}

/// Grant every newly eligible title from the completed battle and final party state.
pub(super) fn award(
    party: &mut Party,
    roster: &[(ActorId, u8)],
    actors: &[Actor],
    ledger: &Ledger,
    encounter: u16,
) -> Vec<ResultNotice> {
    let mut notices = Vec::new();
    for &character in &party.formation {
        let member = &mut party.members[usize::from(character - 1)];
        let actor = roster
            .iter()
            .find(|&&(_, id)| id == character)
            .map(|&(id, _)| (id, &actors[id.index()]));
        if !actor.map_or(
            !member.knocked_out() && !member.ailments.petrified,
            |(_, actor)| actor.available(),
        ) {
            continue;
        }
        let owner = Character::try_from(character).expect("validated party character");
        let manual = actor.is_some_and(|(_, actor)| actor.control != Control::Auto);
        let level = member.level;
        let equipment = member.equipment;
        let mut grant = |eligible, title| {
            if eligible && member.titles.insert(title) {
                notices.push(ResultNotice::Title { character, title });
            }
        };
        for &(recipient, minimum, title) in LEVEL_TITLES {
            grant(owner == recipient && level >= minimum, title);
        }
        let Some((id, _)) = actor else { continue };
        match owner {
            Character::Lloyd => {
                for &(hits, title) in COMBO_TITLES {
                    grant(ledger.maximum_combo >= hits, title);
                }
                grant(
                    manual && ledger.normal_variety[id.index()] >= 3,
                    TETRA_SLASH,
                );
                grant(manual && ledger.assist_commands >= 10, ASSIST_COMMANDER);
                grant(
                    encounter == WOODEN_BLADE_ENCOUNTER
                        && !party.battles.lloyd_non_wooden_blade_used,
                    ETERNAL_APPRENTICE,
                );
            }
            Character::Raine => grant(ledger.items[id.index()] >= 5, ITEM_KEEPER),
            Character::Sheena => {
                grant(manual && ledger.escape_cancellations >= 3, ESCAPE_CANCELLER);
                grant(manual && party.battles.sheena_escapes >= 50, ESCAPE_VETERAN);
            }
            Character::Zelos => grant(gilgamesh_equipment(equipment), GILGAMESH),
            _ => {}
        }
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{ActorAvailability, PreparedBattle};

    fn fixture(character: Character) -> (Party, Actor, Ledger) {
        let character = character as u8;
        let (menus, mut member) = crate::battle::party::projection_tests::fixture().unwrap();
        member.level = 1;
        member.titles.clear();
        let loadout =
            crate::battle::party::loadout(&menus, &member, usize::from(character - 1)).unwrap();
        let actor =
            crate::battle::party::actor(&loadout, &member, Control::Manual, [0.; 3], 0.).unwrap();
        let (_, _, _, mut party) = crate::battle::rewards::tests::reward_fixture();
        party.members = vec![member; 9];
        party.formation = vec![character];
        let ledger = PreparedBattle::new(
            vec![(actor.clone(), Default::default())],
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap()
        .ledger()
        .clone();
        (party, actor, ledger)
    }

    fn awarded(
        party: &mut Party,
        actor: Option<&Actor>,
        ledger: &Ledger,
        encounter: u16,
    ) -> Vec<u8> {
        let character = party.formation[0];
        let roster: Vec<_> = actor
            .into_iter()
            .map(|_| (ActorId::from_index(0).unwrap(), character))
            .collect();
        let actors: Vec<_> = actor.into_iter().cloned().collect();
        let mut titles: Vec<_> = award(party, &roster, &actors, ledger, encounter)
            .into_iter()
            .map(|notice| match notice {
                ResultNotice::Title { title, .. } => title,
                _ => unreachable!(),
            })
            .collect();
        titles.sort_unstable();
        titles
    }

    #[test]
    fn completed_battle_grants_all_eligible_titles_once() {
        let (mut party, mut actor, mut ledger) = fixture(Character::Lloyd);
        party.members[0].level = 100;
        party.members[0].titles.insert(18);
        ledger.maximum_combo = 100;
        ledger.normal_variety[0] = 3;
        ledger.assist_commands = 10;
        actor.availability = ActorAvailability::Dead;
        assert!(awarded(&mut party, Some(&actor), &ledger, WOODEN_BLADE_ENCOUNTER).is_empty());
        actor.availability = ActorAvailability::Active;
        assert_eq!(
            awarded(&mut party, Some(&actor), &ledger, WOODEN_BLADE_ENCOUNTER),
            [
                15,
                16,
                17,
                19,
                20,
                21,
                TETRA_SLASH,
                ASSIST_COMMANDER,
                ETERNAL_APPRENTICE
            ]
        );
        assert!(awarded(&mut party, Some(&actor), &ledger, WOODEN_BLADE_ENCOUNTER).is_empty());

        party.members[0].titles.clear();
        assert_eq!(
            awarded(&mut party, None, &ledger, WOODEN_BLADE_ENCOUNTER),
            [15, 16, 17]
        );
    }

    #[test]
    fn item_title_uses_the_recipients_actor_slot() {
        let (mut party, actor, mut ledger) = fixture(Character::Raine);
        let roster = [(ActorId::from_index(1).unwrap(), Character::Raine as u8)];
        let actors = [actor.clone(), actor];
        ledger.items = vec![5, 4];
        assert!(award(&mut party, &roster, &actors, &ledger, 0).is_empty());
        ledger.items[1] = 5;
        assert!(
            matches!(award(&mut party, &roster, &actors, &ledger, 0).as_slice(),
            [ResultNotice::Title { character, title: ITEM_KEEPER }] if *character == Character::Raine as u8)
        );
    }

    #[test]
    fn escape_achievements_can_be_awarded_together() {
        let (mut party, actor, mut ledger) = fixture(Character::Sheena);
        ledger.escape_cancellations = 3;
        party.battles.sheena_escapes = 50;
        assert_eq!(
            awarded(&mut party, Some(&actor), &ledger, 0),
            [ESCAPE_VETERAN, ESCAPE_CANCELLER]
        );
    }

    #[test]
    fn equipment_title_checks_its_wearer() {
        let (mut party, mut actor, ledger) = fixture(Character::Zelos);
        let equipment = [238, 288, 330, 444, 0, 366];
        party.members[0].equipment = equipment;
        assert!(awarded(&mut party, Some(&actor), &ledger, 0).is_empty());
        party.members[Character::Zelos as usize - 1].equipment = equipment;
        actor.control = Control::Auto;
        assert_eq!(awarded(&mut party, Some(&actor), &ledger, 0), [GILGAMESH]);
    }
}
