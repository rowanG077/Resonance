use super::*;

fn terminal_checkpoint(retry: bool, recovery: u16) -> Result<(Battle, crate::ActionId)> {
    let mut prepared = prepared_combo_arte(true)?;
    let attack =
        crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]));
    *attack = crate::PreparedAttack::new(2);
    attack.chain_at = Some(if retry { 1 } else { 2 });
    attack.chain_retry_at = retry.then_some(2);
    attack.recovery = recovery;
    prepared.actors[2].equipment.max_tp = 100;
    prepared.actors[2].equipment.normal_combo_limit = 1;
    prepared.actors[2].tp = 20;
    for (index, actor) in prepared.actors.iter_mut().enumerate() {
        actor.control = if index == 2 {
            Control::Auto
        } else {
            Control::Manual
        };
    }
    let mut battle = prepared.finish()?;
    let owner = ActorId(2);
    battle.start_admitted_action(
        crate::ActionRequest {
            actor: owner,
            target: ActorId(3),
            action: crate::ActionKey(0),
        },
        &mut Vec::new(),
    )?;
    let id = battle.sequences().next().unwrap().0.to_owned();
    battle.runtime[2].combo.normal_links = 0;
    battle.confirm_actor_contact(owner);
    Ok((battle, id))
}

#[test]
fn autonomous_end_checkpoints_remain_eligible_before_recovery() -> Result<()> {
    for retry in [false, true] {
        for recovery in [0, 3] {
            let (mut battle, id) = terminal_checkpoint(retry, recovery)?;
            let owner = ActorId(2);
            let action = battle.prepared.actor_setup[2].techniques[0].action;
            for _ in 0..30 {
                if battle.action_age(id) == Some(2) {
                    break;
                }
                battle.step(crate::BattleInput::default())?;
            }
            assert_eq!(battle.action_age(id), Some(2));
            assert_eq!(battle.activity(owner), crate::Activity::Action);
            battle.refresh_strategy(&rows(&battle, [1, 1, 6]))?;
            let tp = battle.actors[2].tp;
            let frame = battle.step(crate::BattleInput::default())?;
            assert!(
                frame.cues.iter().any(|cue| matches!(cue,
                    crate::Cue::Started { actor, action: handle, .. }
                        if *actor == owner && battle.action_definition(*handle) == Some(action)
                )),
                "retry {retry}, recovery {recovery}: first visit at the end must remain eligible"
            );
            assert!(battle.sequence(&id).is_none());
            assert_eq!(battle.actors[2].tp, tp - 5);
        }
    }
    Ok(())
}

#[test]
fn recovery_completion_does_not_repeat_autonomous_checkpoints_but_accepts_commands() -> Result<()> {
    for retry in [false, true] {
        for recovery in [0, 3] {
            for explicit in [false, true] {
                let (mut battle, id) = terminal_checkpoint(retry, recovery)?;
                let owner = ActorId(2);
                let action = battle.prepared.actor_setup[2].techniques[0].action;
                // Do Not Use rejects the checkpoint and the normal limit
                // prevents a normal continuation, leaving this action to recover.
                for _ in 0..30 {
                    battle.step(crate::BattleInput::default())?;
                    if battle.activity(owner) == crate::Activity::Recovering {
                        break;
                    }
                }
                assert_eq!(battle.activity(owner), crate::Activity::Recovering);
                assert_eq!(battle.sequence(&id).unwrap().age, 2);
                assert_eq!(
                    battle.action_recovery_remaining(id),
                    Some(u32::from(recovery))
                );
                let random = battle.random.state();
                let tp = battle.actors[2].tp;
                battle.refresh_strategy(&rows(&battle, [1, 1, 6]))?;
                if explicit {
                    assert!(battle.queue_technique_target(owner, action, ActorId(3))?);
                }
                let mut started = false;
                for _ in 0..10 {
                    let frame = battle.step(crate::BattleInput::default())?;
                    started |= frame.cues.iter().any(|cue| {
                        matches!(cue,
                        crate::Cue::Started { actor, .. } if *actor == owner)
                    });
                    if battle.sequence(&id).is_none() {
                        break;
                    }
                }
                assert!(battle.sequence(&id).is_none(), "recovery must complete");
                assert_eq!(
                    started, explicit,
                    "retry {retry}, recovery {recovery}: only an explicit command may chain again"
                );
                assert_eq!(battle.actors[2].tp, tp - if explicit { 5 } else { 0 });
                assert_eq!(battle.random.state(), random, "no autonomous resampling");
                assert_eq!(battle.pending_technique(owner), None);
            }
        }
    }
    Ok(())
}

fn active_companion_combo() -> Result<(Battle, crate::ActionId, Box<crate::action::Sequence>)> {
    let mut prepared = prepared_combo_arte(true)?;
    let attack =
        crate::tests::attack_mut(Arc::make_mut(&mut prepared.resources.actions.entries[0]));
    attack.chain_at = Some(2);
    attack.chain_retry_at = Some(6);
    let mut battle = prepared.finish()?;
    let mut cues = Vec::new();
    battle.start_admitted_action(
        crate::ActionRequest {
            actor: ActorId(2),
            target: ActorId(3),
            action: crate::ActionKey(0),
        },
        &mut cues,
    )?;
    let id = cues
        .iter()
        .find_map(|cue| match cue {
            crate::Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap();
    let mut sequence = battle.take_sequence(&id).unwrap();
    battle.update = sequence.started_at + 1;
    sequence.age = 3;
    battle.confirm_actor_contact(ActorId(2));
    Ok((battle, id, sequence))
}

#[test]
fn late_explicit_commands_chain_between_and_after_autonomous_checkpoints() -> Result<()> {
    for age in [3, 7] {
        let (mut battle, id, mut sequence) = active_companion_combo()?;
        let owner = ActorId(2);
        let action = battle.prepared.actor_setup[2].techniques[0].action;
        // Explicit commands remain usable with autonomous techniques disabled.
        battle.runtime[2]
            .control
            .as_mut()
            .unwrap()
            .disabled_techniques
            .insert(action);
        sequence.age = age;
        assert!(battle.queue_technique_target(owner, action, ActorId(3))?);
        let tp = battle.actors[2].tp;
        let random = battle.random.state();
        let mut cues = Vec::new();
        assert!(battle.chain_normal(id, &sequence, &mut cues)?, "age {age}");
        assert_eq!(battle.pending_technique(owner), None);
        assert_eq!(battle.actors[2].tp, tp - 5);
        assert_eq!(
            battle.random.state(),
            random,
            "command must not sample autonomous choices"
        );
        assert!(cues.iter().any(|cue| matches!(cue,
            crate::Cue::Started { actor, action: next, .. }
                if *actor == owner && battle.sequence(next).is_some_and(|s| s.action == action)
        )));
    }
    Ok(())
}

#[test]
fn late_explicit_commands_preserve_combo_eligibility_checks() -> Result<()> {
    for restriction in [
        "early",
        "expired",
        "contact",
        "tp",
        "curse",
        "target",
        "chain-limit",
    ] {
        let (mut battle, id, mut sequence) = active_companion_combo()?;
        let owner = ActorId(2);
        let action = battle.prepared.actor_setup[2].techniques[0].action;
        assert!(battle.queue_technique_target(owner, action, ActorId(3))?);
        match restriction {
            "early" => sequence.age = 1,
            "expired" => sequence.age = sequence.combo.unwrap().buffer_until + 1,
            "contact" => battle.runtime[2].combo.confirmed_contact = false,
            "tp" => battle.actors[2].tp = 0,
            "curse" => {
                battle.actors[2].conditions.apply_hit(
                    resonance_content::battle_action::HitCondition {
                        condition: resonance_content::battle_action::Condition::Curse,
                        chance: 100,
                        value: 0,
                    },
                );
            }
            "target" => {
                battle.actors[3].hp = 0;
                battle.actors[3].availability = crate::ActorAvailability::Dead;
            }
            "chain-limit" => {
                battle.prepared.actor_setup[2].arte_chain_limit = Some(1);
                battle.runtime[2].combo.normal_links = 1;
            }
            _ => unreachable!(),
        }
        let tp = battle.actors[2].tp;
        let random = battle.random.state();
        let mut cues = Vec::new();
        assert!(
            !battle.chain_normal(id, &sequence, &mut cues)?,
            "{restriction}"
        );
        assert_eq!(battle.actors[2].tp, tp, "{restriction}: no payment");
        assert_eq!(
            battle.random.state(),
            random,
            "{restriction}: no autonomous draw"
        );
        assert!(cues.is_empty(), "{restriction}: no action started");
        assert_eq!(
            battle.pending_technique(owner),
            (restriction != "target").then_some(action)
        );
    }
    Ok(())
}

#[test]
fn pending_command_blocks_autonomous_choice_when_unaffordable() -> Result<()> {
    let mut prepared = prepared_with_spell(crate::TechniqueCapabilities {
        spell: true,
        target: crate::TechniqueTarget::Enemy,
        ..Default::default()
    })?;
    Arc::make_mut(&mut prepared.resources.actions.entries[7]).tp_cost = 20;
    let mut battle = prepared.finish().unwrap();
    assert!(battle.queue_technique(ActorId(2), crate::ActionKey(7))?);
    battle.actors[2].tp = 0;
    battle.advance_ai(ActorId(2), &mut Vec::new())?;
    assert_eq!(
        battle.pending_technique(ActorId(2)),
        Some(crate::ActionKey(7))
    );
    assert_eq!(battle.activity(ActorId(2)), crate::Activity::Idle);
    assert!(battle.runtime[2].task().approach().is_none());
    Ok(())
}

#[test]
fn explicit_command_bypasses_autonomous_enable_setting() -> Result<()> {
    let prepared = prepared_with_spell(crate::TechniqueCapabilities {
        spell: true,
        target: crate::TechniqueTarget::Enemy,
        ..Default::default()
    })?;
    let mut battle = prepared.finish().unwrap();
    battle.runtime[2]
        .control
        .as_mut()
        .unwrap()
        .disabled_techniques
        .insert(crate::ActionKey(7));
    assert!(battle.queue_technique_target(ActorId(2), crate::ActionKey(7), ActorId(4))?);
    battle.actors[4].position = [-200., 0., 0.];

    battle.actors[2].movement.target_direction = [1., 0., 0.];
    assert!(battle.request_queued_technique(ActorId(2), &mut Vec::new())?);
    assert!(battle.runtime[2].task().approach().is_some());
    let mut started = false;
    for _ in 0..120 {
        let frame = battle.step(crate::BattleInput::default())?;
        if frame.cues.iter().any(|cue| {
            matches!(
                cue,
                crate::Cue::Started {
                    actor: ActorId(2),
                    ..
                }
            )
        }) {
            started = true;
            break;
        }
    }
    assert!(
        started,
        "explicitly queued disabled technique did not start"
    );
    let facing = crate::control::direction_from_heading(battle.actors[2].heading);
    assert!(crate::distance::dot(facing, [-1., 0., 0.]) > 0.999);
    Ok(())
}

#[test]
fn queued_support_spell_waits_for_idle_target_resolution() -> Result<()> {
    let prepared = prepared_with_spell(crate::TechniqueCapabilities {
        uses_weapon_reach: true,
        ..spell_capabilities(true)
    })?;
    let mut battle = prepared.finish().unwrap();
    let owner = ActorId(2);
    let recipient = ActorId(1);
    assert!(battle.queue_technique_target(owner, crate::ActionKey(7), recipient)?);
    assert_eq!(battle.queue_pending_technique_chain(owner)?, Some(false));
    assert_eq!(battle.pending_technique(owner), Some(crate::ActionKey(7)));
    let mut cues = Vec::new();
    assert!(battle.request_queued_technique(owner, &mut cues)?);
    assert_eq!(battle.runtime[2].support_target, recipient);
    let action = cues
        .iter()
        .find_map(|cue| match cue {
            crate::Cue::Started { actor, action, .. } if *actor == owner => Some(*action),
            _ => None,
        })
        .expect("queued support starts against its recipient");
    assert_eq!(battle.sequence(&action).unwrap().target, recipient);
    assert!(battle.runtime[2].task().approach().is_none());
    Ok(())
}

#[test]
fn queued_enemy_command_uses_the_edited_target_handler() -> Result<()> {
    let mut prepared = prepared_with_spell(spell_capabilities(false))?;
    prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap()
        .target_strategies[0] = crate::TargetPolicy::Farthest;
    let mut battle = prepared.finish()?;
    let owner = ActorId(2);
    assert!(battle.queue_technique(owner, crate::ActionKey(7))?);
    battle.runtime[2]
        .control
        .as_mut()
        .unwrap()
        .queued_technique
        .as_mut()
        .unwrap()
        .target = None;
    assert!(battle.request_queued_technique(owner, &mut Vec::new())?);
    assert_eq!(battle.target(owner), Some(ActorId(4)));
    Ok(())
}
