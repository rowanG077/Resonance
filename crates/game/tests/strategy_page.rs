//! Strategy behavior uses small fixtures; authored masks and names use cooked data.
mod common;
#[path = "common/menu_fixture.rs"]
mod menu_fixture;
use common::cooked;
use resonance_content::{menu_data::MenuData, prepared::Files, session::SessionData};
use resonance_events::{SavedProgress, party::Party};
use resonance_game::{
    field::{FieldCheckpoint, FieldInput},
    menu::{
        self, Menu, MenuAction, Resources,
        strategy::{Focus, Input, Strategy},
    },
};
use std::sync::Arc;

fn cooked_fixture() -> (Arc<SessionData>, Arc<MenuData>, Party) {
    let session = Arc::new(cooked::<SessionData>("game/session-data.json"));
    let data = Arc::new(cooked::<MenuData>("game/menu-data.json"));
    data.validate().unwrap();
    let mut party = Party::new(&session, Default::default()).unwrap();
    party.formation = vec![1, 2, 3, 4, 5, 7, 8, 9];
    (session, data, party)
}
fn fixture() -> (Arc<SessionData>, Arc<MenuData>, Party) {
    let (session, data, party) = menu_fixture::fixture();
    (session, Arc::new(data), party)
}
fn field_menu(session: Arc<SessionData>, data: Arc<MenuData>, party: Party) -> Menu {
    let mut menu = Menu::new(
        menu::Page::Main,
        Some(FieldCheckpoint {
            allow_incomplete_scripts: false,
            map_id: 330,
            position: [0.; 3],
            heading: 0.,
            camera: Some(common::checkpoint_camera()),
            played_ticks: 17,
            progress: SavedProgress {
                script_state: Default::default(),
                script_globals: vec![0; 256],
                party,
                event_flags: Default::default(),
                event_records: Default::default(),
                random_state: 0,
                gameplay_random: Default::default(),
                tick: 17,
            },
        }),
        false,
    );
    menu.resources = Some(Arc::new(Resources {
        session,
        data,
        files: Arc::new(Files::default()),
    }));
    menu
}

#[test]
fn strategy_cancel_preserves_settings_and_confirm_reopens_options() {
    let (_, data, mut party) = fixture();
    let before = party.members[0].strategy;
    let mut state = Strategy {
        focus: Focus::Options,
        option: 1,
        ..Default::default()
    };
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Setting);
    assert_eq!(party.members[0].strategy, before);
    state
        .step(Some(MenuAction::Confirm), &mut party, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Options);
}

#[test]
#[ignore = "requires locally cooked Strategy masks and character names"]
fn borrowed_strategy_page_covers_all_nine_character_masks_and_names() {
    let (_, data, mut party) = cooked_fixture();
    let mut state = Strategy {
        focus: Focus::Setting,
        ..Default::default()
    };
    for member in 0..9 {
        party.formation = vec![member as u8 + 1];
        for group in 0..3 {
            state.group = group;
            let page = state.page(&party, &data);
            let expected: Vec<_> = data.strategy.groups[group]
                .iter()
                .enumerate()
                .filter_map(|(i, row)| (row.characters & (1u16 << member) != 0u16).then_some(i))
                .collect();
            assert!(!expected.is_empty());
            assert_eq!(page.options(), expected);
            assert_eq!(page.choices(member), party.members[member].strategy);
            assert_eq!(page.character_name(member), data.initial_names[member]);
        }
        party.members[member].name = Some(format!("Member{member}"));
        assert_eq!(
            state.page(&party, &data).character_name(member),
            format!("Member{member}")
        );
    }
    for group in 0..3 {
        let mask = |member: usize| {
            data.strategy.groups[group]
                .iter()
                .map(|row| row.characters & (1u16 << member) != 0u16)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            mask(8),
            mask(5),
            "Kratos retains the original Zelos mask alias"
        );
    }
}

#[test]
fn confirmed_personal_and_named_preset_edits_keep_separate_persistent_authority() {
    let (_, data, mut party) = fixture();
    party.members[0].strategy = [0; 3];
    let mut state = Strategy {
        focus: Focus::Options,
        option: 1,
        transition: menu::Transition::opening(),
        scroll: 1,
        ..Default::default()
    };
    assert!(
        state
            .step(Some(MenuAction::Confirm), &mut party, &data)
            .unwrap()
            .changed
    );
    assert_eq!(party.members[0].strategy[0], 1);
    assert!(
        !state
            .step(Input::default(), &mut party, &data)
            .unwrap()
            .changed
    );
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    assert_eq!(
        party.members[0].strategy[0], 1,
        "nested cancel cannot undo a confirmed edit"
    );
    state.focus = Focus::PresetOptions;
    state.preset_opacity = 255;
    state.preset = 1;
    state.option = 2;
    state
        .step(Some(MenuAction::Confirm), &mut party, &data)
        .unwrap();
    assert_eq!(party.strategy_presets.as_ref().unwrap()[1].members[0][0], 2);
    assert_eq!(party.members[0].strategy[0], 1);
    state.focus = Focus::Presets;
    state
        .step(Some(MenuAction::Alternate), &mut party, &data)
        .unwrap();
    state
        .step(Some(MenuAction::Confirm), &mut party, &data)
        .unwrap();
    let edited = state.rename.value.clone();
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    state.step(Some(MenuAction::Up), &mut party, &data).unwrap();
    assert!(
        state
            .step(Some(MenuAction::Confirm), &mut party, &data)
            .unwrap()
            .changed
    );
    assert_eq!(party.strategy_presets.as_ref().unwrap()[1].name, edited);
    let saved: Party = serde_json::from_slice(&serde_json::to_vec(&party).unwrap()).unwrap();
    assert_eq!(saved.members[0].strategy, party.members[0].strategy);
    assert_eq!(saved.strategy_presets, party.strategy_presets);
    let other = party.strategy_presets.as_ref().unwrap()[0].clone();
    state
        .step(Some(MenuAction::Menu), &mut party, &data)
        .unwrap();
    assert_eq!(
        party.strategy_presets.as_ref().unwrap()[1],
        data.strategy_presets().unwrap()[1]
    );
    assert_eq!(party.strategy_presets.as_ref().unwrap()[0], other);
    assert_eq!(party.members[0].strategy[0], 1);
}

#[test]
fn strategy_scroll_keeps_rows_visible_and_preset_cancel_restores_character_focus() {
    let (_, data, mut party) = fixture();
    let mut state = Strategy {
        character: 3,
        ..Default::default()
    };
    state
        .step(Some(MenuAction::Down), &mut party, &data)
        .unwrap();
    assert_eq!((state.character, state.first), (4, 1));
    assert_ne!(state.scroll, 0);
    state
        .step(Some(MenuAction::Down), &mut party, &data)
        .unwrap();
    assert_eq!((state.character, state.first), (5, 2));
    state
        .step(Some(MenuAction::Alternate), &mut party, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Presets);
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    assert_eq!(
        state.focus,
        Focus::Character,
        "Cancel is immediate during the preset fade"
    );
    state
        .step(Some(MenuAction::Alternate), &mut party, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Presets);
    assert!(!state.preset_closing);
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    for _ in 0..10 {
        state.step(Input::default(), &mut party, &data).unwrap();
    }
    assert_eq!(state.focus, Focus::Character);
    assert_eq!(state.preset_opacity, 0);
    assert_eq!(state.scroll, 0);
}

#[test]
fn equipment_accepts_one_edit_and_cancel_while_its_visuals_settle() {
    use menu::equipment::{Equipment, Focus as EquipmentFocus, equipment_items_for};
    use menu_fixture::SPARE_WEAPON;
    let (session, data, mut party) = fixture();
    party.change_item(&session, SPARE_WEAPON, 1).unwrap();
    let old_weapon = party.members[0].equipment[0];
    let mut character = 0;
    let mut state = Equipment {
        focus: EquipmentFocus::List,
        scroll: 1,
        ..Equipment::opening()
    };
    state.row = equipment_items_for(&party, &session, &data, 0, &state)
        .iter()
        .position(|&id| id == SPARE_WEAPON)
        .unwrap();
    let visit = state
        .step_shared(
            Some(MenuAction::Confirm),
            &mut party,
            &session,
            &data,
            &mut character,
        )
        .unwrap();
    assert!(visit.changed);
    assert_eq!(party.members[0].equipment[0], SPARE_WEAPON);
    assert_ne!(old_weapon, SPARE_WEAPON);
    let committed = serde_json::to_value(&party).unwrap();
    assert!(
        !state
            .step_shared(
                Input::default(),
                &mut party,
                &session,
                &data,
                &mut character
            )
            .unwrap()
            .changed
    );
    assert_eq!(serde_json::to_value(&party).unwrap(), committed);
    assert_ne!(state.transition.page_fade, 0);
    state
        .step_shared(
            Some(MenuAction::Cancel),
            &mut party,
            &session,
            &data,
            &mut character,
        )
        .unwrap();
    assert_eq!(state.focus, EquipmentFocus::Character);
    state
        .step_shared(
            Some(MenuAction::Cancel),
            &mut party,
            &session,
            &data,
            &mut character,
        )
        .unwrap();
    assert!(state.transition.page_closing);
    for _ in 0..20 {
        if state
            .step_shared(
                Input::default(),
                &mut party,
                &session,
                &data,
                &mut character,
            )
            .unwrap()
            .closed
        {
            assert_eq!(serde_json::to_value(&party).unwrap(), committed);
            return;
        }
    }
    panic!("equipment close animation did not settle");
}

#[test]
fn rename_rejects_empty_names_restores_original_and_cancels_without_saving() {
    let (_, data, mut party) = fixture();
    let mut state = Strategy {
        focus: Focus::Presets,
        preset_opacity: 255,
        ..Default::default()
    };
    state
        .step(Some(MenuAction::Alternate), &mut party, &data)
        .unwrap();
    for (mode, cue) in [(1, None), (2, None), (0, Some(1))] {
        assert_eq!(
            state
                .step(Some(MenuAction::Details), &mut party, &data)
                .unwrap()
                .cue,
            cue
        );
        assert_eq!(state.rename.mode, mode);
    }
    let original = state.rename.value.clone();
    state
        .step(Some(MenuAction::NextPosition), &mut party, &data)
        .unwrap();
    assert_eq!(state.rename.position, 1);
    state
        .step(Some(MenuAction::PreviousPosition), &mut party, &data)
        .unwrap();
    assert_eq!(state.rename.position, 0);
    assert_eq!(
        state
            .step(Some(MenuAction::Details), &mut party, &data)
            .unwrap()
            .cue,
        None
    );
    assert_eq!(state.rename.mode, 1, "details changes keyboard mode");
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    assert_eq!(
        (state.rename.column, state.rename.row, state.focus),
        (10, 7, Focus::Rename)
    );
    state.step(Some(MenuAction::Up), &mut party, &data).unwrap();
    state.step(Some(MenuAction::Up), &mut party, &data).unwrap();
    for _ in 0..7 {
        state
            .step(Some(MenuAction::Confirm), &mut party, &data)
            .unwrap();
    }
    assert!(state.rename.value.is_empty());
    state
        .step(Some(MenuAction::Down), &mut party, &data)
        .unwrap();
    assert_eq!(
        state
            .step(Some(MenuAction::Confirm), &mut party, &data)
            .unwrap()
            .cue,
        Some(4)
    );
    assert_eq!(state.focus, Focus::Rename);
    state
        .step(Some(MenuAction::Down), &mut party, &data)
        .unwrap();
    state
        .step(Some(MenuAction::Down), &mut party, &data)
        .unwrap();
    state
        .step(Some(MenuAction::Confirm), &mut party, &data)
        .unwrap();
    assert_eq!(state.rename.value, original);
    state
        .step(Some(MenuAction::Cancel), &mut party, &data)
        .unwrap();
    state
        .step(Some(MenuAction::Confirm), &mut party, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Presets);
    assert!(party.strategy_presets.is_none());
}

#[test]
fn field_strategy_uses_existing_page_repeats_without_changing_rename_axis_meaning() {
    let (session, data, party) = fixture();
    let mut field = field_menu(session, data, party);
    field.page = menu::Page::Strategy;
    let page_down = FieldInput {
        scroll_direction: -1,
        ..Default::default()
    };
    assert_eq!(field.step(page_down), Some(38));
    assert_eq!((field.strategy.character, field.strategy.first), (4, 4));
    assert_eq!(
        field.step(page_down),
        None,
        "held axis acquired a second repeat owner"
    );
    field.step(Default::default());
    assert_eq!(
        field.step(FieldInput {
            scroll_direction: 1,
            ..Default::default()
        }),
        Some(38)
    );
    assert_eq!((field.strategy.character, field.strategy.first), (0, 0));
    field.step(Default::default());
    assert_eq!(
        field.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::NextPage].into(),
            ..Default::default()
        }),
        Some(38)
    );
    assert_eq!((field.strategy.character, field.strategy.first), (4, 4));
    field.strategy.focus = Focus::Rename;
    field.strategy.preset_opacity = 255;
    field.strategy.rename.value = "Reserve".into();
    field.strategy.rename.position = 2;
    assert_eq!(field.step(page_down), None);
    assert_eq!(
        field.strategy.rename.position, 2,
        "vertical C-stick became a horizontal rename command"
    );
    field.step(Default::default());
    assert_eq!(
        field.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::NextPage].into(),
            ..Default::default()
        }),
        Some(38)
    );
    assert_eq!(field.strategy.rename.position, 3);
}
