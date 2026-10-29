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

/// Grant every newly eligible title from the completed battle and final party state.
pub(super) fn award(
    rules: &resonance_content::menu_data::TitleAwardRules,
    party: &mut Party,
    roster: &[(ActorId, Character)],
    actors: &[Actor],
    ledger: &Ledger,
    encounter: u16,
    level_difference: i8,
) -> Vec<ResultNotice> {
    let mut notices = Vec::new();
    let women = roster
        .iter()
        .filter(|&&(_, character)| {
            matches!(
                character,
                Character::Colette | Character::Raine | Character::Sheena | Character::Presea
            )
        })
        .count();
    let men = roster.len() - women;
    let survivors = roster
        .iter()
        .filter(|&&(id, _)| actors[id.index()].available())
        .count();
    for &character in &party.formation {
        let character = Character::try_from(character).expect("validated party character");
        let member = &mut party.members[character.index()];
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
        let manual = actor.is_some_and(|(_, actor)| actor.control != Control::Auto);
        let level = member.level;
        let equipment = member.equipment;
        let steal_uses: u32 = [223, 224]
            .iter()
            .map(|id| u32::from(*member.technique_uses.get(id).unwrap_or(&0)))
            .sum();
        let mut earned = Vec::new();
        let mut grant = |eligible, title| {
            if eligible && member.titles.insert(title) {
                earned.push(ResultNotice::Title { character, title });
            }
        };
        for rule in &rules.levels {
            grant(
                character as u8 == rule.character && level >= rule.minimum,
                rule.title,
            );
        }
        let Some((id, actor)) = actor else {
            notices.extend(earned);
            continue;
        };
        let facts = &ledger.achievements[id.index()];
        for rule in &rules.combos {
            grant(
                character as u8 == rule.character && ledger.maximum_combo >= rule.minimum,
                rule.title,
            );
        }
        match character {
            Character::Lloyd => {
                grant(
                    manual && ledger.normal_variety[id.index()] >= 3,
                    TETRA_SLASH,
                );
                grant(facts.commanded_alone, 24); // Lone General
                grant(facts.assist_commands >= 10, ASSIST_COMMANDER);
                grant(party.battles.hard_victories >= 256, 28); // Berserker
                grant(encounter == 47 && !party.battles.ordinary_escape_used, 23); // Brave Soul
                grant(encounter == 53 && level_difference <= -8, 26); // Gung Ho
                grant(
                    encounter == WOODEN_BLADE_ENCOUNTER
                        && !party.battles.lloyd_non_wooden_blade_used,
                    ETERNAL_APPRENTICE,
                );
            }
            Character::Colette => {
                grant(steal_uses >= 400, 16); // Little Pickpocket
                grant(
                    facts.stayed_poisoned
                        && actor
                            .conditions
                            .effective()
                            .intersects(resonance_battle::conditions::POISON),
                    17,
                ); // Oblivious
                grant(manual && facts.repeated_technique >= 10, 18); // Single-minded
                grant(manual && !facts.used_arte, 19); // Self-control
                grant(manual && !facts.ran, 20); // Don't Run!
                grant(
                    encounter == 15
                        && !party.battles.has_death_this_run()
                        && roster.iter().all(|&(id, _)| ledger.deaths[id.index()] == 0),
                    21,
                ); // Friendship First
            }
            Character::Genis => {
                grant(facts.weapon_changes >= 5, 16); // Experimental
                grant(facts.resisted_spells >= 15, 17); // Study Harder!
                grant(manual && !facts.acted, 18); // Dependent
                grant(facts.spell_elements & 0b11_1111 == 0b11_1111, 19); // Magic Cycle
                grant(encounter == 38 && !party.battles.battle_gel_used, 20); // I Hate Gels!
            }
            Character::Raine => {
                grant(ledger.items[id.index()] >= 5, ITEM_KEEPER);
                grant(roster.len() == 4 && women == 4, 15); // Crimson Rose
                grant(ledger.deaths[id.index()] >= 5, 16); // Never Say Never
                grant(roster.len() >= 2 && survivors == 1, 17); // Survivor
            }
            Character::Sheena => {
                grant(manual && ledger.escape_cancellations >= 3, ESCAPE_CANCELLER);
                grant(manual && party.battles.sheena_escapes >= 50, ESCAPE_VETERAN);
                grant(manual && facts.unison_starts >= 2, 16); // Party Comboist
                grant(
                    manual
                        && ledger
                            .achievements
                            .iter()
                            .any(|facts| facts.unison_starts != 0)
                        && !ledger.damage_outside_unison,
                    17,
                ); // Combo Conductor
            }
            Character::Zelos => {
                grant(manual && ledger.normal_variety[id.index()] >= 3, 12); // Tetra Slash
                grant(roster.len() == 4 && women == 3, 13); // Casanova
                grant(facts.strategy_orders >= 4, 15); // Commander-in-Chief
                grant(facts.voices >= 50, 16); // Loudmouth
            }
            Character::Presea => {
                grant(facts.guard_breaks >= 10, 12); // Fragile Shield
                grant(roster.len() == 4 && men == 3, 13); // Lone Girl
                grant(
                    roster.len() == 4
                        && [
                            Character::Sheena,
                            Character::Zelos,
                            Character::Presea,
                            Character::Regal,
                        ]
                        .into_iter()
                        .all(|character| roster.iter().any(|&(_, active)| active == character)),
                    14,
                ); // Associate
                grant(manual && party.battles.hunting_streak >= 5, 15); // Hunter
            }
            Character::Regal => {
                grant(roster.len() == 4 && men == 4, 12); // Testosterone
                grant(facts.potions >= 5, 13); // Potion King
                grant(level_difference >= 8, 14); // Way of the Jungle
                grant(facts.knockdowns >= 3, 15); // Pratfall King
            }
            Character::Kratos => {
                grant(manual && ledger.normal_variety[id.index()] >= 3, 10); // Tetra Slash
            }
        }
        for rule in &rules.equipment {
            if character as u8 != rule.character
                || !rule.matches(&equipment)
                || !member.titles.insert(rule.title)
            {
                continue;
            }
            // Insert equipment awards in catalogue order among this member's
            // notices, retaining the shipped rules' existing result order.
            let position = earned
                .iter()
                .position(|notice| {
                    matches!(notice,
                ResultNotice::Title { title, .. } if *title > rule.title)
                })
                .unwrap_or(earned.len());
            earned.insert(
                position,
                ResultNotice::Title {
                    character,
                    title: rule.title,
                },
            );
        }
        notices.extend(earned);
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{ActorAvailability, PreparedBattle};

    fn award_rules() -> resonance_content::menu_data::TitleAwardRules {
        use resonance_content::menu_data::{
            ComboTitleAward, EquipmentRequirement, EquipmentTitleAward, TitleAwardRules,
        };
        TitleAwardRules {
            levels: [(20, 15), (40, 16), (100, 17)]
                .into_iter()
                .map(
                    |(minimum, title)| resonance_content::menu_data::LevelTitleAward {
                        character: Character::Lloyd as u8,
                        title,
                        minimum,
                    },
                )
                .collect(),
            combos: [(10, 18), (30, 19), (60, 20), (100, 21)]
                .into_iter()
                .map(|(minimum, title)| ComboTitleAward {
                    character: 1,
                    title,
                    minimum,
                })
                .collect(),
            equipment: vec![EquipmentTitleAward {
                character: 6,
                title: 14,
                requirements: [
                    (vec![0], vec![238]),
                    (vec![1], vec![288]),
                    (vec![2], vec![330]),
                    (vec![3, 4], vec![444]),
                    (vec![5], vec![375, 366]),
                ]
                .into_iter()
                .map(|(slots, items)| EquipmentRequirement { slots, items })
                .collect(),
            }],
        }
    }

    fn fixture(character: Character) -> (Party, Actor, Ledger) {
        let (menus, mut member) = crate::battle::party::projection_tests::fixture().unwrap();
        member.level = 1;
        member.titles.clear();
        let loadout = crate::battle::party::loadout(&menus, &member, character).unwrap();
        let actor =
            crate::battle::party::actor(&loadout, &member, Control::Manual, [0.; 3], 0.).unwrap();
        let (_, _, _, mut party) = crate::battle::rewards::tests::reward_fixture();
        party.members = vec![member; 9];
        party.formation = vec![character as u8];
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
        let character = Character::try_from(party.formation[0]).unwrap();
        let roster: Vec<_> = actor
            .into_iter()
            .map(|_| (ActorId::from_index(0).unwrap(), character))
            .collect();
        let actors: Vec<_> = actor.into_iter().cloned().collect();
        let mut titles: Vec<_> = award(
            &award_rules(),
            party,
            &roster,
            &actors,
            ledger,
            encounter,
            0,
        )
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
    fn level_awards_follow_prepared_thresholds_and_preserve_saved_eligibility() -> anyhow::Result<()>
    {
        let (mut party, mut actor, ledger) = fixture(Character::Sheena);
        let rules = resonance_content::menu_data::TitleAwardRules {
            levels: vec![resonance_content::menu_data::LevelTitleAward {
                character: Character::Sheena as u8,
                title: 11,
                minimum: 23,
            }],
            ..Default::default()
        };
        let roster = [(ActorId::from_index(0)?, Character::Sheena)];
        party.members[4].level = 22;
        assert!(award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).is_empty());
        party.members[4].level = 23;
        actor.availability = ActorAvailability::Dead;
        assert!(award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).is_empty());
        actor.availability = ActorAvailability::Active;
        assert!(matches!(
            award(&rules, &mut party, &roster, &[actor], &ledger, 0, 0).as_slice(),
            [ResultNotice::Title {
                character: Character::Sheena,
                title: 11
            }]
        ));
        let mut restored: Party = serde_json::from_slice(&serde_json::to_vec(&party)?)?;
        assert!(award(&rules, &mut restored, &[], &[], &ledger, 0, 0).is_empty());
        restored.members[4].titles.clear();
        restored.members[4].ailments.petrified = true;
        assert!(award(&rules, &mut restored, &[], &[], &ledger, 0, 0).is_empty());
        restored.members[4].ailments.petrified = false;
        assert_eq!(
            award(&rules, &mut restored, &[], &[], &ledger, 0, 0).len(),
            1
        );
        Ok(())
    }

    #[test]
    fn combo_rules_use_declared_recipient_and_threshold_with_final_eligibility()
    -> anyhow::Result<()> {
        use resonance_content::menu_data::{ComboTitleAward, TitleAwardRules};
        let (mut party, mut actor, mut ledger) = fixture(Character::Sheena);
        let rules = TitleAwardRules {
            combos: vec![ComboTitleAward {
                character: 5,
                title: 11,
                minimum: 23,
            }],
            ..Default::default()
        };
        let roster = [(ActorId::from_index(0)?, Character::Sheena)];
        ledger.maximum_combo = 22;
        assert!(award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).is_empty());
        ledger.maximum_combo = 23;
        assert!(award(&rules, &mut party, &[], &[], &ledger, 0, 0).is_empty());
        actor.availability = ActorAvailability::Dead;
        assert!(award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).is_empty());
        actor.availability = ActorAvailability::Active;
        assert!(matches!(
            award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).as_slice(),
            [ResultNotice::Title {
                character: Character::Sheena,
                title: 11
            }]
        ));
        assert!(award(&rules, &mut party, &roster, &[actor], &ledger, 0, 0).is_empty());
        Ok(())
    }

    #[test]
    fn completed_battle_grants_all_eligible_titles_once() {
        let (mut party, mut actor, mut ledger) = fixture(Character::Lloyd);
        party.members[0].level = 100;
        party.members[0].titles.insert(18);
        ledger.maximum_combo = 100;
        ledger.normal_variety[0] = 3;
        ledger.achievements[0].assist_commands = 10;
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
        let roster = [(ActorId::from_index(1).unwrap(), Character::Raine)];
        let actors = [actor.clone(), actor];
        ledger.items = vec![5, 4];
        ledger.deaths.resize(2, 0);
        ledger.achievements.resize_with(2, Default::default);
        assert!(award(&award_rules(), &mut party, &roster, &actors, &ledger, 0, 0).is_empty());
        ledger.items[1] = 5;
        assert!(
            matches!(award(&award_rules(), &mut party, &roster, &actors, &ledger, 0, 0).as_slice(),
            [ResultNotice::Title { character, title: ITEM_KEEPER }] if *character == Character::Raine)
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
        assert_eq!(awarded(&mut party, Some(&actor), &ledger, 0), [14]);
    }

    #[test]
    fn equipment_rules_follow_declared_recipient_and_keep_shipped_notice_order()
    -> anyhow::Result<()> {
        let (mut party, actor, mut ledger) = fixture(Character::Zelos);
        party.members[5].equipment = [238, 288, 330, 0, 444, 375];
        ledger.normal_variety[0] = 3;
        ledger.achievements[0].strategy_orders = 4;
        ledger.achievements[0].voices = 50;
        let roster = [
            Character::Zelos,
            Character::Colette,
            Character::Raine,
            Character::Sheena,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, c)| Ok((ActorId::from_index(i)?, c)))
        .collect::<anyhow::Result<Vec<_>>>()?;
        let notices = award(
            &award_rules(),
            &mut party,
            &roster,
            &vec![actor; 4],
            &ledger,
            0,
            0,
        );
        assert_eq!(
            notices
                .iter()
                .map(|notice| match notice {
                    ResultNotice::Title { title, .. } => *title,
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            [12, 13, 14, 15, 16]
        );

        let (mut party, mut actor, ledger) = fixture(Character::Sheena);
        let mut rules = award_rules();
        rules.equipment[0].character = 5;
        rules.equipment[0].title = 11;
        rules.equipment[0].requirements[0].items = vec![239];
        let roster = [(ActorId::from_index(0)?, Character::Sheena)];
        party.members[4].equipment = [238, 288, 330, 444, 0, 366];
        assert!(award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).is_empty());
        party.members[4].equipment[0] = 239;
        assert!(award(&rules, &mut party, &[], &[], &ledger, 0, 0).is_empty());
        actor.availability = ActorAvailability::Dead;
        assert!(award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).is_empty());
        actor.availability = ActorAvailability::Active;
        assert!(matches!(
            award(&rules, &mut party, &roster, &[actor.clone()], &ledger, 0, 0).as_slice(),
            [ResultNotice::Title {
                character: Character::Sheena,
                title: 11
            }]
        ));
        assert!(award(&rules, &mut party, &roster, &[actor], &ledger, 0, 0).is_empty());
        Ok(())
    }

    #[test]
    fn battle_title_thresholds_require_the_full_count_and_award_once() {
        type Counter = fn(&mut Party, &mut Ledger, u16);
        let cases: &[(Character, u8, u16, bool, Counter)] = &[
            (Character::Lloyd, 24, 1, false, |_, l, n| {
                l.achievements[0].commanded_alone = n != 0
            }),
            (Character::Lloyd, 28, 256, false, |p, _, n| {
                p.battles.hard_victories = n
            }),
            (Character::Colette, 16, 400, false, |p, _, n| {
                p.members[1].technique_uses = [(223, n / 2), (224, n - n / 2)].into();
            }),
            (Character::Colette, 17, 1, false, |_, l, n| {
                l.achievements[0].stayed_poisoned = n != 0
            }),
            (Character::Colette, 18, 10, true, |_, l, n| {
                l.achievements[0].repeated_technique = n
            }),
            (Character::Genis, 16, 5, false, |_, l, n| {
                l.achievements[0].weapon_changes = n as u8
            }),
            (Character::Genis, 17, 15, false, |_, l, n| {
                l.achievements[0].resisted_spells = n as u8
            }),
            (Character::Genis, 19, 6, false, |_, l, n| {
                l.achievements[0].spell_elements = (1 << n) - 1
            }),
            (Character::Raine, 16, 5, false, |_, l, n| {
                l.deaths[0] = n as u8
            }),
            (Character::Sheena, 16, 2, true, |_, l, n| {
                l.achievements[0].unison_starts = n as u8
            }),
            (Character::Zelos, 12, 3, true, |_, l, n| {
                l.normal_variety[0] = n as u8
            }),
            (Character::Zelos, 15, 4, false, |_, l, n| {
                l.achievements[0].strategy_orders = n as u8
            }),
            (Character::Zelos, 16, 50, false, |_, l, n| {
                l.achievements[0].voices = n as u8
            }),
            (Character::Presea, 12, 10, false, |_, l, n| {
                l.achievements[0].guard_breaks = n as u8
            }),
            (Character::Presea, 15, 5, true, |p, _, n| {
                p.battles.hunting_streak = n as u8
            }),
            (Character::Regal, 13, 5, false, |_, l, n| {
                l.achievements[0].potions = n as u8
            }),
            (Character::Regal, 15, 3, false, |_, l, n| {
                l.achievements[0].knockdowns = n as u8
            }),
            (Character::Kratos, 10, 3, true, |_, l, n| {
                l.normal_variety[0] = n as u8
            }),
        ];
        for &(character, title, threshold, needs_control, set) in cases {
            let (mut party, mut actor, mut ledger) = fixture(character);
            if character == Character::Colette && title == 17 {
                actor.conditions = resonance_battle::conditions::Conditions::new(
                    resonance_battle::conditions::Layers {
                        base: resonance_battle::conditions::POISON,
                        ..Default::default()
                    },
                );
            }
            set(&mut party, &mut ledger, threshold - 1);
            assert!(
                !awarded(&mut party, Some(&actor), &ledger, 0).contains(&title),
                "{character:?}: {title}"
            );
            set(&mut party, &mut ledger, threshold);
            actor.control = Control::Auto;
            assert_eq!(
                awarded(&mut party, Some(&actor), &ledger, 0).contains(&title),
                !needs_control,
                "{character:?}: {title}"
            );
            if needs_control {
                actor.control = Control::Manual;
                assert!(
                    awarded(&mut party, Some(&actor), &ledger, 0).contains(&title),
                    "{character:?}: {title}"
                );
            }
            assert!(!awarded(&mut party, Some(&actor), &ledger, 0).contains(&title));
        }
    }

    #[test]
    fn command_titles_do_not_borrow_another_actors_commands() {
        for (character, title) in [(Character::Lloyd, ASSIST_COMMANDER), (Character::Zelos, 15)] {
            let (mut party, mut actor, mut ledger) = fixture(character);
            let mut other = ledger.achievements[0].clone();
            other.assist_commands = 10;
            other.strategy_orders = 4;
            ledger.achievements.push(other.clone());
            assert!(!awarded(&mut party, Some(&actor), &ledger, 0).contains(&title));
            ledger.achievements[0] = other;
            actor.control = Control::Auto;
            assert!(awarded(&mut party, Some(&actor), &ledger, 0).contains(&title));
        }
    }

    #[test]
    fn restraint_titles_reject_disqualifying_actions() {
        for (character, title) in [
            (Character::Colette, 19),
            (Character::Colette, 20),
            (Character::Genis, 18),
            (Character::Sheena, 17),
        ] {
            let (mut party, mut actor, mut ledger) = fixture(character);
            ledger.achievements[0].unison_starts = 1;
            match (character, title) {
                (Character::Colette, 19) => ledger.achievements[0].used_arte = true,
                (Character::Colette, _) => ledger.achievements[0].ran = true,
                (Character::Genis, _) => ledger.achievements[0].acted = true,
                _ => ledger.damage_outside_unison = true,
            }
            assert!(!awarded(&mut party, Some(&actor), &ledger, 0).contains(&title));
            ledger.achievements[0] = Default::default();
            ledger.achievements[0].unison_starts = 1;
            ledger.damage_outside_unison = false;
            actor.control = Control::Auto;
            assert!(!awarded(&mut party, Some(&actor), &ledger, 0).contains(&title));
            actor.control = Control::Manual;
            assert!(awarded(&mut party, Some(&actor), &ledger, 0).contains(&title));
        }
    }

    #[test]
    fn composition_titles_use_active_members_not_reserves() {
        for (character, title, formation) in [
            (Character::Raine, 15, [4, 2, 5, 7]),
            (Character::Zelos, 13, [6, 2, 4, 5]),
            (Character::Presea, 13, [7, 1, 3, 8]),
            (Character::Presea, 14, [7, 5, 6, 8]),
            (Character::Regal, 12, [8, 1, 3, 9]),
        ] {
            let (mut party, actor, mut ledger) = fixture(character);
            party.formation = formation.to_vec();
            let actors = vec![actor; 4];
            let roster: Vec<_> = formation
                .into_iter()
                .enumerate()
                .map(|(i, c)| {
                    (
                        ActorId::from_index(i).unwrap(),
                        Character::try_from(c).unwrap(),
                    )
                })
                .collect();
            ledger.achievements.resize_with(4, Default::default);
            ledger.normal_variety.resize(4, 0);
            ledger.items.resize(4, 0);
            ledger.deaths.resize(4, 0);
            let granted = |notices: Vec<ResultNotice>| {
                notices.iter().any(|notice| matches!(notice, ResultNotice::Title { character: c, title: t } if *c == character && *t == title))
            };
            assert!(!granted(award(
                &award_rules(),
                &mut party,
                &roster[..3],
                &actors,
                &ledger,
                0,
                0
            )));
            assert!(granted(award(
                &award_rules(),
                &mut party,
                &roster,
                &actors,
                &ledger,
                0,
                0
            )));
        }
    }

    #[test]
    fn survivor_and_level_difference_use_the_active_encounter() {
        let (mut party, actor, mut ledger) = fixture(Character::Raine);
        ledger.achievements.resize_with(2, Default::default);
        let roster = [
            (ActorId::from_index(0).unwrap(), Character::Raine),
            (ActorId::from_index(1).unwrap(), Character::Lloyd),
        ];
        let mut actors = vec![actor.clone(), actor];
        assert!(
            !award(&award_rules(), &mut party, &roster, &actors, &ledger, 0, 0)
                .iter()
                .any(|n| matches!(n, ResultNotice::Title { title: 17, .. }))
        );
        actors[1].availability = ActorAvailability::Dead;
        assert!(
            award(&award_rules(), &mut party, &roster, &actors, &ledger, 0, 0)
                .iter()
                .any(|n| matches!(n, ResultNotice::Title { title: 17, .. }))
        );
        let (mut party, actor, ledger) = fixture(Character::Regal);
        let roster = [(ActorId::from_index(0).unwrap(), Character::Regal)];
        assert!(
            award(
                &award_rules(),
                &mut party,
                &roster,
                std::slice::from_ref(&actor),
                &ledger,
                0,
                7
            )
            .is_empty()
        );
        assert!(matches!(
            award(&award_rules(), &mut party, &roster, &[actor], &ledger, 0, 8).as_slice(),
            [ResultNotice::Title { title: 14, .. }]
        ));
    }

    #[test]
    fn story_challenges_include_deaths_in_the_winning_battle() {
        let (mut party, actor, mut ledger) = fixture(Character::Colette);
        ledger.deaths[0] = 1;
        assert!(!awarded(&mut party, Some(&actor), &ledger, 15).contains(&21));
        ledger.deaths[0] = 0;
        party.battles.deaths[4] = 1;
        assert!(!awarded(&mut party, Some(&actor), &ledger, 15).contains(&21));
        party.battles.deaths[4] = 0;
        assert!(awarded(&mut party, Some(&actor), &ledger, 15).contains(&21));
    }
}
