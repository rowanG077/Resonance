//! Repeated real contact deaths exercise prepared models and shared revival ownership.
use super::*;
use crate::battle::encounter::{Assets, PrepareOptions};
use resonance_battle::{
    ActionRequest, Activity, BattleClock, BattleFrame, BattleInput, BattlePhase, Cue,
};
use resonance_content::menu_data::MenuData;

pub(super) fn check_repeated_contact_deaths(
    assets: &Assets,
    menus: &MenuData,
    options: impl Fn() -> PrepareOptions,
    roles: &DefenseSystemDefinition,
) -> Result<()> {
    let run = || -> Result<Vec<BattleFrame>> {
        let leader = ActorId::from_index(0)?;
        let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
            &mut assets
                .files
                .library()
                .context("named library required")?
                .snapshot(),
            &|| false,
        )?;
        let mut selected = resonance_content::battle_audio::Selection::default();
        let mut prepared = assets.inputs.prepare_fixture(
            menus,
            options(),
            |sound| catalogue.collect(&mut selected, assets.files.diagnostics(), sound),
            |actors| {
                actors[leader.index()].0.equipment.max_tp = 999;
                actors[leader.index()].0.tp = 999;
                Ok(())
            },
        )?;
        let battle = &mut prepared.core;
        battle.suppress_automatic_decisions();
        let fades_on_defeat = battle
            .models()
            .context("models missing")?
            .definition(roles.repair.0)
            .context("repair model missing")?
            .fade_on_defeat;
        let attack =
            battle.record_technique_acquisition(leader, resonance_battle::TechniqueId(1))?;
        // Keep the original attack geometry and death flow, shortening only damage.
        battle.set_instant_kill(true);
        let mut frames: Vec<BattleFrame> = Vec::new();
        let mut combat_ticks = 0;
        let mut paused = 0;
        let mut attacks = 0;
        let mut deaths = Vec::new();
        let mut revivals = Vec::new();
        let mut core_dead = None;
        let mut fully_faded = false;
        let mut retained_dead_body = false;
        let mut contacts = Vec::new();
        for _ in 0..1800 {
            let pause = deaths.len() == 1 && revivals.is_empty() && paused < 5;
            paused += u32::from(pause);
            let next_attack = match attacks {
                0 => Some(roles.repair.0),
                1 | 2
                    if revivals.len() == attacks && combat_ticks > revivals[attacks - 1] + 140 =>
                {
                    Some(if attacks == 1 {
                        roles.repair.0
                    } else {
                        roles.core
                    })
                }
                _ => None,
            };
            let request = if !pause
                && battle.phase() == BattlePhase::Combat
                && !battle.is_paused()
                && frames
                    .last()
                    .is_some_and(|frame| frame.combat_active && frame.clock == BattleClock::Running)
                && battle.activity(leader) == Activity::Idle
                && let Some(target) = next_attack
            {
                let position = battle.actors()[target.index()].position;
                battle.restore_fixture_placement(
                    leader,
                    [position[0] - 180., position[1], position[2]],
                    Some(90.),
                )?;
                battle.restore_fixture_target(leader, target)?;
                attacks += 1;
                Some(ActionRequest {
                    actor: leader,
                    target,
                    action: attack,
                })
            } else {
                None
            };
            let mut frame = battle.step(BattleInput {
                paused: pause,
                actions: request.into_iter().collect(),
                ..Default::default()
            })?;
            combat_ticks += u32::from(frame.combat_active && frame.clock == BattleClock::Running);
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, true)?;
            let model = frame
                .models
                .iter()
                .find(|m| m.actor == roles.repair.0)
                .context("repair model missing")?;
            fully_faded |= !model.visible && model.tint[3] == 0;
            retained_dead_body |= frame.actors[roles.repair.0.index()].availability
                == resonance_battle::ActorAvailability::Dead
                && model.visible
                && model.tint[3] != 0;
            for cue in &frame.cues {
                if matches!(cue, Cue::Hit { .. } | Cue::Rejected { .. }) {
                    contacts.push((combat_ticks, cue.clone()));
                }
                match cue {
                    Cue::Defeated { actor } if *actor == roles.repair.0 => {
                        deaths.push(combat_ticks)
                    }
                    Cue::Defeated { actor } if *actor == roles.core => {
                        core_dead = Some(frame.update)
                    }
                    Cue::Recovered { actor, .. } if *actor == roles.repair.0 => {
                        assert!(model.visible && model.tint[3] != 0 && model.depth_write);
                        revivals.push(combat_ticks);
                    }
                    _ => (),
                }
            }
            if core_dead.is_some_and(|update| frame.update > update) {
                assert!(roles.components.iter().all(|id| {
                    let actor = &frame.actors[id.index()];
                    actor.hp == 0 && !actor.available()
                }));
                assert!(
                    !frame
                        .cues
                        .iter()
                        .any(|cue| matches!(cue, Cue::Recovered { .. }))
                );
            }
            let finished = core_dead.is_some_and(|update| frame.update >= update + 90);
            frames.push(frame);
            if finished {
                break;
            }
        }
        assert_eq!(
            attacks, 3,
            "contact lifecycle did not reach core defeat: deaths={deaths:?}, revivals={revivals:?}, contacts={contacts:?}"
        );
        assert_eq!(deaths.len(), 2, "both attacks must defeat the repair unit");
        assert_eq!(revivals.len(), 2, "both contact deaths must revive");
        assert_eq!(revivals[0] - deaths[0], 60);
        assert_eq!(revivals[1] - deaths[1], 80);
        assert!(
            core_dead.is_some(),
            "third attack must defeat the core: {contacts:?}"
        );
        if fades_on_defeat {
            assert!(
                fully_faded,
                "repair body must finish its declared death fade"
            );
        } else {
            assert!(
                retained_dead_body,
                "declared retained body must stay visible while dead"
            );
        }
        assert_eq!(paused, 5);
        assert!(!assets.files.diagnostics().has_errors() && !battle.is_diagnostic());
        Ok(frames)
    };
    let first = run()?;
    let second = run()?;
    assert_eq!(
        first.len(),
        second.len(),
        "prepared replay duration diverged"
    );
    for (index, (left, right)) in first.iter().zip(&second).enumerate() {
        assert!(
            left == right,
            "prepared lifecycle replay diverged at frame {index}"
        );
    }
    Ok(())
}
