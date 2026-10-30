use super::super::test_support::Fixture;
use super::*;

#[test]
fn unison_cursor_follows_the_character_and_arte_list() -> Result<()> {
    let mut state = Unison::default();
    assert_eq!(main_anchor(&state)?, [32., 88.]);
    state.character = 3;
    assert_eq!(main_anchor(&state)?, [32., 250.]);
    state.focus = Focus::List;
    state.row = 9;
    state.first = 3;
    assert_eq!(main_anchor(&state)?, [280., 248.]);
    state.first = 10;
    assert!(main_anchor(&state).is_err());
    Ok(())
}
#[test]
#[ignore = "requires locally cooked menus; verifies independent loadout drawing"]
fn loadout_has_one_arte_per_character_and_no_button_or_stick_slots() -> Result<()> {
    let mut fixture = Fixture::load()?;
    let state = Unison::default();
    let chosen = state
        .page(&fixture.party, &fixture.session, &fixture.data)
        .techniques()[0];
    fixture.party.members[0].unison_arte = Some(chosen);
    let mut first = fixture.drawing(0);
    let anchor = first.unison(
        state.page(&fixture.party, &fixture.session, &fixture.data),
        false,
    )?;
    first.cursor(anchor, 255);
    fixture.export_drawing(&first, "RESONANCE_UNISON_PREVIEW")?;
    let first = first.batches;
    // Changing regular assignments cannot change this page's chosen arte.
    fixture.party.members[0].shortcuts = [0; 4];
    fixture.party.members[0].arte_bindings.clear();
    let mut second = fixture.drawing(0);
    let anchor = second.unison(
        state.page(&fixture.party, &fixture.session, &fixture.data),
        false,
    )?;
    second.cursor(anchor, 255);
    assert!(first == second.batches);
    for ((_, _, material), batch) in &second.batches {
        if *material == MaterialKey::InputIcons {
            for p in &batch.positions {
                assert!(
                    (240. - p[1]) * 448. / 480. >= 312.,
                    "physical input icon leaked into loadout rows"
                );
            }
        }
    }
    let mut list = state.clone();
    list.focus = Focus::List;
    let mut drawing = fixture.drawing(0);
    let anchor = drawing.unison(
        list.page(&fixture.party, &fixture.session, &fixture.data),
        false,
    )?;
    drawing.cursor(anchor, 255);
    fixture.export_drawing(&drawing, "RESONANCE_UNISON_LIST_PREVIEW")?;
    Ok(())
}
