use super::*;
use crate::conditions::{Condition, ConditionSet};
use crate::{RegalArteFamily, TechniqueCapabilities};

const DIVE: crate::ActionKey = crate::ActionKey(7);
const GROUND: crate::ActionKey = crate::ActionKey(8);

fn fixture(mode: Control) -> Battle {
    let mut prepared = prepared();
    let setup = &mut prepared.resources.actor_setup;
    setup[0].contact_recovery = true;
    for _ in [DIVE, GROUND] {
        prepared
            .resources
            .actions
            .entries
            .push(Arc::new(ActionDefinition {
                normal: None,
                execution: crate::ActionExecution::Attack(crate::tests::attack(90)),
                tp_cost: 8,
            }));
    }
    setup[0].techniques = [
        (DIVE, 185, RegalArteFamily::Aerial),
        (GROUND, 176, RegalArteFamily::AntiAir),
    ]
    .map(|(action, catalogue, family)| crate::PreparedTechnique {
        capabilities: TechniqueCapabilities {
            regal_family: Some(family),
            target: crate::TechniqueTarget::Enemy,
            offensive: true,
            uses_weapon_reach: true,
            ..Default::default()
        },
        ..crate::tests::technique(action, catalogue)
    })
    .to_vec();
    Arc::make_mut(setup[0].control.as_mut().unwrap()).shortcuts = [185; 4];
    prepared.actors[0].control = mode;
    let prepared = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[176, 185],
            &[(185, 49)],
        )])
        .unwrap();
    let mut battle = prepared.finish().unwrap();
    battle.set_task(
        0,
        crate::state::ActorTask::Mobility(Mobility::Jump { launched: true }),
    );
    let owner = &mut battle.actors[0];
    owner.position[1] = 200.;
    owner.movement.vertical = -2.;
    owner.movement.forward = 3.;
    owner.tp = 20;
    battle
}

fn buttons(attack: bool, technique: bool) -> BattleInput {
    BattleInput {
        controllers: vec![ControlInput {
            attack: ButtonInput {
                pressed: attack,
                held: attack,
                ..Default::default()
            },
            technique: ButtonInput {
                pressed: technique,
                held: technique,
                ..Default::default()
            },
            ..ControlInput::neutral(ActorId(0))
        }],
        ..Default::default()
    }
}

#[test]
fn falling_queue_targets_the_live_opponent_and_pays_once() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto, Control::Auto] {
        let mut battle = fixture(mode);
        assert!(!battle.queue_technique_target(ActorId(0), DIVE, ActorId(0))?);
        assert_eq!(battle.actors[0].tp, 20);
        assert_eq!(battle.technique_uses(ActorId(0), 185), Some(49));
        assert_eq!(battle.pending_technique(ActorId(0)), None);
        assert!(battle.queue_technique(ActorId(0), DIVE)?);
        for _ in 0..4 {
            battle.step(BattleInput::default())?;
        }
        let action = battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .find(|row| row.action == DIVE)
            .unwrap();
        assert_eq!(action.target, ActorId(1));
        assert_eq!(battle.actors[0].tp, 12);
        assert_eq!(battle.technique_uses(ActorId(0), 185), Some(50));
        assert_eq!(battle.pending_technique(ActorId(0)), None);
        assert_eq!(battle.pending_technique_target(ActorId(0)), None);
        assert_eq!(battle.pending_technique_issuer(ActorId(0)), None);
    }
    Ok(())
}

#[test]
fn rejected_falling_queue_is_consumed_without_spending_tp() -> Result<()> {
    for reason in 0..4 {
        let mut battle = fixture(Control::Manual);
        assert!(battle.queue_technique(ActorId(0), DIVE)?);
        assert_eq!(
            battle.pending_technique_target(ActorId(0)),
            Some(ActorId(1))
        );
        assert_eq!(battle.pending_technique_issuer(ActorId(0)), Some(0));
        match reason {
            0 => battle.actors[0].position[1] = 0.,
            1 => battle.actors[0].tp = 7,
            2 => {
                battle.actors[0].conditions =
                    crate::conditions::Conditions::new(crate::conditions::Layers {
                        base: ConditionSet::of(&[Condition::Curse]),
                        ..Default::default()
                    })
            }
            3 => {
                battle.learning_members[0].member.forget(185)?;
            }
            _ => unreachable!(),
        }
        let tp = battle.actors[0].tp;
        battle.step(BattleInput::default())?;
        assert_eq!(
            battle.pending_technique(ActorId(0)),
            None,
            "reason {reason}"
        );
        assert_eq!(battle.pending_technique_target(ActorId(0)), None);
        assert_eq!(battle.pending_technique_issuer(ActorId(0)), None);
        assert_eq!(battle.actors[0].tp, tp);
        assert_eq!(battle.technique_uses(ActorId(0), 185), Some(49));
    }
    let mut other = fixture(Control::Manual);
    assert!(other.queue_technique(ActorId(0), GROUND)?);
    other.step(BattleInput::default())?;
    assert_eq!(other.pending_technique(ActorId(0)), Some(GROUND));
    Ok(())
}

#[test]
fn queued_dive_waits_for_descent_and_respects_menu_pause() -> Result<()> {
    let mut battle = fixture(Control::Manual);
    assert!(battle.queue_technique(ActorId(0), DIVE)?);
    battle.actors[0].movement.vertical = 5.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.pending_technique(ActorId(0)), Some(DIVE));
    let position = battle.actors[0].position;
    battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    assert_eq!(battle.actors[0].position, position);
    battle.actors[0].movement.vertical = -1.;
    for _ in 0..3 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.pending_technique(ActorId(0)), None);
    assert_eq!(battle.actors[0].tp, 12);
    Ok(())
}

#[test]
fn fresh_aerial_technique_takes_priority_and_is_paid_once() -> Result<()> {
    let mut battle = fixture(Control::Manual);
    battle.step(buttons(true, true))?;
    for _ in 0..3 {
        battle.step(BattleInput::default())?;
    }
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|row| row.action == DIVE)
    );
    assert_eq!(battle.actors[0].tp, 12);
    assert_eq!(battle.technique_uses(ActorId(0), 185), Some(50));
    Ok(())
}

#[test]
fn held_aerial_technique_does_not_execute_without_a_fresh_edge() -> Result<()> {
    let mut battle = fixture(Control::Manual);
    let mut input = buttons(false, false);
    input.controllers[0].technique.held = true;
    battle.step(input)?;
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    assert_eq!(battle.actors[0].tp, 20);
    assert_eq!(battle.technique_uses(ActorId(0), 185), Some(49));
    Ok(())
}

#[test]
fn rebound_allows_one_queued_aerial_technique_from_breakfall() {
    for enabled in [false, true] {
        let mut battle = fixture(Control::Manual);
        battle.actors[0].equipment.control_ex.rebound = enabled;
        battle.set_task(
            0,
            crate::state::ActorTask::Mobility(crate::mobility::Mobility::Breakfall),
        );
        assert!(battle.queue_technique(ActorId(0), DIVE).unwrap());
        let position = battle.actors[0].position;
        let paused = battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.pending_technique(ActorId(0)), Some(DIVE));
        assert_eq!(battle.actors[0].position, position);
        assert!(
            !paused
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        assert_eq!(battle.actors[0].tp, 20);
        assert_eq!(battle.technique_uses(ActorId(0), 185), Some(49));

        let mut started = Vec::new();
        for _ in 0..12 {
            let frame = battle.step(BattleInput::default()).unwrap();
            started.extend(frame.cues.iter().filter_map(|cue| match cue {
                Cue::Started { action, actor, .. } if *actor == ActorId(0) => Some(*action),
                _ => None,
            }));
        }
        assert_eq!(battle.pending_technique(ActorId(0)), None);
        if enabled {
            assert_eq!(started.len(), 1);
            let action = battle.sequence(&started[0]).unwrap();
            assert_eq!(action.action, DIVE);
            assert_eq!(action.target, ActorId(1));
            assert_eq!(battle.actors[0].tp, 12);
            assert_eq!(battle.technique_uses(ActorId(0), 185), Some(50));
        } else {
            assert!(started.is_empty());
            assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
            assert_eq!(battle.actors[0].tp, 20);
            assert_eq!(battle.technique_uses(ActorId(0), 185), Some(49));
        }
    }
}
