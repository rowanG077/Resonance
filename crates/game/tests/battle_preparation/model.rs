use super::*;
use resonance_battle::BattleClock;

#[test]
#[ignore = "requires current party and equipment publications; CPU only"]
fn equipped_artwork_binds_without_attaching_inventory_models() -> Result<()> {
    use std::collections::BTreeSet;

    let mut fixture = super::all_party_encounter::ColdEncounter::load()?;
    let mut party = resonance_events::party::Party::new(&fixture.session, Default::default())?;
    party.formation = vec![1, 2, 3];
    party.field_leader = 1;
    party.settings.battle_controls = [1, 1, 1, 0];
    for member in &mut party.members {
        member.techniques.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
        member.ex_skills = [0; 4];
        member.compound_ex_skills.clear();
        member.recent_compound_ex_skills.clear();
    }
    for (slot, item) in [(0, 155), (1, 171), (2, 175)] {
        party.members[slot].equipment = [item, 0, 0, 0, 0, 0];
    }
    party
        .change_item(&fixture.session, 135, 1)
        .map_err(anyhow::Error::msg)?;
    let (_, prepared) = fixture.prepare(&party)?;
    let mut equipped = BTreeSet::new();
    let mut inventory = BTreeSet::new();
    for (slot, item, worn) in [
        (0, 155, true),
        (1, 171, true),
        (2, 175, true),
        (0, 135, false),
    ] {
        let owner = prepared.results.actors[slot].0;
        let bundle = prepared
            .core
            .models()
            .unwrap()
            .equipment_for_items(owner, [item, 0]);
        assert!(!bundle.is_empty(), "item {item} was not prepared");
        let resources = if worn { &mut equipped } else { &mut inventory };
        resources.extend(bundle.iter().map(|weapon| weapon.primary_resource()));
    }
    assert!(equipped.is_disjoint(&inventory));
    let owners: BTreeSet<_> = prepared
        .results
        .actors
        .iter()
        .map(|&(actor, _)| actor)
        .collect();
    let mut active = prepared.core;
    let mut frame = active.step(BattleInput::default())?;
    active.advance_models(&mut frame, BattleClock::Running, false)?;
    let shown: BTreeSet<_> = frame
        .weapons
        .iter()
        .filter(|weapon| owners.contains(&weapon.owner))
        .map(|weapon| weapon.resource)
        .collect();
    assert!(
        equipped.is_subset(&shown),
        "equipped models must be visible"
    );
    assert!(
        inventory.is_disjoint(&shown),
        "inventory artwork must stay detached"
    );
    assert!(frame.weapons.iter().all(|weapon| {
        weapon
            .bones
            .iter()
            .flatten()
            .flatten()
            .all(|v| v.is_finite())
    }));
    Ok(())
}
