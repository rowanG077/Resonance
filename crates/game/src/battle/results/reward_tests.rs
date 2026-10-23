//! Candidate-level reward effect extraction. These fixtures exercise the
//! prepared party and live actor namespaces separately from reward arithmetic.
use super::item_tests::{PreparedFixture, native_battle};
use super::{Character, ResultNotice, Selection};
use anyhow::Result;
use resonance_battle::{BattleResult, Side};
use resonance_content::{
    menu_data::{CompoundExSkill, MenuData, ex_effect as ex},
    session::SessionData,
};
use resonance_events::party::Party;
use std::sync::Arc;

fn local_fixture(
    formation: &[u8],
    configure: impl FnOnce(&mut Party, &SessionData, &mut MenuData) -> Result<()>,
) -> Result<PreparedFixture> {
    super::item_tests::native_fixture(
        formation,
        |field, session, menus| {
            menus.items[1].properties.gald_one_and_a_half = true;
            menus.items[2].properties.gald_double = true;
            // Small authored recipes exercise discovery-independent projection, without
            // depending on the private catalogue's unrelated gem choices and resources.
            for (character, skill, required) in [
                (0, ex::HP_GROWTH, vec![1, 2]),
                (0, ex::TP_GROWTH, vec![1, 6]),
                (1, ex::ITEM_FINDER, vec![1, 2]),
                (1, ex::GALD_FINDER, vec![1, 6]),
                (4, ex::SPIRIT_HEALER, vec![1, 2]),
                (5, ex::INCREASE_EXPERIENCE, vec![1, 2]),
                (6, ex::TOUGH_EXPERIENCE, vec![1, 2]),
                (8, ex::INCREASE_EXPERIENCE, vec![1, 2]),
            ] {
                menus.ex_skills.characters[character]
                    .compounds
                    .push(CompoundExSkill { skill, required });
                menus
                    .ex_skills
                    .skills
                    .insert(skill, menus.ex_skills.skills[&1].clone());
            }
            menus
                .ex_skills
                .skills
                .insert(ex::HAPPINESS, menus.ex_skills.skills[&1].clone());
            configure(field, session, menus)
        },
        |mut actors, _| {
            for actor in &mut actors {
                if actor.side == Side::Enemy {
                    actor.hp = 0;
                }
            }
            native_battle(actors)
        },
    )
}

fn enter_controlled_victory(fixture: &mut PreparedFixture) -> Result<()> {
    assert_eq!(
        fixture.battle.recognize_result(),
        Some(BattleResult::Victory)
    );
    fixture.battle.retire_combat()?;
    Ok(())
}

#[test]
fn grade_shop_doubles_signed_grade_in_results_records_and_balance() -> Result<()> {
    for enemy_grade in [-500, 1000] {
        for formations in [1, 2, 3] {
            let mut ordinary = None;
            for enabled in [false, true] {
                let mut fixture = local_fixture(&[1], |party, _, _| {
                    if enabled {
                        party
                            .new_game_plus
                            .benefits
                            .insert(resonance_content::grade::Benefit::IncreasedGrade);
                    }
                    Ok(())
                })?;
                fixture.candidate.setup.formation_count = formations;
                fixture.candidate.setup.enemies[0].grade = enemy_grade;
                fixture.candidate.party.grade_hundredths = 100_000;
                enter_controlled_victory(&mut fixture)?;
                construct_result(&mut fixture)?;
                let grade = fixture.candidate.results.as_ref().unwrap().grade;
                let expected = *ordinary.get_or_insert(grade) * if enabled { 2 } else { 1 };
                assert_eq!(grade, expected);
                assert_eq!(fixture.candidate.party.battles.grade, expected);
                assert_eq!(
                    fixture.candidate.party.grade_hundredths,
                    (100_000 + expected) as u32
                );
            }
        }
    }
    Ok(())
}

#[test]
fn combined_bonus_reaches_results_and_party_including_signed_grade() -> Result<()> {
    for enemy_grade in [-500, 1000] {
        let mut ordinary_grade = None;
        for (formation_count, experience, gald) in
            [(1, 19, 101), (2, 28, 151), (3, 38, 202), (128, 1225, 6514)]
        {
            let mut fixture = local_fixture(&[1], |_, _, _| Ok(()))?;
            fixture.candidate.setup.formation_count = formation_count;
            fixture.candidate.setup.enemies[0].grade = enemy_grade;
            fixture.candidate.setup.enemies[0].reward.experience = 19;
            fixture.candidate.setup.enemies[0].reward.gald = 101;
            fixture.candidate.party.grade_hundredths = 100_000;
            let before_experience = fixture.candidate.party.members[0].experience;
            let before_gald = fixture.candidate.party.gald;
            enter_controlled_victory(&mut fixture)?;
            construct_result(&mut fixture)?;
            let results = fixture.candidate.results.as_ref().unwrap();
            assert_eq!(
                (results.rewards.experience, results.rewards.gald),
                (experience, gald)
            );
            assert_eq!(
                fixture.candidate.party.members[0].experience,
                before_experience + experience + results.rewards.combo_experience
            );
            assert_eq!(fixture.candidate.party.gald, before_gald + gald);
            let grade =
                *ordinary_grade.get_or_insert(results.grade) * (formation_count as i32 + 1) / 2;
            assert_eq!(results.grade, grade);
            assert_eq!(fixture.candidate.party.battles.grade, grade);
            assert_eq!(
                fixture.candidate.party.grade_hundredths,
                (100_000 + grade) as u32
            );
        }
    }
    Ok(())
}

#[test]
fn field_handoff_preserves_battle_duration_and_formation_count() -> Result<()> {
    for formations in [1, 2, 3, 5] {
        let mut fixture = super::item_tests::native_fixture(
            &[1],
            |_, _, _| Ok(()),
            |actors, setup| {
                setup.formation_count = formations;
                native_battle(actors)
            },
        )?;
        for _ in 0..1201 {
            fixture.candidate.world_update(
                &mut fixture.battle,
                resonance_battle::BattleInput::default(),
            )?;
        }
        let outcome = super::item_tests::escape(&mut fixture.candidate, &mut fixture.battle)?;
        assert_eq!(fixture.battle.ledger().combat_ticks, 1201);
        let completed = fixture.candidate.finish(&fixture.battle, &outcome)?;
        let saved: Party = serde_json::from_slice(&serde_json::to_vec(&completed.party)?)?;
        assert_eq!(saved.battles.combat_ticks, 1201);
        assert_eq!(saved.battles.formation_count, formations as u32);
    }
    Ok(())
}

#[test]
fn equipment_and_floor_reward_modifiers_compose_independently_of_skills() -> Result<()> {
    // Defeated owners retain gear multipliers, but cannot supply global skills.
    for (item, skills, fallen, expected) in [
        (1, [0; 4], false, (0, true, false, false)),
        (2, [0; 4], false, (0, false, true, false)),
        (0, [1, 2, 0, 0], false, (10, false, false, false)),
        (0, [1, 6, 0, 0], false, (0, false, false, true)),
        (2, [1, 6, 0, 0], true, (0, false, true, false)),
    ] {
        let mut fixture = local_fixture(&[2, 1], |party, _, _| {
            party.battle_rules.modifiers = resonance_events::battle::Rules::EXPERIENCE_DOUBLE
                | resonance_events::battle::Rules::GALD_DOUBLE
                | resonance_events::battle::Rules::ITEM_DROP_BONUS;
            let member = &mut party.members[1];
            member.equipment[3] = item;
            member.ex_skills = skills;
            if fallen {
                member.hp = 0;
            }
            Ok(())
        })?;
        fixture.candidate.party.members[1]
            .compound_ex_skills
            .clear();
        let modifiers = fixture.candidate.reward_modifiers(&fixture.battle)?;
        assert!(modifiers.floor_experience_double);
        assert!(modifiers.floor_gald_double);
        assert_eq!(
            (
                modifiers.item_drop_bonus,
                modifiers.gald_one_and_a_half,
                modifiers.gald_double,
                modifiers.gald_finder
            ),
            (expected.0 + 25, expected.1, expected.2, expected.3),
            "item {item}, skills {skills:?}, fallen {fallen}"
        );
    }
    Ok(())
}

#[test]
fn happiness_keeps_inactive_owner_separate_from_active_reward_traits() -> Result<()> {
    for active in [false, true] {
        let mut fixture = local_fixture(if active { &[4, 1] } else { &[1] }, |party, _, _| {
            party.members[3].ex_skills[0] = ex::HAPPINESS;
            party.members[0].ex_skills = [1, 2, 6, 0];
            Ok(())
        })?;
        fixture.candidate.party.members[0]
            .compound_ex_skills
            .clear();
        if !active {
            Arc::make_mut(&mut fixture.candidate.menus).titles[3][0].costume =
                Some(resonance_content::menu_data::Costume::Variant1);
        }
        let modifiers = fixture.candidate.reward_modifiers(&fixture.battle)?;
        assert_eq!(modifiers.happiness_luck.is_some(), active);
        assert!(modifiers.happiness_recipient_luck.is_some());
        assert_eq!(modifiers.maximum_vital_growth[0], [true; 2]);
        let unavailable = fixture.candidate.reward_modifiers_with(|_| (false, 0, 1))?;
        assert!(unavailable.happiness_luck.is_none());
        assert!(unavailable.happiness_recipient_luck.is_some());
    }
    Ok(())
}

#[test]
fn invalid_reward_resources_do_not_publish_party_randomness_or_results() -> Result<()> {
    for invalid_drop in [false, true] {
        let mut fixture = local_fixture(&[1, 4], |party, session, _| {
            let member = &mut party.members[0];
            member.experience = session.experience[usize::from(member.level) + 1] - 1;
            Ok(())
        })?;
        enter_controlled_victory(&mut fixture)?;
        fixture.candidate.setup.enemies[0].reward.experience = 100;
        let before = serde_json::to_value(&fixture.candidate.party)?;
        let random = fixture.candidate.gameplay_random;
        let battle_random = fixture.battle.random_state();
        let actors = fixture.battle.actors().to_vec();
        if invalid_drop {
            fixture.candidate.setup.enemies[0].reward.drops[0] = Some(u16::MAX);
            fixture.candidate.setup.enemies[0].reward.drop_chances[0] = 100;
        } else {
            Arc::make_mut(&mut fixture.candidate.menus).titles[0].clear();
        }
        let error = construct_result(&mut fixture).unwrap_err();
        assert!(
            error.to_string().contains(if invalid_drop {
                "missing awarded item"
            } else {
                "missing party title"
            }),
            "{error:#}"
        );
        assert_eq!(serde_json::to_value(&fixture.candidate.party)?, before);
        assert_eq!(fixture.candidate.gameplay_random, random);
        assert_eq!(fixture.battle.random_state(), battle_random);
        assert_eq!(fixture.battle.actors(), actors);
        assert!(fixture.candidate.results.is_none());
    }
    Ok(())
}

#[test]
fn reward_transaction_recovers_tp_once_using_active_skills() -> Result<()> {
    for (active_skill, nearly_full) in [(true, false), (false, false), (true, true)] {
        let mut fixture = local_fixture(&[5, 1], |party, _, _| {
            for index in [4, 0] {
                party.members[index].base_stats[1] = 250;
                party.members[index].tp = if nearly_full { 249 } else { 0 };
            }
            party.members[4].compound_ex_skills.insert(0);
            if active_skill {
                party.members[4].ex_skills = [1, 2, 0, 0];
            }
            Ok(())
        })?;
        enter_controlled_victory(&mut fixture)?;
        if active_skill {
            fixture.candidate.party.members[4]
                .compound_ex_skills
                .clear();
        }
        construct_result(&mut fixture)?;
        let sheena = actor_for(&fixture, Character::Sheena);
        let notices = &fixture.candidate.results.as_ref().unwrap().notices;
        assert!(notices.contains(&ResultNotice::TpRecovery {
            character: Character::Sheena,
            amount: if nearly_full {
                1
            } else if active_skill {
                32
            } else {
                20
            }
        }));
        assert!(notices.contains(&ResultNotice::TpRecovery {
            character: Character::Lloyd,
            amount: if nearly_full { 1 } else { 20 }
        }));
        assert_eq!(
            fixture.battle.actors()[sheena.index()].equipment.max_tp,
            250
        );
        assert_eq!(
            fixture.battle.actors()[sheena.index()].tp,
            if nearly_full {
                250
            } else if active_skill {
                32
            } else {
                20
            }
        );
        let balances: Vec<_> = fixture
            .battle
            .actors()
            .iter()
            .map(|actor| actor.tp)
            .collect();
        let party = serde_json::to_value(&fixture.candidate.party)?;
        assert!(construct_result(&mut fixture).is_err());
        assert_eq!(serde_json::to_value(&fixture.candidate.party)?, party);
        assert_eq!(
            fixture
                .battle
                .actors()
                .iter()
                .map(|actor| actor.tp)
                .collect::<Vec<_>>(),
            balances
        );
        assert_eq!(
            fixture.candidate.party.members[4].tp,
            fixture.battle.actors()[sheena.index()].tp
        );
    }
    Ok(())
}

fn actor_for(fixture: &PreparedFixture, character: Character) -> resonance_battle::ActorId {
    fixture
        .candidate
        .setup
        .actors
        .iter()
        .find(|&&(_, id)| id == character)
        .map(|&(actor, _)| actor)
        .expect("prepared actor")
}

fn construct_result(fixture: &mut PreparedFixture) -> Result<()> {
    let (actor, character) = fixture.candidate.setup.actors[0];
    fixture.candidate.selection = Some(Selection {
        actor,
        character,
        pose: Some(0),
        group: None,
        voice: None,
    });
    fixture.candidate.construct_rewards(&mut fixture.battle)
}

#[test]
fn result_advancement_feedback_uses_the_result_clock_and_pauses_with_it() -> Result<()> {
    use crate::battle::lifecycle::Input;
    use resonance_battle::{ActorId, BattleInput, BattlePhase, Cue};
    let mut fixture = local_fixture(&[1, 2], |_, _, _| Ok(()))?;
    let mut display = super::item_tests::Display;
    for _ in 0..50 {
        fixture.lifecycle.step(
            &mut fixture.battle,
            Input::default(),
            &mut fixture.candidate,
            &mut display,
        )?;
        if fixture.battle.phase() == BattlePhase::Results {
            break;
        }
    }
    assert_eq!(fixture.battle.phase(), BattlePhase::Results);
    fixture.candidate.results.as_mut().unwrap().notices = vec![
        ResultNotice::Level {
            character: Character::Lloyd,
            level: 2,
        },
        ResultNotice::Level {
            character: Character::Colette,
            level: 2,
        },
        ResultNotice::Technique {
            character: Character::Lloyd,
            technique: resonance_battle::TechniqueId(1),
        },
        ResultNotice::CompoundEx {
            character: Character::Colette,
        },
    ];
    for age in 0..=181 {
        if age == 100 {
            let paused = fixture.lifecycle.step(
                &mut fixture.battle,
                Input {
                    battle: BattleInput {
                        paused: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &mut fixture.candidate,
                &mut display,
            )?;
            assert!(
                !paused
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::ResultAdvancement { .. }))
            );
        }
        let frame = fixture.lifecycle.step(
            &mut fixture.battle,
            Input::default(),
            &mut fixture.candidate,
            &mut display,
        )?;
        let batches: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::ResultAdvancement { actors, .. } => Some(actors.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            batches,
            match age {
                100 => vec![vec![ActorId::from_index(0)?, ActorId::from_index(1)?]],
                180 => vec![vec![ActorId::from_index(1)?]],
                _ => vec![],
            },
            "result age {age}"
        );
    }
    Ok(())
}

#[test]
fn zelos_and_kratos_increase_recipes_apply_only_to_the_active_recipient() -> Result<()> {
    for character in [6, 9] {
        let mut fixture = local_fixture(&[character, 1, 2, 3, 5], |party, _, _| {
            party.members[usize::from(character - 1)].ex_skills = [1, 2, 0, 0];
            Ok(())
        })?;
        let index = usize::from(character - 1);
        fixture.candidate.party.members[index]
            .compound_ex_skills
            .clear();
        let modifiers = fixture.candidate.reward_modifiers(&fixture.battle)?;
        let mut expected = [0; 9];
        expected[index] = 10;
        assert_eq!(modifiers.experience_bonus_percent, expected);
    }
    Ok(())
}

#[test]
fn presea_tough_recipe_reads_live_hp_bands() -> Result<()> {
    let mut fixture = local_fixture(&[7, 1], |party, _, _| {
        party.members[6].ex_skills = [1, 2, 0, 0];
        Ok(())
    })?;
    let presea = actor_for(&fixture, Character::Presea);

    for (hp, expected) in [(249, 15), (250, 10), (500, 5), (750, 0), (1000, 0)] {
        fixture.battle.set_actor_vitals(presea, hp, 1000, 0, 100)?;
        let modifiers = fixture.candidate.reward_modifiers(&fixture.battle)?;
        assert_eq!(modifiers.experience_bonus_percent[6], expected);
        assert_eq!(modifiers.experience_bonus_percent[0], 0);
    }
    Ok(())
}

#[test]
fn reward_modifiers_follow_committed_equipment() -> Result<()> {
    let mut fixture = local_fixture(&[1], |party, _, menus| {
        party.members[0].equipment[3] = 1;
        menus.items[1].properties.experience_percent = 50;
        menus.items[2].properties.experience_percent = 100;
        Ok(())
    })?;
    let before = fixture.candidate.reward_modifiers(&fixture.battle)?;
    assert_eq!(before.equipment_experience_percent[0], 50);
    assert!(before.gald_one_and_a_half && !before.gald_double);
    let mut next = fixture.candidate.party.clone();
    next.members[0].equipment[3] = 2;
    assert_eq!(fixture.candidate.reward_modifiers(&fixture.battle)?, before);
    fixture
        .candidate
        .commit_equipment(&mut fixture.battle, next)?;
    let after = fixture.candidate.reward_modifiers(&fixture.battle)?;
    assert_eq!(after.equipment_experience_percent[0], 100);
    assert!(!after.gald_one_and_a_half && after.gald_double);
    assert_eq!(fixture.field.members[0].equipment[3], 1);
    Ok(())
}

#[test]
fn consumed_rescue_slots_snapshot_once_and_equipment_replacement_keeps_new_gear() -> Result<()> {
    use resonance_battle::{EquipmentReplacement, RescueEquipment};
    use resonance_content::menu_data::EquipmentRescue;
    let mut f = local_fixture(&[1], |party, _, _| {
        party.members[0].equipment = [3, 4, 5, 1, 1, 6];
        Ok(())
    })?;
    let menus = Arc::make_mut(&mut f.candidate.menus);
    menus.items[1].properties.rescue = Some(EquipmentRescue::Consumable);
    menus.items[2].properties.rescue = Some(EquipmentRescue::Chance);
    let id = f.candidate.setup.actors[0].0;
    let live = &f.battle.actors()[id.index()];
    let mut attributes = live.equipment.clone();
    attributes.recovery.lethal.equipment = [
        Some(RescueEquipment::Consumed(3)),
        Some(RescueEquipment::Consumable(4)),
        None,
    ];
    f.battle
        .replace_equipment_batch(vec![EquipmentReplacement {
            actor: id,
            attributes,
            conditions: live.conditions.clone(),
            equipment: None,
        }])?;
    let first = f.candidate.snapshot_party(&f.battle)?;
    assert_eq!(first.members[0].equipment, [3, 4, 5, 0, 1, 6]);
    assert_eq!(
        serde_json::to_value(f.candidate.snapshot_party(&f.battle)?)?,
        serde_json::to_value(&first)?
    );
    let mut next = first;
    next.members[0].equipment[3] = 2;
    f.candidate.commit_equipment(&mut f.battle, next)?;
    assert_eq!(
        f.candidate.snapshot_party(&f.battle)?.members[0].equipment,
        [3, 4, 5, 2, 1, 6]
    );
    let live = &f.battle.actors()[id.index()];
    let mut attributes = live.equipment.clone();
    attributes.recovery.lethal.equipment = [Some(RescueEquipment::Consumed(u8::MAX)), None, None];
    f.battle
        .replace_equipment_batch(vec![EquipmentReplacement {
            actor: id,
            attributes,
            conditions: live.conditions.clone(),
            equipment: None,
        }])?;
    let before = serde_json::to_value(&f.candidate.party)?;
    assert!(f.candidate.sync_party(&f.battle).is_err());
    assert_eq!(serde_json::to_value(&f.candidate.party)?, before);
    Ok(())
}
