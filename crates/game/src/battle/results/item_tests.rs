//! Result preparation, equipment, and reward feedback through CPU fixtures.
mod equipment_model;
mod hourglass_lifecycle;
mod magic_mist;
use super::*;
use crate::battle::{
    encounter::{Inputs, PrepareOptions},
    voice::Sound,
};
use crate::menu::MenuAction;
use resonance_battle::{
    BattleInput,
    item::{Provider, Release},
};
use resonance_content::{
    diagnostics::Diagnostics,
    prepared::{Cache, Files},
};
use resonance_events::{
    battle::{DefeatPolicy, Setup as Encounter},
    party::MonsterKnowledge,
};

pub(crate) struct Display;
impl Presentation for Display {
    fn request(&mut self, _: Request, _: Option<&Results>, _: &Battle) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn fixture(formation: &[u8], encounter: u16) -> Result<(Candidate, Battle, Party)> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        field,
        ..
    } = prepared_fixture(formation, encounter, &[])?;
    enter_battle(&mut candidate, &mut battle)?;
    Ok((candidate, battle, field))
}

pub(crate) struct PreparedFixture {
    pub candidate: Candidate,
    pub battle: Battle,
    pub field: Party,
    pub lifecycle: crate::battle::lifecycle::Lifecycle,
}

/// Small native encounter shared by result arithmetic and lifecycle boundary tests.
pub(crate) fn native_fixture(
    formation: &[u8],
    configure: impl FnOnce(&mut Party, &SessionData, &mut MenuData) -> Result<()>,
    prepare: impl FnOnce(Vec<resonance_battle::Actor>, &mut Setup) -> Result<Battle>,
) -> Result<PreparedFixture> {
    use crate::battle::{lifecycle::Lifecycle, party, rewards};
    use resonance_battle::{Control, Side};
    let (session, catalogue, titles, mut field) = rewards::tests::reward_fixture();
    let (mut menus, _) = party::projection_tests::fixture()?;
    menus.titles = titles;
    field.formation = formation.to_vec();
    configure(&mut field, &session, &mut menus)?;
    let roster = formation
        .iter()
        .take(4)
        .enumerate()
        .map(|(slot, &character)| Ok((ActorId::from_index(slot)?, character)))
        .collect::<Result<Vec<_>>>()?;
    let mut actors = roster
        .iter()
        .map(|&(_, character)| {
            let member = &field.members[usize::from(character - 1)];
            party::actor(
                &party::loadout(&menus, member, usize::from(character - 1))?,
                member,
                Control::Manual,
                [0.; 3],
                0.,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let enemy_id = ActorId::from_index(actors.len())?;
    let mut enemy = actors[0].clone();
    enemy.side = Side::Enemy;
    enemy.control = Control::Enemy;
    actors.push(enemy);
    let mut setup = Setup {
        devils_arms_unlocked: false,
        enemies: vec![PreparedEnemy {
            actor: enemy_id,
            level: 1,
            grade: 0,
            reward: rewards::EnemyReward {
                monster: 0,
                experience: 0,
                gald: 0,
                drops: [None; 2],
                drop_chances: [0; 2],
            },
        }],
        level_difference: 0,
        actors: roster,
        formation: 1,
        style: ResultStyle {
            play_music: false,
            celebrate: false,
        },
        story: 0,
        colette_state: 0,
        victory_story_flags: [false; 2],
        groups: vec![],
        performances: vec![],
        postures: vec![],
        victory_voices: Default::default(),
    };
    let battle = prepare(actors, &mut setup)?;
    let candidate = Candidate::new(
        setup,
        battle.actors(),
        field.clone(),
        Default::default(),
        Arc::new(session),
        Arc::new(menus),
        Arc::new(catalogue),
    )?;
    Ok(PreparedFixture {
        candidate,
        battle,
        field,
        lifecycle: Lifecycle::new(None),
    })
}

pub(crate) fn prepared_fixture(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
) -> Result<PreparedFixture> {
    prepared_fixture_with_party(formation, encounter, techniques, |_, _, _| Ok(()))
}

pub(crate) fn prepared_fixture_with_party(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
    configure: impl FnOnce(&mut Party, &SessionData, &MenuData) -> Result<()>,
) -> Result<PreparedFixture> {
    prepared_fixture_with_story(formation, encounter, techniques, 2500, configure)
}

pub(crate) fn prepared_fixture_with_story(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
    story: i32,
    configure: impl FnOnce(&mut Party, &SessionData, &MenuData) -> Result<()>,
) -> Result<PreparedFixture> {
    load_fixture(
        formation,
        encounter,
        techniques,
        Diagnostics::default(),
        |party, session, menus, _| configure(party, session, menus),
    )?
    .prepare(story, |sound| Ok(Some(sound)))
}

pub(crate) struct LoadedFixture {
    pub inputs: Inputs,
    party: Party,
    session: Arc<SessionData>,
    menus: Arc<MenuData>,
    catalogue: Arc<Catalogue>,
}

pub(crate) fn load_fixture(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
    diagnostics: Diagnostics,
    configure: impl FnOnce(&mut Party, &SessionData, &MenuData, &mut Files) -> Result<()>,
) -> Result<LoadedFixture> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
        });
    let mut cache = Cache::default();
    let mut retained = Files::load_with_diagnostics(
        &root,
        &["fields/map-332.preload.json"],
        &mut cache,
        || false,
        diagnostics,
    )?;
    let menus: Arc<MenuData> = Arc::new(retained.json("game/menu-data.json")?);
    let mut session: SessionData = retained.json("game/session-data.json")?;
    session.rules = Some(menus.clone());
    let session = Arc::new(session);
    let mut party = Party::new(&session, Default::default())?;
    party.formation = formation.to_vec();
    party.field_leader = formation[0];
    party.settings.battle_controls = [0; 4];
    for &character in formation {
        party
            .raise_level(&session, usize::from(character - 1), 30, None, || 7)
            .map_err(anyhow::Error::msg)?;
    }
    for member in &mut party.members {
        member.techniques.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
        member.ex_skills = [0; 4];
        member.compound_ex_skills.clear();
        member.recent_compound_ex_skills.clear();
    }
    party
        .change_item(&session, 1, 2)
        .map_err(anyhow::Error::msg)?;
    party
        .change_item(&session, 37, 4)
        .map_err(anyhow::Error::msg)?;
    for &(character, technique) in techniques {
        let member = &mut party.members[usize::from(character - 1)];
        member.techniques.insert(technique);
    }
    configure(&mut party, &session, &menus, &mut retained)?;
    party.validate(&session)?;
    let inputs = Inputs::load(
        &root,
        &retained,
        &menus,
        &session,
        &party,
        Encounter {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(encounter),
            arena: 13,
            defeat: DefeatPolicy::ResumeEvent,
            music: None,
        },
        &mut cache,
        || false,
    )?;
    let catalogue = inputs.catalogue.clone();
    Ok(LoadedFixture {
        inputs,
        party,
        session,
        menus,
        catalogue,
    })
}

impl LoadedFixture {
    pub fn prepare(
        self,
        story: i32,
        sound: impl FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    ) -> Result<PreparedFixture> {
        let Self {
            inputs,
            party,
            session,
            menus,
            catalogue,
        } = self;
        let prepared = inputs.prepare(
            &menus,
            PrepareOptions {
                random_seed: 0x2345,
                map: 332,
                world_music: 0,
                story,
                story3: false,
                colette_state: 0,
                devils_arms_unlocked: false,
                victory_story_flags: [true; 2],
                overlimit_boost: false,
            },
            sound,
        )?;
        let lifecycle = prepared.lifecycle;
        let candidate = Candidate::new(
            prepared.results,
            prepared.core.actors(),
            party.clone(),
            Default::default(),
            session,
            menus,
            catalogue,
        )?;
        Ok(PreparedFixture {
            candidate,
            battle: prepared.core,
            field: party,
            lifecycle,
        })
    }
}

pub(crate) fn step(
    candidate: &mut Candidate,
    battle: &mut Battle,
) -> Result<resonance_battle::BattleFrame> {
    let cues = candidate.world_update(battle, BattleInput::default())?;
    Ok(battle.publish(cues))
}

pub(crate) fn enter_battle(candidate: &mut Candidate, battle: &mut Battle) -> Result<()> {
    for _ in 0..600 {
        if battle.phase() != resonance_battle::BattlePhase::Entry {
            return Ok(());
        }
        step(candidate, battle)?;
        ensure!(!battle.is_diagnostic(), "battle entry entered diagnostics");
    }
    ensure!(
        battle.phase() != resonance_battle::BattlePhase::Entry,
        "battle entry did not complete"
    );
    Ok(())
}

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

pub(crate) fn escape(candidate: &mut Candidate, battle: &mut Battle) -> Result<BattleOutcome> {
    // Isolate the persistent finish boundary from the authored escape timeline.
    // Actual event/once-only commit is covered by the existing Completed tests.
    battle.recognize_escape(true)?;
    ensure!(
        battle.recognize_result() == Some(BattleResult::Escaped),
        "escape not recognized"
    );
    candidate.recorded_escape = true;
    candidate.party.battles.record_escape();
    battle
        .finish_result()?
        .outcome
        .context("missing escape outcome")
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
        |actors, _| {
            resonance_battle::PreparedBattle::new(
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
        .find(|&&(_, character)| character != 4)
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
    if let Some(&(raine, _)) = roster.iter().find(|&&(_, character)| character == 4) {
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
                reward: enemy.reward.clone(),
            });
            actors.push(actors.last().unwrap().clone());
            resonance_battle::PreparedBattle::new(
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
    assert_eq!(candidate.setup.actors[0].1, 4);
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
    use resonance_battle::ButtonInput;

    fn edge() -> ButtonInput {
        ButtonInput {
            held: true,
            pressed: true,
            released: false,
        }
    }

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
            controller: 2,
            enabled: 0xff,
        };
        let opened = owner.step_with_candidate(
            command::Input {
                controller: 2,
                open: edge(),
                ..Default::default()
            },
            Some(admission),
            &mut battle,
            &mut candidate,
        )?;
        assert!(opened.paused);
        assert_eq!(owner.controller(), Some(2));
        assert!(matches!(owner.frame().unwrap().view, View::Strip));
        for _ in 0..2 {
            visit(
                &mut owner,
                &mut candidate,
                &mut battle,
                command::Input {
                    controller: 2,
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
                controller: 2,
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
                controller: 2,
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
                controller: 0,
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
                vec![]
            } else {
                vec![(2, recipe)]
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
            .find(|&&(_, id)| id == 2)
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
