use super::*;
use resonance_battle::{Battle, Cue};

fn encounter(character: u8, learned: &[u16]) -> Result<(Battle, resonance_battle::ActorId)> {
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
    let (_, prepared) = fixture.prepare(&party)?;
    let owner = prepared.results.actors[0].0;
    let mut battle = prepared.core;
    battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(true));
    for _ in 0..240 {
        if battle.phase() != resonance_battle::BattlePhase::Entry {
            return Ok((battle, owner));
        }
        battle.step(BattleInput::default())?;
    }
    bail!("entry did not complete")
}

#[path = "martial/attached.rs"]
mod attached;

#[path = "martial/destruction.rs"]
mod destruction;

#[path = "martial/mirage_201.rs"]
mod mirage_201;

#[path = "martial/eagle_dive.rs"]
mod eagle_dive;
