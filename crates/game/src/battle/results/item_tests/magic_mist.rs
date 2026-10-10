//! Actual encounter preparation and Candidate global equipment projection.
use super::*;
use crate::menu::MenuAction;

#[test]
#[ignore = "requires cooked encounter and equipment assets; CPU only"]
fn magic_mist_equipment_page_refresh_preserves_all_divide_and_rng() -> Result<()> {
    use crate::menu::Input;
    const MAGIC_MIST: u16 = 456;
    const ALL_DIVIDE: u16 = 38;

    let PreparedFixture {
        mut candidate,
        mut battle,
        field,
        ..
    } = prepared_fixture_with_party(&[1, 2, 4], 0, &[], |party, session, _| {
        party.members[0].equipment[3] = 0;
        party.members[8].equipment[3] = MAGIC_MIST;
        party
            .change_item(session, ALL_DIVIDE, 1)
            .map_err(anyhow::Error::msg)?;
        party
            .change_item(session, MAGIC_MIST, 1)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    })?;
    enter_battle(&mut candidate, &mut battle)?;
    let user = candidate.setup.actors[0].0;
    release(
        &mut candidate,
        &mut battle,
        Release {
            user,
            target: user,
            item: ALL_DIVIDE,
        },
    )?;
    assert!(battle.all_divide_active());
    battle.toggle_escape(user)?;
    let mut growth = Vec::new();

    for equipped in [true, false, true] {
        let random = battle.random_state();
        let committed_equipment = candidate.party.members[0].equipment;
        let committed_inventory = candidate.party.items.clone();
        let (mut page, mut character) = candidate.begin_equipment(&mut battle, 0)?;
        for _ in 0..12 {
            candidate.step_equipment(&mut battle, &mut page, &mut character, Input::default())?;
        }
        // Accessory1 is overview row4, backed by saved equipment slot3.
        for _ in 0..4 {
            candidate.step_equipment(
                &mut battle,
                &mut page,
                &mut character,
                Some(MenuAction::Down),
            )?;
        }
        assert_eq!(page.slot, 4);
        if equipped {
            for _ in 0..2 {
                candidate.step_equipment(
                    &mut battle,
                    &mut page,
                    &mut character,
                    Some(MenuAction::Confirm),
                )?;
            }
        } else {
            candidate.step_equipment(
                &mut battle,
                &mut page,
                &mut character,
                Some(MenuAction::Alternate),
            )?;
        }
        assert_eq!(
            candidate.equipment_draft.as_ref().unwrap().members[0].equipment[3],
            if equipped { MAGIC_MIST } else { 0 }
        );
        assert_eq!(
            candidate
                .equipment_draft
                .as_ref()
                .unwrap()
                .items
                .get(&MAGIC_MIST)
                .copied()
                .unwrap_or(0),
            u8::from(!equipped)
        );
        assert_eq!(candidate.party.members[0].equipment, committed_equipment);
        assert_eq!(candidate.party.items, committed_inventory);
        assert!(candidate.equipment_draft.is_some());
        close_equipment(&mut candidate, &mut battle, &mut page, &mut character)?;
        assert!(battle.all_divide_active());
        assert_eq!(battle.random_state(), random);
        assert!(candidate.equipment_draft.is_none());
        let before = step(&mut candidate, &mut battle)?.escape.unwrap().gauge;
        let after = step(&mut candidate, &mut battle)?.escape.unwrap().gauge;
        growth.push(after - before);
    }
    // Reserve gear cannot supply the bonus after the combatant unequips it.
    assert!(growth[1] > 0 && growth[0] > growth[1]);
    assert_eq!(growth[0], growth[2], "re-equipping must restore the bonus");
    assert_eq!(field.members[0].equipment[3], 0);
    assert_eq!(field.items[&MAGIC_MIST], 1);
    assert!(battle.all_divide_active());
    assert!(!battle.is_diagnostic());
    Ok(())
}
