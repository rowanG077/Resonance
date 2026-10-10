//! Candidate accounting tests isolate known membership while retaining real action admission.
use super::item_tests::{
    Display, PreparedFixture, enter_battle, escape, prepared_fixture_with_party,
};
use super::*;
use crate::battle::{command, lifecycle};
use crate::menu::MenuAction;
use resonance_battle::{ActionRequest, Activity, BattleFrame, BattleInput, ButtonInput};

fn fixture(formation: &[u8], initial: u16, previous: Option<&Party>) -> Result<PreparedFixture> {
    prepared_fixture_with_party(formation, 2, &[(1, 1), (3, 66)], |party, _, _| {
        if let Some(previous) = previous {
            *party = previous.clone();
        }
        party.formation = formation.to_vec();
        party.field_leader = formation[0];
        party.settings.battle_controls = [0; 4];
        // Keep durable battle stats while limiting level-based learning.
        party.members[0].level = 1;
        party.members[2].level = 2;
        party.members[0].techniques = [1].into();
        party.members[2].techniques = [66].into();

        if previous.is_none() {
            party.members[0].technique_uses.insert(1, 17);
            party.members[2].technique_uses.insert(66, initial);
            party.members[2].technique_uses.insert(67, 432); // Forgotten history, never bound.
        } else {
            // The next preparation must read the committed serialized totals;
            // do not repair persistence using this fixture's expectation.
            assert_eq!(party.members[0].technique_uses[&1], 17);
            assert_eq!(party.members[2].technique_uses[&66], initial);
            assert_eq!(party.members[2].technique_uses[&67], 432);
        }
        Ok(())
    })
}

fn genis(f: &PreparedFixture) -> ActorId {
    f.candidate
        .setup
        .actors
        .iter()
        .find(|(_, character)| *character == 3)
        .unwrap()
        .0
}

fn command_visit(
    f: &mut PreparedFixture,
    command_input: command::Input,
) -> Result<(BattleFrame, Vec<command::Event>)> {
    let controllers = f
        .lifecycle
        .command_frame()
        .map(|frame| resonance_battle::ControlInput {
            target_step: command_input.step,
            ..resonance_battle::ControlInput::neutral(frame.actor)
        })
        .into_iter()
        .collect();
    let frame = f.lifecycle.step(
        &mut f.battle,
        lifecycle::Input {
            command: command_input,
            battle: BattleInput {
                controllers,
                ..Default::default()
            },
            ..Default::default()
        },
        &mut f.candidate,
        &mut Display,
    )?;
    Ok((frame, f.lifecycle.take_command_events()))
}

fn selected_tech_target(fixture: &PreparedFixture) -> Option<ActorId> {
    match fixture.lifecycle.command_frame()?.view {
        command::View::TechTarget { target } => target,
        _ => None,
    }
}

fn edge() -> ButtonInput {
    ButtonInput {
        held: true,
        pressed: true,
        released: false,
    }
}

fn release_fire_ball(f: &mut PreparedFixture, initial: u16) -> Result<()> {
    enter_battle(&mut f.candidate, &mut f.battle)?;
    let actor = genis(f);
    let request = ActionRequest {
        actor,
        target: f.battle.target(actor).context("missing Fire Ball target")?,
        action: f.battle.prepared_technique(actor, 66).unwrap().action,
    };
    let expected = (initial + 1).min(999);
    let mut chanted = false;
    for update in 0..600 {
        f.candidate.world_update(
            &mut f.battle,
            BattleInput {
                actions: if update == 0 { vec![request] } else { vec![] },
                ..Default::default()
            },
        )?;
        // Live usage is immediately observable without publishing a Party snapshot.
        let total = f.battle.technique_uses(actor, 66).unwrap();
        assert_eq!(f.candidate.party.members[2].technique_uses[&66], initial);
        assert!(
            !f.candidate.party.members[2]
                .technique_uses
                .contains_key(&204)
        );
        assert_eq!(f.candidate.party.members[2].technique_uses[&67], 432);
        assert!(!f.battle.is_diagnostic());
        chanted |= matches!(f.battle.activity(actor), Activity::Casting { .. });
        if total > initial {
            assert!(chanted);
            assert_eq!(total, expected);
            // Casting snapshots the prior total; release does not update it.
            assert_eq!(
                f.battle.actors()[actor.index()].proficiency,
                (initial / 50).min(5) as u8
            );
            return Ok(());
        }
        assert_eq!(total, initial);
    }
    anyhow::bail!("actual Fire Ball did not reach the ordinary release counter")
}

#[test]
#[ignore = "requires cooked technique/profile assets; CPU only"]
fn tech_command_selector_owns_target_lifetime_and_live_confirmation() -> Result<()> {
    let mut fixture = prepared_fixture_with_party(&[1, 3], 2, &[(1, 1)], |party, _, _| {
        // The command channel also owns targeting for an automatic actor.
        party.settings.battle_controls = [2, 0, 0, 0];
        party.members[0].shortcuts[0] = 1;
        Ok(())
    })?;
    enter_battle(&mut fixture.candidate, &mut fixture.battle)?;
    let actor = fixture
        .candidate
        .setup
        .actors
        .iter()
        .find(|(_, character)| *character == 1)
        .map(|&(actor, _)| actor)
        .context("Tech fixture lost the Auto caster")?;
    command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            open: edge(),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        fixture.lifecycle.command_frame().unwrap().view,
        command::View::Strip
    ));
    command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            confirm_a: edge(),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        fixture.lifecycle.command_frame().unwrap().view,
        command::View::Tech(_)
    ));

    // Drive the real Candidate-backed page to its positive enemy-target exit.
    let mut target_frame = None;
    for visit in 0..100 {
        let (frame, _) = command_visit(
            &mut fixture,
            command::Input {
                controller: 0,
                shared_menu: (visit >= 20).then_some(MenuAction::Confirm),
                ..Default::default()
            },
        )?;
        if frame.target_selector == Some(actor) {
            target_frame = Some(frame);
            break;
        }
    }
    assert!(
        target_frame.is_some(),
        "real Tech page did not enter target state"
    );
    let ordinary_target = fixture
        .battle
        .target(actor)
        .context("Tech caster has no ordinary target")?;
    let first_tech_target =
        selected_tech_target(&fixture).context("Tech page has no selected target")?;
    assert!(matches!(
        fixture.lifecycle.command_frame().unwrap().view,
        command::View::TechTarget { .. }
    ));

    // Neutral frames keep the command-owned selector alive and do not change
    // the ordinary controller target.
    let (neutral, _) = command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            ..Default::default()
        },
    )?;
    assert_eq!(neutral.target_selector, Some(actor));
    assert_eq!(fixture.battle.target(actor), Some(ordinary_target));

    // Direction and A share one command visit.  Direction is applied before
    // confirmation, including for an Auto owner whose BattleInput omits it.
    let (cycled, _) = command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            step: 1,
            ..Default::default()
        },
    )?;
    let cycled_target =
        selected_tech_target(&fixture).context("Tech page lost its cycled target")?;
    assert!(cycled.target_selector == Some(actor));
    assert_ne!(cycled_target, first_tech_target);
    assert_eq!(fixture.battle.target(actor), Some(ordinary_target));

    let expected = fixture
        .battle
        .select_target_step(actor, Some(cycled_target), 1);
    let (second_cycle, _) = command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            step: 1,
            ..Default::default()
        },
    )?;
    assert_eq!(selected_tech_target(&fixture), expected);
    assert_eq!(second_cycle.targets[actor.index()], expected);
    assert_eq!(fixture.battle.target(actor), Some(ordinary_target));

    let (cancelled, events) = command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            cancel_b: edge(),
            ..Default::default()
        },
    )?;
    assert!(events.contains(&command::Event::Cue(3)));
    assert!(cancelled.target_selector.is_none());
    assert!(matches!(
        fixture.lifecycle.command_frame().unwrap().view,
        command::View::Strip
    ));
    assert_eq!(fixture.battle.target(actor), Some(ordinary_target));
    assert_eq!(selected_tech_target(&fixture), None);

    // Reopen the page and enter the target state again, then make the live TP
    // admission fail.  A failed A leaves the temporary selector open.
    command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            confirm_a: edge(),
            ..Default::default()
        },
    )?;
    for visit in 0..100 {
        let (frame, _) = command_visit(
            &mut fixture,
            command::Input {
                controller: 0,
                shared_menu: (visit >= 20).then_some(MenuAction::Confirm),
                ..Default::default()
            },
        )?;
        if frame.target_selector == Some(actor) {
            break;
        }
    }
    fixture.battle.set_actor_vitals(actor, 100, 100, 0, 30)?;
    let (low_tp, denied_events) = command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            confirm_a: edge(),
            ..Default::default()
        },
    )?;
    assert_eq!(low_tp.target_selector, Some(actor));
    assert!(denied_events.contains(&command::Event::Cue(4)));
    assert!(!denied_events.contains(&command::Event::Cue(2)));
    assert_eq!(fixture.battle.pending_technique_target(actor), None);

    // Restore TP and confirm with a same-visit direction.  The successful
    // target confirmation closes to combat and records the caster's physical
    // controller as issuer.
    fixture.battle.set_actor_vitals(actor, 100, 100, 30, 30)?;
    let pre_confirm_target =
        selected_tech_target(&fixture).context("Tech page lost its pre-confirm target")?;
    let (closed, events) = command_visit(
        &mut fixture,
        command::Input {
            controller: 0,
            step: -1,
            confirm_a: edge(),
            ..Default::default()
        },
    )?;
    assert!(closed.target_selector.is_none());
    assert!(fixture.lifecycle.command_frame().is_none());
    assert!(events.contains(&command::Event::VoiceStreamsPaused(false)));
    assert_eq!(fixture.battle.pending_technique_issuer(actor), Some(0));
    let queued_target = fixture
        .battle
        .pending_technique_target(actor)
        .context("successful Tech confirmation did not retain a target")?;
    assert_ne!(queued_target, pre_confirm_target);
    assert_eq!(fixture.battle.target(actor), Some(ordinary_target));
    let action = fixture.battle.prepared_technique(actor, 1).unwrap().action;
    assert!(!fixture.battle.queue_technique(actor, action)?);
    assert_eq!(
        fixture.battle.pending_technique_target(actor),
        Some(queued_target)
    );
    Ok(())
}

#[test]
#[ignore = "requires current prepared battle assets; CPU only"]
fn learned_technique_appears_on_menu_entry_and_preserves_later_user_edits() -> Result<()> {
    use crate::menu::techniques::Edit;
    let mut f = prepared_fixture_with_party(&[8, 1], 2, &[], |_, _, _| Ok(()))?;
    enter_battle(&mut f.candidate, &mut f.battle)?;
    let actor = f.candidate.setup.actors[0].0;
    let action = f.battle.record_technique_acquisition(actor, 201)?;
    f.battle.forget_technique(actor, action)?;
    f.candidate.begin_tech(&mut f.battle, 0, [false; 4])?;
    assert!(!f.candidate.party.members[7].techniques.contains(&201));
    assert_eq!(f.candidate.party.members[7].shortcuts, [0; 4]);
    f.battle.record_technique_acquisition(actor, 201)?;
    f.candidate
        .world_update(&mut f.battle, BattleInput::default())?;
    assert_eq!(f.battle.technique_is_current(actor, 201), Some(true));
    assert_eq!(f.battle.shortcuts(actor).unwrap()[0], 201);
    assert!(!f.candidate.party.members[7].techniques.contains(&201));

    let page = f.candidate.begin_tech(&mut f.battle, 0, [false; 4])?;
    let view = f
        .candidate
        .battle_tech_view(&page.state, &page.connected, &f.battle);
    assert_eq!(view.selected_technique().unwrap().technique, 201);
    assert!(view.available(7, 201));
    for edit in [
        Edit::Shortcut {
            member: 7,
            slot: 0,
            selected: None,
        },
        Edit::Enabled {
            member: 7,
            technique: 201,
            enabled: false,
        },
    ] {
        Candidate::apply_tech_edit(
            &f.candidate.setup,
            &mut f.battle,
            &mut f.candidate.party,
            edit,
        )?;
    }
    f.candidate.begin_tech(&mut f.battle, 0, [false; 4])?;
    assert_eq!(f.battle.shortcuts(actor).unwrap()[0], 0);
    assert_eq!(f.candidate.party.members[7].shortcuts[0], 0);
    assert!(
        f.candidate.party.members[7]
            .disabled_techniques
            .contains(&201)
    );
    let outcome = escape(&mut f.candidate, &mut f.battle)?;
    let completed = f.candidate.finish(&f.battle, &outcome)?;
    let saved: Party = serde_json::from_slice(&serde_json::to_vec(&completed.party)?)?;
    assert!(saved.members[7].techniques.contains(&201));
    assert!(saved.members[7].disabled_techniques.contains(&201));
    assert_eq!(saved.members[7].shortcuts[0], 0);
    assert_eq!(saved.members[7].technique_uses[&201], 0);
    let next = prepared_fixture_with_party(&[8, 1], 2, &[], |party, _, _| {
        *party = saved;
        Ok(())
    })?;
    let actor = next.candidate.setup.actors[0].0;
    let action = next.battle.prepared_technique(actor, 201).unwrap().action;
    assert_eq!(next.battle.technique_is_current(actor, 201), Some(true));
    assert_eq!(next.battle.shortcuts(actor), Some(&[0; 4]));
    assert!(!next.battle.technique_enabled(actor, action));
    assert_eq!(next.battle.technique_uses(actor, 201), Some(0));
    Ok(())
}

#[test]
#[ignore = "requires cooked technique/profile assets; CPU only"]
fn usage_projects_at_menu_and_finish_boundaries_across_two_battles() -> Result<()> {
    let mut saved = None;
    for (battle_index, formation) in [[1, 3], [3, 1]].into_iter().enumerate() {
        let initial = 49 + battle_index as u16;
        let mut f = fixture(&formation, initial, saved.as_ref())?;
        let actor = genis(&f);
        assert_eq!(actor.index(), 1 - battle_index);
        release_fire_ball(&mut f, initial)?;
        let total = initial + 1;
        let ledger = f.battle.ledger().clone();
        for _ in 0..3 {
            f.candidate.begin_strategy(&f.battle)?;
            assert_eq!(f.candidate.party.members[2].technique_uses[&66], total);
        }
        assert_eq!(f.battle.ledger(), &ledger);
        assert_eq!(f.field.members[2].technique_uses[&66], initial);
        let outcome = escape(&mut f.candidate, &mut f.battle)?;
        let completed = f.candidate.finish(&f.battle, &outcome)?;
        assert_eq!(completed.party.members[2].technique_uses[&66], total);
        assert_eq!(completed.party.members[2].technique_uses[&67], 432);
        if battle_index == 0 {
            let mut field = super::strategy_tests::field_call(f.field)?;
            let request = field
                .world
                .battle_request
                .take()
                .context("missing field caller")?;
            let duplicate = Completed {
                party: completed.party.clone(),
                gameplay_random: completed.gameplay_random,
                result: completed.result,
            };
            completed.commit(&mut field.world, &request)?;
            let bytes = serde_json::to_vec(field.world.party.as_ref().unwrap())?;
            assert!(duplicate.commit(&mut field.world, &request).is_err());
            assert_eq!(
                serde_json::to_vec(field.world.party.as_ref().unwrap())?,
                bytes
            );
            saved = Some(serde_json::from_slice(&bytes)?);
        } else {
            saved = Some(completed.party);
        }
    }
    assert_eq!(saved.unwrap().members[2].technique_uses[&66], 51);
    Ok(())
}

#[test]
#[ignore = "requires cooked technique/profile assets; CPU only"]
fn failed_vital_snapshot_does_not_publish_technique_counts() -> Result<()> {
    let mut f = fixture(&[3, 1], 49, None)?;
    // Deliberately stale projection rows expose accidental partial writes; no
    // source event is fabricated and the immutable core totals remain49/17.
    f.candidate.party.members[2].technique_uses.insert(66, 1);
    f.candidate.party.members[0].technique_uses.insert(1, 2);
    let before = serde_json::to_value(&f.candidate.party)?;
    let late_id = f.candidate.setup.actors[1].0;
    let late = f.battle.actors()[late_id.index()].clone();
    f.battle
        .set_actor_vitals(late_id, 65_536, 65_536, late.tp, late.equipment.max_tp)?;
    assert!(f.candidate.sync_party(&f.battle).is_err());
    assert_eq!(serde_json::to_value(&f.candidate.party)?, before);
    f.battle.set_actor_vitals(
        late_id,
        late.hp,
        late.equipment.max_hp,
        late.tp,
        late.equipment.max_tp,
    )?;
    f.candidate.sync_party(&f.battle)?;
    assert_eq!(f.candidate.party.members[2].technique_uses[&66], 49);
    assert_eq!(f.candidate.party.members[0].technique_uses[&1], 17);
    assert_eq!(f.candidate.party.members[2].technique_uses[&67], 432);
    Ok(())
}
