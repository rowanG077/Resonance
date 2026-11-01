//! Result preparation, equipment, and reward feedback through CPU fixtures.
use resonance_battle::ControllerPort;
mod equipment_model;
mod hourglass_lifecycle;
mod magic_mist;
use super::*;
use crate::menu::MenuAction;
use resonance_battle::{
    BattleInput,
    item::{Provider, Release},
};
use resonance_content::diagnostics::Diagnostics;
use resonance_events::party::MonsterKnowledge;

pub(crate) use super::test_support::*;

pub(super) fn close_equipment(
    candidate: &mut Candidate,
    battle: &mut Battle,
    page: &mut crate::menu::equipment::Equipment,
    character: &mut usize,
) -> Result<()> {
    for _ in 0..12 {
        candidate.step_equipment(battle, page, character, Default::default())?;
    }
    for _ in 0..20 {
        if candidate
            .step_equipment(battle, page, character, Some(MenuAction::Cancel))?
            .closed
        {
            return Ok(());
        }
    }
    anyhow::bail!("equipment page did not close")
}

pub(super) fn refresh_equipment(candidate: &mut Candidate, battle: &mut Battle) -> Result<()> {
    let (mut page, mut character) = candidate.begin_equipment(battle, 0)?;
    close_equipment(candidate, battle, &mut page, &mut character)
}

pub(crate) fn release(
    candidate: &mut Candidate,
    battle: &mut Battle,
    request: Release,
) -> Result<()> {
    battle.queue_item(request)?;
    for _ in 0..300 {
        step(candidate, battle)?;
        if battle.pending_item().is_none() {
            return Ok(());
        }
    }
    anyhow::bail!("queued item did not complete")
}

#[test]
fn grade_shop_large_stacks_allow_item_use_and_magic_lens() -> Result<()> {
    let mut fixture = native_fixture(
        &[1],
        |party, _, menus| {
            party
                .new_game_plus
                .benefits
                .insert(resonance_content::grade::Benefit::ThirtyItems);
            party.items = [(1, 30), (37, 30)].into();
            menus.items.resize(38, menus.items[0].clone());
            for item in [1, 37] {
                menus.items[item].battle_usable = true;
            }
            Ok(())
        },
        |actors, _| native_battle(actors),
    )?;
    let candidate = &mut fixture.candidate;
    let user = candidate.setup.actors[0].0;
    let enemy = candidate.setup.enemies[0].actor;
    for count in [30, 21, 20] {
        candidate.party.items.insert(1, count);
        candidate.party.items.insert(37, count);
        candidate
            .acquire_item(Release {
                user,
                target: user,
                item: 1,
            })?
            .consume();
        let mut loan = candidate.acquire_scan(Release {
            user,
            target: enemy,
            item: 37,
        })?;
        loan.scan();
        loan.consume();
        assert_eq!(candidate.party.items[&1], count - 1);
        assert_eq!(candidate.party.items[&37], count - 1);
    }
    candidate.party.new_game_plus.benefits.clear();
    candidate.party.items.insert(1, 21);
    candidate.party.items.insert(37, 21);
    assert!(
        candidate
            .acquire_item(Release {
                user,
                target: user,
                item: 1
            })
            .is_err()
    );
    assert!(
        candidate
            .acquire_scan(Release {
                user,
                target: enemy,
                item: 37
            })
            .is_err()
    );
    Ok(())
}

#[test]
fn candidate_item_loans_preserve_authoritative_storage() -> Result<()> {
    let PreparedFixture {
        mut candidate,
        field,
        ..
    } = native_fixture(
        &[4, 1],
        |party, _, menus| {
            party.items = [(1, 2), (37, 4)].into();
            menus.items.resize(38, menus.items[0].clone());
            for item in [1, 37] {
                menus.items[item].battle_usable = true;
            }
            Ok(())
        },
        |actors, _| native_battle(actors),
    )?;
    let roster = candidate.setup.actors.clone();
    for &(actor, _) in &roster {
        let request = Release {
            user: actor,
            target: actor,
            item: 1,
        };
        let loan = candidate.acquire_item(request)?;
        drop(loan);
        assert_eq!(candidate.items().counts[&1], 2);
    }
    let user = roster[0].0;
    let item = Release {
        user,
        target: user,
        item: 1,
    };
    for remaining in [Some(1), None] {
        let mut loan = candidate.acquire_item(item)?;
        loan.record_gel_use();
        loan.consume();
        assert_eq!(candidate.items().counts.get(&1).copied(), remaining);
        assert!(candidate.party.battles.battle_gel_used);
    }
    assert_eq!(candidate.party.found_items, field.found_items);
    assert_eq!(candidate.party.recent_items, field.recent_items);
    assert_eq!(candidate.party.battles.items, field.battles.items);
    let before = serde_json::to_value(&candidate.party)?;
    assert!(candidate.acquire_item(item).is_err());
    assert_eq!(serde_json::to_value(&candidate.party)?, before);

    let prepared_enemy = candidate.setup.enemies.first().unwrap();
    let (enemy, monster) = (prepared_enemy.actor, prepared_enemy.reward.monster);
    let ordinary = roster
        .iter()
        .find(|&&(_, character)| character != Character::Raine)
        .unwrap()
        .0;
    let scan = Release {
        user: ordinary,
        target: enemy,
        item: 37,
    };
    // Neither invalid mapping nor missing quantity may create a knowledge row.
    for request in [
        Release {
            user: enemy,
            ..scan
        },
        Release {
            target: user,
            ..scan
        },
    ] {
        let before = serde_json::to_value(&candidate.party)?;
        assert!(candidate.acquire_scan(request).is_err());
        assert_eq!(serde_json::to_value(&candidate.party)?, before);
    }
    candidate.party.items.remove(&37);
    let before = serde_json::to_value(&candidate.party)?;
    assert!(candidate.acquire_scan(scan).is_err());
    assert_eq!(serde_json::to_value(&candidate.party)?, before);
    candidate.party.items.insert(37, 4);
    candidate.party.monsters.insert(
        monster,
        MonsterKnowledge {
            drops: [true, false],
            steal: true,
            variant: 3,
            ..Default::default()
        },
    );
    let mut loan = candidate.acquire_scan(scan)?;
    assert!(loan.scan());
    loan.consume();
    assert!(candidate.party.monsters[&monster].scanned);
    assert!(!candidate.party.monsters[&monster].location);
    if let Some(&(raine, _)) = roster
        .iter()
        .find(|&&(_, character)| character == Character::Raine)
    {
        for learned in [true, false] {
            let mut loan = candidate.acquire_scan(Release {
                user: raine,
                target: enemy,
                item: 37,
            })?;
            assert_eq!(loan.scan(), learned);
            loan.consume();
        }
        assert!(candidate.party.monsters[&monster].location);
    }
    let knowledge = &candidate.party.monsters[&monster];
    assert_eq!(knowledge.drops, [true, false]);
    assert!(knowledge.steal);
    assert_eq!(knowledge.variant, 3);
    assert_eq!(candidate.party.battles.items, field.battles.items);
    Ok(())
}

#[test]
fn controlled_candidate_actor_identities_share_one_species_knowledge_row() -> Result<()> {
    let PreparedFixture { mut candidate, .. } = native_fixture(
        &[4, 1],
        |party, _, menus| {
            party.items = [(37, 4)].into();
            menus.items.resize(38, menus.items[0].clone());
            menus.items[37].battle_usable = true;
            Ok(())
        },
        |mut actors, setup| {
            let enemy = &setup.enemies[0];
            setup.enemies.push(PreparedEnemy {
                actor: ActorId::from_index(actors.len())?,
                level: enemy.level,
                grade: enemy.grade,
                steal: enemy.steal,
                steal_chance: enemy.steal_chance,
                reward: enemy.reward.clone(),
            });
            actors.push(actors.last().unwrap().clone());
            native_battle(actors)
        },
    )?;
    let first = candidate.setup.enemies[0].actor;
    let second = candidate.setup.enemies[1].actor;
    let monster = candidate.setup.enemies[0].reward.monster;
    assert_ne!(first, second);
    let raine = candidate.setup.actors[0].0;
    let lloyd = candidate.setup.actors[1].0;
    let rows = candidate.party.monsters.len();
    assert!(!candidate.party.monsters.contains_key(&monster));
    for (user, target, learned) in [
        (lloyd, first, true),
        (raine, second, true),
        (raine, first, false),
        (lloyd, second, false),
    ] {
        let mut loan = candidate.acquire_scan(Release {
            user,
            target,
            item: 37,
        })?;
        assert_eq!(loan.scan(), learned);
        loan.consume();
        assert_eq!(candidate.party.monsters.len(), rows + 1);
    }
    assert!(candidate.party.monsters[&monster].scanned);
    assert!(candidate.party.monsters[&monster].location);
    assert!(!candidate.party.items.contains_key(&37));
    Ok(())
}

#[test]
#[ignore = "requires current Items/profile/script publications; CPU only"]
fn synchronous_item_release_spends_once_and_finish_maps_usage_to_character() -> Result<()> {
    let (mut candidate, mut battle, field) = fixture(&[4, 1], 2)?;
    let user = candidate.setup.actors[0].0;
    assert_eq!(candidate.setup.actors[0].1, Character::Raine);
    let hp = battle.actors()[user.index()].hp;
    let grade = battle.ledger().grade();
    release(
        &mut candidate,
        &mut battle,
        Release {
            user,
            target: user,
            item: 1,
        },
    )?;
    assert_eq!(battle.actors()[user.index()].hp, hp); // Full-vitals gel still spends.
    assert_eq!(candidate.items().counts[&1], 1);
    assert_eq!(candidate.party.battles.items, field.battles.items);
    assert_eq!(battle.ledger().items[user.index()], 1);
    assert_eq!(battle.ledger().grade(), grade - 5);
    assert!(!battle.is_diagnostic());
    let outcome = escape(&mut candidate, &mut battle)?;
    let completed = candidate.finish(&battle, &outcome)?;
    assert_eq!(completed.party.items[&1], 1);
    assert!(completed.party.battles.battle_gel_used);
    assert_eq!(completed.party.battles.items[3], field.battles.items[3] + 1);
    assert_eq!(completed.party.battles.items[0], field.battles.items[0]);
    assert_eq!(field.items[&1], 2);
    Ok(())
}

#[test]
#[ignore = "requires current Items/profile/script publications; CPU only"]
fn all_divide_and_hourglass_inventory_return_queues_selected_user_and_spends_at_release()
-> Result<()> {
    use crate::battle::command::{self, ActorSelection, InputKind, View};

    fn visit(
        owner: &mut command::Owner,
        candidate: &mut Candidate,
        battle: &mut Battle,
        input: command::Input,
    ) -> Result<command::Step> {
        owner.step_with_candidate(input, None, battle, candidate)
    }

    for item in [38, 39] {
        let PreparedFixture {
            mut candidate,
            mut battle,
            field,
            ..
        } = prepared_fixture_with_party(&[4, 1], 2, &[], |party, session, _| {
            party.items.clear();
            party
                .change_item(session, item, 2)
                .map_err(anyhow::Error::msg)?;
            Ok(())
        })?;
        enter_battle(&mut candidate, &mut battle)?;
        let user = candidate.setup.actors[0].0;
        let menu_actor = candidate.setup.actors[1].0;
        assert_ne!(user, menu_actor);
        assert!(battle.can_queue_item(user)?);
        let random = battle.random_state();
        let grade = battle.ledger().grade();
        let party_before = serde_json::to_value(&candidate.party)?;
        let mut owner = command::Owner::default();
        let admission = command::Admission {
            actor: menu_actor,
            controller: ControllerPort::Three,
            enabled: 0xff,
        };
        let opened = owner.step_with_candidate(
            command::Input {
                controller: ControllerPort::Three,
                open: edge(),
                ..Default::default()
            },
            Some(admission),
            &mut battle,
            &mut candidate,
        )?;
        assert!(opened.paused);
        assert_eq!(owner.controller(), Some(ControllerPort::Three));
        assert!(matches!(owner.frame().unwrap().view, View::Strip));
        for _ in 0..2 {
            visit(
                &mut owner,
                &mut candidate,
                &mut battle,
                command::Input {
                    controller: ControllerPort::Three,
                    step: -1,
                    ..Default::default()
                },
            )?;
        }
        visit(
            &mut owner,
            &mut candidate,
            &mut battle,
            command::Input {
                controller: ControllerPort::Three,
                confirm_a: edge(),
                ..Default::default()
            },
        )?;
        let frame = owner.frame().unwrap();
        assert_eq!(frame.actor, menu_actor);
        assert!(matches!(
            frame.view,
            View::User(ActorSelection { actor, slot: 0, eligible: true, .. }) if actor == user
        ));
        let entered = visit(
            &mut owner,
            &mut candidate,
            &mut battle,
            command::Input {
                controller: ControllerPort::Three,
                confirm_a: edge(),
                ..Default::default()
            },
        )?;
        assert!(entered.events.contains(&command::Event::Cue(2)));
        assert!(entered.paused);
        assert_eq!(owner.input_kind(), InputKind::SharedMenu);
        assert!(matches!(owner.frame().unwrap().view, View::Inventory(_)));
        assert_eq!(battle.pending_item(), None);

        let mut ready = false;
        for _ in 0..20 {
            let opened = visit(
                &mut owner,
                &mut candidate,
                &mut battle,
                command::Input::default(),
            )?;
            assert!(opened.paused);
            let View::Inventory(list) = owner.frame().unwrap().view else {
                panic!("item{item} lost inventory during opening");
            };
            assert_eq!(list.rows.len(), 1);
            assert_eq!(list.rows[0].id, item);
            assert_eq!(battle.pending_item(), None);
            if list.fade == 0 {
                ready = true;
                break;
            }
        }
        assert!(ready, "item inventory should finish opening");
        let accepted = visit(
            &mut owner,
            &mut candidate,
            &mut battle,
            command::Input {
                // Shared inventory accepts the merged menu action from another pad.
                controller: ControllerPort::One,
                shared_menu: Some(MenuAction::Confirm),
                ..Default::default()
            },
        )?;
        assert!(accepted.events.contains(&command::Event::Cue(2)));
        assert!(accepted.paused);
        assert_eq!(battle.pending_item(), None);
        let mut closed = false;
        for _ in 0..20 {
            let returned = visit(
                &mut owner,
                &mut candidate,
                &mut battle,
                command::Input::default(),
            )?;
            if owner.controller().is_none() {
                assert!(returned.paused);
                assert_eq!(returned.events, [command::Event::VoiceStreamsPaused(false)]);
                assert!(owner.frame().is_none());
                closed = true;
                break;
            }
            assert!(returned.paused);
            assert!(matches!(owner.frame().unwrap().view, View::Inventory(_)));
            assert_eq!(battle.pending_item(), None);
        }
        assert!(closed, "item selection should finish its transition");
        let request = Release {
            user,
            target: user,
            item,
        };
        assert_eq!(battle.pending_item(), Some(request));
        assert_eq!(battle.random_state(), random);
        assert_eq!(battle.ledger().grade(), grade);
        assert_eq!(serde_json::to_value(&candidate.party)?, party_before);
        assert!(!battle.all_divide_active());
        assert_eq!(battle.hourglass_remaining(), 0);

        let resumed = visit(
            &mut owner,
            &mut candidate,
            &mut battle,
            command::Input::default(),
        )?;
        assert!(!resumed.paused);
        assert!(owner.frame().is_none());
        assert_eq!(battle.pending_item(), Some(request));
        for _ in 0..300 {
            step(&mut candidate, &mut battle)?;
            if battle.pending_item().is_none() {
                break;
            }
        }
        assert_eq!(battle.pending_item(), None);
        assert_eq!(candidate.party.items[&item], 1);
        assert_eq!(field.items[&item], 2);
        assert_eq!(candidate.party.battles.items, field.battles.items);
        assert_eq!(battle.ledger().items[user.index()], 1);
        assert_eq!(battle.ledger().items[menu_actor.index()], 0);
        assert_eq!(battle.ledger().grade(), grade - 5);
        assert_eq!(battle.all_divide_active(), item == 38);
        assert_eq!(battle.hourglass_remaining() > 0, item == 39);
        assert!(!battle.is_diagnostic());
        for _ in 0..10 {
            step(&mut candidate, &mut battle)?;
        }
        assert_eq!(candidate.party.items[&item], 1);
        assert_eq!(battle.ledger().items[user.index()], 1);
    }
    Ok(())
}

#[test]
#[ignore = "requires current Items/profile/script publications; CPU only"]
fn diagnostic_after_successful_item_cannot_produce_a_committable_candidate() -> Result<()> {
    let (mut candidate, mut battle, field) = fixture(&[4, 1], 2)?;
    let user = candidate.setup.actors[0].0;
    let request = Release {
        user,
        target: user,
        item: 1,
    };
    release(&mut candidate, &mut battle, request)?;
    assert_eq!(candidate.items().counts[&1], 1);
    assert_eq!(battle.ledger().items[user.index()], 1);
    // A second use is queued against a stale quantity. The live core must
    // cancel at preflight, mark the skipped simulation diagnostic and continue.
    for _ in 0..300 {
        if battle.can_queue_item(user)? {
            break;
        }
        step(&mut candidate, &mut battle)?;
    }
    candidate.party.items.remove(&1);
    let diagnostics = Diagnostics::new(false);
    battle.set_diagnostics(diagnostics.clone());
    release(&mut candidate, &mut battle, request)?;
    assert!(battle.is_diagnostic());
    assert!(!diagnostics.entries().is_empty());
    assert_eq!(battle.ledger().items[user.index()], 1);
    let outcome = escape(&mut candidate, &mut battle)?;
    assert_eq!(
        candidate
            .finish(&battle, &outcome)
            .err()
            .unwrap()
            .to_string(),
        "diagnostic battle cannot commit its candidate"
    );
    assert_eq!(field.items[&1], 2);
    assert_eq!(field.battles.items[3], 0);
    Ok(())
}

#[test]
#[ignore = "requires current Conditions/items/profile/script publications; CPU only"]
fn live_item_conditions_export_base_only() -> Result<()> {
    use resonance_battle::{
        Element,
        conditions::{
            Condition::{Enchanted, Flare, PhysicalProtection, Quartz},
            ConditionSet,
        },
    };
    let (mut candidate, mut battle, _) = fixture(&[4, 1], 2)?;
    let user = candidate.setup.actors[0].0;
    for item in [14, 18, 44] {
        candidate
            .party
            .change_item(&candidate.session, item, 1)
            .map_err(anyhow::Error::msg)?;
        for _ in 0..300 {
            if battle.can_queue_item(user)? {
                break;
            }
            step(&mut candidate, &mut battle)?;
        }
        release(
            &mut candidate,
            &mut battle,
            Release {
                user,
                target: user,
                item,
            },
        )?;
    }
    let actor = &battle.actors()[user.index()];
    let effects = ConditionSet::of(&[Flare, PhysicalProtection, Quartz, Enchanted]);
    assert_eq!(actor.conditions.base().intersection(effects), effects);
    assert_eq!(actor.elements.enchantment, Some(Element::Water));
    assert_eq!(
        candidate.conditions(&battle)[0],
        actor.conditions.effective()
    );
    candidate.party.members[3].ailments.paralysis = true; // Controlled stale saved data.
    candidate.sync_party(&battle)?;
    assert_eq!(candidate.party.members[3].ailments, Default::default());

    Ok(())
}

#[test]
#[ignore = "requires current party/profile/script publications; CPU only"]
fn colette_critical_up_activation_and_finish_preserve_discovery_once() -> Result<()> {
    for (remembered, recent) in [(false, false), (true, false), (true, true)] {
        let PreparedFixture {
            mut candidate,
            mut battle,
            field,
            ..
        } = prepared_fixture_with_party(&[2, 1], 2, &[], |party, _, menus| {
            let recipe = menus.ex_skills.characters[1]
                .compounds
                .iter()
                .position(|row| row.skill == 69)
                .unwrap() as u8;
            let member = &mut party.members[1];
            member.ex_gems = [1, 1, 0, 0];
            member.ex_skills = [1, 2, 0, 0];
            if remembered {
                member.compound_ex_skills.insert(recipe);
            }
            if recent {
                member.recent_compound_ex_skills.insert(recipe);
            }
            Ok(())
        })?;
        let recipe = candidate.menus.ex_skills.characters[1]
            .compounds
            .iter()
            .position(|row| row.skill == 69)
            .unwrap() as u8;
        let field_before = serde_json::to_value(&field)?;
        assert_eq!(
            candidate.new_ex_skills,
            if remembered {
                BTreeSet::new()
            } else {
                [Character::Colette].into()
            }
        );
        assert!(
            candidate.party.members[1]
                .compound_ex_skills
                .contains(&recipe)
        );
        assert_eq!(
            candidate.party.members[1]
                .recent_compound_ex_skills
                .contains(&recipe),
            !remembered || recent
        );
        assert_eq!(
            field.members[1].compound_ex_skills.contains(&recipe),
            remembered
        );
        let colette = candidate
            .setup
            .actors
            .iter()
            .find(|&&(_, character)| character == Character::Colette)
            .unwrap()
            .0;
        let equipment = field.members[1]
            .equipment_traits(&candidate.menus)
            .critical_chance_bonus;
        assert_eq!(
            battle.actors()[colette.index()]
                .equipment
                .damage
                .critical_chance_bonus,
            equipment.saturating_add(5).min(100)
        );
        for _ in 0..300 {
            if battle.phase() != resonance_battle::BattlePhase::Entry {
                break;
            }
            step(&mut candidate, &mut battle)?;
        }
        assert!(battle.phase() != resonance_battle::BattlePhase::Entry);
        // Exercise the existing real finish transaction without inventing an
        // escape gauge route or a fabricated victory acknowledgement.
        let outcome = escape(&mut candidate, &mut battle)?;
        let completed = candidate.finish(&battle, &outcome)?;
        let restored: Party = serde_json::from_value(serde_json::to_value(&completed.party)?)?;
        assert!(restored.members[1].compound_ex_skills.contains(&recipe));
        assert_eq!(
            restored.members[1]
                .recent_compound_ex_skills
                .contains(&recipe),
            !remembered || recent
        );
        assert_eq!(restored.members[1].ex_skills, [1, 2, 0, 0]);
        assert_eq!(serde_json::to_value(&field)?, field_before);
    }
    Ok(())
}

#[test]
fn stealing_consumes_each_enemy_once_even_when_inventory_is_full() -> Result<()> {
    use resonance_battle::item::StealRequest;
    let PreparedFixture { mut candidate, .. } = native_fixture(
        &[2],
        |party, _, menus| {
            party.items.clear();
            party.found_items.clear();
            party.recent_items.clear();
            menus.items.resize(11, menus.items[0].clone());
            menus.items[10].category = 0;
            menus.presentation.items = Some(resonance_content::menu_data::ItemCaptions {
                items: (0..11)
                    .map(|_| {
                        Some(resonance_content::menu_data::ItemText {
                            name: "Panacea Bottle".into(),
                            description: String::new(),
                            details: String::new(),
                        })
                    })
                    .collect(),
                item_categories: vec![],
                inventory_categories: vec![],
                item_group_prompt: None,
                item_bottle_count: None,
            });
            Ok(())
        },
        |mut actors, setup| {
            setup.enemies[0].steal = Some(10);
            setup.enemies[0].steal_chance = 21;
            let enemy = &setup.enemies[0];
            setup.enemies.push(PreparedEnemy {
                actor: ActorId::from_index(actors.len())?,
                level: enemy.level,
                grade: enemy.grade,
                steal: enemy.steal,
                steal_chance: enemy.steal_chance,
                reward: enemy.reward.clone(),
            });
            actors.push(actors.last().unwrap().clone());
            native_battle(actors)
        },
    )?;
    let first = candidate.setup.enemies[0].actor;
    let second = candidate.setup.enemies[1].actor;
    let monster = candidate.setup.enemies[0].reward.monster;
    let drops = candidate.setup.enemies[0].reward.drops;
    let request = StealRequest {
        user: ActorId::from_index(0)?,
        target: first,
        adjusted_roll: 21,
        guaranteed: false,
        gald: false,
    };
    assert!(candidate.steal(request)?.is_none());
    assert!(candidate.party.items.is_empty());
    assert!(
        candidate
            .steal(StealRequest {
                adjusted_roll: 12,
                ..request
            })?
            .is_some()
    );
    assert_eq!(candidate.party.items[&10], 1);
    assert!(candidate.party.found_items.contains(&10));
    assert_eq!(candidate.party.recent_items[0], 10);
    assert!(candidate.party.monsters[&monster].steal);
    assert!(
        candidate
            .steal(StealRequest {
                adjusted_roll: 0,
                guaranteed: true,
                ..request
            })?
            .is_none()
    );
    candidate.party.items.insert(10, 20);
    candidate.party.found_items.clear();
    candidate.party.recent_items.clear();
    assert!(
        candidate
            .steal(StealRequest {
                target: second,
                guaranteed: true,
                ..request
            })?
            .is_some()
    );
    assert_eq!(candidate.party.items[&10], 20);
    assert!(candidate.party.found_items.is_empty() && candidate.party.recent_items.is_empty());
    candidate.party.items.remove(&10);
    assert!(
        candidate
            .steal(StealRequest {
                target: second,
                guaranteed: true,
                ..request
            })?
            .is_none()
    );
    assert_eq!(candidate.setup.enemies[0].reward.drops, drops);
    // A guaranteed attempt cannot create an absent steal item.
    candidate.stolen_enemies.clear();
    candidate.setup.enemies[0].steal = None;
    assert!(
        candidate
            .steal(StealRequest {
                guaranteed: true,
                ..request
            })?
            .is_none()
    );
    // Rover can take Gald without an item; capacity still consumes the attempt.
    candidate.setup.enemies[0].reward.gald = 127;
    candidate.party.gald = 99_999_995;
    let rover = StealRequest {
        guaranteed: true,
        gald: true,
        ..request
    };
    assert_eq!(candidate.steal(rover)?.as_deref(), Some("12 Gald"));
    assert_eq!(candidate.party.gald, 99_999_999);
    assert!(candidate.steal(rover)?.is_none());
    assert_eq!(candidate.setup.enemies[0].reward.gald, 127);

    candidate.stolen_enemies.remove(&second);
    candidate.setup.enemies[1].reward.gald = 127;
    candidate.party.gald = 500;
    candidate.party.items.insert(10, 20);
    assert_eq!(
        candidate
            .steal(StealRequest {
                target: second,
                ..rover
            })?
            .as_deref(),
        Some("Panacea Bottle + 12 Gald")
    );
    assert_eq!(candidate.party.gald, 512);
    assert_eq!(candidate.party.items[&10], 20);
    Ok(())
}
