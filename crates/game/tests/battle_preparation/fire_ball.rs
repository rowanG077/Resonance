use super::*;
use resonance_battle::{Activity, BattlePhase, ContactSource, Cue};
use resonance_events::party::Party;

#[test]
#[ignore = "requires current party, encounter and audio publications; CPU only"]
fn prepared_genis_cast_pays_once_and_releases_a_damaging_volley() -> Result<()> {
    let mut fixture = super::all_party_encounter::ColdEncounter::load()?;
    let mut party = Party::new(&fixture.session, Default::default())?;
    party.formation = vec![1, 3, 2];
    party.settings.battle_controls = [1, 1, 1, 0];
    let (_, prepared) = fixture.prepare(&party)?;
    let owner = prepared.results.actors[1].0;
    let mut battle = prepared.core;
    for _ in 0..240 {
        if battle.phase() == BattlePhase::Combat && battle.activity(owner) == Activity::Idle {
            break;
        }
        battle.step(Default::default())?;
    }
    const FIRE_BALL: u16 = 66;
    let action = battle.record_technique_acquisition(owner, FIRE_BALL)?;
    let target = battle.target(owner).context("Fire Ball has no target")?;
    let cost = battle
        .technique_tp_cost(owner, action)
        .context("missing prepared spell cost")?;
    let tp = battle.actors()[owner.index()].tp;
    let uses = battle.technique_uses(owner, FIRE_BALL).unwrap_or(0);
    let mut released = None;
    let mut projectiles = std::collections::BTreeSet::new();
    let mut hit = false;
    for tick in 0..600 {
        let frame = battle.step(BattleInput {
            actions: (tick == 0)
                .then_some(ActionRequest {
                    actor: owner,
                    target,
                    action,
                })
                .into_iter()
                .collect(),
            ..Default::default()
        })?;
        for cue in &frame.cues {
            match *cue {
                Cue::Released {
                    actor,
                    action,
                    parent: Some(_),
                    ..
                } if actor == owner => {
                    assert!(released.replace(action).is_none(), "cast released twice");
                }
                Cue::ProjectileStarted { projectile, action } if Some(action) == released => {
                    projectiles.insert(projectile);
                }
                Cue::Hit {
                    source: ContactSource::Projectile(projectile),
                    result,
                    ..
                } if projectiles.contains(&projectile) => hit |= result.hp_change < 0,
                _ => {}
            }
        }
        if projectiles.len() == 3 && hit {
            break;
        }
    }
    assert!(
        hit && projectiles.len() == 3,
        "prepared Fire Ball did not release its damaging volley"
    );
    assert_eq!(
        u32::from(battle.actors()[owner.index()].tp),
        u32::from(tp) - cost
    );
    assert_eq!(battle.technique_uses(owner, FIRE_BALL), Some(uses + 1));
    Ok(())
}
