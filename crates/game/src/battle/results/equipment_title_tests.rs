use super::item_tests::{
    PreparedFixture, close_equipment as finish_equipment, enter_battle, prepared_fixture_with_party,
};
use super::*;
use resonance_content::diagnostics::Diagnostics;

fn close_equipment(
    fixture: &mut PreparedFixture,
    edit: impl FnOnce(&mut Candidate) -> Result<()>,
) -> Result<()> {
    let (mut page, mut character) = fixture.candidate.begin_equipment(&mut fixture.battle, 0)?;
    edit(&mut fixture.candidate)?;
    finish_equipment(
        &mut fixture.candidate,
        &mut fixture.battle,
        &mut page,
        &mut character,
    )
}

fn equip(candidate: &mut Candidate, weapon: u16) -> Result<()> {
    ensure!(
        candidate
            .equipment_draft
            .as_mut()
            .unwrap()
            .equip_slot(&candidate.session, 0, 0, weapon)
            .map_err(anyhow::Error::msg)?,
        "weapon edit did not change inventory"
    );
    Ok(())
}

#[test]
#[ignore = "requires current prepared battle assets; CPU only"]
fn wooden_blade_equip_history_commits_only_final_changed_weapon_and_survives_return() -> Result<()>
{
    let mut fixture = prepared_fixture_with_party(&[1, 2], 2, &[], |party, session, _| {
        party.members[0].equipment[0] = 135;
        party
            .change_item(session, 136, 1)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    })?;
    enter_battle(&mut fixture.candidate, &mut fixture.battle)?;
    let random = fixture.battle.random_state();
    close_equipment(&mut fixture, |candidate| {
        equip(candidate, 136)?;
        equip(candidate, 135)
    })?;
    assert!(!fixture.candidate.party.battles.lloyd_non_wooden_blade_used);
    close_equipment(&mut fixture, |candidate| equip(candidate, 136))?;
    assert!(fixture.candidate.party.battles.lloyd_non_wooden_blade_used);
    close_equipment(&mut fixture, |candidate| equip(candidate, 135))?;
    assert!(fixture.candidate.party.battles.lloyd_non_wooden_blade_used);
    assert!(!fixture.field.battles.lloyd_non_wooden_blade_used);
    assert_eq!(fixture.field.members[0].equipment[0], 135);
    assert_eq!(fixture.battle.random_state(), random);
    Ok(())
}

#[test]
#[ignore = "requires current prepared battle assets; CPU only"]
fn rejected_equipment_keeps_party_actors_and_title_history_under_both_error_policies() -> Result<()>
{
    for paranoid in [false, true] {
        let PreparedFixture {
            mut candidate,
            mut battle,
            field,
            ..
        } = prepared_fixture_with_party(&[1, 2], 2, &[], |party, session, _| {
            party.members[0].equipment[0] = 135;
            party
                .change_item(session, 136, 1)
                .map_err(anyhow::Error::msg)?;
            Ok(())
        })?;
        battle.set_diagnostics(Diagnostics::new(paranoid));
        enter_battle(&mut candidate, &mut battle)?;
        let (mut page, mut character) = candidate.begin_equipment(&mut battle, 0)?;
        let party = serde_json::to_value(&candidate.party)?;
        let field_before = serde_json::to_value(&field)?;
        let random = battle.random_state();
        let actors = battle.actors().to_vec();
        equip(&mut candidate, 136)?;
        // Invalid data on the later actor must reject the entire draft.
        candidate.equipment_draft.as_mut().unwrap().members[1].ex_skills = [u8::MAX, 0, 0, 0];
        let closed = finish_equipment(&mut candidate, &mut battle, &mut page, &mut character);
        assert_eq!(closed.is_err(), paranoid);
        assert!(
            battle
                .diagnostics()
                .entries()
                .iter()
                .any(|entry| entry.scope == "battle equipment"
                    && entry.message.contains("missing EX skill"))
        );
        assert_eq!(serde_json::to_value(&candidate.party)?, party);
        assert!(!candidate.party.battles.lloyd_non_wooden_blade_used);
        assert_eq!(battle.actors(), actors.as_slice());
        assert_eq!(battle.random_state(), random);
        assert!(candidate.equipment_draft.is_none());
        assert_eq!(serde_json::to_value(&field)?, field_before);
    }
    Ok(())
}
