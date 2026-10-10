use super::*;

fn prepared_shortcuts(mode: Control) -> Battle {
    shortcuts_prepared(mode).finish().unwrap()
}

#[test]
fn tech_live_mode_and_queue_edits_preserve_the_current_battle_prefix() -> Result<()> {
    let mut battle = prepared_shortcuts(Control::Manual);
    let snapshot = battle.snapshot();

    let ledger = battle.ledger().clone();

    battle.set_control_mode(ActorId(0), Control::SemiAuto)?;
    assert_eq!(battle.actors[0].control, Control::SemiAuto);
    let mut expected = snapshot;
    expected.actors[0].control = Control::SemiAuto;
    assert_eq!(battle.snapshot(), expected);
    assert_eq!(battle.ledger(), &ledger);

    assert!(battle.queue_technique(ActorId(0), crate::ActionKey(7))?);
    assert!(!battle.queue_technique(ActorId(0), crate::ActionKey(7))?);
    assert_eq!(
        battle.publish(Vec::new()).cues,
        [Cue::TechniqueQueued { actor: ActorId(0) }]
    );
    assert_eq!(
        battle.pending_technique(ActorId(0)),
        Some(crate::ActionKey(7))
    );
    assert!(
        battle
            .validate_forget_technique(ActorId(0), crate::ActionKey(7))
            .is_err()
    );

    let mut fresh = prepared_shortcuts(Control::Manual);
    fresh
        .runtime
        .get_mut(0)
        .and_then(|state| state.control.as_mut())
        .unwrap()
        .disabled_techniques
        .insert(crate::ActionKey(7));
    // The AI policy toggle does not reject an explicit player command.
    assert!(fresh.queue_technique(ActorId(0), crate::ActionKey(7))?);
    assert!(fresh.set_control_mode(ActorId(0), Control::Auto).is_err());
    assert!(
        fresh
            .set_control_mode(ActorId(u8::MAX), Control::Manual)
            .is_err()
    );
    Ok(())
}

#[test]
fn manual_tech_queue_does_not_bypass_the_player_callback() -> Result<()> {
    let mut battle = prepared_shortcuts(Control::Manual);
    assert!(battle.queue_technique(ActorId(0), crate::ActionKey(7))?);
    for _ in 0..8 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(
        battle.pending_technique(ActorId(0)),
        Some(crate::ActionKey(7))
    );
    assert!(battle.sequences().next().is_none());
    assert_eq!(battle.actors[0].tp, 40);
    Ok(())
}

fn edit_only_slot(
    battle: &mut Battle,
    slot: usize,
    selected: Option<crate::ActionKey>,
) -> Result<()> {
    let frame = battle.snapshot();
    let ledger = battle.ledger().clone();
    let mut expected = *battle.shortcuts(ActorId(0)).unwrap();
    expected[slot] = selected.map_or(0, |action| {
        battle
            .technique_catalogue_for_action(ActorId(0), action)
            .unwrap()
    });
    battle
        .prepare_shortcut(ActorId(0), slot, selected)?
        .commit();
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&expected));
    assert_eq!(battle.snapshot(), frame);
    assert_eq!(battle.ledger(), &ledger);
    Ok(())
}

#[test]
fn shortcut_preflight_drop_clear_and_change_affect_only_later_fresh_input() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        for (slot, stick) in [[0, 0], [0, 80], [0, -80], [80, 0]].into_iter().enumerate() {
            let mut battle = prepared_shortcuts(mode);
            let replacement = crate::ActionKey(7 + (slot + 1) % 4);
            let before = *battle.shortcuts(ActorId(0)).unwrap();
            let frame = battle.snapshot();
            {
                let _edit = battle.prepare_shortcut(ActorId(0), slot, Some(replacement))?;
            }
            assert_eq!(battle.shortcuts(ActorId(0)), Some(&before));
            assert_eq!(battle.snapshot(), frame);
            edit_only_slot(&mut battle, slot, None)?;
            let empty = battle.step(player_buttons(false, true, false, stick))?;
            assert!(empty.actions.is_empty());
            assert_eq!(empty.actors[0].activity, Activity::Idle);
            assert_eq!(empty.actors[0].tp, 40);
            edit_only_slot(&mut battle, slot, Some(replacement))?;
            battle.step(player_buttons(false, true, false, stick))?;
            battle.step(BattleInput::default())?;
            assert_eq!(battle.sequence(&ActionId(1)).unwrap().action, replacement);
            assert_eq!(battle.actors[0].tp, 36);
        }
    }
    Ok(())
}

#[test]
fn invalid_live_shortcut_edits_are_atomic_and_do_not_replace_buffers() -> Result<()> {
    let mut battle = prepared_shortcuts(Control::Manual);
    battle.step(input([0; 2], true))?;
    battle.step(BattleInput::default())?;
    battle.step(player_buttons(false, true, false, [0; 2]))?;
    let allowed = crate::ActionKey(8);
    let frame = battle.snapshot();
    let shortcuts = *battle.shortcuts(ActorId(0)).unwrap();

    let ledger = battle.ledger().clone();
    for (actor, slot, value) in [
        (ActorId(u8::MAX), 0, Some(allowed)),
        (ActorId(1), 0, Some(allowed)),
        (ActorId(0), 4, None),
        (ActorId(0), usize::MAX, Some(allowed)),
        (ActorId(0), 0, Some(crate::ActionKey(0))),
    ] {
        assert!(battle.validate_shortcut(actor, slot, value).is_err());
        assert!(battle.prepare_shortcut(actor, slot, value).is_err());
        assert_eq!(battle.snapshot(), frame);
        assert_eq!(battle.shortcuts(ActorId(0)), Some(&shortcuts));
        assert_eq!(battle.ledger(), &ledger);
    }
    assert_eq!(battle.shortcuts(ActorId(1)), None);
    assert_eq!(battle.shortcuts(ActorId(u8::MAX)), None);
    let mut starts = 0;
    for _ in 0..30 {
        let frame = battle.step(BattleInput::default())?;
        for cue in frame.cues {
            if let Cue::Started {
                actor: ActorId(0),
                action,
                ..
            } = cue
            {
                assert_eq!(battle.action_definition(action), Some(crate::ActionKey(7)));
                starts += 1;
            }
        }
    }
    assert_eq!(
        starts, 1,
        "rejected edits lost or duplicated the buffered technique"
    );
    assert_eq!(battle.actors()[0].tp, 36);
    Ok(())
}

#[test]
fn prepared_shortcut_catalogue_checks_unassigned_rows_and_initial_membership() {
    for fault in 0..4 {
        let mut prepared = shortcuts_prepared(Control::Manual);
        // None of the malformed row cases can hide behind an empty live slot.
        Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts =
            [0; 4];
        let setup = &mut prepared.resources.actor_setup[0];
        match fault {
            0 => setup.techniques.push(setup.techniques[0]),
            1 => setup.techniques[0].action = crate::ActionKey(0),
            2 => setup.techniques[0].player_range[1] = f32::INFINITY,
            _ => {
                Arc::make_mut(setup.control.as_mut().unwrap()).shortcuts[0] =
                    setup.techniques.remove(0).catalogue;
            }
        }
        assert!(prepared.finish().is_err());
    }
}

#[test]
fn shortcut_edit_does_not_require_available_actor_but_rejects_results_phase() -> Result<()> {
    for availability in [
        crate::ActorAvailability::Dead,
        crate::ActorAvailability::Petrified,
    ] {
        let mut battle = prepared_shortcuts(Control::Auto);
        battle.actors[0].availability = availability;
        let replacement = crate::ActionKey(8);
        edit_only_slot(&mut battle, 0, Some(replacement))?;
        assert_eq!(battle.actors[0].availability, availability);
    }
    let mut battle = prepared_shortcuts(Control::Manual);
    battle.recognize_escape(true)?;
    assert_eq!(
        battle.recognize_result(),
        Some(crate::BattleResult::Escaped)
    );
    battle.retire_combat()?;
    let frame = battle.snapshot();
    assert!(battle.prepare_shortcut(ActorId(0), 0, None).is_err());
    assert_eq!(battle.snapshot(), frame);
    Ok(())
}

#[test]
fn unsupported_learned_shortcut_stays_occupied_through_acquisition_and_edits() -> Result<()> {
    let mut prepared = shortcuts_prepared(Control::Manual);
    Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts =
        [2, 34, 0, 0];
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[2, 34],
            &[(1, 0)],
        )])?
        .finish()?;
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[2, 34, 0, 0]));
    let frame = battle.step(player_buttons(false, true, false, [0; 2]))?;
    assert!(frame.actions.is_empty());
    assert_eq!(battle.actors[0].tp, 40);
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[2, 34, 0, 0]));

    assert_eq!(
        battle.record_technique_acquisition(ActorId(0), 1)?,
        crate::ActionKey(7)
    );
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[2, 34, 1, 0]));
    battle
        .prepare_shortcut(ActorId(0), 0, Some(crate::ActionKey(9)))?
        .commit();
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[34, 34, 1, 0]));
    battle.forget_technique(ActorId(0), crate::ActionKey(7))?;
    assert_eq!(battle.shortcuts(ActorId(0)), Some(&[34, 34, 0, 0]));
    Ok(())
}

#[test]
fn clearing_a_shortcut_does_not_stop_the_running_action_from_turning() -> Result<()> {
    let mut battle = prepared_shortcuts(Control::Manual);
    battle.step(player_buttons(false, true, false, [0; 2]))?;
    battle.step(BattleInput::default())?;
    battle.actors[0].heading = 0.;
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.prepare_shortcut(ActorId(0), 0, None)?.commit();
    let age = battle.sequence(&ActionId(1)).unwrap().age;
    for _ in 0..5 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.actors[0].heading, 90.);
    assert!(battle.sequence(&ActionId(1)).unwrap().age > age);
    Ok(())
}

#[test]
fn automatic_captured_action_is_not_reinterpreted_as_a_manual_slot() -> Result<()> {
    let mut battle = companion_prepared_for(2)
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[1],
            &[(1, 49)],
        )])?
        .finish()?;
    battle.start_actor_command(
        crate::ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(0),
            target: ActorId(1),
        },
        &mut vec![],
    )?;
    let selected = crate::ActionKey(7);
    edit_only_slot(&mut battle, 0, Some(selected))?;
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.actors[0].tp, 40);
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));
    edit_only_slot(&mut battle, 0, None)?;
    battle.confirm_actor_contact(ActorId(0));

    let frame = battle.step(BattleInput::default())?;
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Completed {
            action: ActionId(1)
        }
    )));
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|sequence| sequence.action == crate::ActionKey(7))
    );
    assert_eq!(battle.shortcuts(ActorId(0)).unwrap()[0], 0);
    assert_eq!(battle.actors[0].tp, 35);
    assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    for _ in 0..3 {
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].tp, 35);
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(50));
    }
    Ok(())
}

#[test]
fn outside_visit_preselection_does_not_bypass_auto_contact_admission() -> Result<()> {
    let mut battle = companion_battle(true);
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    assert!(battle.queue_companion_chain(ActorId(0), crate::ActionKey(7))?);

    let frame = battle.step(BattleInput::default())?;
    assert!(frame.cues.iter().all(|cue| !matches!(
        cue,
        Cue::Completed {
            action: ActionId(1)
        }
    )));
    assert!(battle.sequence(&ActionId(1)).is_some());
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .all(|sequence| sequence.action != crate::ActionKey(7))
    );
    assert_eq!(battle.actors[0].tp, 40);
    Ok(())
}

#[test]
fn saved_disabled_arte_can_be_reenabled_for_autonomous_combo() -> Result<()> {
    for enable in [false, true] {
        let mut prepared = companion_prepared_for(2);
        prepared.resources.actor_setup[0].techniques[0].catalogue = 35;
        prepared.resources.actor_setup[0]
            .disabled_techniques
            .insert(crate::ActionKey(7));
        let prepared =
            prepared.with_technique_learning_members(vec![crate::tests::counted_techniques(
                ActorId(0),
                &[35],
                &[],
            )])?;
        let mut battle = prepared.finish()?;
        assert!(!battle.technique_enabled(ActorId(0), crate::ActionKey(7)));
        battle.start_actor_command(
            crate::ActionRequest {
                actor: ActorId(0),
                action: crate::ActionKey(0),
                target: ActorId(1),
            },
            &mut vec![],
        )?;
        for _ in 0..15 {
            battle.step(BattleInput::default())?;
        }
        battle.confirm_actor_contact(ActorId(0));
        if enable {
            assert!(battle.set_technique_enabled(ActorId(0), crate::ActionKey(7), true)?);
            assert!(!battle.set_technique_enabled(ActorId(0), crate::ActionKey(7), true)?);
        }
        let tp = battle.actors[0].tp;
        let mut admitted = false;
        for _ in 0..4 {
            battle.step(BattleInput::default())?;
            admitted |= battle
                .sequences()
                .map(|(_, sequence)| sequence)
                .any(|sequence| sequence.action == crate::ActionKey(7));
        }
        assert_eq!(admitted, enable);
        assert_eq!(battle.actors[0].tp, tp - if enable { 5 } else { 0 });
    }
    Ok(())
}

#[test]
fn explicit_command_chains_even_when_autonomous_use_is_disabled() -> Result<()> {
    let mut battle = companion_battle(true);
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    battle.confirm_actor_contact(ActorId(0));
    battle.set_technique_enabled(ActorId(0), crate::ActionKey(7), false)?;
    assert!(battle.queue_technique(ActorId(0), crate::ActionKey(7))?);

    battle.step(BattleInput::default())?;
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|sequence| sequence.action == crate::ActionKey(7))
    );
    assert_eq!(battle.pending_technique(ActorId(0)), None);
    Ok(())
}

#[test]
fn explicit_martial_losing_tp_keeps_the_normal_and_queue() -> Result<()> {
    let mut battle = companion_battle(true);
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    battle.confirm_actor_contact(ActorId(0));
    assert!(battle.queue_technique(ActorId(0), crate::ActionKey(7))?);
    battle.actors[0].tp = 0;

    battle.step(BattleInput::default())?;
    assert!(battle.sequence(&ActionId(1)).is_some());
    assert_eq!(
        battle.pending_technique(ActorId(0)),
        Some(crate::ActionKey(7))
    );
    Ok(())
}

#[test]
fn explicit_tech_target_admission_checks_recipient_cost_and_duplicate_queue() -> Result<()> {
    let mut prepared = shortcuts_prepared(Control::Manual);
    prepared.resources.actor_setup[0].techniques[0]
        .capabilities
        .target = crate::TechniqueTarget::Ally;
    prepared.actors[1].side = Side::Party;
    prepared.actors[1].hp = 0;
    prepared.actors[1].availability = crate::ActorAvailability::Dead;
    prepared.targets = [2, 2, 0].map(ActorId).to_vec();
    let mut battle = prepared.finish()?;
    let owner = ActorId(0);
    let ally = ActorId(1);
    let action = crate::ActionKey(7);
    let before_tp = battle.actors()[0].tp;
    assert!(!battle.queue_technique_target(owner, action, ally)?);
    assert!(!battle.queue_technique_target(owner, action, ActorId(2))?);
    assert!(!battle.queue_technique_target(owner, action, ActorId(u8::MAX))?);
    assert_eq!(battle.pending_technique(owner), None);
    assert_eq!(battle.actors()[0].tp, before_tp);
    assert!(battle.queue_technique_target(owner, action, owner)?);
    assert!(!battle.queue_technique_target(owner, action, owner)?);
    assert_eq!(battle.pending_technique(owner), Some(action));
    assert_eq!(battle.pending_technique_target(owner), Some(owner));
    assert_eq!(
        battle.actors()[0].tp,
        before_tp,
        "queuing does not pay before action admission"
    );

    let mut prepared = shortcuts_prepared(Control::Manual);
    prepared.actors[0].tp = 0;
    let mut battle = prepared.finish()?;
    assert!(!battle.queue_technique_target(owner, action, ActorId(1))?);
    assert_eq!(battle.pending_technique(owner), None);
    Ok(())
}

#[test]
fn revival_tech_target_accepts_only_fallen_allies() -> Result<()> {
    let mut prepared = shortcuts_prepared(Control::Manual);
    let capabilities = &mut prepared.resources.actor_setup[0].techniques[0].capabilities;
    capabilities.target = crate::TechniqueTarget::Ally;
    capabilities.revives = true;
    prepared.actors[1].side = Side::Party;
    prepared.actors[1].hp = 0;
    prepared.actors[1].availability = crate::ActorAvailability::Dead;
    prepared.targets = [2, 2, 0].map(ActorId).to_vec();
    let mut battle = prepared.finish()?;
    assert!(!battle.queue_technique_target(ActorId(0), crate::ActionKey(7), ActorId(0))?);
    assert!(battle.queue_technique_target(ActorId(0), crate::ActionKey(7), ActorId(1))?);
    Ok(())
}

#[test]
fn support_commands_start_on_the_selected_recipient_without_enemy_approach() -> Result<()> {
    for (target_kind, recipient, revive) in [
        (crate::TechniqueTarget::Ally, ActorId(1), false),
        (crate::TechniqueTarget::SelfTarget, ActorId(0), false),
        (crate::TechniqueTarget::Ally, ActorId(1), true),
    ] {
        let mut prepared = companion_prepared_for(2);
        let capabilities = &mut prepared.resources.actor_setup[0].techniques[0].capabilities;
        capabilities.target = target_kind;
        capabilities.revives = revive;
        prepared.actors[1].side = Side::Party;
        if revive {
            prepared.actors[1].hp = 0;
            prepared.actors[1].availability = crate::ActorAvailability::Dead;
        }
        prepared.actors[2].position = [10_000., 0., 0.];

        prepared.targets = [2, 2, 0].map(ActorId).to_vec();
        let mut battle = prepared.finish()?;
        let owner = ActorId(0);
        let position = battle.actors()[0].position;
        let tp = battle.actors()[0].tp;
        assert!(battle.queue_technique_target(owner, crate::ActionKey(7), recipient)?);
        let frame = battle.step(BattleInput::default())?;
        let started = frame
            .cues
            .iter()
            .find_map(|cue| match cue {
                crate::Cue::Started { actor, action, .. } if *actor == owner => Some(*action),
                _ => None,
            })
            .expect("support command starts without approaching the distant enemy");
        assert_eq!(battle.sequence(&started).unwrap().target, recipient);
        assert_eq!(battle.actors()[0].position, position);
        assert_eq!(battle.target(owner), Some(ActorId(2)));
        assert_eq!(battle.actors()[0].tp, tp - 5);
        let next = battle.step(BattleInput::default())?;
        assert!(!next.cues.iter().any(|cue| matches!(cue,
            crate::Cue::Started { actor, .. } if *actor == owner)));
        assert_eq!(battle.actors()[0].tp, tp - 5);
        assert_eq!(battle.actors()[0].position, position);
    }
    Ok(())
}

#[test]
fn queued_support_losing_its_recipient_is_cancelled_without_payment() -> Result<()> {
    let mut prepared = companion_prepared_for(2);
    prepared.resources.actor_setup[0].techniques[0]
        .capabilities
        .target = crate::TechniqueTarget::Ally;
    prepared.actors[1].side = Side::Party;
    prepared.targets = [2, 2, 0].map(ActorId).to_vec();
    let mut battle = prepared.finish()?;
    let owner = ActorId(0);
    let tp = battle.actors()[0].tp;
    assert!(battle.queue_technique_target(owner, crate::ActionKey(7), ActorId(1))?);
    battle.actors[1].hp = 0;
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    let frame = battle.step(BattleInput::default())?;
    assert!(!frame.cues.iter().any(|cue| matches!(cue,
        crate::Cue::Started { actor, .. } if *actor == owner)));
    assert_eq!(battle.pending_technique(owner), None);
    assert_eq!(battle.pending_technique_target(owner), None);
    assert_eq!(battle.pending_technique_issuer(owner), None);
    assert_eq!(battle.actors()[0].tp, tp);
    Ok(())
}

#[test]
fn approaching_command_losing_its_recipient_does_not_retarget_or_pay() -> Result<()> {
    let mut battle = companion_prepared_for(2).finish()?;
    let owner = ActorId(0);
    let tp = battle.actors()[0].tp;
    assert!(battle.queue_technique_target(owner, crate::ActionKey(7), ActorId(1))?);
    battle.step(BattleInput::default())?;
    assert_eq!(battle.activity(ActorId(0)), Activity::Approaching);
    battle.actors[1].hp = 0;
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.pending_technique(owner), None);
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .all(|sequence| sequence.action != crate::ActionKey(7))
    );
    assert_eq!(battle.actors()[0].tp, tp);
    Ok(())
}
