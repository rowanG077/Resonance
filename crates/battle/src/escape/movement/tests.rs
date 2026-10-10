use super::*;
use crate::Activity;
use crate::{
    ActionDefinition, BattleInput, CompanionDefinition, Control, ControlDefinition, ControlInput,
    ControlMotions, DecisionDefinition, EscapeActorDefinition, EscapeDefinition, MotionBinding,
    PolicyLimits, PreparedBattle,
};
use std::sync::Arc;

fn prepared_fixture(control: Control) -> Result<PreparedBattle> {
    let motion = |clip| MotionBinding { model: 7, clip };
    let action = ActionDefinition {
        normal: None,
        execution: crate::ActionExecution::Attack(crate::PreparedAttack {
            chain_at: None,
            end_at: 30,
            opening: None,
            events: vec![],
            recovery: 2,
        }),
        tp_cost: 0,
    };
    let mut actions = crate::ActionDefinitions::default();
    let crate::ActionExecution::Attack(attack) = action.execution else {
        unreachable!()
    };
    let normals = crate::tests::normal_controls(&mut actions, attack, [0., 50.]);
    let mut player = crate::tests::actor(Side::Party);
    player.control = control;
    player.hp = player.equipment.max_hp;
    player.body.collider = Some(crate::Collider::sphere(1.));
    let mut enemy = crate::tests::actor(Side::Enemy);
    enemy.control = Control::Enemy;
    enemy.position = [100., 0., 0.];

    enemy.body.collider = Some(crate::Collider::sphere(1.));
    let mut actors = vec![crate::ActorSetup::default(); 2];
    actors[0].control = Some(Arc::new(ControlDefinition {
        walk_speed: 3.,
        run_speed: 6.,
        turn_ticks: 8,
        motions: Some(ControlMotions {
            walk: motion(1),
            run: motion(19),
            stop: motion(18),
            landing: motion(0),
        }),
        shortcuts: [0; 4],
        normals,
    }));
    actors[0].companion = Some(CompanionDefinition {
        initial_policy: [0; 3],
        defaults: [1, 5, 1],
        limits: [PolicyLimits {
            tp: 30,
            healing: 0,
            support_level: -8,
        }; 9],
        level: 30,
        level_difference: 0,
    });
    if control == Control::Auto {
        actors[0].decision = Some(DecisionDefinition {
            idle_ticks: 0,
            idle_variation: 0,
        });
    }
    Ok(PreparedBattle::new(
        vec![player, enemy].into_iter().zip(actors).collect(),
        actions,
        1,
    )?
    .with_escape(EscapeDefinition {
        allowed: true,
        level_difference: 0,
        magic_mist: false,
        actors: vec![EscapeActorDefinition {
            actor: ActorId(0),
            request: None,
            cancel: None,
            success: None,
        }],
    })
    .with_arena_boundary())
}

fn fixture(control: Control) -> Result<Battle> {
    prepared_fixture(control)?.finish()
}

fn recognize(battle: &mut Battle) -> Result<()> {
    battle.recognize_escape(false)?;
    assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
    Ok(())
}

#[test]
fn dash_run_motion_and_escape_share_the_live_speed_multiplier() -> Result<()> {
    let mut battle = fixture(Control::Manual)?;
    battle.actors[0].equipment.speed_multiplier = 1.1;
    let mut input = ControlInput::neutral(ActorId(0));
    input.stick[0] = 80;
    battle.step(BattleInput {
        controllers: vec![input],
        ..Default::default()
    })?;

    for _ in 0..16 {
        battle.step(BattleInput {
            controllers: vec![input],
            ..Default::default()
        })?;
    }
    assert!((battle.actors[0].movement.forward - 6.6).abs() < 0.00001);
    recognize(&mut battle)?;
    for _ in 0..16 {
        battle.step(BattleInput::default())?;
    }
    assert!((battle.actors[0].movement.forward - 6.6).abs() < 0.00001);

    battle.actors[0].equipment.speed_multiplier = 1.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].movement.forward, 6.);
    Ok(())
}

#[test]
fn departure_replaces_combat_activity_and_respects_menu_pause() -> Result<()> {
    for (mode, forced) in [(Control::Manual, false), (Control::Auto, true)] {
        let mut battle = fixture(mode)?;
        if forced {
            battle.enter_stun(ActorId(0), &mut vec![]);
        } else {
            battle.enter_player_guard(0);
        }
        if forced {
            battle.prepared.escape = None;
            battle.actors[0].time_stop = 30;
            battle.timed_hold = Some(crate::overlimit::Hold {
                remaining: 30,
                actor: Some(ActorId(1)),
            });
        }
        battle.recognize_escape(forced)?;
        assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
        let initial = battle.actors[0].position;
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(battle.actors[0].position, initial);
        let mut input = ControlInput::neutral(ActorId(0));
        input.attack.pressed = true;
        input.technique.pressed = true;
        input.guard.held = true;
        input.taunt.pressed = true;
        input.stick = [127, 127];
        let frame = battle.step(BattleInput {
            controllers: vec![input],
            ..Default::default()
        })?;
        assert_eq!(battle.activity(ActorId(0)), Activity::Escaping);
        assert!(battle.actors[0].position[0] < initial[0]);
        assert!(!battle.actors[0].guard.active);
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        assert!(!battle.is_diagnostic());
        battle.step(BattleInput::default())?;
    }
    let mut battle = fixture(Control::Manual)?;
    recognize(&mut battle)?;
    battle.actors[0].availability = crate::ActorAvailability::Petrified;
    let position = battle.actors[0].position;
    battle.step(BattleInput::default())?;
    assert_ne!(battle.activity(ActorId(0)), Activity::Escaping);
    assert_eq!(battle.actors[0].position, position);
    Ok(())
}

#[test]
fn departure_cancels_an_active_action_without_repaying_or_completing_it() -> Result<()> {
    let mut battle = fixture(Control::Manual)?;
    Arc::make_mut(&mut battle.prepared.actions.entries[0]).tp_cost = 5;
    let frame = battle.step(BattleInput {
        actions: vec![crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }],
        ..Default::default()
    })?;
    let action = frame
        .cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .expect("attack started");
    let tp = battle.actors[0].tp;
    recognize(&mut battle)?;
    let frame = battle.step(BattleInput::default())?;
    assert!(frame.cues.contains(&Cue::Interrupted { action }));
    assert!(!frame.cues.contains(&Cue::Completed { action }));
    assert!(battle.sequence(&action).is_none());
    assert_eq!(battle.activity(ActorId(0)), Activity::Escaping);
    assert_eq!(battle.actors[0].tp, tp);
    Ok(())
}

#[test]
fn departure_works_with_unavailable_run_feedback() -> Result<()> {
    for missing in ["binding", "clip"] {
        let mut prepared = prepared_fixture(Control::Manual)?;
        if missing == "binding" {
            Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).motions =
                None;
        }
        let mut battle = prepared.finish()?;
        battle.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(false));
        if missing == "clip" {
            Arc::make_mut(battle.prepared.actor_setup[0].control.as_mut().unwrap())
                .motions
                .as_mut()
                .unwrap()
                .run
                .clip = 255
        }
        recognize(&mut battle)?;
        battle.step(BattleInput::default())?;
        assert_eq!(battle.activity(ActorId(0)), Activity::Escaping);
        assert!(battle.actors[0].position[0] < 0.);
        assert!(!battle.is_diagnostic());
        assert!(!battle.diagnostics().has_errors());
    }
    Ok(())
}

#[test]
fn departure_leaves_the_arena_and_lands_even_with_a_coincident_target() -> Result<()> {
    for coincident in [false, true] {
        let mut battle = fixture(Control::Manual)?;
        battle.actors[0].position = [849., 4., 0.];
        battle.actors[0].facing_direction = [-1., 0., 0.];
        if coincident {
            battle.actors[1].position = battle.actors[0].position;
        }
        recognize(&mut battle)?;
        for _ in 0..8 {
            battle.step(BattleInput::default())?;
        }
        assert!(battle.actors[0].position[0] > 850.);
        assert_eq!(battle.actors[0].position[1], 0.);
        assert_eq!(
            battle.actors[0].movement.steering.arena_contact(),
            crate::ArenaContact::None
        );
    }
    Ok(())
}

#[test]
fn escape_cancels_a_pending_attack_without_spending_tp_or_counting_use() -> Result<()> {
    let mut battle = fixture(Control::SemiAuto)?;
    Arc::make_mut(&mut battle.prepared.actions.entries[0]).tp_cost = 5;
    assert!(battle.request_approach(
        ActorId(0),
        ActorId(1),
        crate::ActionKey(0),
        crate::ApproachParameters {
            minimum: 0.,
            maximum: 20.,
            motion: Some(MotionBinding { model: 7, clip: 19 }),
            motion_rate: 0.5,
            speed: 6.,
            turn_ticks: 8,
        }
    )?);
    let tp = battle.actors[0].tp;
    let uses = battle.technique_counts(ActorId(0)).cloned();
    recognize(&mut battle)?;
    let frame = battle.step(BattleInput {
        actions: vec![crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }],
        ..Default::default()
    })?;
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            actor: ActorId(0),
            reason: crate::Rejection::BattleEnding,
        }
    )));
    assert!(!frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Started {
            actor: ActorId(0),
            ..
        }
    )));
    assert!(battle.runtime[0].task().approach().is_none());
    assert_eq!(battle.activity(ActorId(0)), Activity::Escaping);
    assert_eq!(battle.actors[0].tp, tp);
    assert_eq!(battle.technique_counts(ActorId(0)), uses.as_ref());
    Ok(())
}

fn enemy_fixture() -> Result<Battle> {
    let mut prepared = prepared_fixture(Control::Manual)?;

    prepared.resources.actor_setup[1].enemy_decision = Some(crate::EnemyDecisionDefinition {
        strategy: crate::TargetPolicy::Nearest,
        difficulty: 0,
        choices: vec![crate::EnemyChoice {
            action: crate::ActionKey(0),
            return_to_formation: true,
            weight: 1,
            requirements: Default::default(),
            target_policy: Some(crate::TargetPolicy::Nearest),
            guard_chance: 0,
            range: [0, 0],
            tp: 0,
            approach_minimum: 0.,
            approach_range: 120.,
        }],
        back_row: vec![],
        walk_speed: 2.,
        walk_motion: None,
        turn_ticks: 8,
    });

    prepared.resources.actor_setup[1].decision = Some(DecisionDefinition {
        idle_ticks: 0,
        idle_variation: 0,
    });
    prepared.finish()
}

#[test]
fn escape_stops_idle_enemies_without_selecting_actions() -> Result<()> {
    let mut battle = enemy_fixture()?;
    battle.runtime[1].idle_timer = 2;
    battle.actors[1].movement.forward = 5.;
    let position = battle.actors[1].position;
    recognize(&mut battle)?;
    for _ in 0..3 {
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
        assert_eq!(frame.actors[1].position, position);
        assert!(!frame.cues.iter().any(|cue| matches!(
            cue,
            crate::Cue::Started {
                actor: ActorId(1),
                ..
            }
        )));
    }
    Ok(())
}

#[test]
fn escape_rejects_a_pending_enemy_action_without_use() -> Result<()> {
    let mut battle = enemy_fixture()?;
    Arc::make_mut(&mut battle.prepared.actions.entries[0]).tp_cost = 5;
    assert!(battle.request_approach(
        ActorId(1),
        ActorId(0),
        crate::ActionKey(0),
        crate::ApproachParameters {
            minimum: 0.,
            maximum: 200.,
            motion: Some(MotionBinding { model: 7, clip: 0 }),
            motion_rate: 0.5,
            speed: 2.,
            turn_ticks: 8,
        }
    )?);
    let tp = battle.actors[1].tp;
    let uses = battle.technique_counts(ActorId(1)).cloned();
    recognize(&mut battle)?;
    let frame = battle.step(BattleInput::default())?;
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            actor: ActorId(1),
            reason: crate::Rejection::BattleEnding,
        }
    )));
    assert!(!frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Started {
            actor: ActorId(1),
            ..
        }
    )));
    assert!(battle.runtime[1].task().approach().is_none());
    assert_eq!(battle.activity(ActorId(1)), Activity::Idle);
    assert_eq!(battle.actors[1].tp, tp);
    assert_eq!(battle.technique_counts(ActorId(1)), uses.as_ref());
    assert!(
        !battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|sequence| sequence.actor == ActorId(1))
    );
    Ok(())
}
