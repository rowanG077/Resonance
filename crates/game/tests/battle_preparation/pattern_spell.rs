//! Exercise cooked Ray through ordinary casting and the independent release lifecycle.
#![cfg(feature = "test-support")]
use super::*;
use resonance_battle::{ActorId, ContactSource, Cue, TechniqueId};
use resonance_events::party::Party;

#[derive(Debug, PartialEq)]
struct Trace {
    shots: Vec<u32>,
    completed: Option<u32>,
    hits: usize,
    random_state: u64,
}

fn run(cancel_before: bool, cancel_after: bool, miss: bool) -> Result<Trace> {
    let mut fixture = super::all_party_encounter::ColdEncounter::load()?;
    let mut party = Party::new(&fixture.session, Default::default())?;
    party.formation = vec![4, 1];
    party.field_leader = 4;
    party.settings.battle_controls = [1; 4];
    party.members[3].base_stats[1] = 999;
    party.members[3].tp = 999;
    let (assets, prepared) = fixture.prepare(&party)?;
    let owner = prepared.results.actors[0].0;
    let mut battle = prepared.core;
    battle.suppress_automatic_decisions();
    enter_battle(&mut battle)?;
    for index in 0..battle.actors().len() {
        let id = ActorId::from_index(index)?;
        let row = &battle.actors()[index];
        let (tp, max_tp) = (row.tp, row.equipment.max_tp);
        battle.set_actor_vitals(id, 1_000_000, 1_000_000, tp, max_tp)?;
    }
    let action = battle.record_technique_acquisition(owner, TechniqueId(114))?;
    let target = battle.target(owner).context("Ray target missing")?;
    let cost = battle
        .technique_tp_cost(owner, action)
        .context("Ray cost missing")?;
    let initial_tp = battle.actors()[owner.index()].tp;
    let mut parent = None;
    let mut release = None;
    let mut release_tick = None;
    let mut shots = Vec::new();
    let mut projectiles = std::collections::BTreeSet::new();
    let mut completed = None;
    let mut hits = 0;
    for tick in 0..1800 {
        if miss && release_tick.is_some_and(|at| tick == at + 70) {
            // Ray can hit any enemy in its fixed pattern, not only the selected target.
            let enemies: Vec<_> = battle
                .actors()
                .iter()
                .enumerate()
                .filter_map(|(index, actor)| {
                    (actor.side == resonance_battle::Side::Enemy).then_some(index)
                })
                .collect();
            for index in enemies {
                battle.restore_fixture_placement(
                    ActorId::from_index(index)?,
                    [10000., 0., 10000.],
                    None,
                )?;
            }
        }
        let interrupt = (cancel_before && tick == 1)
            || (cancel_after && release_tick.is_some_and(|at| tick == at + 1));
        let frame = battle.step(BattleInput {
            actions: (tick == 0)
                .then_some(ActionRequest {
                    actor: owner,
                    target,
                    action,
                })
                .into_iter()
                .collect(),
            interrupt: if interrupt {
                parent.into_iter().collect()
            } else {
                vec![]
            },
            ..Default::default()
        })?;
        for cue in &frame.cues {
            match *cue {
                Cue::Started {
                    action: handle,
                    actor,
                    ..
                } if actor == owner && parent.is_none() => parent = Some(handle),
                Cue::Released {
                    action: handle,
                    actor,
                    ..
                } if actor == owner => {
                    assert!(release.replace(handle).is_none(), "Ray released twice");
                    release_tick = Some(tick);
                }
                Cue::ProjectileStarted {
                    projectile, action, ..
                } if Some(action) == release => {
                    shots.push(tick - release_tick.unwrap());
                    projectiles.insert(projectile);
                }
                Cue::Hit {
                    source: ContactSource::Projectile(id),
                    ..
                } if projectiles.contains(&id) => hits += 1,
                Cue::Completed { action } if Some(action) == release => {
                    completed = Some(tick - release_tick.unwrap())
                }
                _ => {}
            }
        }
        let paid = if release.is_some() { cost } else { 0 };
        assert_eq!(
            u32::from(battle.actors()[owner.index()].tp),
            u32::from(initial_tp) - paid
        );
        if completed.is_some() || (cancel_before && tick == 30) {
            break;
        }
    }
    if cancel_before {
        assert!(parent.is_some() && release.is_none() && shots.is_empty());
    } else {
        assert_eq!(shots, [123, 131, 139, 147, 155, 163, 171, 179, 187]);
        assert_eq!(completed, Some(293));
        assert_eq!(battle.action_age(release.unwrap()), None);
        if miss {
            assert_eq!(hits, 0);
        } else {
            assert!(hits > 0);
        }
    }
    assert!(!battle.is_diagnostic() && !assets.files.diagnostics().has_errors());
    Ok(Trace {
        shots,
        completed,
        hits,
        random_state: battle.random_state(),
    })
}

#[test]
#[ignore = "requires a full current cook at RESONANCE_TEST_ASSETS; CPU only"]
fn prepared_ray_pays_at_release_keeps_pattern_after_cancellation_and_repeats_rng() -> Result<()> {
    run(true, false, false)?;
    let normal = run(false, false, false)?;
    assert_eq!(normal, run(false, false, false)?);
    let detached = run(false, true, true)?;
    assert_eq!(detached.shots, normal.shots);
    assert_eq!(detached.completed, normal.completed);
    Ok(())
}
