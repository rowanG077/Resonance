//! Original assets remain local. This is behavior evidence, not image/audio acceptance.
#[path = "common/action.rs"]
mod action;
mod common;
use action::{Action, Action::*};
use common::{asset_root, cooked};
use resonance_content::{field::FieldAssets, menu_data::MenuData, session::SessionData};
use resonance_events::{PersistentState, party::Party};
use resonance_game::field::{FieldCheckpoint, FieldEntry, FieldInput, FieldSession};
use std::{fs, sync::Arc};

fn classroom(mut entry: FieldEntry) -> FieldSession {
    let root = asset_root();
    entry.menu_data = Some(Arc::new(cooked("game/menu-data.json")));
    entry.text = Arc::new(cooked("game/text.json"));
    let assets: FieldAssets = cooked("fields/map-340.json");
    FieldSession::enter(
        &fs::read(root.join(&assets.script.path)).unwrap(),
        cooked(&assets.messages),
        &assets,
        entry,
    )
    .unwrap()
}
/// Start after the lesson; callers choose when initialization has reached their checkpoint.
fn classroom_entry(data: &Arc<SessionData>, party: Party, story: i32) -> FieldEntry {
    let mut entry = FieldEntry {
        data: Some(data.clone()),
        persistent: PersistentState {
            party: Some(party),
            ..Default::default()
        },
        position: [-52., -619., 0.],
        ..Default::default()
    };
    entry
        .persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, story)
        .unwrap();
    entry
}

fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    serde_json::from_slice(&serde_json::to_vec(value).unwrap()).unwrap()
}

fn readable(page: &resonance_game::dialogue::DialoguePlayer) -> bool {
    !page.closed && !page.persistent && page.fully_revealed()
}

fn press(session: &mut FieldSession, input: impl Into<FieldInput>) {
    session.step(input.into()).unwrap();
    session.step(Default::default()).unwrap();
    settle_menu_motion(session);
}

fn menu_in_motion(menu: &resonance_game::menu::Menu) -> bool {
    use resonance_game::menu::Page;
    menu.main_animating()
        || match menu.page {
            Page::Equip => menu.equipment.transition.animating() || menu.equipment.scroll != 0,
            Page::Tech => menu.tech.animating(),
            Page::Unison => menu.unison.animating(),
            Page::ExSkills => menu.ex_skills.animating(),
            Page::Cooking => menu.cooking.transition.animating() || menu.cooking.scroll != 0,
            Page::Customize => {
                menu.customize.transition.animating()
                    || menu.customize.scroll != 0
                    || menu.customize.color_scroll != 0
            }
            _ => false,
        }
}

fn settle_menu_motion(session: &mut FieldSession) {
    for _ in 0..40 {
        if session
            .menu
            .as_ref()
            .is_none_or(|menu| !menu_in_motion(menu))
        {
            return;
        }
        let tick = session.events.tick();
        assert!(session.checkpoint().is_err());
        session.step(Default::default()).unwrap();
        assert_eq!(
            session.events.tick(),
            tick,
            "menu motion must pause the field"
        );
    }
    panic!("menu transition did not finish");
}

#[test]
#[ignore = "requires locally cooked school grounds and menus; no devices"]
fn party_order_and_field_leader_survive_menu_close_and_field_restart() {
    use resonance_game::menu::{Menu, Page};
    let root = asset_root();
    let data = Arc::new(cooked("game/session-data.json"));
    let menus: Arc<MenuData> = Arc::new(cooked("game/menu-data.json"));
    let assets: FieldAssets = cooked("fields/map-332.json");
    let enter = |mut entry: FieldEntry| {
        entry.menu_data = Some(menus.clone());
        FieldSession::enter(
            &fs::read(root.join(&assets.script.path)).unwrap(),
            cooked(&assets.messages),
            &assets,
            entry,
        )
        .unwrap()
    };
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    let mut memory = symphonia_script_vm::Memory::default();
    memory
        .write(0x40, symphonia_script::Width::S32, 2500)
        .unwrap();
    let fields = [330, 332, 340].into();
    let mut field = enter(FieldEntry {
        data: Some(data.clone()),
        persistent: PersistentState {
            memory,
            party: Some(party),
            event_flags: [520].into(), // The memory-circle tutorial has been read.
            ..Default::default()
        },
        position: [1968., 1005., 0.],
        heading: 276.,
        available_fields: fields,
        ..Default::default()
    });
    advance_to(&mut field, |f| f.checkpoint().is_ok(), |_, _| false);

    for key in [OpenMenu, Down, Down, Down, Accept] {
        press(&mut field, key);
    }
    assert_eq!(field.menu.as_ref().unwrap().page, Page::Party);
    assert_eq!(field.events.world.party.as_ref().unwrap().field_leader, 2);
    assert_eq!(
        field.events.world.controlled_actor, 1,
        "the field stays frozen until menu close"
    );
    for key in [OpenMenu, Up, Accept, OpenMenu, Down, Cancel] {
        press(&mut field, key);
    }
    assert_eq!(
        field.events.world.party.as_ref().unwrap().formation,
        [2, 1, 3]
    );
    assert_eq!(field.events.world.party.as_ref().unwrap().field_leader, 2);
    assert!(field.menu.as_ref().unwrap().swap_character.is_none());
    for key in [Down, Accept, Cancel, Cancel] {
        press(&mut field, key);
    }
    assert!(field.menu.is_none());
    assert_eq!(field.events.world.controlled_actor, 3);
    assert!(!field.events.world.actors[&1].visible);
    assert_eq!(field.events.world.actors[&3].resource, 3);
    let checkpoint = field.checkpoint().unwrap();
    assert_eq!(checkpoint.position, [1968., 1005., 0.]);
    assert_eq!(checkpoint.heading, 276.);
    let checkpoint: FieldCheckpoint = roundtrip(&checkpoint);
    let mut restored = enter(
        checkpoint
            .clone()
            .entry(&assets, data.clone(), [330, 332, 340].into())
            .unwrap(),
    );
    advance_to(&mut restored, |f| f.checkpoint().is_ok(), |_, _| false);
    assert_eq!(restored.events.world.controlled_actor, 3);
    assert_eq!(
        restored.checkpoint().unwrap().progress.party.formation,
        [2, 1, 3]
    );
    assert_eq!(
        restored.checkpoint().unwrap().progress.party.field_leader,
        3
    );
    assert_eq!(
        restored
            .events
            .world
            .field_camera
            .as_ref()
            .unwrap()
            .current()
            .actor,
        3
    );

    // Loading an incapacitated leader selects the first mobile member, even
    // when manual selection is locked. Revival does not undo that selection.
    let mut knocked_out_save = restored.checkpoint().unwrap();
    let party = &mut knocked_out_save.progress.party;
    party.leader_locked = true;
    party.members[2].conditions = 0x8000_0000;
    party.members[2].hp = 0;
    party.members[1].conditions = 0x100;
    let mut restored = enter(
        knocked_out_save
            .entry(&assets, data.clone(), [330, 332, 340].into())
            .unwrap(),
    );
    advance_to(&mut restored, |f| f.checkpoint().is_ok(), |_, _| false);
    restored.step(Default::default()).unwrap();
    assert_eq!(restored.events.world.controlled_actor, 1);
    let saved = restored.checkpoint().unwrap();
    assert_eq!(saved.progress.party.field_leader, 1);
    assert_eq!(saved.position, checkpoint.position);
    assert_eq!(saved.heading, checkpoint.heading);
    assert_eq!(
        restored
            .events
            .world
            .field_camera
            .as_ref()
            .unwrap()
            .current()
            .actor,
        1
    );
    let party = restored.events.world.party.as_mut().unwrap();
    party.members[2].conditions = 0;
    party.members[2].hp = 1;
    restored.step(Default::default()).unwrap();
    assert_eq!(restored.events.world.controlled_actor, 1);
    // If nobody can lead, keep the current actor rather than inventing one.
    let party = restored.events.world.party.as_mut().unwrap();
    for member in &mut party.members {
        member.conditions = 0x100;
    }
    restored.step(Default::default()).unwrap();
    assert_eq!(restored.events.world.controlled_actor, 1);

    // Reserve members scroll independently of the battle's four active slots.
    let mut menu = Menu::new(Page::Party, Some(checkpoint), false);
    let party = &mut menu.checkpoint.as_mut().unwrap().progress.party;
    party.formation = (1..=8).collect();
    party.validate(&data).unwrap();
    let press = |menu: &mut Menu, action: Action| {
        let cue = menu.step(action.input());
        menu.step(Default::default());
        cue
    };
    assert_eq!(press(&mut menu, Next), Some(0x26));
    assert_eq!((menu.character, menu.first_character), (4, 4));
    for _ in 0..4 {
        press(&mut menu, Down);
    }
    assert_eq!(menu.character, 7);
    for key in [OpenMenu, Previous, OpenMenu] {
        press(&mut menu, key);
    }
    let party = &mut menu.checkpoint.as_mut().unwrap().progress.party;
    assert_eq!(party.formation, [1, 2, 3, 8, 5, 6, 7, 4]);
    assert_eq!(party.field_leader, 3);
    party.leader_locked = true;
    assert_eq!(press(&mut menu, Accept), Some(4));
    let party = &mut menu.checkpoint.as_mut().unwrap().progress.party;
    party.leader_locked = false;
    for condition in [0x8000_0000, 0x100] {
        menu.checkpoint.as_mut().unwrap().progress.party.members[7].conditions = condition;
        assert_eq!(press(&mut menu, Accept), Some(4));
        let party = &menu.checkpoint.as_ref().unwrap().progress.party;
        assert_eq!(party.field_leader, 3);
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF menus/classroom; no devices"]
fn customization_drafts_commit_to_field_and_save_without_changing_battle_control_modes() {
    use resonance_game::menu::{Page, customize::Focus};
    let data = Arc::new(cooked("game/session-data.json"));
    let party = Party::new(&data, Default::default()).unwrap();
    let original = party.settings.preferences.clone();
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, |s| s.checkpoint().is_ok(), |_, _| false);

    for key in [OpenMenu, Left, Accept, Down, Down, Accept] {
        press(&mut session, key);
    }
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(menu.page, Page::Customize);
    assert_eq!(
        menu.customize.draft,
        menu.resources.as_ref().unwrap().data.customize.defaults
    );
    for key in [Right, Up, Accept] {
        press(&mut session, key);
    }
    assert_eq!(
        session.menu.as_ref().unwrap().customize.draft,
        original,
        "Cancel discards edits and stays in Customize"
    );
    for key in [
        Down, Right, Down, Right, Down, Right, Down, Left, Down, Accept, Accept, Right, Cancel,
        Down, Accept, Left, Down, Left, Down, Left, Down, Down, Down, Right, Cancel, Down, Accept,
        Right, Cancel, Down, Right, Down, Right, Down, Right, Down, Right, Down, Right, Down,
        Right, Down, Accept, Right, Up,
    ] {
        press(&mut session, key);
    }
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(menu.customize.focus, Focus::Position);
    assert_eq!(menu.customize.draft.screen_position, [1, -1]);
    for key in [Start, Left, Down, Cancel] {
        press(&mut session, key);
    }
    let expected = session.menu.as_ref().unwrap().customize.draft.clone();
    expected.validate().unwrap();
    assert_eq!(expected.message_speed, 4);
    assert_eq!(expected.battle_rank, 1);
    assert_eq!((expected.window, expected.background), (2, 4));
    assert_eq!(expected.colors.menu, [88, 72, 64, 216]);
    assert_eq!(expected.volumes.channels(), [120, 120, 120, 127, 127]);
    assert_eq!(expected.button_map, [1, 0, 2, 3, 4, 5, 6]);
    assert!(
        !expected.stereo
            && !expected.skit_notifications
            && !expected.movie_subtitles
            && !expected.event_voiceover
            && !expected.battle_voiceover
            && !expected.rumble
            && !expected.battle_auto_zoom
    );
    assert_eq!(expected.screen_position, [-1, 1]);
    let settings = &session.events.world.party.as_ref().unwrap().settings;
    assert_eq!(
        settings.preferences, original,
        "edits remain a draft until B exits"
    );
    press(&mut session, Cancel);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Main);
    let settings = &session.events.world.party.as_ref().unwrap().settings;
    assert_eq!(settings.preferences, expected);
    assert_eq!(settings.battle_controls, [1, 2, 2, 2]);
    // Restore defaults in the draft, then cancel that reset before committing.
    for key in [Accept, Down, Down, Accept, Up, Right, Accept] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().customize.draft, original);
    for key in [Left, Accept, Cancel] {
        press(&mut session, key);
    }
    session.step(Cancel.input()).unwrap();
    settle_menu_motion(&mut session);
    let checkpoint = session.checkpoint().unwrap();
    let restored: FieldCheckpoint = roundtrip(&checkpoint);
    assert_eq!(restored.progress.party.settings.preferences, expected);
    restored.progress.party.validate(&data).unwrap();
    let mut invalid = restored.progress.party;
    invalid.settings.preferences.button_map[0] = invalid.settings.preferences.button_map[1];
    assert!(
        invalid.validate(&data).is_err(),
        "duplicate bindings must be rejected on load"
    );
}

#[test]
#[ignore = "requires locally cooked GQSEAF recipes; no devices"]
fn cooking_consumption_recovery_training_and_recipe_scripts() {
    use resonance_content::menu_data::{MealEffect, RECIPE_COUNT};
    use resonance_events::{EventRuntime, GameWorld, ResourceLibrary, party::CookingError};
    use symphonia_script::{NativeCall, Program, Width};
    let mut data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    menus.validate().unwrap();
    Arc::make_mut(&mut data).ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    assert_eq!(
        party
            .cook(&menus, || panic!("rejected meal drew RNG"))
            .unwrap_err(),
        CookingError::MissingIngredients
    );
    party.change_item(&data, 121, 3).unwrap();
    for member in &mut party.members {
        member.hp = 1;
        member.luck = 0;
    }
    let hungry = party.clone();
    let mut draws = 0;
    let meal = party
        .cook(&menus, || {
            draws += 1;
            0
        })
        .unwrap();
    assert_eq!(draws, 3);
    assert!(meal.success);
    assert_eq!(meal.ingredients, [121]);
    assert_eq!(meal.effects, [(MealEffect::HpRecovery, 7)].into());
    assert_eq!(party.items[&121], 2);
    assert_eq!(party.members[0].cooking[0], 1);
    for &id in &party.formation {
        let member = &party.members[usize::from(id - 1)];
        assert_eq!(member.hp, 1 + member.maximum_vitals()[0] * 7 / 100);
    }
    let saved = serde_json::to_value(&party).unwrap();
    assert_eq!(
        party
            .cook(&menus, || panic!("full party drew RNG"))
            .unwrap_err(),
        CookingError::Full
    );
    assert_eq!(serde_json::to_value(&party).unwrap(), saved);
    let loaded: Party = serde_json::from_value(saved).unwrap();
    loaded.validate(&data).unwrap();
    assert!(loaded.cooking.full);
    assert_eq!(loaded.members[0].cooking[0], 1);
    for (last_draw, expected) in [(0, None), (20, Some(1)), (99, Some(4))] {
        let mut failed = hungry.clone();
        let mut random = [99, 0, last_draw].into_iter();
        let meal = failed.cook(&menus, || random.next().unwrap()).unwrap();
        assert!(!meal.success);
        assert_eq!(meal.effects.get(&MealEffect::HpRecovery).copied(), expected);
        assert_eq!(failed.members[0].cooking[0], 2);
        assert_eq!(failed.items[&121], 2);
    }
    let mut bonus = hungry.clone();
    bonus.members[2].ex_gems[0] = 2;
    bonus.members[2].ex_skills[0] = menus.cooking.bonus_skill;
    assert_eq!(
        bonus.cook(&menus, || 0).unwrap().effects[&MealEffect::HpRecovery],
        12
    );
    bonus.cooking.full = false;
    bonus.formation = vec![1, 2];
    assert_eq!(
        bonus.cook(&menus, || 0).unwrap().effects[&MealEffect::HpRecovery],
        7
    );
    // Exercise every authored character/grade variant, including empty extras,
    // category alternatives, revival/cures and effects for the next battle.
    let mut stock = hungry.clone();
    stock.cooking.known = (1 << RECIPE_COUNT) - 1;
    for (id, item) in menus.items.iter().enumerate() {
        if (7..=12).contains(&item.category) {
            stock.change_item(&data, id as u16, 10).unwrap();
        }
    }
    for recipe in 0..RECIPE_COUNT {
        for chef in 0..9 {
            for grade in 0..3 {
                for roll in [0, 99] {
                    let mut attempt = stock.clone();
                    attempt.cooking.recipe = recipe as u8;
                    attempt.cooking.chef = chef as u8;
                    attempt.formation = vec![chef as u8 + 1];
                    attempt.field_leader = chef as u8 + 1;
                    attempt.members[chef].cooking[recipe] = grade * 3;
                    let meal = attempt.cook(&menus, || roll).unwrap();
                    assert!(!meal.ingredients.is_empty());
                    assert!(attempt.cooking.full);
                    attempt.validate(&data).unwrap();
                }
            }
        }
    }
    let mut code = Vec::new();
    for (call, recipe) in [
        (NativeCall::ForgetRecipe, 0),
        (NativeCall::LearnRecipe, 2),
        (NativeCall::LearnRecipe, 31),
        (NativeCall::HasRecipe, 31),
    ] {
        code.extend([
            0x0200,
            recipe,
            0,
            0x3000,
            0x4000,
            0x2000 | u16::from(call as u8),
        ]);
    }
    code.push(0x20ff);
    let mut words = vec![10, 0, 0, 1, 0, 2, 0, 42, 0, code.len() as u16];
    words.extend(code);
    words.push(0x20ff);
    let program = Arc::new(
        Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    let mut world = GameWorld::default();
    world.party = Some(hungry);
    let events = EventRuntime::with_state(
        program,
        Arc::new(ResourceLibrary {
            session_data: Some(data),
            ..Default::default()
        }),
        world,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        events.world.party.as_ref().unwrap().cooking.known,
        0x8000_0004
    );
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), i32::MIN);
}

#[test]
#[ignore = "requires locally cooked GQSEAF recipes/classroom; no devices"]
fn cooking_menu_commits_party_and_rng_and_preserves_them_in_saves() {
    use resonance_game::menu::{
        Page,
        cooking::{Content, Focus, Notice},
    };
    let data = Arc::new(cooked("game/session-data.json"));
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    party.change_item(&data, 121, 3).unwrap();
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, |s| s.checkpoint().is_ok(), |_, _| false);

    for key in [OpenMenu, Right, Right, Right, Down, Accept, Accept, Right] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().cooking_selection(), (1, 0));
    for key in [Up, Down] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().cooking_selection(), (1, 0));
    press(&mut session, Cancel);
    assert_eq!(session.menu.as_ref().unwrap().cooking_selection(), (0, 0));
    for key in [Accept, Right, Accept] {
        press(&mut session, key);
    }
    assert_eq!(session.events.world.party.as_ref().unwrap().cooking.chef, 1);
    for key in [Down, Accept, Right, Accept] {
        press(&mut session, key);
    }
    let popup = session
        .menu
        .as_ref()
        .unwrap()
        .cooking
        .popup
        .as_ref()
        .unwrap();
    assert!(matches!(
        popup.content,
        Content::Notice(Notice::UnknownRecipe)
    ));
    press(&mut session, Cancel);
    assert_eq!(session.menu.as_ref().unwrap().cooking.focus, Focus::Recipes);
    press(&mut session, Next);
    assert_eq!(session.menu.as_ref().unwrap().cooking.first, 14);
    press(&mut session, Previous);
    press(&mut session, Up);
    assert_eq!(session.menu.as_ref().unwrap().cooking.recipe, 1);
    press(&mut session, Cancel);
    let menu = session.menu.as_ref().unwrap();
    let mut expected = menu.checkpoint.as_ref().unwrap().progress.clone();
    let meal = expected
        .cook(&menu.resources.as_ref().unwrap().data)
        .unwrap();
    // Effects may advance while the menu is open. Committing a meal must not
    // restore the field generator captured when the menu opened.
    session.events.world.random();
    let field_random = session.events.world.random_state;
    press(&mut session, Alternate);
    assert_eq!(session.events.world.random_state, field_random);
    assert_eq!(
        session.events.world.gameplay_random,
        expected.gameplay_random
    );
    assert_eq!(
        serde_json::to_value(&session.events.world.party).unwrap(),
        serde_json::to_value(&expected.party).unwrap()
    );
    let menu = session.menu.as_ref().unwrap();
    let Content::Meal(displayed) = &menu.cooking.popup.as_ref().unwrap().content else {
        panic!("missing result")
    };
    assert_eq!(
        serde_json::to_value(displayed).unwrap(),
        serde_json::to_value(meal).unwrap()
    );
    press(&mut session, Accept);
    press(&mut session, Alternate);
    let popup = session
        .menu
        .as_ref()
        .unwrap()
        .cooking
        .popup
        .as_ref()
        .unwrap();
    assert!(matches!(popup.content, Content::Notice(Notice::Full)));
    assert_eq!(session.events.world.random_state, field_random);
    assert_eq!(
        session.events.world.gameplay_random,
        expected.gameplay_random
    );
    for key in [Accept, Cancel] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Main);
    press(&mut session, Accept);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Cooking);
    assert!(session.menu.as_ref().unwrap().cooking.choose_recipe);
    press(&mut session, Cancel);
    session.step(Cancel.input()).unwrap();
    settle_menu_motion(&mut session);
    let saved = session.checkpoint().unwrap();
    let loaded: FieldCheckpoint = roundtrip(&saved);
    assert_eq!(loaded.progress.random_state, field_random);
    assert_eq!(loaded.progress.gameplay_random, expected.gameplay_random);
    assert_eq!(
        serde_json::to_value(&loaded.progress.party).unwrap(),
        serde_json::to_value(expected.party).unwrap()
    );
    let (world, _) = loaded.progress.into_state(&data).unwrap().into_world();
    assert_eq!(world.gameplay_random, expected.gameplay_random);
}

#[test]
#[ignore = "requires locally cooked GQSEAF synopsis/classroom; no devices"]
fn synopsis_script_records_variants_paging_and_saved_metadata() {
    use resonance_events::{EventRuntime, GameWorld, ResourceLibrary};
    use resonance_game::menu::Page;
    use symphonia_script::{NativeCall, Program, Width};
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    let level = party.members[0].level;
    // Use the actual native ABI, including overwriting and hiding an entry.
    let mut code = Vec::new();
    for (id, value) in (0..26).map(|id| (id, 1)).chain([(1, 0), (4, 2), (25, 3)]) {
        for value in [id, value, 7] {
            code.extend([0x0200, value as u16, 0, 0x3000, 0x4000]);
        }
        code.push(0x2000 | u16::from(NativeCall::SetScenarioTimer as u8));
    }
    code.push(0x20ff);
    let mut words = vec![10, 0, 0, 1, 0, 2, 0, 42, 0, code.len() as u16];
    words.extend(code);
    words.push(0x20ff);
    let program = Arc::new(
        Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    let mut memory = symphonia_script_vm::Memory::default();
    memory.write(0x40, Width::S32, 2000).unwrap();
    let mut world = GameWorld::default();
    world.party = Some(party);
    world.tick = 101;
    world.calendar_time = Some(1_700_000_000);
    let mut events =
        EventRuntime::with_state(program, Arc::new(ResourceLibrary::default()), world, memory)
            .unwrap();
    let record = &events.world.event_records[&4];
    assert_eq!(
        (record.value, record.extra, record.level, record.recorded_at),
        (2, 7, Some(level), Some(1_700_000_000))
    );
    events
        .world
        .party
        .as_mut()
        .unwrap()
        .raise_level(&data, 0, level + 1, None, || 0)
        .unwrap();
    let progress = roundtrip::<resonance_events::SavedProgress>(&events.save_progress().unwrap());
    let mut session = classroom(FieldEntry {
        persistent: progress.into_state(&data).unwrap(),
        data: Some(data.clone()),
        position: [-52., -619., 0.],
        ..Default::default()
    });
    advance_to(&mut session, |s| s.checkpoint().is_ok(), |_, _| false);
    let tick = session.events.tick();
    let press = |session: &mut FieldSession, action: Action| {
        session.step(action.input()).unwrap();
        session.step(Default::default()).unwrap();
        settle_menu_motion(session);
        for _ in 0..13 {
            let Some(menu) = &session.menu else { break };
            let state = &menu.synopsis;
            if menu.page != Page::Synopsis
                || state.list_scroll == 0
                    && state.text_scroll == 0
                    && state.transition.page_fade == 0
                    && !state.transition.page_closing
                    && !state.text_closing
                    && (!state.reading || state.text_opacity == 255)
            {
                break;
            }
            session.step(Default::default()).unwrap();
        }
        settle_menu_motion(session);
    };
    for key in [OpenMenu, Right, Right, Right, Right, Accept] {
        press(&mut session, key);
    }
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(menu.page, Page::Synopsis);
    assert_eq!(menu.synopsis_records().len(), 25);
    assert!(!menu.synopsis_records().contains(&1));
    assert_eq!(
        menu.synopsis_entry().1.level,
        Some(level),
        "journal level followed the current party level"
    );
    session.step(Accept.input()).unwrap();
    session.step(Next.input()).unwrap();
    assert_eq!(session.menu.as_ref().unwrap().synopsis.line, 0);
    assert_eq!(session.menu.as_ref().unwrap().synopsis.text_opacity, 21);
    press(&mut session, Idle);
    press(&mut session, Next);
    assert_eq!(session.menu.as_ref().unwrap().synopsis.line, 13);
    press(&mut session, Next);
    assert_eq!(session.menu.as_ref().unwrap().synopsis.line, 13);
    for key in [Up, Previous] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().synopsis.line, 0);
    for key in [Cancel, Next] {
        press(&mut session, key);
    }
    assert_eq!(
        (
            session.menu.as_ref().unwrap().synopsis.row,
            session.menu.as_ref().unwrap().synopsis.first
        ),
        (12, 12)
    );
    for key in [Next, Next] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().synopsis.row, 24);
    for key in [Previous, Previous, Down, Down, Down, Accept] {
        press(&mut session, key);
    }
    let (entry, record) = session.menu.as_ref().unwrap().synopsis_entry();
    assert_eq!(entry.title, "Iselia Forest");
    assert_eq!(record.value, 2);
    assert_eq!(entry.lines(record.value).len(), 30);
    assert_eq!(entry.lines(1).len(), 14);
    assert_eq!(
        session.events.tick(),
        tick,
        "reading the journal advanced the field"
    );
    press(&mut session, Cancel);
    let state = &session.menu.as_ref().unwrap().synopsis;
    let selection = (state.row, state.first);
    for key in [Cancel, Accept] {
        press(&mut session, key);
    }
    let state = &session.menu.as_ref().unwrap().synopsis;
    assert_eq!((state.row, state.first), selection);
    for key in [Cancel, Cancel] {
        press(&mut session, key);
    }
    let saved = session.checkpoint().unwrap();
    let record = &saved.progress.event_records[&4];
    assert_eq!(
        (record.level, record.recorded_at, record.tick),
        (Some(level), Some(1_700_000_000), 101)
    );
    let assets: FieldAssets = cooked("fields/map-340.json");
    let restored = classroom(saved.entry(&assets, data, [340].into()).unwrap());
    assert_eq!(
        restored.events.world.event_records[&4].recorded_at,
        Some(1_700_000_000)
    );
    let legacy: resonance_events::EventRecord =
        serde_json::from_str(r#"{"value":1,"extra":0,"tick":0}"#).unwrap();
    assert!(legacy.level.is_none() && legacy.recorded_at.is_none());
}

#[test]
#[ignore = "requires locally cooked GQSEAF strategy/classroom; no devices"]
fn strategy_presets_rename_and_current_orders_survive_reload() {
    use resonance_game::menu::{Page, strategy::Focus};
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    menus.validate().unwrap();
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3, 4, 5, 7, 8, 9];
    let before = serde_json::to_value(&party).unwrap();
    assert!(
        party.set_strategy(&menus.strategy, 0, 1, 7, None).is_err(),
        "Lloyd could select healing-only AI"
    );
    assert!(
        party
            .set_strategy(&menus.strategy, usize::MAX, 0, 0, None)
            .is_err()
    );
    assert!(party.set_strategy(&menus.strategy, 0, 3, 0, None).is_err());
    assert!(
        party
            .set_strategy(&menus.strategy, 0, 0, 0, Some(3))
            .is_err()
    );
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
    assert!(party.set_strategy(&menus.strategy, 0, 0, 1, None).unwrap());
    assert!(!party.set_strategy(&menus.strategy, 0, 0, 1, None).unwrap());
    assert!(
        party
            .set_strategy(&menus.strategy, 1, 0, 2, Some(1))
            .unwrap()
    );
    assert_eq!(
        party.members[1].strategy[0], 0,
        "editing a command changed current AI"
    );
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
    let tick = session.events.world.tick;
    let press = |session: &mut FieldSession, action: Action| {
        session.step(action.input()).unwrap();
        session.step(Default::default()).unwrap();
        settle_menu_motion(session);
        for _ in 0..12 {
            if session.menu.as_ref().is_none_or(|menu| {
                menu.page != Page::Strategy
                    || menu.strategy.transition.page_fade == 0
                        && !menu.strategy.transition.page_closing
                        && !menu.strategy.preset_closing
                        && (!menu.strategy.focus.preset() || menu.strategy.preset_opacity == 255)
            }) {
                break;
            }
            session.step(Default::default()).unwrap();
        }
        settle_menu_motion(session);
    };
    for key in [OpenMenu, Right, Right, Next, Accept, Accept, Accept] {
        press(&mut session, key);
    }
    for _ in 0..15 {
        session.step(Default::default()).unwrap();
    }
    press(&mut session, Down);
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(menu.strategy.description_previous, Some([0, 1]));
    assert_eq!(menu.strategy_description(), Some([0, 2]));
    assert_eq!(menu.strategy.description_opacity, 31);
    press(&mut session, Down);
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(menu.strategy.description_previous, Some([0, 1]));
    assert_eq!(menu.strategy_description(), Some([0, 3]));
    assert_eq!(menu.strategy.description_opacity, 63);
    for key in [Up, Accept] {
        press(&mut session, key);
    }
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Strategy);
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[0].strategy[0],
        2
    );
    for key in [Cancel, Alternate, Accept, Accept, Accept, Up, Accept] {
        press(&mut session, key);
    }
    let party = session.events.world.party.as_ref().unwrap();
    assert_eq!(party.strategy_presets.as_ref().unwrap()[0].members[0][0], 0);
    assert_eq!(party.members[0].strategy[0], 2);
    for key in [Cancel, Cancel, Alternate, Accept, Cancel, Up, Accept] {
        press(&mut session, key);
    }
    assert_eq!(
        session.menu.as_ref().unwrap().strategy.focus,
        Focus::Presets
    );
    assert_eq!(
        session
            .events
            .world
            .party
            .as_ref()
            .unwrap()
            .strategy_presets
            .as_ref()
            .unwrap()[0]
            .name,
        "Aeserve"
    );
    // Reset only the selected preset; cancelling a name edit preserves its saved name.
    for key in [
        Right, OpenMenu, Left, Alternate, Right, Accept, Cancel, Accept,
    ] {
        press(&mut session, key);
    }
    let presets = session
        .events
        .world
        .party
        .as_ref()
        .unwrap()
        .strategy_presets
        .as_ref()
        .unwrap();
    assert_eq!(presets[0].name, "Aeserve");
    assert_eq!(presets[1], menus.strategy.presets[1]);
    // Delete the whole name, reject confirmation, then recover the saved name.
    for key in [Alternate, Cancel, Up, Up] {
        press(&mut session, key);
    }
    for _ in 0..7 {
        press(&mut session, Accept);
    }
    for key in [Down, Accept] {
        press(&mut session, key);
    }
    let state = &session.menu.as_ref().unwrap().strategy;
    assert_eq!(state.focus, Focus::Rename);
    assert!(state.rename.value.is_empty());
    for key in [Down, Down, Accept] {
        press(&mut session, key);
    }
    assert_eq!(
        session.menu.as_ref().unwrap().strategy.rename.value,
        "Aeserve"
    );
    for key in [Cancel, Accept] {
        press(&mut session, key);
    }
    // Reach the last reserve in both editors; paging cannot change the setting's member.
    for key in [
        Cancel, Next, Down, Down, Down, Next, Previous, Next, Accept, Previous,
    ] {
        press(&mut session, key);
    }
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(
        (
            menu.strategy.character,
            menu.strategy.first,
            menu.strategy.focus
        ),
        (7, 4, Focus::Setting)
    );
    for key in [Down, Accept] {
        press(&mut session, key);
    }
    for _ in 0..7 {
        press(&mut session, Down);
    }
    for key in [Accept, Cancel, Alternate, Accept, Previous] {
        press(&mut session, key);
    }
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(
        (
            menu.strategy.character,
            menu.strategy.first,
            menu.strategy.focus
        ),
        (3, 0, Focus::PresetCharacter)
    );
    for key in [Next, Accept, Down, Accept] {
        press(&mut session, key);
    }
    for _ in 0..8 {
        press(&mut session, Up);
    }
    for key in [Accept, Cancel, Cancel] {
        press(&mut session, key);
    }
    let party = session.events.world.party.as_ref().unwrap();
    assert_eq!(party.members[8].strategy[1], 7);
    assert_eq!(party.strategy_presets.as_ref().unwrap()[0].members[8][1], 0);
    assert_eq!(
        session.events.world.tick, tick,
        "Strategy advanced the field"
    );
    let menu = session.menu.as_ref().unwrap();
    assert_eq!((menu.character, menu.first_character), (4, 4));
    for _ in 0..3 {
        press(&mut session, Cancel);
    }
    settle_menu_motion(&mut session);
    assert!(session.player_has_control());
    let saved = session.checkpoint().unwrap();
    let saved = roundtrip::<FieldCheckpoint>(&saved);
    let assets: FieldAssets = cooked("fields/map-340.json");
    let loaded = classroom(saved.entry(&assets, data.clone(), [340].into()).unwrap());
    let party = loaded.events.world.party.as_ref().unwrap();
    party.validate(&data).unwrap();
    assert_eq!(party.members[0].strategy[0], 2);
    assert_eq!(party.members[8].strategy[1], 7);
    let presets = party.strategy_presets.as_ref().unwrap();
    assert_eq!(presets[0].name, "Aeserve");
    assert_eq!(presets[0].members[0][0], 0);
    assert_eq!(presets[0].members[8][1], 0);
    assert_eq!(presets[1], menus.strategy.presets[1]);
}

#[test]
#[ignore = "requires locally cooked GQSEAF techniques/classroom; no devices"]
fn technique_actions_shortcuts_and_ai_settings_survive_reload() {
    use resonance_events::party::TechniqueShortcut;
    use resonance_game::menu::{Menu, Page, Resources, techniques::Focus};
    let mut data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    menus.validate().unwrap();
    Arc::make_mut(&mut data).ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3, 4];
    party.members[0].techniques.insert(1);
    party.members[0].shortcuts[0] = 1;
    party.members[1].techniques.insert(35);
    party.members[2].techniques.insert(66);
    party.members[0].techniques.extend([2, 3]);
    party.members[0].technique_uses.insert(2, 500);
    party.members[0].disabled_techniques.insert(2);
    let shortcut = |character, technique| {
        Some(TechniqueShortcut {
            character,
            technique,
        })
    };
    assert!(party.assign_technique(0, 1, shortcut(0, 2)).unwrap());
    assert!(!party.assign_technique(0, 1, shortcut(0, 2)).unwrap());
    assert!(party.assign_technique(2, 4, shortcut(0, 3)).unwrap());
    let before = serde_json::to_value(&party).unwrap();
    for (slot, owner, tech) in [(1, 1, 35), (4, usize::MAX, 3), (4, 0, 20)] {
        assert!(
            party
                .assign_technique(0, slot, shortcut(owner, tech))
                .is_err()
        );
    }
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
    assert!(!party.forget_technique(&menus, 0, 1).unwrap());
    assert!(party.forget_technique(&menus, 0, 2).unwrap());
    assert!(!party.members[0].techniques.contains(&3));
    assert!(party.members[0].disabled_techniques.is_empty());
    assert_eq!(party.members[0].shortcuts[1], 0);
    assert_eq!(party.members[2].assist_shortcuts[0], None);
    assert_eq!(party.members[0].technique_uses[&2], 500);
    assert!(party.assign_technique(0, 4, shortcut(1, 35)).unwrap());
    party.members[3].techniques.extend([98, 101, 121]);
    party.members[3].base_stats[1] = 100;
    party.members[3].tp = 100;
    party.members[0].hp = 1;
    let heal = party.members[0].maximum_vitals()[0] * 30 / 100;
    assert_eq!(
        party.cast_technique(&menus, 3, 0, 98, false).unwrap(),
        Some(104)
    );
    assert_eq!(party.members[0].hp, 1 + heal);
    assert_eq!(party.members[3].tp, 92);
    for (hp, condition, tp) in [
        (party.members[0].maximum_vitals()[0], 0, 92),
        (1, 0x100, 92),
        (1, 0, 7),
    ] {
        party.members[0].hp = hp;
        party.members[3].conditions = condition;
        party.members[3].tp = tp;
        let before = serde_json::to_value(&party).unwrap();
        assert_eq!(party.cast_technique(&menus, 3, 0, 98, false).unwrap(), None);
        assert_eq!(
            serde_json::to_value(&party).unwrap(),
            before,
            "rejected spell consumed TP or changed a target"
        );
    }
    party.members[3].conditions = 0;
    party.members[3].tp = 100;
    party.members[0].conditions = 0x8000_0001;
    party.members[0].hp = 0;
    party.members[0].tp = 0;
    assert_eq!(
        party.cast_technique(&menus, 3, 0, 121, false).unwrap(),
        Some(132)
    );
    assert_eq!(
        (
            party.members[0].hp,
            party.members[0].tp,
            party.members[0].conditions
        ),
        (heal, 0, 0)
    );
    assert_eq!(party.members[3].tp, 52);
    party.members[0].conditions = 0x21;
    assert_eq!(
        party.cast_technique(&menus, 3, 0, 101, false).unwrap(),
        Some(132)
    );
    assert_eq!(party.members[0].conditions, 0);
    let mut group = party.clone();
    group.formation.push(5);
    group.members[3].techniques.extend([99, 102]);
    group.members[3].tp = 100;
    for (index, conditions) in [(0, 0x240), (2, 0xa0), (4, 0x3e0)] {
        group.members[index].hp = 1;
        group.members[index].conditions = conditions;
    }
    group.members[1].hp = 0;
    group.members[1].conditions = 0x8000_03e0;
    // A group cast includes reserves, skips knocked-out members and charges once.
    assert_eq!(
        group.cast_technique(&menus, 3, 1, 99, false).unwrap(),
        Some(104)
    );
    assert_eq!(group.members[3].tp, 72);
    for index in [0, 2, 4] {
        assert_eq!(
            group.members[index].hp,
            1 + group.members[index].maximum_vitals()[0] * 45 / 100
        );
    }
    assert_eq!(
        group.cast_technique(&menus, 3, 1, 102, false).unwrap(),
        Some(132)
    );
    assert_eq!(group.members[3].tp, 48);
    assert_eq!(
        (
            group.members[0].conditions,
            group.members[2].conditions,
            group.members[4].conditions
        ),
        (0, 0, 0)
    );
    assert_eq!(
        (group.members[1].hp, group.members[1].conditions),
        (0, 0x8000_03e0)
    );
    let unchanged = serde_json::to_value(&group).unwrap();
    assert_eq!(
        group.cast_technique(&menus, 3, 4, 102, false).unwrap(),
        None
    );
    assert_eq!(serde_json::to_value(&group).unwrap(), unchanged);
    party.members[3].equipment[3] = 406;
    assert_eq!(party.members[3].technique_cost(&menus, 98, false), 5);
    party.members[3].equipment[3] = 407;
    assert_eq!(party.members[3].technique_cost(&menus, 98, false), 4);
    let mut personal = party.clone();
    personal.items.insert(41, 1);
    assert!(personal.set_ex_gem(&data, 3, 0, 2).unwrap());
    assert!(personal.set_ex_skill(&data, 3, 0, 31).unwrap());
    personal.members[0].hp = 1;
    personal.members[3].tp = 1;
    assert_eq!(personal.members[3].technique_cost(&menus, 98, true), 1);
    assert_eq!(personal.members[3].technique_cost(&menus, 98, false), 4);
    let unchanged = serde_json::to_value(&personal).unwrap();
    assert_eq!(
        personal.cast_technique(&menus, 3, 0, 98, false).unwrap(),
        None
    );
    assert_eq!(serde_json::to_value(&personal).unwrap(), unchanged);
    assert_eq!(
        personal.cast_technique(&menus, 3, 0, 98, true).unwrap(),
        Some(104)
    );
    assert_eq!(
        (personal.members[0].hp, personal.members[3].tp),
        (1 + heal, 0)
    );
    assert_eq!(
        personal.cast_technique(&menus, 3, 0, 98, true).unwrap(),
        None
    );
    party.formation = vec![1, 2, 3];
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
    let tick = session.events.world.tick;
    let accept = Accept.input();
    let cancel = Cancel.input();
    let menu_key = OpenMenu.input();
    press(&mut session, menu_key);
    press(&mut session, accept);
    press(&mut session, accept);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Tech);
    press(&mut session, accept);
    press(&mut session, accept);
    assert_eq!(
        session.menu.as_ref().unwrap().tech.focus,
        Focus::Shortcuts,
        "unchanged valid assignment stayed in the list"
    );
    press(&mut session, Down.input());
    press(&mut session, accept);
    press(&mut session, accept);
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[0].shortcuts[1],
        1
    );
    press(&mut session, Next.input());
    assert_eq!(session.menu.as_ref().unwrap().tech.focus, Focus::List);
    press(&mut session, menu_key);
    assert!(
        session.events.world.party.as_ref().unwrap().members[1]
            .disabled_techniques
            .contains(&35)
    );
    press(&mut session, cancel);
    press(&mut session, Up.input());
    press(&mut session, Left.input());
    assert_eq!(
        session
            .events
            .world
            .party
            .as_ref()
            .unwrap()
            .settings
            .battle_controls[1],
        1
    );
    assert_eq!(
        session.events.world.tick, tick,
        "Tech menu advanced the field"
    );
    press(&mut session, cancel);
    press(&mut session, cancel);
    settle_menu_motion(&mut session);
    assert!(session.player_has_control());
    let saved = session.checkpoint().unwrap();
    let saved = roundtrip::<FieldCheckpoint>(&saved);
    let assets: FieldAssets = cooked("fields/map-340.json");
    let loaded = classroom(saved.entry(&assets, data.clone(), [340].into()).unwrap());
    let party = loaded.events.world.party.as_ref().unwrap();
    party.validate(&data).unwrap();
    assert_eq!(party.members[0].shortcuts[1], 1);
    assert!(party.members[1].disabled_techniques.contains(&35));
    assert_eq!(party.settings.battle_controls[1], 1);
    assert_eq!(party.members[0].technique_uses[&2], 500);
    assert_eq!(party.members[0].assist_shortcuts[0], shortcut(1, 35));

    let mut checkpoint = session.checkpoint().unwrap();
    let party = &mut checkpoint.progress.party;
    party.formation.push(4);
    party.members[0].hp = 1;
    party.members[1].hp = 0;
    party.members[1].conditions = 0x8000_0000;
    party.members[3].equipment[3] = 0;
    party.members[3].tp = 56;
    let mut menu = Menu::new(Page::Tech, Some(checkpoint), false);
    menu.resources = Some(Arc::new(Resources {
        session: data,
        data: Arc::new(menus),
    }));
    menu.character = 3;
    menu.tech.focus = Focus::List;
    let press_menu = |menu: &mut Menu, input| {
        let cue = menu.step(input);
        menu.step(Default::default());
        for _ in 0..40 {
            if !menu_in_motion(menu) {
                break;
            }
            menu.step(Default::default());
        }
        assert!(!menu_in_motion(menu), "menu transition did not finish");
        cue
    };
    let healing = menu.selected_technique().unwrap();
    assert_eq!(menu.step(accept), Some(2));
    assert_eq!(menu.tech.description_previous, Some(healing));
    assert_eq!(menu.tech.description_fade, 224);
    for _ in 0..15 {
        menu.step(Default::default());
    }
    assert_eq!(menu.tech.description_previous, None);
    assert_eq!(menu.tech_description(), None);
    assert_eq!(menu.selected_technique(), Some(healing));
    assert_eq!((menu.tech.focus, menu.tech.target), (Focus::Target, 0));
    assert_eq!(press_menu(&mut menu, Up.input()), None);
    assert_eq!(press_menu(&mut menu, accept), Some(104));
    press_menu(&mut menu, Down.input());
    assert_eq!(press_menu(&mut menu, accept), Some(4));
    press_menu(&mut menu, cancel);
    assert_eq!(menu.tech.description_previous, None);
    assert_eq!(menu.tech.description_fade, 224);
    assert_eq!(menu.tech_description(), Some(healing));
    for _ in 0..15 {
        menu.step(Default::default());
    }
    assert_eq!(menu.tech.description_previous, Some(healing));
    menu.tech.row = menu
        .technique_list()
        .iter()
        .position(|&id| id == 121)
        .unwrap();
    press_menu(&mut menu, accept);
    assert_eq!((menu.tech.focus, menu.tech.target), (Focus::Target, 1));
    assert_eq!(press_menu(&mut menu, accept), Some(132));
    assert_eq!(menu.tech.focus, Focus::List);
    let party = &menu.checkpoint.as_ref().unwrap().progress.party;
    assert!(party.members[1].hp > 0);
    assert_eq!(party.members[1].conditions, 0);
    assert_eq!(party.members[3].tp, 0);
    assert_eq!(press_menu(&mut menu, accept), Some(4));
    assert_eq!(menu.tech.focus, Focus::List);
    let party = &mut menu.checkpoint.as_mut().unwrap().progress.party;
    party.formation.push(5);
    party.members[1].hp = 0;
    party.members[1].conditions = 0x8000_0000;
    menu.page = Page::Main;
    menu.selected = 0;
    menu.character = 1;
    press_menu(&mut menu, accept);
    assert_eq!(press_menu(&mut menu, accept), Some(4));
    assert!(matches!(menu.page, Page::Character(_)));
    menu.character = 0;
    press_menu(&mut menu, accept);
    let next = Next.input();
    let previous = Previous.input();
    press_menu(&mut menu, next);
    assert_eq!(menu.character, 1);
    press_menu(&mut menu, next);
    assert_eq!(menu.character, 2);
    press_menu(&mut menu, previous);
    assert_eq!(menu.character, 1);
    press_menu(&mut menu, previous);
    assert_eq!(menu.character, 0);
    menu.character = 2;
    menu.tech.focus = Focus::Character;
    press_menu(&mut menu, previous);
    assert_eq!((menu.character, menu.tech.focus), (0, Focus::Character));
    press_menu(&mut menu, accept);
    menu.tech.slot = 3;
    press_menu(&mut menu, next);
    press_menu(&mut menu, previous);
    assert_eq!(menu.tech.slot, 0);
    menu.character = 4;
    menu.tech.focus = Focus::Character;
    assert!(menu.tech_auto());
    assert_eq!(menu.tech_columns(), 2);
    assert_eq!(press_menu(&mut menu, menu_key), None);
    assert_eq!(menu.tech.focus, Focus::Character);
    menu.character = 2;
    assert!(menu.tech_unison_available());
    press_menu(&mut menu, menu_key);
    assert!(menu.tech.unison);
    assert_eq!(menu.tech.focus, Focus::Shortcuts);
    press_menu(&mut menu, previous);
    assert_eq!(menu.character, 2, "shortcut editor changed owner");
    press_menu(&mut menu, Up.input());
    assert_eq!(menu.tech.slot, 3);
    press_menu(&mut menu, Down.input());
    assert_eq!(menu.tech.slot, 0);
    press_menu(&mut menu, accept);
    let choice = menu.selected_technique().unwrap().technique;
    let before = menu.checkpoint.as_ref().unwrap().progress.party.members[2].clone();
    press_menu(&mut menu, accept);
    let after = &menu.checkpoint.as_ref().unwrap().progress.party.members[2];
    assert_eq!(after.shortcuts[0], choice);
    assert_eq!((after.hp, after.tp), (before.hp, before.tp));
    assert_eq!(after.disabled_techniques, before.disabled_techniques);
    let remove = Alternate.input();
    assert_eq!(press_menu(&mut menu, remove), Some(1));
    assert_eq!(press_menu(&mut menu, remove), None);
    press_menu(&mut menu, cancel);
    assert_eq!(menu.tech.focus, Focus::Character);
    assert!(!menu.tech.unison);
    for control in [0, 1, 2] {
        press_menu(&mut menu, Start.input());
        assert_eq!(
            menu.checkpoint
                .as_ref()
                .unwrap()
                .progress
                .party
                .settings
                .battle_controls[2],
            control
        );
        assert_eq!(menu.tech.focus, Focus::Character);
    }
    menu.character = 0;
    menu.checkpoint.as_mut().unwrap().progress.party.members[0]
        .techniques
        .extend(1..=12);
    press_menu(&mut menu, accept);
    menu.tech.slot = 5;
    menu.tech.row = 9;
    menu.tech.first = 2;
    press_menu(&mut menu, cancel);
    press_menu(&mut menu, accept);
    assert_eq!(
        menu.tech.slot, 0,
        "header entry retained the old shortcut slot"
    );
    assert_eq!(
        (menu.tech.row, menu.tech.first),
        (9, 2),
        "manual header entry reset the list scroll"
    );
    for _ in 0..6 {
        press_menu(&mut menu, Down.input());
    }
    assert_eq!(menu.tech.focus, Focus::Character);
}

#[test]
#[ignore = "requires locally cooked GQSEAF equipment/classroom; no devices"]
fn equipment_preview_optimization_and_menu_transfers_survive_reload() {
    use resonance_game::menu::{Page, equipment::Focus};
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    for id in [155, 156, 275, 454] {
        party.change_item(&data, id, 1).unwrap();
    }
    let mut owners = party.clone();
    for id in [236, 284, 327, 363] {
        owners.change_item(&data, id, 1).unwrap();
        let kind = data.items[usize::from(id)].equipment_kind.unwrap();
        let slot = owners.members[8].preferred_equipment_slot(kind).unwrap();
        assert!(
            !owners.equip_slot(&data, 5, slot, id).unwrap(),
            "Zelos-only gear fitted Kratos"
        );
        assert!(owners.equip_slot(&data, 8, slot, id).unwrap());
    }
    let mut full = party.clone();
    full.change_item(&data, 274, data.items[274].stack_limit as i8)
        .unwrap();
    let full_before = serde_json::to_value(&full).unwrap();
    assert!(full.optimize_equipment(&data, &menus, 0, false).is_err());
    assert_eq!(
        serde_json::to_value(&full).unwrap(),
        full_before,
        "a failed armor swap partially optimized the weapon"
    );
    let before = serde_json::to_value(&party).unwrap();
    let preview = party.members[0].preview_equipment(&menus, 0, 155);
    assert_eq!(preview.slash, party.members[0].base_stats[2] / 10 + 930);
    assert_eq!(
        serde_json::to_value(&party).unwrap(),
        before,
        "preview modified the party"
    );
    assert!(party.optimize_equipment(&data, &menus, 0, false).unwrap());
    assert_eq!(party.members[0].equipment[0], 155);
    assert_eq!(party.members[0].equipment[1], 275);
    assert_eq!(
        party.items.get(&454),
        Some(&1),
        "optimization consumed an accessory"
    );
    assert!(party.optimize_equipment(&data, &menus, 0, true).unwrap());
    assert_eq!(party.members[0].equipment[0], 156);
    assert_eq!(party.items.get(&155), Some(&1));
    assert!(!party.optimize_equipment(&data, &menus, 0, true).unwrap());
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
    let tick = session.events.world.tick;
    let accept = Accept.input();
    let cancel = Cancel.input();
    press(&mut session, OpenMenu.input());
    press(&mut session, Down.input());
    for _ in 0..2 {
        press(&mut session, Right.input());
    }
    press(&mut session, accept);
    press(&mut session, accept);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Equip);
    press(&mut session, Alternate.input());
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[0].equipment[0],
        156,
        "weapon could be removed"
    );
    press(&mut session, Down.input());
    press(&mut session, Alternate.input());
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[0].equipment[1],
        0
    );
    press(&mut session, accept);
    let menu = session.menu.as_ref().unwrap();
    assert_eq!(menu.equipment.focus, Focus::List);
    assert_eq!(
        menu.equipment_items(),
        [275, 274],
        "alphabetical list order"
    );
    press(&mut session, Next.input());
    assert_eq!(
        session.menu.as_ref().unwrap().equipment.row,
        0,
        "shoulder buttons must not page the equipment list"
    );
    press(&mut session, Down.input());
    press(&mut session, accept);
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[0].equipment[1],
        274
    );
    assert_eq!(
        session.events.world.party.as_ref().unwrap().items.get(&275),
        Some(&1)
    );
    assert_eq!(
        session.events.world.tick, tick,
        "equipment menu advanced the field"
    );
    for _ in 0..3 {
        press(&mut session, cancel);
    }
    settle_menu_motion(&mut session);
    assert!(session.player_has_control());
    let saved = session.checkpoint().unwrap();
    let saved = roundtrip::<FieldCheckpoint>(&saved);
    let assets: FieldAssets = cooked("fields/map-340.json");
    let loaded = classroom(saved.entry(&assets, data.clone(), [340].into()).unwrap());
    let party = loaded.events.world.party.as_ref().unwrap();
    party.validate(&data).unwrap();
    assert_eq!(party.members[0].equipment[0..2], [156, 274]);
    assert_eq!(party.items.get(&275), Some(&1));
    assert_eq!(party.items.get(&155), Some(&1));
}

#[test]
#[ignore = "requires locally cooked GQSEAF items/classroom; no devices"]
fn inventory_actions_preserve_party_state_and_menu_healing_survives_reload() {
    use resonance_game::menu::{Page, items::Focus};
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    menus.validate().unwrap();
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3, 4, 5];
    for id in [1, 8, 10, 11, 22, 26, 27, 454] {
        party.change_item(&data, id, 3).unwrap();
    }
    let inventory = party.items.clone();
    assert_eq!(party.use_item(&data, &menus, 1, 0).unwrap(), None);
    assert_eq!(party.items, inventory, "a healthy target consumed a gel");
    let mut group = party.clone();
    assert!(!group.can_use_group_item(&menus, 8));
    assert_eq!(group.use_item(&data, &menus, 8, 0).unwrap(), None);
    group.members[1].hp = 0;
    group.members[1].conditions = 0x8000_0000;
    assert!(group.can_use_group_item(&menus, 8));
    assert_eq!(group.use_item(&data, &menus, 8, 1).unwrap(), Some(104));
    assert_eq!(group.items[&8], inventory[&8] - 1);
    assert_eq!(group.members[1].hp, 0);
    group.members[0].hp = 0;
    group.members[0].conditions = 0x8000_0000;
    assert!(!group.can_use_group_item(&menus, 8));
    assert_eq!(group.use_item(&data, &menus, 8, 1).unwrap(), None);
    assert_eq!(group.items[&8], inventory[&8] - 1);
    party.members[1].conditions = 0x8000_0000;
    party.members[1].hp = 0;
    party.members[1].tp = 0;
    assert_eq!(party.use_item(&data, &menus, 1, 1).unwrap(), None);
    assert!(party.use_item(&data, &menus, 11, 1).unwrap().is_some());
    assert_eq!(
        party.members[1].hp,
        (u32::from(party.members[1].base_stats[0]) * 30 / 100) as u16
    );
    assert_eq!(
        party.members[1].tp,
        (u32::from(party.members[1].base_stats[1]) * 15 / 100) as u16
    );
    party.members[1].conditions = 0xfe3 | 0x10000;
    party.use_item(&data, &menus, 10, 1).unwrap();
    assert_eq!(party.members[1].conditions, 0x10000);
    let base = party.members[0].base_stats;
    party.use_item(&data, &menus, 26, 0).unwrap();
    party.use_item(&data, &menus, 27, 0).unwrap();
    assert_eq!(party.members[0].base_stats[0], base[0] + base[0] / 20);
    assert_eq!(party.members[0].base_stats[2], base[2] + 10);
    party.members[0].hp = 1;
    party.members[2].hp = 1;
    let tablets = party.items[&8];
    party.use_item(&data, &menus, 8, 0).unwrap();
    assert!(party.members[0].hp > 1 && party.members[2].hp > 1);
    assert_eq!(
        party.items[&8],
        tablets - 1,
        "a party item was consumed once per target"
    );
    party
        .change_item(&data, 2, data.items[2].stack_limit as i8)
        .unwrap();
    let before = party.items.clone();
    assert!(!party.transform_item(&data, &menus, 22, 1).unwrap());
    assert_eq!(
        party.items, before,
        "a full transformation target consumed ingredients"
    );
    party.change_item(&data, 2, -1).unwrap();
    assert!(party.transform_item(&data, &menus, 22, 1).unwrap());
    assert_eq!(party.items[&1], before[&1] - 1);
    assert_eq!(party.items[&22], before[&22] - 1);
    assert_eq!(party.items[&2], before[&2]);
    party.equip_slot(&data, 0, 3, 454).unwrap();
    party.members[0].hp = party.members[0].maximum_vitals()[0];
    party.validate(&data).unwrap();
    party.unequip(&data, 0, 3).unwrap();
    assert_eq!(party.members[0].hp, party.members[0].base_stats[0]);
    party.members[0].hp = 1;
    party.members[4].hp = 1;
    party.items.retain(|id, _| *id == 1);
    party.items.insert(1, 2);
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
    let field_tick = session.events.world.tick;
    let press = |session: &mut FieldSession, input| {
        session.step(input).unwrap();
        session.step(FieldInput::default()).unwrap();
        while session.menu.as_ref().is_some_and(|menu| {
            menu.inventory.focus == Focus::Target
                && (menu.inventory.target_closing || menu.inventory.target_opacity < 255)
        }) {
            session.step(FieldInput::default()).unwrap();
        }
        settle_menu_motion(session);
        for _ in 0..24 {
            if session
                .menu
                .as_ref()
                .is_none_or(|menu| menu.page != Page::Items || menu.inventory.page_fade == 0)
            {
                break;
            }
            session.step(Default::default()).unwrap();
        }
        settle_menu_motion(session);
    };
    let confirm = Accept.input();
    let cancel = Cancel.input();
    press(&mut session, OpenMenu.input());
    press(&mut session, Down.input());
    press(&mut session, confirm);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Items);
    let before_discard = session.events.world.party.as_ref().unwrap().items.clone();
    press(&mut session, Alternate.input());
    assert_eq!(
        session.menu.as_ref().unwrap().inventory.focus,
        Focus::Discard(false)
    );
    press(&mut session, confirm);
    assert_eq!(session.menu.as_ref().unwrap().inventory.focus, Focus::List);
    assert_eq!(
        session.events.world.party.as_ref().unwrap().items,
        before_discard,
        "confirming the default No must preserve the item"
    );
    press(&mut session, confirm);
    assert_eq!(
        session.menu.as_ref().unwrap().inventory.focus,
        Focus::Target
    );
    press(&mut session, Right.input());
    assert_eq!(session.menu.as_ref().unwrap().inventory.target, 4);
    assert_eq!(session.menu.as_ref().unwrap().character, 0);
    press(&mut session, confirm);
    let reserve_hp = 1
        + (u32::from(session.events.world.party.as_ref().unwrap().members[4].base_stats[0]) * 30
            / 100) as u16;
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[4].hp,
        reserve_hp
    );
    for (direction, expected) in [
        ([0., -1.], 4),
        ([-1., 0.], 0),
        ([0., -1.], 1),
        ([1., 0.], 1),
        ([0., 1.], 0),
    ] {
        press(
            &mut session,
            FieldInput {
                direction,
                ..Default::default()
            },
        );
        assert_eq!(session.menu.as_ref().unwrap().inventory.target, expected);
    }
    press(&mut session, confirm);
    let party = session.events.world.party.as_ref().unwrap();
    let restored_hp = 1 + (u32::from(party.members[0].base_stats[0]) * 30 / 100) as u16;
    assert_eq!(party.members[0].hp, restored_hp);
    assert_eq!(party.members[4].hp, reserve_hp);
    assert_eq!(session.events.world.tick, field_tick);
    for _ in 0..4 {
        if session.menu.is_some() {
            press(&mut session, cancel);
        }
    }
    settle_menu_motion(&mut session);
    assert!(session.player_has_control());
    let checkpoint: FieldCheckpoint = roundtrip(&session.checkpoint().unwrap());
    let assets: FieldAssets = cooked("fields/map-340.json");
    let loaded = classroom(
        checkpoint
            .clone()
            .entry(&assets, data.clone(), [340].into())
            .unwrap(),
    );
    let party = loaded.events.world.party.as_ref().unwrap();
    party.validate(&data).unwrap();
    assert_eq!(party.members[0].hp, restored_hp);
    assert_eq!(party.members[4].hp, reserve_hp);
    assert_eq!(
        party.items.get(&1),
        None,
        "the last gel returned after loading"
    );
    let mut book = resonance_game::menu::Menu::new(Page::Items, Some(checkpoint), false);
    book.resources = Some(Arc::new(resonance_game::menu::Resources {
        session: data.clone(),
        data: Arc::new(menus.clone()),
    }));
    let press_book = |book: &mut resonance_game::menu::Menu, input| {
        let cue = book.step(input);
        book.step(Default::default());
        for _ in 0..24 {
            let moving = match book.page {
                Page::Items => {
                    book.inventory.page_closing
                        || book.inventory.page_fade != 0
                        || matches!(book.inventory.focus, Focus::Target | Focus::Transform(_))
                            && (book.inventory.target_closing
                                || book.inventory.target_opacity < 255)
                }
                Page::Collection => book.collection.page_closing || book.collection.page_fade != 0,
                _ => false,
            };
            if !moving {
                break;
            }
            book.step(Default::default());
        }
        cue
    };
    book.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .change_item(&data, 121, 1)
        .unwrap();
    book.inventory.category = 7;
    for input in [Alternate.input(), Up.input(), confirm] {
        press_book(&mut book, input);
    }
    assert!(book.inventory.notice.is_some());
    assert_eq!(
        book.checkpoint.as_ref().unwrap().progress.party.items[&121],
        1
    );
    assert_eq!(press_book(&mut book, cancel), Some(2));
    assert!(
        !book
            .checkpoint
            .as_ref()
            .unwrap()
            .progress
            .party
            .items
            .contains_key(&121)
    );
    book.inventory.category = 8;
    let party = &mut book.checkpoint.as_mut().unwrap().progress.party;
    party.change_item(&data, 70, 1).unwrap();
    party.found_items.extend(
        menus
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.inventory_category().is_some())
            .map(|(id, _)| id as u16),
    );
    book.collection.category = 7;
    let figurine_book = menus
        .items
        .iter()
        .position(|item| item.view == Some(resonance_content::menu_data::ItemView::FigurineBook))
        .unwrap() as u16;
    assert!(!book.collection_items().0[..6].contains(&figurine_book));
    book.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .figurines
        .insert(0);
    assert_eq!(book.collection_items().0[1], figurine_book);
    book.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .figurines
        .clear();
    book.collection.category = 0;
    let before = serde_json::to_value(&book.checkpoint).unwrap();
    assert_eq!(press_book(&mut book, confirm), Some(2));
    assert_eq!(book.page, Page::Collection);
    let (_, count) = book.collection_items();
    assert!(count > 24);
    assert_eq!(press_book(&mut book, PageDown.input()), Some(38));
    assert_eq!((book.collection.row, book.collection.first), (24, 24));
    assert_eq!(press_book(&mut book, PageUp.input()), Some(38));
    assert_eq!((book.collection.row, book.collection.first), (0, 0));
    for category in 0..8 {
        assert_eq!(book.collection.category, category);
        let (items, total) = book.collection_items();
        assert_eq!(
            items.len(),
            total,
            "category completion must include discovered items no longer held"
        );
        assert!(!items.is_empty());
        press_book(&mut book, Next.input());
    }
    press_book(
        &mut book,
        FieldInput {
            previous_page: true,
            direction: [1., 0.],
            ..Default::default()
        },
    );
    assert_eq!((book.collection.category, book.collection.row), (0, 1));
    press_book(
        &mut book,
        FieldInput {
            next_page: true,
            direction: [-1., 0.],
            ..Default::default()
        },
    );
    assert_eq!((book.collection.category, book.collection.row), (0, 0));
    press_book(&mut book, cancel);
    assert!(book.collection.categories);
    for input in [Previous.input(), Next.input()] {
        assert_eq!(press_book(&mut book, input), None);
        assert_eq!(book.collection.category, 0);
    }
    for (input, category) in [
        (
            FieldInput {
                next_page: true,
                direction: [-1., 0.],
                ..Default::default()
            },
            7,
        ),
        (
            FieldInput {
                previous_page: true,
                direction: [1., 0.],
                ..Default::default()
            },
            0,
        ),
    ] {
        assert_eq!(press_book(&mut book, input), Some(1));
        assert_eq!(book.collection.category, category);
    }
    press_book(&mut book, cancel);
    assert_eq!(book.page, Page::Items);
    assert_eq!(book.inventory.category, 8);
    assert_eq!(
        serde_json::to_value(&book.checkpoint).unwrap(),
        before,
        "reading the book changed the save"
    );
    book.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .found_items
        .clear();
    press_book(&mut book, confirm);
    for input in [confirm, Right.input(), PageDown.input()] {
        press_book(&mut book, input);
    }
    assert_eq!(book.collection_items().0.len(), 0);
    assert_eq!((book.collection.row, book.collection.first), (0, 0));

    let mut list = key_item_menu(70);
    for id in 1..=36 {
        list.checkpoint
            .as_mut()
            .unwrap()
            .progress
            .party
            .change_item(&data, id, 1)
            .unwrap();
    }
    list.inventory.category = 1;
    let down = Down.input();
    assert_eq!(press_book(&mut list, PageDown.input()), Some(38));
    assert_eq!((list.inventory.row, list.inventory.first), (18, 18));
    press_book(&mut list, cancel);
    press_book(&mut list, confirm);
    assert_eq!((list.inventory.row, list.inventory.first), (0, 0));
    for _ in 0..9 {
        press_book(&mut list, down);
    }
    assert_eq!((list.inventory.row, list.inventory.first), (18, 2));
    assert_eq!(list.step(Right.input()), None);
    assert_eq!(list.inventory.row, 18, "scrolling accepted navigation");
    list.step(Default::default());
    list.step(Default::default());
    assert_eq!(press_book(&mut list, PageUp.input()), Some(38));
    assert_eq!((list.inventory.row, list.inventory.first), (16, 0));
    press_book(&mut list, cancel);
    press_book(&mut list, down);
    assert_eq!((list.inventory.row, list.inventory.first), (0, 0));
    press_book(&mut list, Next.input());
    assert!(list.inventory_items().is_empty());
    press_book(&mut list, cancel);
    assert_eq!(press_book(&mut list, Previous.input()), None);
    assert_eq!(list.inventory.category, 2);
    assert_eq!(press_book(&mut list, confirm), Some(2));
    assert_eq!(list.inventory.focus, Focus::List);
    assert_eq!(press_book(&mut list, confirm), None);
    press_book(
        &mut list,
        FieldInput {
            previous_page: true,
            direction: [1., 0.],
            ..Default::default()
        },
    );
    assert_eq!(list.inventory.category, 2);
    press_book(&mut list, cancel);
    press_book(
        &mut list,
        FieldInput {
            previous_page: true,
            direction: [1., 0.],
            ..Default::default()
        },
    );
    assert_eq!(list.inventory.category, 3);
    press_book(
        &mut list,
        FieldInput {
            next_page: true,
            direction: [-1., 0.],
            ..Default::default()
        },
    );
    assert_eq!(list.inventory.category, 2);

    let mut rune = key_item_menu(70);
    rune.inventory.category = 1;
    let party = &mut rune.checkpoint.as_mut().unwrap().progress.party;
    party.items = [(1, 1), (2, data.items[2].stack_limit), (22, 2)].into();
    rune.inventory.row = 2;
    let before = rune
        .checkpoint
        .as_ref()
        .unwrap()
        .progress
        .party
        .items
        .clone();
    press_book(&mut rune, confirm);
    assert_eq!(rune.inventory_items(), [1]);
    press_book(&mut rune, cancel);
    assert_eq!(rune.inventory.focus, Focus::List);
    assert_eq!(rune.inventory.row, 2);
    assert_eq!(
        rune.checkpoint.as_ref().unwrap().progress.party.items,
        before
    );
    press_book(&mut rune, confirm);
    assert_eq!(press_book(&mut rune, confirm), Some(4));
    assert_eq!(
        rune.inventory.notice.as_ref().unwrap(),
        &menus.labels["transform_full"]
    );
    assert_eq!(rune.inventory.transform.result, None);
    press_book(&mut rune, cancel);
    assert_eq!(
        rune.checkpoint.as_ref().unwrap().progress.party.items,
        before
    );
    rune.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .change_item(&data, 2, -1)
        .unwrap();
    let before = rune
        .checkpoint
        .as_ref()
        .unwrap()
        .progress
        .party
        .items
        .clone();
    assert_eq!(press_book(&mut rune, confirm), Some(2));
    assert_eq!(rune.inventory.transform.result, Some(1));
    assert_eq!(
        rune.checkpoint.as_ref().unwrap().progress.party.items,
        before,
        "the transformation committed before its result was acknowledged"
    );
    press_book(&mut rune, cancel);
    assert_eq!(rune.inventory.focus, Focus::List);
    assert_eq!(rune.inventory.row, 1);
    assert_eq!(
        rune.checkpoint.as_ref().unwrap().progress.party.items,
        [(2, data.items[2].stack_limit), (22, 1)].into()
    );
    assert_eq!(press_book(&mut rune, confirm), Some(4));
    assert_eq!(
        rune.inventory.notice.as_ref().unwrap(),
        &menus.labels["transform_empty"]
    );
    press_book(&mut rune, confirm);
    rune.checkpoint.as_mut().unwrap().progress.party.items = [(22, 1), (398, 1)].into();
    rune.inventory.row = 0;
    press_book(&mut rune, confirm);
    assert_eq!(rune.inventory_items(), [398]);
    press_book(&mut rune, confirm);
    press_book(&mut rune, confirm);
    assert_eq!(rune.inventory.focus, Focus::List);
    assert!(rune.inventory_items().is_empty());
    assert_eq!(
        rune.checkpoint.as_ref().unwrap().progress.party.items,
        [(399, 1)].into()
    );
}

#[test]
#[ignore = "requires locally cooked menu/classroom data; no devices"]
fn rename_gem_preserves_names_through_cancel_save_and_dialogue() {
    use resonance_content::menu_data::RENAME_GEM;
    use resonance_events::{EventRuntime, GameWorld, ResourceLibrary, dialogue::TextToken};
    use resonance_game::menu::{Menu, Page, rename::Focus};
    use symphonia_script::{
        NativeCall, Program,
        message::{Message, Token},
    };
    let mut menu = key_item_menu(70);
    let data = menu.resources.as_ref().unwrap().session.clone();
    let press = |menu: &mut Menu, action: Action| {
        menu.step(FieldInput::default());
        menu.step(action.input())
    };
    let settle = |menu: &mut Menu| {
        for _ in 0..30 {
            menu.step(FieldInput::default());
        }
    };
    menu.page = Page::Status;
    assert!(!menu.can_rename());
    assert_eq!(press(&mut menu, Accept), None);
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .change_item(&data, RENAME_GEM, 1)
        .unwrap();
    assert_eq!(press(&mut menu, Accept), Some(2));
    settle(&mut menu);
    assert_eq!(menu.page, Page::Rename);
    for key in [Accept, Right, Accept] {
        press(&mut menu, key);
    }
    assert_eq!(menu.rename.value, "MIoyd");
    for key in [Cancel, Up, Accept] {
        press(&mut menu, key);
    }
    settle(&mut menu);
    assert_eq!(menu.page, Page::Status);
    assert_eq!(menu.character_name(0), "MIoyd");
    assert_eq!(menu.full_name(0), "MIoyd Irving");
    let saved = serde_json::to_value(menu.checkpoint.as_ref().unwrap()).unwrap();
    press(&mut menu, Accept);
    settle(&mut menu);
    for _ in 0..6 {
        press(&mut menu, Alternate);
    }
    assert_eq!(menu.rename.value, "");
    press(&mut menu, Up);
    assert_eq!(press(&mut menu, Accept), Some(4));
    assert_eq!(menu.rename.focus, Focus::Commands);
    for key in [Right, Accept, Right, Accept] {
        press(&mut menu, key);
    }
    settle(&mut menu);
    assert_eq!(
        serde_json::to_value(menu.checkpoint.as_ref().unwrap()).unwrap(),
        saved
    );
    menu.page = Page::Items;
    menu.inventory.category = 0;
    menu.inventory.row = menu
        .inventory_items()
        .iter()
        .position(|&id| id == RENAME_GEM)
        .unwrap();
    press(&mut menu, Accept);
    settle(&mut menu);
    press(&mut menu, Accept);
    settle(&mut menu);
    assert_eq!(menu.page, Page::Rename);
    for key in [OpenMenu, Accept] {
        press(&mut menu, key);
    }
    for _ in 0..12 {
        press(&mut menu, Accept);
    }
    assert_eq!(menu.rename.position, 5);
    for key in [Cancel, Up, Right, Right, Accept] {
        press(&mut menu, key);
    }
    settle(&mut menu);
    assert_eq!(menu.page, Page::Items);
    let restored: FieldCheckpoint = serde_json::from_value(saved).unwrap();
    restored.progress.party.validate(&data).unwrap();
    assert_eq!(restored.progress.party.items[&RENAME_GEM], 1);
    let mut code = Vec::new();
    for value in [0i32, 0, -2, 4, 0, 0, 0, 0] {
        code.extend([
            0x0200,
            value as u16,
            (value as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    code.extend([0x2000 | NativeCall::ConfigureDialogue as u16, 0x20ff]);
    let mut words = vec![10, 0, 0, 1, 0, 2, 0, 42, 0, code.len() as u16];
    words.extend(code);
    words.push(0x20ff);
    let program = Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let library = ResourceLibrary {
        actor_names: ResourceLibrary::character_names(),
        messages: vec![Message {
            tokens: vec![Token::Control {
                opcode: 1,
                expression: [0x0200u16, 1, 0, 0x3000, 0x20ff]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect(),
            }],
        }],
        ..Default::default()
    };
    let mut world = GameWorld::default();
    world.party = Some(restored.progress.party.clone());
    let events = EventRuntime::with_state(
        Arc::new(program),
        Arc::new(library),
        world,
        Default::default(),
    )
    .unwrap();
    let dialogue = &events.world.dialogue[&0];
    for message in [&dialogue.speaker, &dialogue.body] {
        assert!(matches!(&message.tokens[..], [TextToken::Text { text }] if text == "MIoyd"));
    }
    let mut invalid = restored.progress.party;
    invalid.members[0].name = Some("Bad\nName".into());
    assert!(invalid.validate(&data).is_err());
}

fn key_item_menu(item: u16) -> resonance_game::menu::Menu {
    use resonance_game::menu::{Menu, Page, Resources};
    let data = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    menus.validate().unwrap();
    let mut session = classroom(classroom_entry(
        &data,
        Party::new(&data, Default::default()).unwrap(),
        2000,
    ));
    advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
    let checkpoint = roundtrip(&session.checkpoint().unwrap());
    let mut menu = Menu::new(Page::Items, Some(checkpoint), false);
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .change_item(&data, item, 1)
        .unwrap();
    menu.resources = Some(Arc::new(Resources {
        session: data,
        data: Arc::new(menus),
    }));
    menu.inventory.category = 8;
    menu.inventory.row = menu
        .inventory_items()
        .iter()
        .position(|&id| id == item)
        .unwrap();
    menu
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu/classroom data; no devices"]
fn figurine_book_filters_saved_ownership_and_preserves_the_checkpoint() {
    use resonance_game::menu::{Menu, Page};
    let mut menu = key_item_menu(72);
    let press = |menu: &mut Menu, input| {
        let cue = menu.step(input);
        for _ in 0..32 {
            menu.step(Default::default());
        }
        cue
    };
    let confirm = Accept.input();
    let cancel = Cancel.input();
    let down = Down.input();
    let next = PageDown.input();
    let previous = PageUp.input();
    assert_eq!(press(&mut menu, confirm), Some(4));
    assert_eq!(menu.page, Page::Items);
    menu.checkpoint.as_mut().unwrap().progress.party.figurines =
        (0..13).chain([53, 108, 118, 125, 172, 287]).collect();
    let before = serde_json::to_value(&menu.checkpoint).unwrap();
    press(&mut menu, confirm);
    assert_eq!(menu.figurine().unwrap().name, "Lloyd Irving");
    assert_eq!(press(&mut menu, previous), None);
    assert_eq!(press(&mut menu, next), Some(38));
    assert_eq!((menu.figurines.row, menu.figurines.first), (12, 12));
    assert_eq!(press(&mut menu, next), None);
    for _ in 0..6 {
        press(&mut menu, down);
    }
    assert_eq!(menu.figurine().unwrap().id, 287);
    assert_eq!(press(&mut menu, down), None);
    assert_eq!(press(&mut menu, previous), Some(38));
    assert_eq!((menu.figurines.row, menu.figurines.first), (6, 0));
    assert_eq!(menu.figurines.view.model_row, menu.figurines.row);
    assert_eq!(press(&mut menu, confirm), None);
    press(&mut menu, cancel);
    assert_eq!(menu.page, Page::Items);
    assert_eq!(serde_json::to_value(&menu.checkpoint).unwrap(), before);
    let restored: Option<FieldCheckpoint> = serde_json::from_value(before.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), before);
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu/classroom data; no devices"]
fn training_manual_filters_learned_topics_and_bounds_each_reading_page() {
    use resonance_game::menu::{Menu, Page};
    let mut menu = key_item_menu(73);
    let press = |menu: &mut Menu, input| {
        let cue = menu.step(input);
        menu.step(Default::default());
        for _ in 0..24 {
            let moving = match menu.page {
                Page::Items => menu.inventory.page_closing || menu.inventory.page_fade != 0,
                Page::Manual => menu.manual.page_closing || menu.manual.page_fade != 0,
                _ => false,
            };
            if !moving {
                break;
            }
            menu.step(Default::default());
        }
        cue
    };
    let confirm = Accept.input();
    let cancel = Cancel.input();
    let down = Down.input();
    let next = PageDown.input();
    assert_eq!(menu.step(confirm), Some(2));
    assert_eq!(menu.page, Page::Items);
    assert!(menu.inventory.page_closing);
    assert_eq!(menu.step(down), None);
    for _ in 0..24 {
        menu.step(Default::default());
    }
    assert_eq!(menu.page, Page::Manual);
    assert!(menu.manual_chapters().is_empty());
    assert_eq!(press(&mut menu, confirm), None);
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .event_flags
        .extend([104, 108, 114]);
    assert_eq!(
        menu.manual_chapters()
            .iter()
            .map(|(c, _)| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Battle Basics", "Status Effects"]
    );
    assert_eq!(
        menu.manual_chapters()[0]
            .1
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["Movement", "Targeting"]
    );
    let before = serde_json::to_value(&menu.checkpoint).unwrap();
    assert_eq!(press(&mut menu, confirm), Some(2));
    assert_eq!(press(&mut menu, PageUp.input()), None);
    assert_eq!(press(&mut menu, next), Some(38));
    assert_eq!(menu.manual.paragraph, 1);
    assert_eq!(press(&mut menu, next), None);
    press(&mut menu, down);
    assert_eq!((menu.manual.topic, menu.manual.paragraph), (1, 0));
    press(&mut menu, down);
    assert_eq!(menu.manual.topic, 0);
    press(&mut menu, cancel);
    press(&mut menu, down);
    assert_eq!(menu.manual.chapter, 1);
    press(&mut menu, confirm);
    assert_eq!(press(&mut menu, down), None);
    for _ in 0..6 {
        assert_eq!(press(&mut menu, next), Some(38));
    }
    assert_eq!(menu.manual.paragraph, 6);
    assert_eq!(press(&mut menu, next), None);
    press(&mut menu, cancel);
    press(&mut menu, down);
    assert_eq!(menu.manual.chapter, 0);
    press(&mut menu, cancel);
    assert_eq!(menu.page, Page::Items);
    assert_eq!(serde_json::to_value(&menu.checkpoint).unwrap(), before);
    press(&mut menu, confirm);
    assert_eq!(menu.page, Page::Manual);
    assert_eq!(
        (
            menu.manual.chapter,
            menu.manual.topic,
            menu.manual.paragraph
        ),
        (0, 0, 0)
    );
    assert!(!menu.manual.reading);
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu/classroom data; no devices"]
fn world_map_directory_uses_saved_visits_and_preserves_inventory() {
    use resonance_game::menu::{Menu, Page, world_map::Focus};
    let mut menu = key_item_menu(69);
    let menus = menu.resources.as_ref().unwrap().data.clone();
    let party = &menu.checkpoint.as_ref().unwrap().progress.party;
    assert_eq!(party.travel.current_location, Some(2));
    assert_eq!(party.travel.visited_locations, [2].into());
    let press = |menu: &mut Menu, input| {
        let cue = menu.step(input);
        menu.step(Default::default());
        for _ in 0..24 {
            let map = &menu.world_map;
            let moving = match menu.page {
                Page::Items => menu.inventory.page_closing || menu.inventory.page_fade != 0,
                Page::WorldMap => {
                    map.page_closing
                        || map.page_fade != 0
                        || map.location_scroll != 0
                        || map.item_scroll != 0
                        || map.shops_opacity
                            != if map.focus == Focus::Locations {
                                0
                            } else {
                                255
                            }
                        || map.items_opacity != if map.focus == Focus::Items { 255 } else { 0 }
                }
                _ => false,
            };
            if !moving {
                break;
            }
            menu.step(Default::default());
        }
        cue
    };
    let confirm = Accept.input();
    assert_eq!(press(&mut menu, confirm), Some(2));
    assert_eq!(menu.page, Page::WorldMap);
    assert_eq!(
        menu.map_locations().iter().map(|v| v.0).collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(menu.step(confirm), Some(2));
    assert_eq!(menu.world_map.focus, Focus::Shops);
    assert_eq!(menu.world_map.shops_opacity, 0);
    assert_eq!(menu.step(confirm), None);
    press(&mut menu, Default::default());
    assert_eq!(press(&mut menu, confirm), Some(4));
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .travel
        .visited_shops
        .insert(1);
    let before = serde_json::to_value(&menu.checkpoint).unwrap();
    assert_eq!(press(&mut menu, confirm), Some(2));
    assert_eq!(menu.world_map.focus, Focus::Items);
    assert_eq!(press(&mut menu, PageDown.input()), Some(38));
    assert_eq!((menu.world_map.item, menu.world_map.first_item), (8, 8));
    menu.step(Up.input());
    assert_eq!(
        (
            menu.world_map.item,
            menu.world_map.first_item,
            menu.world_map.item_scroll
        ),
        (7, 7, -1)
    );
    assert_eq!(menu.step(Down.input()), None);
    assert_eq!(menu.world_map.item, 7, "scrolling accepted navigation");
    press(&mut menu, Default::default());
    let cancel = Cancel.input();
    for _ in 0..3 {
        press(&mut menu, cancel);
    }
    assert_eq!(menu.page, Page::Items);
    assert_eq!(serde_json::to_value(&menu.checkpoint).unwrap(), before);
    for location in menus.world_map.locations.values() {
        for variant in &location.shop_variants {
            let mut globals = vec![0; 256];
            globals[variant.global] = variant.at_least - 1;
            assert_eq!(location.shops(&globals), location.shops);
            globals[variant.global] += 1;
            assert_eq!(location.shops(&globals), variant.shops);
        }
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu/classroom data; no devices"]
fn monster_list_browsing_preserves_discoveries_and_pages_like_the_oracle() {
    use resonance_events::party::MonsterKnowledge;
    use resonance_game::menu::{Menu, Page};
    let mut menu = key_item_menu(71);
    let party = &mut menu.checkpoint.as_mut().unwrap().progress.party;
    party.monsters = (0..16)
        .map(|id| (id, MonsterKnowledge::default()))
        .collect();
    party.monsters.get_mut(&3).unwrap().variant = 1;
    let press = |menu: &mut Menu, input| {
        let cue = menu.step(input);
        for _ in 0..32 {
            menu.step(Default::default());
        }
        cue
    };
    let confirm = Accept.input();
    let cancel = Cancel.input();
    let alternate = Alternate.input();
    let right = Right.input();
    let down = Down.input();
    assert_eq!(press(&mut menu, confirm), Some(2));
    assert_eq!(menu.page, Page::Monsters);
    assert_eq!(menu.step(right), Some(1));
    assert_eq!(menu.monsters.row, 1);
    assert_eq!(menu.monsters.view.model_row, 0);
    menu.step(Default::default());
    assert_eq!(menu.monsters.view.model_opacity, 223);
    assert_eq!(menu.monsters.view.model_row, 0);
    assert_eq!(
        menu.step(right),
        None,
        "input waits for the old preview to fade"
    );
    press(&mut menu, Default::default());
    assert_eq!(menu.monsters.view.model_row, 1);
    for _ in 0..2 {
        press(&mut menu, right);
    }
    assert_eq!(menu.monster().unwrap().0.id, 3);
    assert_eq!(
        press(&mut menu, down),
        None,
        "unscanned repeat statistics stay hidden"
    );
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .monsters
        .get_mut(&3)
        .unwrap()
        .scanned = true;
    let before = serde_json::to_value(&menu.checkpoint).unwrap();
    assert_eq!(press(&mut menu, down), Some(1));
    assert_eq!(menu.monsters.variant, 1);
    assert_eq!(press(&mut menu, down), None);
    assert_eq!(press(&mut menu, alternate), Some(1));
    assert_eq!(press(&mut menu, PageDown.input()), Some(38));
    assert_eq!((menu.monsters.list_row, menu.monsters.first), (15, 12));
    assert_eq!(press(&mut menu, PageUp.input()), Some(38));
    assert_eq!((menu.monsters.list_row, menu.monsters.first), (3, 0));
    press(&mut menu, down);
    press(&mut menu, cancel);
    assert_eq!((menu.monsters.row, menu.monsters.variant), (3, 1));
    press(&mut menu, alternate);
    press(&mut menu, down);
    press(&mut menu, confirm);
    assert_eq!((menu.monsters.row, menu.monsters.variant), (4, 0));
    press(&mut menu, Next.input());
    assert_eq!(menu.monsters.row, 14);
    press(&mut menu, right);
    press(&mut menu, right);
    assert_eq!(menu.monsters.row, 0);
    for _ in 0..100 {
        menu.step(FieldInput {
            preview_direction: [1., 1.],
            ..Default::default()
        });
    }
    assert_eq!(menu.monsters.distance, 1400.);
    press(&mut menu, Start.input());
    assert_eq!((menu.monsters.yaw, menu.monsters.distance), (330., 960.));
    let sample = menu.monsters.view.animation_tick;
    menu.busy = true;
    press(&mut menu, right);
    assert_eq!(menu.monsters.view.animation_tick, sample);
    assert_eq!(menu.monsters.row, 0);
    menu.busy = false;
    press(&mut menu, cancel);
    assert_eq!(menu.page, Page::Items);
    assert_eq!(serde_json::to_value(&menu.checkpoint).unwrap(), before);
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .monsters
        .clear();
    press(&mut menu, confirm);
    assert_eq!(menu.page, Page::Monsters);
    assert!(menu.monster().is_none());
    assert_eq!(press(&mut menu, alternate), None);
    assert_eq!(press(&mut menu, cancel), Some(3));
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu/classroom data; no devices"]
fn status_title_changes_survive_menu_close_and_save_reload_and_change_growth() {
    use resonance_content::menu_data::Element;
    use resonance_game::menu::Page;
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let menus: MenuData = cooked("game/menu-data.json");
    menus.validate().unwrap();
    let mut party = Party::new(&data, Default::default()).unwrap();
    let item = |name| menus.items.iter().position(|i| i.name == name).unwrap() as u16;
    let mut equipped = party.members[0].clone();
    equipped.equipment = [
        item("Flamberge"),
        0,
        0,
        item("Aquamarine"),
        item("Garnet"),
        0,
    ];
    let traits = equipped.equipment_traits(&menus);
    assert_eq!(traits.attack_element, Some(Element::Water));
    assert_eq!(traits.resistance[Element::Fire as usize], 2);
    assert_eq!(traits.resistance[Element::Water as usize], 2);
    equipped.equipment[4] = item("Aquamarine");
    assert_eq!(
        equipped.equipment_traits(&menus).resistance[Element::Water as usize],
        4
    );
    for (first, second, effect) in [
        (
            "Poison Charm",
            "Krona Symbol",
            "Nullify all physical ailments",
        ),
        ("Emerald Ring", "Faerie Ring", "Consume 1/2 less TP"),
    ] {
        for (a, b) in [(first, second), (second, first)] {
            equipped.equipment = [0, 0, 0, item(a), item(b), 0];
            let effects = equipped.equipment_traits(&menus).effects;
            assert_eq!(effects.len(), 1);
            assert_eq!(
                menus.status.equipment_effects[&effects[0]].description,
                effect
            );
        }
    }
    party.formation = vec![1, 2, 3];
    party.members[0].titles.insert(2);
    let mut session = classroom(classroom_entry(&data, party, 2000));
    advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
    let field_tick = session.events.world.tick;
    let effect_tick = session.effect_clock.tick();
    let played = session.play_time.total();
    let press = |session: &mut FieldSession, input| {
        session.step(input).unwrap();
        session.step(FieldInput::default()).unwrap();
        for _ in 0..32 {
            settle_menu_motion(session);
            if session.menu.as_ref().is_none_or(|m| {
                !matches!(m.page, Page::Status | Page::Titles) || !m.status.animating()
            }) {
                return;
            }
            session.step(FieldInput::default()).unwrap();
        }
        panic!("Status transition did not finish");
    };
    let confirm = Accept.input();
    let down = Down.input();
    let cancel = Cancel.input();
    press(&mut session, OpenMenu.input());
    for _ in 0..3 {
        press(&mut session, Right.input());
    }
    let start = Start.input();
    press(&mut session, start);
    assert!(session.menu.as_ref().unwrap().party_statistics);
    press(&mut session, confirm);
    assert_eq!(
        session.menu.as_ref().unwrap().page,
        Page::Character(resonance_game::menu::CharacterMenu::Status)
    );
    press(&mut session, start);
    assert!(!session.menu.as_ref().unwrap().party_statistics);
    press(&mut session, confirm);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Status);
    press(&mut session, Next.input());
    assert!(session.menu.as_ref().unwrap().status.details);
    press(&mut session, confirm);
    assert_eq!(
        session.menu.as_ref().unwrap().page,
        Page::Status,
        "renaming is locked"
    );
    press(&mut session, Previous.input());
    assert!(!session.menu.as_ref().unwrap().status.details);
    press(&mut session, down);
    press(&mut session, confirm);
    assert_eq!(session.menu.as_ref().unwrap().page, Page::Titles);
    press(&mut session, down);
    press(&mut session, confirm);
    assert_eq!(
        session.events.world.party.as_ref().unwrap().members[0].title,
        2
    );
    assert_eq!(
        session.events.world.tick, field_tick,
        "field scripts advanced under the menu"
    );
    assert_eq!(
        u64::from(session.effect_clock.tick() - effect_tick),
        session.play_time.total() - played,
        "effect phase must advance while field scripts are paused"
    );
    session.step(cancel).unwrap();
    for _ in 0..11 {
        session.step(FieldInput::default()).unwrap();
    }
    let menu = session.menu.as_ref().unwrap();
    assert_eq!((menu.page, menu.status.page_fade), (Page::Status, 255));
    assert!(!menu.status.closing);
    assert!(session.checkpoint().is_err());
    session.step(FieldInput::default()).unwrap();
    let menu = session.menu.as_ref().unwrap();
    assert_eq!((menu.page, menu.main_fade), (Page::Main, 206));
    assert_eq!(session.events.world.tick, field_tick);
    settle_menu_motion(&mut session);
    session.step(cancel).unwrap();
    settle_menu_motion(&mut session);
    assert!(session.player_has_control());
    let saved: FieldCheckpoint = roundtrip(&session.checkpoint().unwrap());
    let assets: FieldAssets = cooked("fields/map-340.json");
    let loaded = classroom(saved.entry(&assets, data.clone(), [340].into()).unwrap());
    let mut party = loaded.events.world.party.clone().unwrap();
    assert_eq!(party.members[0].title, 2);
    let before = party.members[0].base_stats;
    let growth = menus.titles[0][1].growth;
    party
        .raise_level(&data, 0, party.members[0].level + 1, Some(growth), || 0)
        .unwrap();
    for i in 0..7 {
        assert_eq!(
            party.members[0].base_stats[i],
            before[i] + u16::from(data.characters[0].growth[i].base) + u16::from(growth[i])
        );
    }
    party.validate(&data).unwrap();
}
fn ready(session: &FieldSession) -> bool {
    session.dialogue.values().any(readable)
}
fn skip_movie(session: &mut FieldSession) {
    if let Some(movie) = &session.events.world.movie
        && movie.operation.is_pending()
    {
        movie.operation.complete(None).unwrap();
    }
}
fn advance_to(
    session: &mut FieldSession,
    reached: impl Fn(&FieldSession) -> bool,
    mut accept: impl FnMut(&FieldSession, u32) -> bool,
) {
    for update in 0..20_000 {
        if reached(session) {
            return;
        }
        skip_movie(session);
        let interact = accept(session, update);
        session
            .step(FieldInput {
                interact,
                ..Default::default()
            })
            .unwrap();
        session.events.world.audio_commands.clear();
    }
    assert!(
        reached(session),
        "field {} checkpoint not reached: {:?}",
        session.map_id,
        session.checkpoint().err()
    );
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn skit_playback_suspends_field_and_persists_viewed_state() {
    let skits: Arc<resonance_content::skit::SkitCatalog> = Arc::new(cooked("game/skits.json"));
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let mut party = Party::new(&data, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    let mut session = classroom(FieldEntry {
        skits: Some(skits.clone()),
        ..classroom_entry(&data, party, 2000)
    });
    advance_to(&mut session, |s| s.checkpoint().is_ok(), |_, _| false);
    assert!(session.skit_prompt().is_none());
    advance_to(&mut session, |s| s.skit_prompt().is_some(), |_, _| false);
    let prompt = session.skit_prompt().expect("timed notification missing");
    assert_eq!(
        (prompt.id, prompt.title, prompt.opacity),
        (600, "It'll Be Fine", 8)
    );
    session
        .events
        .world
        .party
        .as_mut()
        .unwrap()
        .settings
        .preferences
        .skit_notifications = false;
    let prompt = session.skit_prompt().expect("skit button stays visible");
    assert!(
        !prompt.title_visible,
        "disabled setting hides only the title"
    );
    session
        .events
        .world
        .party
        .as_mut()
        .unwrap()
        .settings
        .preferences
        .skit_notifications = true;
    assert!(session.skit_prompt().unwrap().title_visible);
    for _ in 0..31 {
        session.step(Default::default()).unwrap();
    }
    assert_eq!(session.skit_prompt().unwrap().opacity, 255);

    let checkpoint = session.checkpoint().unwrap();
    session.step(OpenMenu.input()).unwrap();
    assert!(session.skit_prompt().is_none());
    assert!(session.checkpoint().is_err());
    settle_menu_motion(&mut session);
    session.step(Cancel.input()).unwrap();
    settle_menu_motion(&mut session);
    assert_eq!(session.skit_prompt().unwrap().opacity, 255);
    session.events.world.party.as_mut().unwrap().formation.pop();
    session.step(Default::default()).unwrap();
    assert!(
        session.skit_prompt().is_none(),
        "required party member is absent"
    );

    let assets: FieldAssets = cooked("fields/map-340.json");
    let mut entry = checkpoint
        .entry(&assets, data.clone(), [340].into())
        .unwrap();
    entry.skits = Some(skits.clone());
    let mut restored = classroom(entry);
    advance_to(&mut restored, |s| s.checkpoint().is_ok(), |_, _| false);
    assert_eq!(restored.story_progress().unwrap(), 2000);
    assert!(
        restored.skit_prompt().is_none(),
        "quickload should restart the notification timer"
    );
    for _ in 0..1200 {
        restored.step(Default::default()).unwrap();
    }
    let open = Skit.input();
    assert!(
        restored
            .step(open)
            .unwrap_err()
            .to_string()
            .contains("not prepared")
    );
    assert!(
        restored.skit_prompt().is_some(),
        "a failed start must retain the notification"
    );
    let files = resonance_content::prepared::Files::load(
        &asset_root(),
        &["fields/map-340.preload.json"],
        &mut Default::default(),
        || false,
    )
    .unwrap();
    restored.prepare_skits(&files).unwrap();
    let before_playback = restored.checkpoint().unwrap();
    restored
        .events
        .world
        .party
        .as_mut()
        .unwrap()
        .settings
        .preferences
        .skit_notifications = false;
    restored.step(open).unwrap();
    assert!(!restored.player_has_control());
    assert!(
        restored
            .checkpoint()
            .unwrap_err()
            .to_string()
            .contains("skit")
    );
    let field_tick = restored.events.tick();
    let mut subtitles = Vec::new();
    for _ in 0..2400 {
        if restored.active_skit.is_none() {
            break;
        }
        restored.step(Default::default()).unwrap();
        assert_eq!(
            restored.events.tick(),
            field_tick,
            "field advanced during skit"
        );
        if let Some(skit) = &restored.active_skit {
            let scene = skit.events.world.skit.as_ref().unwrap();
            if !scene.subtitle.is_empty() && subtitles.last() != Some(&scene.subtitle) {
                subtitles.push(scene.subtitle.clone());
            }
            assert_eq!(scene.portraits.len(), 3);
        }
    }
    assert!(restored.active_skit.is_none(), "skit never completed");
    assert_eq!(
        subtitles,
        [
            "I wonder if Raine is going to be mad at us.",
            "Don't worry. All we have to do is\nget back to class before she does.",
            "But wasn't Professor Raine\ngoing to the temple, too?",
            "What if we run into her?",
            "Ah, haha...we'll be fine...probably.",
        ]
    );
    let saved = restored.checkpoint().unwrap();
    assert!(saved.progress.party.viewed_skits.contains(&600));
    for _ in 0..1200 {
        restored.step(Default::default()).unwrap();
    }
    assert!(
        restored.skit_prompt().is_none(),
        "viewed skit was announced again"
    );
    let mut entry = before_playback.entry(&assets, data, [340].into()).unwrap();
    entry.skits = Some(skits);
    let mut skipped = classroom(entry);
    skipped.prepare_skits(&files).unwrap();
    advance_to(&mut skipped, |s| s.skit_prompt().is_some(), |_, _| false);
    skipped.step(open).unwrap();
    let skip = OpenMenu.input();
    skipped.step(skip).unwrap();
    assert!(
        skipped.active_skit.is_some(),
        "skip was accepted before the opening guard"
    );
    for _ in 0..119 {
        skipped.step(Default::default()).unwrap();
    }
    skipped.step(skip).unwrap();
    assert!(skipped.active_skit.is_none());
    assert!(
        skipped
            .checkpoint()
            .unwrap()
            .progress
            .party
            .viewed_skits
            .contains(&600)
    );
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn classroom_examination_and_rewards_survive_repeated_interaction_and_reload() {
    use std::collections::BTreeMap;
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let interact = |session: &mut FieldSession, id| {
        let position = session.events.world.actors[&id].position;
        let approach = [position[0], position[1] - 80., position[2]];
        assert!(
            session.ground_surface(approach).is_some(),
            "target has no reachable approach"
        );
        let player = session
            .events
            .world
            .actors
            .get_mut(&session.events.world.controlled_actor)
            .unwrap();
        player.position = approach;
        player.face(180.);
        assert_eq!(session.interaction_target(), Some(id));
        // Teleporting to a probe can queue an aisle touch handler. Let it
        // retire before sending the interaction that this fixture measures.
        for _ in 0..4 {
            session.step(Idle.input()).unwrap();
            if session.player_has_control() {
                break;
            }
        }
        assert!(
            session.player_has_control(),
            "probe {id}, story {} did not settle: {:?}",
            session.story_progress().unwrap(),
            session.events.pending_operations()
        );
        assert_eq!(session.interaction_target(), Some(id), "settled probe {id}");
        if id == 202 {
            assert_eq!(
                session.action_prompt().map(|p| p.action),
                Some(resonance_game::field::FieldAction::Examine),
                "the hole must advertise its interaction before confirmation"
            );
        }
        session.step(Accept.input()).unwrap();
        assert!(
            !session.player_has_control(),
            "probe {id}, story {} did not start at {:?}",
            session.story_progress().unwrap(),
            session.events.world.actors[&session.events.world.controlled_actor].position
        );
        assert!(session.action_prompt().is_none());
        let mut pages = BTreeMap::new();
        for tick in 0..2000 {
            for p in session.dialogue.values().filter(|p| !p.closed) {
                pages.insert((p.operation.id(), p.page), p.current().text());
            }
            session
                .step(FieldInput {
                    interact: tick % 30 == 10,
                    ..Default::default()
                })
                .unwrap();
            session.events.world.audio_commands.clear();
            if session.player_has_control() {
                return pages.into_values().collect::<Vec<_>>();
            }
        }
        panic!("interaction {id} did not return control");
    };
    for story in [1000, 2000] {
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.formation = if story == 1000 {
            vec![1]
        } else {
            vec![1, 2, 3]
        };
        let mut session = classroom(classroom_entry(&data, party, story));
        advance_to(&mut session, FieldSession::player_has_control, |_, _| false);
        assert!(!session.events.world.actors[&202].visible);
        // Consecutive source positions across the hole's interaction boundary.
        for (y, target) in [(492., None), (496., Some(202))] {
            let player = session.events.world.actors.get_mut(&1).unwrap();
            player.position = [-441., y, 0.];
            player.face(180.);
            session.step(Idle.input()).unwrap();
            assert_eq!(session.interaction_target(), target);
            assert_eq!(
                session.action_prompt().map(|p| p.action),
                target.map(|_| resonance_game::field::FieldAction::Examine)
            );
        }
        let hole = interact(&mut session, 202);
        assert_eq!(hole[0], "When did this hole get here?");
        if story == 1000 {
            assert_eq!(hole.len(), 1);
            assert!(
                !session.events.world.party.as_ref().unwrap().members[1]
                    .titles
                    .contains(&3)
            );
            continue;
        }
        assert!(hole.iter().any(|p| p.contains("Klutz")));
        assert!(
            session.events.world.party.as_ref().unwrap().members[1]
                .titles
                .contains(&3)
        );
        assert!(
            interact(&mut session, 303)
                .iter()
                .any(|p| p == "Acquired Magic Lens.")
        );
        assert_eq!(session.events.world.party.as_ref().unwrap().items[&37], 1);
        let checkpoint = session.checkpoint().unwrap();
        let assets: FieldAssets = cooked("fields/map-340.json");
        let mut restored = classroom(
            checkpoint
                .entry(&assets, data.clone(), [340].into())
                .unwrap(),
        );
        advance_to(&mut restored, FieldSession::player_has_control, |_, _| {
            false
        });
        assert!(
            !interact(&mut restored, 202)
                .iter()
                .any(|p| p.contains("obtained the title"))
        );
        assert!(
            !interact(&mut restored, 303)
                .iter()
                .any(|p| p.starts_with("Acquired"))
        );
        assert_eq!(restored.events.world.party.as_ref().unwrap().items[&37], 1);
        assert!(
            restored.events.world.party.as_ref().unwrap().members[1]
                .titles
                .contains(&3)
        );
    }
}

#[test]
#[ignore = "requires all locally cooked skits; scenario/resource coverage, no devices"]
fn cooked_skit_scenarios_have_complete_native_and_portrait_resources() {
    use resonance_events::{EventRuntime, GameWorld, ResourceLibrary};
    let root = asset_root();
    let catalog: Arc<resonance_content::skit::SkitCatalog> = Arc::new(cooked("game/skits.json"));
    let text: Arc<resonance_content::session::GameText> = Arc::new(cooked("game/text.json"));
    let mut failures = Vec::new();
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    for (&id, paths) in &catalog.resources {
        let result = (|| -> anyhow::Result<()> {
            let resources = Arc::new(ResourceLibrary {
                skits: Some(catalog.clone()),
                text: text.clone(),
                session_data: Some(data.clone()),
                messages: serde_json::from_slice(&fs::read(root.join(&paths.messages))?)?,
                actor_names: ResourceLibrary::character_names(),
                ..Default::default()
            });
            let program = Arc::new(symphonia_script::Program::decode(&fs::read(
                root.join(&paths.script),
            )?)?);
            let mut world = GameWorld::default();
            world.skit = Some(Default::default());
            world.party = Some(resonance_events::party::Party::new(
                &data,
                Default::default(),
            )?);
            let mut events =
                EventRuntime::with_state(program, resources, world, Default::default())?;
            let mut dialogue = Default::default();
            for tick in 0..36_000 {
                if events.main_finished() {
                    return Ok(());
                }
                events.step()?;
                resonance_game::dialogue::step_requests(
                    &mut events.world,
                    &mut dialogue,
                    tick % 30 == 10,
                    false,
                )?;
                events.world.audio_commands.clear();
                for portrait in events.world.skit.as_ref().unwrap().portraits.values() {
                    let asset = &catalog.portraits[&portrait.resource];
                    let tile_size = resonance_content::skit::TILE_SIZE;
                    anyhow::ensure!(
                        portrait.tiles.len()
                            == (asset.size[0].div_ceil(tile_size)
                                * asset.size[1].div_ceil(tile_size))
                                as usize
                            && portrait.tiles.iter().all(|tile| {
                                asset
                                    .images
                                    .get(usize::from(tile.image))
                                    .is_some_and(|image| {
                                        tile.block
                                            < image.size[0].div_ceil(tile_size)
                                                * image.size[1].div_ceil(tile_size)
                                    })
                            }),
                        "portrait references an uncooked image block"
                    );
                }
            }
            anyhow::bail!("skit never completed: {:?}", events.pending_operations())
        })();
        if let Err(error) = result {
            failures.push(format!("skit {id}: {error:#}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn steady_keyboard_walking_keeps_the_walk_clip_across_loops() {
    let mut session = classroom(Default::default());
    advance_to(
        &mut session,
        |s| s.events.world.input_enabled,
        |_, tick| tick % 120 == 0,
    );
    assert!(session.events.world.input_enabled);
    let player = session.events.world.controlled_actor;
    let actor = session.events.world.actors.get_mut(&player).unwrap();
    actor.position = [-52., -619., 0.];
    actor.face(180.);
    for _ in 0..240 {
        session.step(FieldInput::default()).unwrap();
    }
    let mut binding = None;
    let mut previous_sample = 0.;
    let mut loops = 0;
    for update in 0..535 {
        // Recorded near-camera Dolphin route: down to the front wall, then
        // repeated left/right passes across the classroom's guarded triggers.
        let direction = match update {
            10..40 => [0., -1.],
            90..150 | 310..430 => [1., 0.],
            170..290 | 450..535 => [-1., 0.],
            _ => [0.; 2],
        };
        session
            .step(FieldInput {
                direction,
                ..Default::default()
            })
            .unwrap();
        session.events.world.audio_commands.clear();
        if direction[0] == 0. {
            binding = None;
            continue;
        }
        assert!(
            session.events.world.input_enabled,
            "spurious input lock at update {update}"
        );
        let animation = session.events.world.actors[&player]
            .animation
            .as_ref()
            .unwrap();
        assert_eq!(animation.slot, 36, "walk interrupted at update {update}");
        if let Some(start) = binding {
            assert_eq!(
                animation.start_tick, start,
                "walk restarted at update {update}"
            );
        }
        binding = Some(animation.start_tick);
        let sample = animation.sample(session.events.tick(), 0, animation.duration_ticks as f32);
        loops += u32::from(sample < previous_sample);
        previous_sample = sample;
    }
    assert!(loops >= 6);
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn eraser_impact_keeps_sound_motion_and_seeded_dust_together() {
    use resonance_events::AudioCommand;
    use resonance_game::field::replay::InputReplay;
    let mut session = classroom(Default::default());
    let anchor = InputReplay {
        actor: 100,
        position: [0., -10., 0.],
        animation_slot: 80,
        animation_sample: 19.5,
        duration_updates: 21,
        accept_updates: Vec::new(),
    };
    advance_to(&mut session, |s| anchor.matches(s), |s, _| ready(s));
    assert!(anchor.matches(&session));
    assert!(session.events.world.billboards.is_empty());
    // Isolate the impact's random draws from unrelated idle decisions.
    for actor in session.events.world.actors.values_mut() {
        actor.autonomy = None;
    }
    // Recovered from the independent impact checkpoint's RNG state. The
    // next sixteen original draws produce these eight growth/spin pairs.
    session.events.world.random_state = 0xdf7fa20d;
    session.step(FieldInput::default()).unwrap();
    let world = &session.events.world;
    assert_eq!(world.billboards.len(), 8);
    assert_eq!(
        world
            .audio_commands
            .iter()
            .filter(|c| matches!(c, AudioCommand::Sound { id: 236, .. }))
            .count(),
        1
    );
    let animation = world.actors[&100].animation.as_ref().unwrap();
    assert_eq!(world.tick - animation.phase_tick, 40);
    // The binding callback holds sample zero; the script waits 40 updates at
    // half speed before issuing the impact sound and all eight dust sprites.
    assert_eq!(animation.sample(world.tick, 0, 70.), 20.);
    let observed = [
        ([90., -635., 140.], 1.3, 1.),
        ([70., -635., 140.], 2.48, 1.),
        ([80., -635., 150.], 2.4, -1.),
        ([80., -635., 130.], 2.82, -1.),
        ([85., -635., 145.], 2.98, -1.),
        ([85., -635., 135.], 1.49, -1.),
        ([75., -635., 145.], 1.84, -1.),
        ([75., -635., 135.], 2.29, -1.),
    ];
    for (p, &(position, growth, spin)) in world.billboards.values().zip(&observed) {
        assert_eq!(p.born, world.tick);
        assert_eq!(p.position, position);
        assert_eq!(p.size, [10.; 2]);
        assert!((p.size_delta - growth).abs() < 0.00001);
        assert_eq!(p.angular_velocity, [0., 0., spin]);
        assert_eq!(p.alpha(world.tick), 75.);
    }
    for age in 1..=20 {
        session.step(FieldInput::default()).unwrap();
        if age == 5 {
            // Independent Dolphin impact checkpoint: authored time 11.25
            // (cooked sample 22.5), dust timer 175, first size 16.5 and alpha 70.
            let world = &session.events.world;
            assert_eq!(
                world.actors[&100]
                    .animation
                    .as_ref()
                    .unwrap()
                    .sample(world.tick, 0, 70.),
                22.5
            );
            let first = world.billboards.values().next().unwrap();
            assert!((first.size[0] - 16.5).abs() < 0.0001);
            assert_eq!(first.alpha(world.tick), 70.);
        }
    }
    for (p, &(_, growth, spin)) in session.events.world.billboards.values().zip(&observed) {
        assert!((p.size[0] - (10. + growth * 20.)).abs() < 0.0001);
        assert_eq!(p.rotation, [0., 0., spin * 20.]);
        assert_eq!(p.alpha(session.events.world.tick), 55.);
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn raine_walk_matches_observed_service_boundaries_and_ramp_motion() {
    use resonance_game::field::replay::InputReplay;
    use std::collections::BTreeMap;
    let mut session = classroom(Default::default());
    let replay: InputReplay = serde_json::from_str(include_str!(
        "../../../tools/oracle/cases/raine-mithos-input.json"
    ))
    .unwrap();
    replay.validate().unwrap();
    let mut pages = BTreeMap::new();
    advance_to(
        &mut session,
        |s| replay.matches(s),
        |s, _| {
            let tick = s.events.tick();
            s.dialogue.values().any(|p| {
                readable(p)
                    && tick - *pages.entry((p.operation.id(), p.page)).or_insert(tick) >= 180
            })
        },
    );
    assert!(replay.matches(&session));
    // Independent MemoryWatcher samples from raine-wait-stages-silent and
    // raine-walk-position-silent. The first two confirmations were consumed at
    // updates 21/171; DTM polling happened one VI before the window consumed A.
    let observed = BTreeMap::from([
        (189, ([238., 445., 0.], 181.)),
        (201, ([218.146_06, 434.411_25, 0.], 298.)),
        (233, ([168.762_68, 436.302_86, 0.7296725], 267.)),
        (250, ([147.415_25, 437.295_53, 16.153194], 267.)),
        (387, ([-60., 445., 27.141], 267.)),
        (400, ([-60., 445., 27.141], 267.)),
    ]);
    for update in 1..=400 {
        session
            .step(FieldInput {
                interact: replay.accept_at(update).unwrap(),
                ..Default::default()
            })
            .unwrap();
        session.events.world.audio_commands.clear();
        let actor = &session.events.world.actors[&4];
        if let Some(&(position, heading)) = observed.get(&update) {
            for (actual, expected) in actor.position.into_iter().zip(position) {
                assert!(
                    (actual - expected).abs() < 0.0001,
                    "update {update}: {:?} != {position:?}",
                    actor.position
                );
            }
            assert_eq!(actor.heading, heading, "update {update}");
        }
        if update == 189 {
            assert_eq!(
                actor.motion.as_ref().unwrap().target,
                [208., 429., 0.],
                "repositioning must preserve the active walk"
            );
        }
        if update == 201 {
            assert_eq!(actor.motion.as_ref().unwrap().target, [-60., 445., 27.]);
        }
        if update == 387 {
            assert!(actor.motion.is_none());
        }
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn conversations_wait_for_facing_then_return_smoothly_for_colette_and_a_classmate() {
    for (id, position) in [(2, [-88., -229., 0.]), (305, [-60., -619., 0.])] {
        let mut session = classroom(Default::default());
        advance_to(
            &mut session,
            |s| s.events.world.input_enabled,
            |s, _| ready(s),
        );
        assert!(session.events.world.input_enabled);
        let previous = session.events.world.actors[&id].target_heading;
        // Register the same observer position as the oracle; earlier tests
        // cover walking/collision. This test isolates conversation sequencing.
        let player = session.events.world.actors.get_mut(&1).unwrap();
        player.position = position;
        player.face(180.);
        assert_eq!(session.interaction_target(), Some(id));
        let initial_heading = session.events.world.actors[&id].heading;
        session.step(Accept.input()).unwrap();
        assert!(!session.events.world.input_enabled);
        let request = session.events.world.dialogue[&0].clone();
        assert_eq!(request.opening_actor, Some(id));
        let facing = session.events.world.actors[&id].target_heading;
        if id == 2 {
            assert_eq!(facing, 351.);
        }
        // Interaction itself advances the first turn update.
        let mut outbound_updates =
            usize::from(session.events.world.actors[&id].heading != initial_heading);
        let mut previous_heading = session.events.world.actors[&id].heading;
        for _ in 0..120 {
            if session
                .dialogue
                .get(&0)
                .is_some_and(|p| p.operation.id() == request.operation.id())
            {
                break;
            }
            assert!(
                session.talking.is_empty(),
                "mouth moved before the window opened"
            );
            session.step(FieldInput::default()).unwrap();
            let heading = session.events.world.actors[&id].heading;
            if heading != previous_heading {
                outbound_updates += 1;
            }
            previous_heading = heading;
        }
        assert_eq!(session.events.world.actors[&id].heading, facing);
        // The paired held-conversation state uses Lloyd's event idle +0x74
        // (30 authored frames), not his ordinary +0x0c idle (29 frames).
        assert_eq!(
            session.events.world.actors[&1]
                .animation
                .as_ref()
                .unwrap()
                .slot,
            116
        );
        if id == 2 {
            // Independent Dolphin VI observations 126..159: 180 -> 351.
            // Counting only motion also excludes the window's opening stages.
            assert_eq!(outbound_updates, 34, "Colette's approach turn timing");
        }
        assert!(
            session
                .dialogue
                .get(&0)
                .is_some_and(|p| p.operation.id() == request.operation.id())
        );
        // Let the normal page player reveal, hold, and dismiss the line.
        for _ in 0..1000 {
            if session.events.world.input_enabled {
                break;
            }
            let ready = session
                .dialogue
                .values()
                .any(|p| !p.closed && p.fully_revealed());
            session
                .step(FieldInput {
                    interact: ready,
                    ..Default::default()
                })
                .unwrap();
            if request.operation.is_pending() {
                assert_eq!(session.events.world.actors[&id].target_heading, facing);
            }
        }
        assert!(session.events.world.input_enabled);
        assert_eq!(session.events.world.actors[&id].target_heading, previous);
        assert_ne!(
            session.events.world.actors[&id].heading, previous,
            "return turn snapped"
        );
        let mut return_updates = 0;
        for _ in 0..120 {
            let before = session.events.world.actors[&id].heading;
            session.step(FieldInput::default()).unwrap();
            let after = session.events.world.actors[&id].heading;
            let distance = (after - before + 180.).rem_euclid(360.) - 180.;
            if distance != 0. {
                return_updates += 1;
            }
            assert!(
                distance.abs() <= 6.,
                "actor {id} jumped from {before} to {after}"
            );
        }
        assert_eq!(session.events.world.actors[&id].heading, previous);
        assert_eq!(
            session.events.world.actors[&1]
                .animation
                .as_ref()
                .unwrap()
                .slot,
            12
        );
        if id == 2 {
            // Independent Dolphin VI observations 332..365: 351 -> 180.
            assert_eq!(return_updates, 34, "Colette's return turn timing");
        }
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn walking_to_the_door_runs_both_choices_and_joins_the_party_once() {
    let assets: FieldAssets = cooked("fields/map-340.json");
    let data: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let effects: resonance_content::effect::FieldEffects = cooked(&assets.effects);
    for stay in [false, true] {
        let mut session = classroom(FieldEntry {
            persistent: PersistentState {
                party: Some(Party::new(&data, Default::default()).unwrap()),
                ..Default::default()
            },
            data: Some(data.clone()),
            ..Default::default()
        });
        advance_to(
            &mut session,
            |s| s.events.world.input_enabled,
            |s, _| ready(s),
        );
        assert_eq!(session.story_progress().unwrap(), 1000);
        // Ordinary camera-relative input across the aisle and up to the door.
        // This must detect the registered line; no direct trigger invocation.
        for (direction, updates) in [([-1., 0.], 138), ([0., 1.], 90), ([-1., 0.], 60)] {
            for _ in 0..updates {
                session
                    .step(FieldInput {
                        direction,
                        ..Default::default()
                    })
                    .unwrap();
                session.events.world.audio_commands.clear();
            }
        }
        assert!(
            !session.events.world.input_enabled,
            "door never acquired control"
        );
        let mut saw_question = false;
        let mut saw_choice = false;
        let mut joins = 0;
        let mut pastor_yaw = Vec::new();
        let mut checked_clips = std::collections::BTreeSet::new();
        for _ in 0..20000 {
            let choosing = session
                .events
                .world
                .choices
                .values()
                .any(|c| c.operation.is_pending());
            saw_choice |= choosing;
            let move_choice = stay
                && session
                    .events
                    .world
                    .choices
                    .values()
                    .any(|c| c.operation.is_pending() && c.selected_line < c.last_line);
            for p in session.dialogue.values() {
                let text: String = p.current().text();
                saw_question |= text.contains("Where are you going?");
            }
            session
                .step(FieldInput {
                    interact: !move_choice && ready(&session),
                    direction: if move_choice { [0., -1.] } else { [0., 0.] },
                    ..Default::default()
                })
                .unwrap();
            if stay
                && let Some(camera) = session.events.world.field_camera.as_ref()
                && let Some(motion) = &camera.motion
            {
                let [x, y, z] = motion.position.value;
                let [pitch, _, yaw] = motion.angles.value;
                if (-439.001..=-264.999).contains(&x)
                    && (-931.001..=-910.999).contains(&y)
                    && (282.999..=313.001).contains(&z)
                    && (71.999..=76.001).contains(&pitch)
                    && pastor_yaw.last().is_none_or(|last| *last > -14.999)
                {
                    // The sixty-update pastor pan crosses zero, not a full revolution.
                    assert!((-15.001..=15.001).contains(&yaw), "pastor yaw {yaw}");
                    assert!((yaw - (15. - (pitch - 72.) * 7.5)).abs() < 0.001);
                    pastor_yaw.push(yaw);
                }
            }
            for emote in session.events.world.emotes.values() {
                assert!(
                    effects.emotes.contains_key(&emote.kind),
                    "uncooked emote {}",
                    emote.kind
                );
            }
            for (&id, actor) in &session.events.world.actors {
                if let Some(animation) = &actor.animation
                    && checked_clips.insert((
                        actor.resource,
                        animation.source,
                        animation.resource,
                        animation.slot,
                    ))
                    && let Some(model) = assets.actors.iter().find(|m| m.resource == actor.resource)
                {
                    for (part, spec) in model.parts.iter().enumerate() {
                        assert!(
                            spec.clips
                                .iter()
                                .any(|c| animation.matches(c, actor.resource)),
                            "actor {id}, part {part}: uncooked binding {:#x}/{}",
                            animation.resource,
                            animation.slot
                        );
                    }
                }
            }
            for command in session.events.world.audio_commands.drain(..) {
                if matches!(
                    command,
                    resonance_events::AudioCommand::Sound { id: 80, .. }
                ) {
                    joins += 1;
                }
            }
            if session.story_progress().unwrap() == 2000 && session.events.world.input_enabled {
                break;
            }
        }
        assert!(saw_question && saw_choice);
        if stay {
            assert!(pastor_yaw.len() >= 59, "pastor pan was not exercised");
            assert!(pastor_yaw.iter().any(|yaw| yaw.abs() < 0.251));
            assert!(
                pastor_yaw
                    .last()
                    .is_some_and(|yaw| (*yaw + 15.).abs() < 0.001)
            );
        }
        assert!(
            session.events.world.input_enabled,
            "doorway stalled (stay={stay}): waits {:?}; pages {:?}",
            session.events.pending_operations(),
            session
                .dialogue
                .values()
                .map(|p| (
                    p.closed,
                    p.visible,
                    p.current().glyphs.len(),
                    p.current().text()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(session.story_progress().unwrap(), 2000);
        assert_eq!(
            session.events.world.party.as_ref().unwrap().formation,
            [1, 2, 3]
        );
        assert_eq!(joins, 1);
        assert!(!session.events.world.actors.contains_key(&2));
        assert!(!session.events.world.actors.contains_key(&3));
        for _ in 0..120 {
            session.step(FieldInput::default()).unwrap();
        }
        assert!(
            session.events.world.input_enabled,
            "completed doorway scene retriggered"
        );
        assert_eq!(
            session.events.world.party.as_ref().unwrap().formation,
            [1, 2, 3]
        );
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets"]
fn original_chosen_answer_waits_for_its_complete_spoken_audio() {
    let audio: resonance_content::field_audio::FieldAudio = cooked("fields/map-340-audio.json");
    let mut session = classroom(Default::default());
    session.voice_durations = Arc::new(
        audio
            .voices
            .iter()
            .map(|(&id, v)| {
                (
                    id,
                    (f64::from(v.frames) / f64::from(v.sample_rate)
                        * resonance_game::clock::UPDATE_HZ)
                        .ceil() as u32,
                )
            })
            .collect(),
    );
    let required = session.voice_durations[&655379];
    let mut started = None;
    let mut mouth_moved = false;
    for _ in 0..20000 {
        if let Some(movie) = &session.events.world.movie
            && movie.operation.is_pending()
        {
            movie.operation.complete(None).unwrap();
        }
        let interact = session
            .dialogue
            .values()
            .any(|d| !d.closed && !d.persistent && d.fully_revealed() && d.voice_finished());
        session
            .step(FieldInput {
                interact,
                ..Default::default()
            })
            .unwrap();
        let tick = session.events.tick();
        if started.is_some() && session.talking.contains_key(&2) {
            mouth_moved = true;
        }
        for command in session.events.world.audio_commands.drain(..) {
            if let resonance_events::AudioCommand::Voice(id) = command {
                if id == 655379 {
                    started = Some(tick);
                }
                if id == 655380 {
                    let elapsed = tick - started.expect("Colette spoke first");
                    assert!(
                        elapsed >= required,
                        "Raine interrupted after {elapsed} updates; voice needs {required}"
                    );
                    assert!(mouth_moved, "Colette's voice must drive her mouth");
                    return;
                }
            }
        }
    }
    panic!("original scenario never reached Raine's reply");
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets"]
fn original_classroom_reaches_control_walks_and_runs_every_child_conversation() {
    let mut session = classroom(Default::default());
    advance_to(
        &mut session,
        |s| s.events.world.input_enabled,
        |_, update| update % 30 == 10,
    );
    assert!(
        session.events.world.input_enabled,
        "script failed to hand control to player"
    );
    assert_eq!(
        session
            .events
            .memory()
            .read(0x40, symphonia_script::Width::S32)
            .unwrap(),
        1000
    );
    assert_eq!(session.events.world.actors[&1].position, [92., -679., 0.]);
    let camera = session.events.world.field_camera.as_ref().unwrap();
    // Independent classroom-lesson-four-silent Dolphin checkpoint.
    for (actual, expected) in camera.position.into_iter().zip([75.44142, -1278., 186.]) {
        assert!((actual - expected).abs() < 0.002);
    }
    assert_eq!(camera.target, [92., -329., 87.]);
    // The window-side attribute regions select a different script light.
    // These are the current colors in the same independent Dolphin state.
    for id in [1, 2, 301, 303, 304, 305] {
        let light = session.character_light(id);
        assert_eq!(light.shade, [45; 3], "actor {id}");
        assert_eq!(light.bright, [60; 3], "actor {id}");
    }
    for id in [3, 302, 306] {
        let light = session.character_light(id);
        assert_eq!(light.shade, [48; 3], "actor {id}");
        assert_eq!(light.bright, [64; 3], "actor {id}");
    }
    assert!((session.events.world.actors[&301].position[2] - 27.141).abs() < 0.001);
    // Walk along the clear aisle, then toward NPC 305. Input, collision, and
    // target selection use the same field service as presentation.
    for _ in 0..38 {
        session.step(Left.input()).unwrap();
    }
    for _ in 0..15 {
        session.step(Up.input()).unwrap();
    }
    assert_eq!(session.interaction_target(), Some(305));
    let previous_heading = session.events.world.actors[&305].target_heading;
    session.step(Accept.input()).unwrap();
    assert!(!session.events.world.input_enabled);
    let player = &session.events.world.actors[&1];
    let npc = &session.events.world.actors[&305];
    let expected_heading = (player.position[0] - npc.position[0])
        .atan2(npc.position[1] - player.position[1])
        .to_degrees()
        .rem_euclid(360.)
        .trunc();
    assert_eq!(npc.target_heading, expected_heading);
    let dialogue = &session.events.world.dialogue[&0];
    let text = format!("{:?}", dialogue.body.tokens);
    assert!(text.contains("Let's leave everything to"));
    let operation = dialogue.operation.clone();
    for _ in 0..10 {
        session.step(Right.input()).unwrap();
    }
    assert!(!session.events.world.input_enabled);
    let waiting = session.events.world.actors[&1].position;
    for _ in 0..120 {
        if session
            .dialogue
            .get(&0)
            .is_some_and(|p| p.operation.id() == operation.id())
        {
            break;
        }
        session.step(FieldInput::default()).unwrap();
    }
    assert!(
        session
            .dialogue
            .get(&0)
            .is_some_and(|p| p.operation.id() == operation.id())
    );
    advance_to(
        &mut session,
        |s| s.dialogue[&0].accepts_input() && !s.dialogue[&0].fully_revealed(),
        |_, _| false,
    );
    session.step(Accept.input()).unwrap();
    assert!(
        operation.is_pending(),
        "an early press must not dismiss text"
    );
    assert!(!session.dialogue[&0].fully_revealed());
    assert!(!session.dialogue[&0].closed);
    // Let each page reveal and fade normally before issuing its advance edge.
    advance_to(
        &mut session,
        |_| operation.progress().outcome.is_some(),
        |s, _| {
            s.dialogue
                .get(&0)
                .is_some_and(|p| readable(p) && p.accepts_input())
        },
    );
    for _ in 0..4 {
        if session.events.world.input_enabled {
            break;
        }
        session.step(FieldInput::default()).unwrap();
    }
    assert!(session.events.world.input_enabled);
    assert_eq!(
        session.events.world.actors[&305].target_heading,
        previous_heading
    );
    assert_eq!(session.events.world.actors[&1].position, waiting);
    assert_eq!(
        session
            .events
            .memory()
            .read(0x40, symphonia_script::Width::S32)
            .unwrap(),
        1000
    );
    // Enumerate the other child scripts directly after the ordinary walk
    // above. This covers dialogue/glyph behavior, not their navigation paths.
    let art: resonance_content::font::DialogueArt = cooked("ui/dialogue.json");
    let font: resonance_content::font::BitmapFont = cooked(art.font);
    let mut saw_curly_quote = false;
    for actor in 301..=306 {
        assert!(session.events.interact(actor).unwrap(), "child {actor}");
        let mut saw_dialogue = false;
        for update in 0..3000 {
            session
                .step(FieldInput {
                    interact: update % 30 == 10,
                    ..Default::default()
                })
                .unwrap();
            for player in session.dialogue.values().filter(|p| !p.closed) {
                saw_dialogue = true;
                for glyph in &player.current().glyphs {
                    let ch = glyph.character;
                    saw_curly_quote |= ch == '“';
                    assert!(
                        ch == '\n' || font.glyphs.contains_key(&ch),
                        "child {actor} requested uncooked glyph {ch:?}"
                    );
                }
            }
            session.events.world.audio_commands.clear();
            if session.events.world.input_enabled {
                break;
            }
        }
        assert!(saw_dialogue, "child {actor} never opened dialogue");
        assert!(
            session.events.world.input_enabled,
            "child {actor} did not finish"
        );
    }
    assert!(
        saw_curly_quote,
        "reported curly-quote conversation was not exercised"
    );
}
