use super::super::test_support::Fixture;
use super::*;
use resonance_events::party::TechniqueShortcut;

#[test]
fn unison_cursor_follows_current_slot_and_list_viewport() -> Result<()> {
    let mut state = Unison::default();
    assert_eq!(main_anchor(&state)?, [72., 90.]);
    state.character = 3;
    state.slot = 3;
    assert_eq!(main_anchor(&state)?, [384., 292.]);
    state.focus = Focus::List;
    state.row = 9;
    state.first = 3;
    assert_eq!(main_anchor(&state)?, [24., 260.]);
    state.first = 10;
    assert!(main_anchor(&state).is_err());
    Ok(())
}
#[test]
#[ignore = "requires locally cooked menus; CPU drawing only"]
fn description_uses_current_shortcut_and_tp_context() -> Result<()> {
    let mut fixture = Fixture::load()?;
    let state = Unison {
        slot: 1,
        ..Default::default()
    };
    let current = state
        .page(&fixture.party, &fixture.session, &fixture.data)
        .selection()
        .unwrap();
    let mut page = fixture.drawing(7);
    page.unison(
        state.page(&fixture.party, &fixture.session, &fixture.data),
        false,
    )?;
    let mut description = fixture.drawing(7);
    description.plane = 3;
    description.technique_description(&fixture.party, &fixture.data, current, false)?;
    assert!(
        description.batches[&(3, DrawRole::Text, FONT)] == page.batches[&(3, DrawRole::Text, FONT)]
    );
    // Battle mode excludes the save-point TP reduction.
    fixture.party.members[3].ex_skills = [31, 0, 0, 0];
    let heal = TechniqueShortcut {
        character: 3,
        technique: 98,
    };
    assert_eq!(
        fixture.party.members[3].technique_cost(&fixture.data, 98, false),
        8
    );
    assert_eq!(
        fixture.party.members[3].technique_cost(&fixture.data, 98, true),
        1
    );
    for (save_point, value) in [(false, "8"), (true, "1")] {
        let mut actual = fixture.drawing(8);
        actual.technique_description(&fixture.party, &fixture.data, heal, save_point)?;
        let mut expected = fixture.drawing(8);
        let x = 488. + expected.text_width("TP : ", 16.)?;
        expected.text(value, [x, 336.], 16., WHITE)?;
        let needle = &expected.batches[&(1, DrawRole::Text, FONT)];
        let haystack = &actual.batches[&(1, DrawRole::Text, FONT)];
        let index = haystack
            .positions
            .windows(needle.positions.len())
            .position(|window| window == needle.positions)
            .unwrap();
        assert_eq!(&haystack.uv[index..index + needle.uv.len()], &needle.uv);
        assert_eq!(
            &haystack.colors[index..index + needle.colors.len()],
            &needle.colors
        );
    }
    Ok(())
}
