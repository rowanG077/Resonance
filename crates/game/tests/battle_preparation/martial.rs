use super::*;
use resonance_battle::{Battle, Cue};

fn encounter(
    character: u8,
    learned: &[u16],
    overlimit: bool,
) -> Result<(Battle, resonance_battle::ActorId)> {
    let mut fixture = crate::all_party_encounter::ColdEncounter::load()?;
    let mut party = resonance_events::party::Party::new(&fixture.session, Default::default())?;
    party.formation = vec![character];
    party.field_leader = character;
    party.settings.battle_controls = [0; 4];
    party.settings.preferences.battle_rank = 2;
    for member in &mut party.members {
        member.techniques.clear();
        member.technique_uses.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
    }
    let member = &mut party.members[usize::from(character - 1)];
    member.techniques.extend(learned);
    member.shortcuts[0] = learned.first().copied().unwrap_or(0);
    member.base_stats[0] = 4096;
    member.hp = 4096;
    member.overlimit = if overlimit { 100 } else { 0 };
    let (_, prepared) = fixture.prepare(&party)?;
    let owner = prepared.results.actors[0].0;
    let mut battle = prepared.core;
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(true));
    enter_battle(&mut battle)?;
    Ok((battle, owner))
}

#[test]
#[ignore = "requires current native source-cooked Raine/resident effect owners"]
fn inspect_magic_225_prepares_as_an_enemy_targeted_scan() -> Result<()> {
    let (battle, owner) = encounter(4, &[225], false)?;
    let technique = battle
        .prepared_technique(owner, resonance_battle::TechniqueId(225))
        .context("Inspect Magic has no prepared casting action")?;
    let enemy = battle
        .actors()
        .iter()
        .position(|actor| actor.side == resonance_battle::Side::Enemy)
        .context("missing scan target")?;
    assert!(battle.technique_target_eligible(
        owner,
        technique.action,
        resonance_battle::ActorId::from_index(enemy)?
    ));
    assert!(!battle.technique_target_eligible(owner, technique.action, owner));
    Ok(())
}

#[test]
#[ignore = "requires current source-cooked Raine owners; works with either source library"]
fn raine_first_aid_prepares_without_admitting_an_unpublished_finisher() -> Result<()> {
    let (battle, owner) = encounter(4, &[98], false)?;
    assert!(
        battle
            .prepared_technique(owner, resonance_battle::TechniqueId(98))
            .is_some()
    );
    Ok(())
}

#[test]
#[ignore = "requires current source-cooked Grand Cross and Divine Judgment recipes"]
fn added_angel_spell_60_and_144_prepare_from_authored_scene_recipes() -> Result<()> {
    for (character, catalogue) in [(2, 60), (6, 144), (6, 124)] {
        let (battle, owner) = encounter(character, &[catalogue], false)?;
        assert!(
            battle
                .prepared_technique(owner, resonance_battle::TechniqueId(catalogue))
                .is_some()
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires current source-cooked Time Stop; optional RESONANCE_TEST_OVERLAY keeps canonical assets unchanged"]
fn time_stop_227_prepares_from_the_authored_scene_recipe() -> Result<()> {
    let (battle, owner) = encounter(4, &[227], false)?;
    assert!(
        battle
            .prepared_technique(owner, resonance_battle::TechniqueId(227))
            .is_some()
    );
    Ok(())
}

#[test]
#[ignore = "requires source-cooked Regal actions; optional RESONANCE_TEST_OVERLAY keeps canonical assets unchanged"]
fn added_regal_artes_prepare_from_native_action_tracks() -> Result<()> {
    let (battle, owner) = encounter(8, &[196, 198, 199, 200], false)?;
    for catalogue in [196, 198, 199, 200] {
        assert!(
            battle
                .prepared_technique(owner, resonance_battle::TechniqueId(catalogue))
                .is_some(),
            "missing Regal arte {catalogue}"
        );
    }
    Ok(())
}

#[path = "martial/attached.rs"]
mod attached;

#[path = "martial/destruction.rs"]
mod destruction;

#[path = "martial/mirage_201.rs"]
mod mirage_201;

#[path = "martial/eagle_dive.rs"]
mod eagle_dive;

#[path = "martial/presea_equipped.rs"]
mod presea_equipped;
