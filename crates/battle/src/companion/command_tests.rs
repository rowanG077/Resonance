use super::*;

#[test]
fn pending_command_blocks_autonomous_choice_when_unaffordable() -> Result<()> {
    let mut prepared = prepared()?;
    Arc::make_mut(&mut prepared.resources.actions.entries[7]).tp_cost = 20;
    prepared.resources.actor_setup[2].techniques = vec![technique(
        crate::ActionKey(7),
        66,
        crate::TechniqueCapabilities {
            spell: true,
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        },
    )];
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
    let mut prepared = prepared()?;
    prepared.resources.actor_setup[2].techniques = vec![technique(
        crate::ActionKey(7),
        66,
        crate::TechniqueCapabilities {
            spell: true,
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        },
    )];
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
    let mut prepared = prepared()?;
    prepared.resources.actor_setup[2].techniques = vec![technique(
        crate::ActionKey(7),
        66,
        crate::TechniqueCapabilities {
            spell: true,
            healing: true,
            uses_weapon_reach: true,
            target: crate::TechniqueTarget::Ally,
            ..Default::default()
        },
    )];
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
