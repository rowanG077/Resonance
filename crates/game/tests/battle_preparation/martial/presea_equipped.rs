use super::*;

#[test]
#[ignore = "requires current Presea and Crimson Devastation publications; strict CPU preparation only"]
fn crimson_devastation_prepares_the_current_axe_and_inventory_variants() -> Result<()> {
    let mut fixture = crate::all_party_encounter::ColdEncounter::load()?;
    for equipped in [242, 243] {
        let mut party = resonance_events::party::Party::new(&fixture.session, Default::default())?;
        party.formation = vec![7];
        party.field_leader = 7;
        party.settings.battle_controls = [0; 4];
        party
            .change_item(&fixture.session, 242, 1)
            .map_err(anyhow::Error::msg)?;
        party
            .change_item(&fixture.session, 243, 1)
            .map_err(anyhow::Error::msg)?;
        let member = &mut party.members[6];
        member.equipment[0] = equipped;
        member.techniques = [154, 261].into();
        member.disabled_techniques.clear();
        member.unison_arte = None;
        member.shortcuts = [154, 0, 0, 0];
        member.assist_shortcuts = [None; 2];
        let (_, prepared) = fixture.prepare(&party)?;
        let owner = prepared.results.actors[0].0;
        for item in [242, 243] {
            let weapons = prepared
                .core
                .models()
                .context("missing prepared party models")?
                .equipment_for_items(owner, [item, 0]);
            assert!(!weapons.is_empty(), "axe {item} was not prepared");
        }
        assert!(
            prepared
                .effect_banks
                .iter()
                .any(|bank| bank.member(1).is_some_and(|member| member
                    .particles
                    .values()
                    .filter(|particle| particle.model.is_some_and(|binding| binding.slot == 14))
                    .count()
                    == 2)),
            "Crimson Devastation's two equipped-axe particles were not admitted"
        );
        assert!(!prepared.core.is_diagnostic());
    }
    Ok(())
}
