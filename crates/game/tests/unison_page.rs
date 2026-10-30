//! The Unison loadout is independent of physical inputs and regular arte shortcuts.
mod common;
#[path = "common/menu_fixture.rs"]
mod menu_fixture;
use resonance_content::input::{
    ArteBinding, AxisDirection, InputChord, PhysicalInput, StandardAction,
};
use resonance_game::menu::{
    MenuAction as A,
    techniques::Edit,
    unison::{Focus, Unison},
};

#[test]
fn selecting_unison_changes_only_the_independent_loadout() {
    let (session, data, mut party) = menu_fixture::fixture();
    let mut state = Unison::opening(0, &party).unwrap();
    party.members[0].shortcuts = [1, 2, 3, 4];
    party.members[0].arte_bindings = vec![ArteBinding {
        technique: 1,
        chord: InputChord(vec![PhysicalInput::Key("F11".into())]),
    }];
    let shortcuts = party.members[0].shortcuts;
    let bindings = party.members[0].arte_bindings.clone();
    let map = party.settings.preferences.keymap.clone();
    state
        .step(Some(A::Confirm), &mut party, &session, &data)
        .unwrap();
    state
        .step(Some(A::Down), &mut party, &session, &data)
        .unwrap();
    let chosen = state.page(&party, &session, &data).selection().unwrap();
    assert!(
        state
            .step(Some(A::Confirm), &mut party, &session, &data)
            .unwrap()
            .changed
    );
    assert_eq!(party.members[0].unison_arte, Some(chosen.technique));
    assert_eq!(party.members[0].shortcuts, shortcuts);
    assert_eq!(party.members[0].arte_bindings, bindings);
    assert_eq!(party.settings.preferences.keymap, map);
    party.settings.preferences.keymap.assign(
        StandardAction::Confirm,
        PhysicalInput::Axis(AxisDirection::RightDown),
    );
    party.members[0].shortcuts = [0; 4];
    party.members[0].arte_bindings.clear();
    assert_eq!(
        state.page(&party, &session, &data).selection(),
        Some(chosen)
    );
    let saved: resonance_events::party::Party =
        serde_json::from_slice(&serde_json::to_vec(&party).unwrap()).unwrap();
    assert_eq!(saved.members[0].unison_arte, Some(chosen.technique));
    assert!(
        state
            .step(Some(A::Alternate), &mut party, &session, &data)
            .unwrap()
            .changed
    );
    assert_eq!(party.members[0].unison_arte, None);
}

#[test]
fn choices_are_learned_unison_eligible_artes_in_catalogue_order() {
    let (session, mut data, mut party) = menu_fixture::fixture();
    let allowed = &session.characters[0].allowed_techniques;
    data.techniques[usize::from(allowed[1])].unison_usable = false;
    party.members[0].techniques.remove(&allowed[2]);
    let state = Unison::opening(0, &party).unwrap();
    let expected: Vec<_> = allowed
        .iter()
        .copied()
        .filter(|id| *id != allowed[1] && *id != allowed[2])
        .collect();
    assert_eq!(state.page(&party, &session, &data).techniques(), expected);
    assert!(
        party
            .assign_unison_arte(&data, 0, Some(allowed[1]))
            .is_err()
    );
    assert!(
        party
            .assign_unison_arte(&data, 0, Some(allowed[2]))
            .is_err()
    );
    assert_eq!(party.members[0].unison_arte, None);
}

#[test]
fn loadout_selects_characters_without_direction_slots_and_list_scrolling_is_stable() {
    let (session, data, mut party) = menu_fixture::fixture();
    let mut state = Unison::opening(3, &party).unwrap();
    assert_eq!(state.page(&party, &session, &data).party_count(), 4);
    state
        .step(Some(A::Down), &mut party, &session, &data)
        .unwrap();
    assert_eq!(state.character, 0);
    state
        .step(Some(A::Confirm), &mut party, &session, &data)
        .unwrap();
    for _ in 0..10 {
        state
            .step(Some(A::Down), &mut party, &session, &data)
            .unwrap();
    }
    assert_eq!((state.row, state.first), (10, 3));
    state
        .step(Some(A::Up), &mut party, &session, &data)
        .unwrap();
    assert_eq!((state.row, state.first), (9, 3));
    state
        .step(Some(A::Cancel), &mut party, &session, &data)
        .unwrap();
    assert_eq!(state.focus, Focus::Loadout);
    assert!(
        state
            .step(Some(A::Cancel), &mut party, &session, &data)
            .unwrap()
            .closed
    );
}

#[test]
fn failed_edit_leaves_loadout_and_page_unchanged() {
    let (session, data, mut party) = menu_fixture::fixture();
    let mut state = Unison::opening(0, &party).unwrap();
    state
        .step(Some(A::Confirm), &mut party, &session, &data)
        .unwrap();
    let before = state.clone();
    let saved = serde_json::to_value(&party).unwrap();
    let result = state.step_with_edit(Some(A::Confirm), &mut party, &session, &data, |_, edit| {
        assert!(matches!(
            edit,
            Edit::Unison {
                member: 0,
                technique: Some(_)
            }
        ));
        anyhow::bail!("not prepared")
    });
    assert!(result.is_err());
    assert_eq!(state, before);
    assert_eq!(serde_json::to_value(&party).unwrap(), saved);
}

#[test]
fn empty_choices_and_cancellation_do_not_read_missing_arte_text() {
    let (session, mut data, mut party) = menu_fixture::fixture();
    data.techniques.clear();
    let mut state = Unison::opening(0, &party).unwrap();
    assert_eq!(
        state
            .step(Some(A::Confirm), &mut party, &session, &data)
            .unwrap()
            .cue,
        Some(4)
    );
    for focus in [Focus::Loadout, Focus::List] {
        state.focus = focus;
        state.step(None, &mut party, &session, &data).unwrap();
        state
            .step(Some(A::Cancel), &mut party, &session, &data)
            .unwrap();
    }
}
