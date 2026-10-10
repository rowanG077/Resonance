//! Shared U. Attack state boundaries. Synthetic learned lists test UI only;
//! they do not broaden prepared battle actions or establish live use accounting.
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
        techniques::{Context, Edit, Focus as TechFocus, Tech},
        unison::{Focus, Input, Unison},
    },
};
use std::sync::Arc;

fn fixture() -> (Arc<SessionData>, MenuData, Party) {
    let (session, data, mut party) = menu_fixture::fixture();
    party.formation.truncate(5);
    (session, data, party)
}
fn list(party: &mut Party, session: &SessionData, data: &MenuData) -> Unison {
    let mut state = Unison::opening(0, party).unwrap();
    state
        .step(Some(MenuAction::Confirm), party, session, data)
        .unwrap();
    assert_eq!(state.focus, Focus::List);
    state
}

#[test]
fn first_four_retained_member_catalogue_order_and_nonusable_assignment() {
    let (session, mut data, mut party) = fixture();
    let state = Unison::opening(3, &party).unwrap();
    assert_eq!(state.character, 3);
    assert_eq!(state.page(&party, &session, &data).party_count(), 4);
    assert_eq!(Unison::opening(4, &party).unwrap().character, 0);
    party.formation.truncate(2);
    assert_eq!(Unison::opening(3, &party).unwrap().character, 0);
    let mut state = list(&mut party, &session, &data);
    assert_eq!(
        state.page(&party, &session, &data).techniques(),
        session.characters[0].allowed_techniques[..12]
    );
    state.row = 1;
    let chosen = state.page(&party, &session, &data).selection().unwrap();
    data.techniques[usize::from(chosen.technique)].unison_usable = false;
    let visit = state
        .step(Some(MenuAction::Confirm), &mut party, &session, &data)
        .unwrap();
    assert_eq!(visit.cue, Some(2));
    assert_eq!(party.members[0].shortcuts[0], chosen.technique);
    assert_eq!(
        state.page(&party, &session, &data).selection(),
        Some(chosen)
    );
}

#[test]
fn cancel_closes_and_confirm_opens_without_moving_selection() {
    let (session, data, mut party) = fixture();
    let before = party.members[0].shortcuts;
    let mut state = Unison::opening(0, &party).unwrap();
    let visit = state
        .step(Some(MenuAction::Cancel), &mut party, &session, &data)
        .unwrap();
    assert!(visit.closed);
    assert_eq!((state.character, state.slot), (0, 0));
    assert_eq!(party.members[0].shortcuts, before);
    let mut state = Unison::opening(0, &party).unwrap();
    state
        .step(Some(MenuAction::Confirm), &mut party, &session, &data)
        .unwrap();
    assert_eq!(
        (state.focus, state.character, state.slot),
        (Focus::List, 0, 0)
    );
}

#[test]
fn idle_and_cancel_do_not_read_technique_descriptions() {
    let (session, mut data, mut party) = fixture();
    data.techniques.clear();
    let before = serde_json::to_value(&party).unwrap();
    for focus in [Focus::Slots, Focus::List] {
        let mut state = Unison::opening(0, &party).unwrap();
        state.focus = focus;
        let before_state = state.clone();
        assert_eq!(
            state
                .step(Input::default(), &mut party, &session, &data)
                .unwrap()
                .cue,
            None
        );
        assert_eq!(state, before_state);
        let cancelled = state
            .step(Some(MenuAction::Cancel), &mut party, &session, &data)
            .unwrap();
        assert_eq!(cancelled.cue, Some(3));
        assert_eq!(cancelled.closed, focus == Focus::Slots);
    }
    let context = Context::Field {
        at_save_point: false,
        connected: &[false; 4],
    };
    for focus in [
        TechFocus::Shortcuts,
        TechFocus::List,
        TechFocus::AssistList,
        TechFocus::Target,
    ] {
        let mut state = Tech::opening(0, context, &party, &session, &data).unwrap();
        state.focus = focus;
        state.return_to = TechFocus::List;
        let before_state = state.clone();
        assert_eq!(
            state
                .step(Input::default(), &mut party, &session, &data, context)
                .unwrap()
                .cue,
            None
        );
        assert_eq!(state, before_state);
        assert_eq!(
            state
                .step(
                    Some(MenuAction::Cancel),
                    &mut party,
                    &session,
                    &data,
                    context
                )
                .unwrap()
                .cue,
            Some(3)
        );
    }
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
}

#[test]
fn shortcut_assignment_checks_its_selected_description_only() {
    let (session, mut data, mut party) = fixture();
    let mut unison = list(&mut party, &session, &data);
    let selected = unison.page(&party, &session, &data).selection().unwrap();
    data.techniques
        .truncate(usize::from(selected.technique) + 1);
    assert_eq!(
        unison
            .step(Some(MenuAction::Confirm), &mut party, &session, &data)
            .unwrap()
            .cue,
        Some(2)
    );
    assert_eq!(party.members[0].shortcuts[0], selected.technique);

    let context = Context::Field {
        at_save_point: false,
        connected: &[false; 4],
    };
    let mut tech = Tech::opening(0, context, &party, &session, &data).unwrap();
    tech.focus = TechFocus::List;
    tech.slot = 1;
    assert_eq!(
        tech.step(
            Some(MenuAction::Confirm),
            &mut party,
            &session,
            &data,
            context
        )
        .unwrap()
        .cue,
        Some(2)
    );
    assert_eq!(party.members[0].shortcuts[1], selected.technique);

    data.techniques.clear();
    unison.focus = Focus::List;
    tech.focus = TechFocus::List;
    let before = serde_json::to_value(&party).unwrap();
    let unison_before = unison.clone();
    let tech_before = tech.clone();
    assert!(
        unison
            .step_with_edit(
                Some(MenuAction::Confirm),
                &mut party,
                &session,
                &data,
                |_, _| panic!("missing description must not commit")
            )
            .is_err()
    );
    assert!(
        tech.request_step(Some(MenuAction::Confirm), &party, &session, &data, context)
            .is_err()
    );
    assert_eq!(unison, unison_before);
    assert_eq!(tech, tech_before);
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
}

#[test]
fn every_first_four_roster_size_and_page_repeat_edge() {
    let (session, data, base) = fixture();
    for count in 1..=4 {
        let mut party = base.clone();
        party.formation.truncate(count);
        let mut state = Unison::opening(0, &party).unwrap();
        for cell in 1..count * 4 {
            let visit = state
                .step(Some(MenuAction::Down), &mut party, &session, &data)
                .unwrap();
            assert_eq!(visit.cue, Some(1));
            assert_eq!((state.character, state.slot), (cell / 4, cell % 4));
        }
        assert_eq!(
            state
                .step(Some(MenuAction::Down), &mut party, &session, &data)
                .unwrap()
                .cue,
            None
        );
        for _ in 1..count * 4 {
            state
                .step(Some(MenuAction::Up), &mut party, &session, &data)
                .unwrap();
        }
        assert_eq!((state.character, state.slot), (0, 0));
    }
    let mut party = base;
    let mut state = list(&mut party, &session, &data);
    let first = state
        .step(Some(MenuAction::PageDown), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first, first.cue), (8, 8, Some(38)));
    state
        .step(Some(MenuAction::PageDown), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first), (11, 8));
    assert_eq!(
        state
            .step(Some(MenuAction::PageDown), &mut party, &session, &data)
            .unwrap()
            .cue,
        None
    );
    state
        .step(Some(MenuAction::PageUp), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first), (3, 0));
    state
        .step(Some(MenuAction::PageUp), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first), (0, 0));
}

#[test]
fn scrolling_keeps_selection_visible_and_cancel_leaves_assignments_unchanged() {
    let (session, data, mut party) = fixture();
    let assignments = party.members[0].shortcuts;
    let mut state = list(&mut party, &session, &data);
    state.row = 7;
    state
        .step(Some(MenuAction::Down), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first), (8, 1));
    // Navigation is available on the very next input, including scrolling.
    state
        .step(Some(MenuAction::Down), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first), (9, 2));
    state
        .step(Some(MenuAction::Cancel), &mut party, &session, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Slots);
    assert_eq!(party.members[0].shortcuts, assignments);
}

#[test]
fn assignment_commits_once_and_clears_without_changing_use_counts() {
    let (session, data, mut party) = fixture();
    let mut state = list(&mut party, &session, &data);
    state.row = 2;
    let expected = state.page(&party, &session, &data).selection().unwrap();
    let uses = party.members[0].technique_uses.clone();
    let mut calls = 0;
    let visit = state
        .step_with_edit(
            Some(MenuAction::Confirm),
            &mut party,
            &session,
            &data,
            |actual, edit| {
                calls += 1;
                assert_eq!(
                    edit,
                    Edit::Shortcut {
                        member: 0,
                        slot: 0,
                        selected: Some(expected)
                    }
                );
                edit.apply_field(actual, &data)
            },
        )
        .unwrap();
    assert_eq!(calls, 1);
    assert!(visit.changed);
    assert_eq!(state.focus, Focus::Slots);
    assert_eq!(
        state.page(&party, &session, &data).selection(),
        Some(expected)
    );
    state
        .step(Some(MenuAction::Confirm), &mut party, &session, &data)
        .unwrap();
    let same = state
        .step(Some(MenuAction::Confirm), &mut party, &session, &data)
        .unwrap();
    assert_eq!(same.cue, Some(2));
    assert!(!same.changed);
    let clear = state
        .step(Some(MenuAction::Alternate), &mut party, &session, &data)
        .unwrap();
    assert!(clear.changed);
    assert!(state.page(&party, &session, &data).selection().is_none());
    assert!(
        !state
            .step(Some(MenuAction::Alternate), &mut party, &session, &data)
            .unwrap()
            .changed
    );
    assert_eq!(party.members[0].technique_uses, uses);
}

#[test]
fn empty_or_stale_selection_is_disabled_and_failed_assignment_is_atomic() {
    let (session, data, mut party) = fixture();
    let mut state = Unison::opening(0, &party).unwrap();
    party.members[0].techniques.clear();
    party.members[0].shortcuts = [0; 4];
    let before = serde_json::to_value(&party).unwrap();
    for focus in [Focus::Slots, Focus::List] {
        state.focus = focus;
        let visit = state
            .step_with_edit(
                Some(MenuAction::Confirm),
                &mut party,
                &session,
                &data,
                |_, _| panic!("empty choice must not reach assignment"),
            )
            .unwrap();
        assert_eq!(visit.cue, Some(4));
        assert!(!visit.changed && !visit.closed);
        assert_eq!(state.focus, focus);
    }
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
    assert!(
        !state
            .step(Some(MenuAction::Cancel), &mut party, &session, &data)
            .unwrap()
            .closed
    );
    assert_eq!(state.focus, Focus::Slots);
    assert!(
        state
            .step(Some(MenuAction::Cancel), &mut party, &session, &data)
            .unwrap()
            .closed
    );

    let (session, data, mut party) = fixture();
    let mut state = list(&mut party, &session, &data);
    state.row = 999;
    let before = serde_json::to_value(&party).unwrap();
    let unavailable = state
        .step_with_edit(
            Some(MenuAction::Confirm),
            &mut party,
            &session,
            &data,
            |_, _| panic!("stale choice must not reach assignment"),
        )
        .unwrap();
    assert_eq!(unavailable.cue, Some(4));
    state.row = 2;
    let selection = state.clone();
    assert!(
        state
            .step_with_edit(
                Some(MenuAction::Confirm),
                &mut party,
                &session,
                &data,
                |_, _| { anyhow::bail!("missing prepared live binding") }
            )
            .is_err()
    );
    assert_eq!(state, selection);
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
    state
        .step(Some(MenuAction::Cancel), &mut party, &session, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Slots);
}

#[test]
fn field_unison_unlocks_prioritizes_inputs_and_shares_saved_shortcuts() {
    let (session, data, party) = fixture();
    let data = Arc::new(data);
    let mut globals = vec![0; 256];
    globals[16] = menu::unison::UNLOCK_STORY - 1;
    let mut field = Menu::new(
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
                script_globals: globals,
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
    field.resources = Some(Arc::new(Resources {
        session,
        data,
        files: Arc::new(Files::default()),
    }));
    let press = |field: &mut Menu, input| {
        let cue = field.step(input);
        for _ in 0..40 {
            field.step(Default::default());
            if !field.main_animating() {
                return cue;
            }
        }
        panic!("menu transition did not finish");
    };
    let accept = FieldInput {
        pressed_buttons: [resonance_events::input::Button::Accept].into(),
        ..Default::default()
    };
    field.selected = 1;
    assert_eq!(press(&mut field, accept), Some(4));
    assert_eq!(field.page, menu::Page::Main);
    field.checkpoint.as_mut().unwrap().progress.script_globals[16] += 1;
    assert_eq!(press(&mut field, accept), Some(2));
    assert_eq!(field.page, menu::Page::Unison);
    for extra in [
        FieldInput {
            pressed_buttons: [resonance_events::input::Button::Skit].into(),
            ..Default::default()
        },
        FieldInput {
            pressed_buttons: [resonance_events::input::Button::PreviousPage].into(),
            ..Default::default()
        },
        FieldInput {
            pressed_buttons: [resonance_events::input::Button::NextPage].into(),
            ..Default::default()
        },
        FieldInput {
            direction: [-1., 0.],
            ..Default::default()
        },
    ] {
        field.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Accept].into(),
            ..extra
        });
        assert_eq!(field.unison.focus, Focus::List);
        field.step(FieldInput {
            pressed_buttons: [
                resonance_events::input::Button::Accept,
                resonance_events::input::Button::Cancel,
            ]
            .into(),
            ..Default::default()
        });
        assert_eq!(field.unison.focus, Focus::Slots);
        assert_eq!(field.party().members[0].shortcuts[0], 0);
        field.step(FieldInput::default());
        assert_eq!(field.unison.focus, Focus::Slots);
    }
    press(&mut field, accept);
    let chosen = field.unison_selection().unwrap().technique;
    press(&mut field, accept);
    assert_eq!(field.party().members[0].shortcuts[0], chosen);
    let restored: FieldCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(field.checkpoint.as_ref().unwrap()).unwrap())
            .unwrap();
    let restored = restored
        .progress
        .into_state(&field.resources.as_ref().unwrap().session)
        .unwrap();
    assert_eq!(restored.party.unwrap().members[0].shortcuts[0], chosen);
    press(
        &mut field,
        FieldInput {
            direction: [1., 0.],
            ..Default::default()
        },
    );
    assert_eq!(field.unison.character, 2);
    press(
        &mut field,
        FieldInput {
            pressed_buttons: [resonance_events::input::Button::Cancel].into(),
            ..Default::default()
        },
    );
    assert_eq!(field.page, menu::Page::Main);
    assert_eq!(field.character, 2);
    assert_eq!(field.checkpoint.as_ref().unwrap().progress.tick, 17);
    field.character = 0;
    field.selected = 0;
    press(&mut field, accept);
    press(&mut field, accept);
    assert_eq!(field.page, menu::Page::Tech);
    assert_eq!(field.selected_technique().unwrap().technique, chosen);
}

#[test]
#[ignore = "requires locally cooked character technique catalogues"]
fn cooked_unison_uses_learned_techniques_in_catalogue_order() {
    let session = cooked::<SessionData>("game/session-data.json");
    let data = cooked::<MenuData>("game/menu-data.json");
    let mut party = Party::new(&session, Default::default()).unwrap();
    let expected = &session.characters[0].allowed_techniques[..12];
    party.members[0].techniques = expected.iter().copied().collect();
    let state = list(&mut party, &session, &data);
    assert_eq!(state.page(&party, &session, &data).techniques(), expected);
}
