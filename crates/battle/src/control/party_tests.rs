use super::*;

fn fixture() -> Result<Battle> {
    party_fixture(&[Control::Manual, Control::Auto])
}

fn party_fixture(controls: &[Control]) -> Result<Battle> {
    let prepared = companion_prepared(false);
    let mut actors = controls
        .iter()
        .enumerate()
        .map(|(slot, &mode)| {
            let mut actor = prepared.actors[0].clone();
            actor.control = mode;
            actor.control_slot = ControllerPort::from_index(slot)?;
            actor.position[0] = -200. * slot as f32;
            Ok((actor, prepared.resources.actor_setup[0].clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    actors.push((prepared.actors[1].clone(), Default::default()));
    PreparedBattle::new(actors, prepared.resources.actions, 1)?
        .with_camera(crate::CameraDefinition {
            leader: ActorId(0),
            stage_pitch: 0.,
            auto_zoom: true,
            yaw_limits: None,
        })?
        .finish()
}

#[test]
fn unaffordable_assist_warns_on_the_recipient_once_per_press() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        for slot in 0..2 {
            let mut battle = party_fixture(&[mode, Control::Auto])?;
            let owner = ActorId(0);
            let recipient = ActorId(1);
            battle
                .prepare_assist_shortcut(owner, slot, Some((recipient, crate::ActionKey(7))))?
                .commit();
            battle.actors[recipient.index()].tp = 0;
            let rejection = Cue::Rejected {
                actor: recipient,
                reason: crate::Rejection::InsufficientTp,
            };
            let mut input = ControlInput::neutral(owner);
            input.assist[slot] = ButtonInput {
                pressed: true,
                held: true,
                released: false,
            };
            let frame = battle.step(BattleInput {
                controllers: vec![input],
                ..Default::default()
            })?;
            assert_eq!(frame.cues, [rejection.clone()]);
            assert_eq!(frame.actors[recipient.index()].tp, 0);
            assert!(battle.pending_technique(recipient).is_none());
            assert_eq!(battle.ledger.achievements[owner.index()].assist_commands, 0);

            input.assist[slot].pressed = false;
            let held = battle.step(BattleInput {
                controllers: vec![input],
                ..Default::default()
            })?;
            assert!(!held.cues.contains(&rejection));

            battle.actors[recipient.index()].tp = 5;
            input.assist[slot].pressed = true;
            let admitted = battle.step(BattleInput {
                controllers: vec![input],
                ..Default::default()
            })?;
            assert!(
                admitted
                    .cues
                    .contains(&Cue::TechniqueQueued { actor: recipient })
            );
            assert!(!admitted.cues.contains(&rejection));
            assert_eq!(battle.ledger.achievements[owner.index()].assist_commands, 1);
        }
    }
    Ok(())
}

#[test]
fn assigned_ally_arte_reports_insufficient_tp_without_queueing() -> Result<()> {
    let mut battle = fixture()?;
    let recipient = ActorId(1);
    battle.actors[recipient.index()].tp = 0;
    let mut input = ControlInput::neutral(recipient);
    input.assigned_arte = Some(crate::TechniqueId(1));
    battle.actors[recipient.index()].conditions =
        crate::conditions::Conditions::new(crate::conditions::Layers {
            base: crate::conditions::Condition::Curse.into(),
            ..Default::default()
        });
    let unavailable = battle.step(BattleInput {
        controllers: vec![input],
        ..Default::default()
    })?;
    assert!(unavailable.cues.is_empty());
    assert!(battle.pending_technique(recipient).is_none());
    battle.actors[recipient.index()].conditions = Default::default();
    let rejected = battle.step(BattleInput {
        controllers: vec![input],
        ..Default::default()
    })?;
    assert_eq!(
        rejected.cues,
        [Cue::Rejected {
            actor: recipient,
            reason: crate::Rejection::InsufficientTp,
        }]
    );
    assert_eq!(rejected.actors[recipient.index()].tp, 0);
    assert!(battle.pending_technique(recipient).is_none());
    battle.actors[recipient.index()].tp = 5;
    let admitted = battle.step(BattleInput {
        controllers: vec![input],
        ..Default::default()
    })?;
    assert!(
        admitted
            .cues
            .contains(&Cue::TechniqueQueued { actor: recipient })
    );
    Ok(())
}

#[test]
fn party_reorder_preserves_active_actions_targets_and_camera_pose() -> Result<()> {
    let mut battle = fixture()?;
    assert!(battle.start_actor_command(
        crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(2),
            action: crate::ActionKey(0),
        },
        &mut vec![],
    )?);
    let action = *battle.runtime[0].task().action().unwrap().0;
    let before = battle.snapshot();
    let age = battle.action_age(action);
    let random = battle.random_state();
    battle.reorder_party(&[ActorId(1), ActorId(0)])?;
    assert_eq!(battle.actors[0].control, Control::Auto);
    assert_eq!(battle.actors[0].control_slot, ControllerPort::Two);
    assert_eq!(battle.actors[1].control, Control::Manual);
    assert_eq!(battle.actors[1].control_slot, ControllerPort::One);
    assert_eq!(
        battle.camera.as_ref().unwrap().definition.leader,
        ActorId(1)
    );
    assert_eq!(battle.snapshot().camera, before.camera);
    assert_eq!(battle.snapshot().targets, before.targets);
    assert_eq!(battle.action_age(action), age);
    assert_eq!(battle.random_state(), random);
    for (after, before) in battle.actors.iter().zip(&before.actors) {
        assert_eq!(after.position, before.position);
        assert_eq!(after.hp, before.hp);
        assert_eq!(after.tp, before.tp);
    }
    for (slot, id) in [ActorId(1), ActorId(0)].into_iter().enumerate() {
        assert_eq!(
            battle.actors[id.index()].movement.steering.home_offset,
            before.actors[slot].movement.steering.home_offset
        );
    }
    assert_eq!(
        battle.actors[1].movement.steering.home,
        battle.actors[1].position
    );
    battle.actors[1].position = [300., 0., 100.];
    battle.remember_idle_home(1);
    battle.remember_idle_home(0);
    assert_eq!(battle.formation_home(ActorId(0))?, [100., 0., 100.]);
    Ok(())
}

#[test]
fn party_reorder_keeps_multiplayer_leadership_and_escape_in_slot_order() -> Result<()> {
    for automatic in [false, true] {
        let mut battle = party_fixture(&[Control::SemiAuto, Control::Manual, Control::Auto])?;
        battle.reorder_party(&[ActorId(2), ActorId(1), ActorId(0)])?;
        assert_eq!(battle.actors[1].control_slot, ControllerPort::Two);
        assert_eq!(battle.actors[1].control, Control::Manual);
        assert!(battle.suppress_recoil(ActorId(1), ActorId(3), 1000.));
        if automatic {
            battle.set_control_mode(ActorId(1), Control::Auto)?;
            battle.set_control_mode(ActorId(2), Control::Auto)?;
            assert!(!battle.suppress_recoil(ActorId(1), ActorId(3), 1000.));
        }
        assert_eq!(battle.preferred_party_leader(), Some(ActorId(2)));
        battle.recognize_escape(false)?;
        battle.enter_escape();
        assert_eq!(battle.ledger().ordinary_escape, Some(ActorId(2)));
    }
    Ok(())
}

#[test]
fn party_reorder_grade_follows_current_slots() -> Result<()> {
    use crate::conditions::{Condition, Conditions, Layers};

    for (victim, death_grade, result_grade) in [(0, -50, -75), (1, -100, -150)] {
        let mut battle = fixture()?;
        let order = [ActorId(1), ActorId(0)];
        battle.reorder_party(&order)?;
        for actor in &mut battle.actors[..2] {
            actor.conditions = Conditions::new(Layers {
                base: Condition::PoisonMild.into(),
                ..Default::default()
            });
        }
        battle.actors[victim].hp = 0;
        battle.defeat_actor(ActorId::from_index(victim)?, &mut vec![]);
        assert_eq!(battle.ledger().grade(), death_grade);
        battle.actors[2].hp = 0;
        battle.defeat_actor(ActorId(2), &mut vec![]);
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        battle.retire_combat()?;
        battle.ledger.elapsed_ticks = 901;
        let conditions = order.map(|id| battle.actors[id.index()].conditions.effective());
        assert_eq!(battle.finalize_grade(&conditions, &[], 0)?, result_grade);
    }
    Ok(())
}

#[test]
fn invalid_party_reorder_leaves_ownership_unchanged() -> Result<()> {
    let mut battle = fixture()?;
    let before = battle.actors.clone();
    for order in [
        vec![],
        vec![ActorId(0)],
        vec![ActorId(1), ActorId(1)],
        vec![ActorId(2), ActorId(0)],
    ] {
        assert!(battle.reorder_party(&order).is_err());
        assert_eq!(battle.actors, before);
    }
    battle.prepared.actor_setup[0].companion = None;
    assert!(battle.reorder_party(&[ActorId(1), ActorId(0)]).is_err());
    assert_eq!(battle.actors, before);
    Ok(())
}
