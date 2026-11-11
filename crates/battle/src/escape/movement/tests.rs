use super::*;
use crate::Activity;
use crate::{
    ActionDefinition, BattleInput, CompanionDefinition, Control, ControlDefinition, ControlInput,
    ControlMotions, DecisionDefinition, EscapeActorDefinition, EscapeDefinition, MotionBinding,
    PolicyLimits, PreparedBattle, ResourceId,
};
use std::sync::Arc;

fn prepared_fixture(control: Control) -> Result<PreparedBattle> {
    let motion = |clip| MotionBinding {
        model: ResourceId(7),
        clip,
    };
    let action = ActionDefinition {
        normal: None,
        execution: crate::ActionExecution::Attack(crate::PreparedAttack {
            recovery: 2,
            ..crate::PreparedAttack::new(30)
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
        shortcuts: [None; 4],
        normals,
    }));
    actors[0].companion = Some(CompanionDefinition {
        character: crate::Character::Lloyd,
        target_strategies: crate::TargetPolicy::PARTY,
        positioning: crate::tests::companion_positioning(),
        normal_selection: crate::tests::companion_normal_selection(),
        initial_policy: [0; 3],
        defaults: [1, 5, 1],
        limits: [PolicyLimits {
            tp_reserve_percent: 30,
            healing_threshold_percent: 0,
            buff_max_level_advantage: 8,
            idle_tp_reserve_percent: 30,
        }; 9],
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

fn prepared_two_enemy_fixture(control: Control) -> Result<PreparedBattle> {
    let mut prepared = prepared_fixture(control)?;
    let mut other = prepared.actors[1].clone();
    other.position = [200., 0., 0.];
    prepared.actors.push(other);
    prepared.resources.actor_setup.push(Default::default());
    Ok(PreparedBattle::new(
        prepared
            .actors
            .into_iter()
            .zip(prepared.resources.actor_setup)
            .collect(),
        prepared.resources.actions,
        prepared.random_seed,
    )?
    .with_escape(prepared.resources.escape.unwrap())
    .with_arena_boundary())
}

fn set_launched_jump(battle: &mut Battle) {
    battle.actors[0].position[1] = 200.;
    battle.actors[0].movement.vertical = -2.;
    battle.set_task(
        0,
        ActorTask::Mobility(crate::mobility::Mobility::Jump { launched: true }),
    );
}

fn recognize(battle: &mut Battle) -> Result<()> {
    battle.recognize_escape(false)?;
    assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
    Ok(())
}

fn started_action(frame: &crate::BattleFrame, actor: ActorId) -> Option<crate::ActionId> {
    frame.cues.iter().find_map(|cue| match cue {
        Cue::Started {
            actor: owner,
            action,
            ..
        } if *owner == actor => Some(*action),
        _ => None,
    })
}

fn action_cues(frame: &crate::BattleFrame) -> Vec<&Cue> {
    frame
        .cues
        .iter()
        .filter(|cue| matches!(cue, Cue::Started { .. } | Cue::Completed { .. }))
        .collect()
}

#[test]
fn dash_run_motion_and_escape_share_the_live_speed_multiplier() -> Result<()> {
    let mut battle = fixture(Control::Manual)?;
    battle.actors[0].equipment.speed_multiplier = 1.1;
    let mut input = ControlInput::neutral(ActorId(0));
    input.stick[0] = 80;
    battle.step(BattleInput::from(input))?;

    for _ in 0..16 {
        battle.step(BattleInput::from(input))?;
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
        let frame = battle.step(BattleInput::from(input))?;
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
fn ordinary_departure_finishes_the_active_action_and_recovery_before_running() -> Result<()> {
    let mut battle = fixture(Control::Manual)?;
    let definition = Arc::make_mut(&mut battle.prepared.actions.entries[0]);
    definition.tp_cost = 5;
    crate::tests::attack_mut(definition).chain_at = Some(3);
    let frame = battle.step(BattleInput::from(crate::ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: crate::ActionKey(0),
    }))?;
    let action = started_action(&frame, ActorId(0)).expect("attack started");
    // A successor buffered before success must not extend the departing combo.
    battle.runtime[0].combo.buffered = Some(crate::action_selection::BufferedAction::Normal(
        crate::NormalAttack::Thrust,
    ));
    let tp = battle.actors[0].tp;
    recognize(&mut battle)?;

    let mut completed = false;
    let mut recovered = false;
    let mut escaped = false;
    for _ in 0..64 {
        let mut input = ControlInput::neutral(ActorId(0));
        input.attack.pressed = true;
        input.technique.pressed = true;
        input.guard.held = true;
        input.stick = [127, 127];
        let frame = battle.step(BattleInput::from(input))?;
        assert!(!frame.cues.contains(&Cue::Interrupted { action }));
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        completed |= frame.cues.contains(&Cue::Completed { action });
        recovered |= battle.activity(ActorId(0)) == Activity::Recovering;
        if battle.activity(ActorId(0)) == Activity::Escaping {
            assert!(completed && recovered);
            escaped = true;
            break;
        }
    }
    assert!(escaped, "ordinary Escape never reached departure");
    assert!(battle.sequence(&action).is_none());
    assert_eq!(battle.actors[0].tp, tp);
    Ok(())
}

#[test]
fn ordinary_departure_finishes_the_current_jump_and_accepts_raw_double_jump() -> Result<()> {
    let mut battle = fixture(Control::Manual)?;
    battle.actors[0].equipment.control_ex.double_jump = true;
    battle.actors[0].position[1] = 50.;
    battle.actors[0].movement.vertical = -2.;
    battle.set_task(
        0,
        ActorTask::Mobility(crate::mobility::Mobility::Jump { launched: true }),
    );
    recognize(&mut battle)?;

    let mut input = ControlInput::neutral(ActorId(0));
    input.vertical_pressed = 1;
    input.stick[1] = 127;
    battle.step(BattleInput::from(input))?;
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    assert!(battle.actors[0].position[1] > 50.);
    assert!(battle.actors[0].movement.vertical > 0.);
    assert!(battle.actors[0].control_ex_state.double_jump_used);

    for _ in 0..64 {
        battle.step(BattleInput::from(input))?;
        if battle.activity(ActorId(0)) == Activity::Escaping {
            assert_eq!(battle.actors[0].position[1], 0.);
            return Ok(());
        }
    }
    anyhow::bail!("ordinary Escape never finished jump and landing recovery")
}

#[test]
fn escape_jump_attack_keeps_combat_admission_and_recovery_timing() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        for breakfall in [false, true] {
            let mut combat = fixture(mode)?;
            let mut escaping = fixture(mode)?;
            for battle in [&mut combat, &mut escaping] {
                battle.actors[0].equipment.control_ex.rebound = breakfall;
                battle.actors[0].position[1] = 100.;
                battle.actors[0].movement.vertical = -2.;
                battle.set_task(
                    0,
                    ActorTask::Mobility(if breakfall {
                        crate::mobility::Mobility::Breakfall
                    } else {
                        crate::mobility::Mobility::Jump { launched: true }
                    }),
                );
            }
            recognize(&mut escaping)?;
            let mut started = None;
            let mut completed = false;
            let mut recovered = false;
            let mut departed = false;
            for tick in 0..96 {
                let mut input = ControlInput::neutral(ActorId(0));
                input.attack.pressed = tick == 0;
                let combat_frame = combat.step(BattleInput::from(input))?;
                let escape_frame = escaping.step(BattleInput::from(input))?;
                if tick == 0 {
                    assert_eq!(escaping.activity(ActorId(0)), Activity::Approaching);
                }
                assert_eq!(action_cues(&escape_frame), action_cues(&combat_frame));
                if let Some(action) = started_action(&escape_frame, ActorId(0)) {
                    assert!(
                        started.replace(action).is_none(),
                        "jump command unexpectedly chained"
                    );
                }
                completed |= started
                    .is_some_and(|action| escape_frame.cues.contains(&Cue::Completed { action }));
                recovered |= escaping.activity(ActorId(0)) == Activity::Recovering;
                if escaping.activity(ActorId(0)) == Activity::Escaping {
                    assert!(started.is_some() && completed && recovered);
                    departed = true;
                    break;
                }
                assert_eq!(escaping.activity(ActorId(0)), combat.activity(ActorId(0)));
                assert_eq!(escape_frame.actions, combat_frame.actions);
                assert_eq!(escaping.actors[0].position, combat.actors[0].position);
            }
            assert!(departed, "jump command never completed departure handoff");
        }
    }
    Ok(())
}

#[test]
fn escape_jump_target_release_and_attack_keep_combat_target_and_timing() -> Result<()> {
    for (mode, attack_tick, release_tick) in [
        (Control::Manual, 1, 1),
        (Control::SemiAuto, 1, 1),
        (Control::Manual, 0, 1),
        (Control::SemiAuto, 0, 1),
        (Control::Manual, 0, 4),
        (Control::SemiAuto, 0, 4),
    ] {
        let mut combat = prepared_two_enemy_fixture(mode)?.finish()?;
        let mut escaping = prepared_two_enemy_fixture(mode)?.finish()?;
        for battle in [&mut combat, &mut escaping] {
            set_launched_jump(battle);
            assert_eq!(battle.target(ActorId(0)), Some(ActorId(1)));
        }
        recognize(&mut escaping)?;
        let mut started = false;
        for tick in 0..8 {
            let mut input = ControlInput::neutral(ActorId(0));
            input.target.pressed = tick == 0;
            input.target.held = tick < release_tick;
            input.target.released = tick == release_tick;
            input.attack.pressed = tick == attack_tick;
            let combat_frame = combat.step(BattleInput::from(input))?;
            let escape_frame = escaping.step(BattleInput::from(input))?;
            assert!(combat_frame.target_selector.is_none());
            assert!(escape_frame.target_selector.is_none());
            assert_eq!(escaping.target(ActorId(0)), combat.target(ActorId(0)));
            assert_eq!(escape_frame.actions, combat_frame.actions);
            assert_eq!(escaping.actors[0].position, combat.actors[0].position);
            if tick == release_tick {
                let changed = Cue::TargetChanged {
                    actor: ActorId(0),
                    target: ActorId(2),
                };
                assert!(combat_frame.cues.contains(&changed));
                assert!(escape_frame.cues.contains(&changed));
                assert_eq!(escaping.target(ActorId(0)), Some(ActorId(2)));
                assert_eq!(
                    escaping.runtime[0].target_settle_remaining,
                    combat.runtime[0].target_settle_remaining
                );
                assert!(escaping.runtime[0].target_settle_remaining > 0);
            }
            assert_eq!(action_cues(&escape_frame), action_cues(&combat_frame));
            if let Some(action) = started_action(&escape_frame, ActorId(0)) {
                let target = if tick < release_tick {
                    ActorId(1)
                } else {
                    ActorId(2)
                };
                assert_eq!(escaping.sequence(&action).unwrap().target, target);
                assert_eq!(combat.sequence(&action).unwrap().target, target);
                started = true;
            }
        }
        assert!(started);
    }
    Ok(())
}

#[test]
fn escape_jump_refreshes_target_after_a_living_enemy_leaves_the_arena() -> Result<()> {
    for (mode, select_before_departure) in [
        (Control::Manual, false),
        (Control::SemiAuto, false),
        (Control::Manual, true),
        (Control::SemiAuto, true),
    ] {
        let setup = || -> Result<Battle> {
            let mut prepared = prepared_two_enemy_fixture(mode)?;
            let departure = prepared.resources.actions.insert(ActionDefinition {
                normal: None,
                execution: crate::ActionExecution::LeaveArena,
                tp_cost: 0,
            });
            prepared.resources.actor_setup[1].enemy_decision =
                Some(crate::tests::enemy_decision(vec![
                    crate::tests::enemy_choice(departure),
                ]));
            prepared.actors[1].position = [899.5, 0., 0.];
            prepared.actors[1].hp = prepared.actors[1].equipment.max_hp;
            let mut battle = prepared.finish()?;
            battle.runtime[0].set_target(ActorId(1));
            set_launched_jump(&mut battle);
            battle.start_admitted_action(
                crate::ActionRequest {
                    actor: ActorId(1),
                    target: ActorId(0),
                    action: departure,
                },
                &mut Vec::new(),
            )?;
            Ok(battle)
        };
        let mut combat = setup()?;
        let mut escaping = setup()?;
        recognize(&mut escaping)?;
        for battle in [&mut combat, &mut escaping] {
            battle.step(BattleInput::default())?;
            let mut input = ControlInput::neutral(ActorId(0));
            input.attack.pressed = select_before_departure;
            battle.step(BattleInput::from(input))?;
            assert_eq!(
                battle.actors[1].availability,
                crate::ActorAvailability::Absent
            );
            assert_eq!(battle.actors[1].hp, battle.actors[1].equipment.max_hp);
            assert_eq!(
                battle.target(ActorId(0)),
                Some(ActorId(1)),
                "the next actor visit must refresh this stale target"
            );
            if select_before_departure {
                assert!(battle.jump_approach(0));
            }
        }
        let mut started = false;
        for tick in 0..8 {
            let mut input = ControlInput::neutral(ActorId(0));
            input.attack.pressed = !select_before_departure && tick == 0;
            let combat_frame = combat.step(BattleInput::from(input))?;
            let escape_frame = escaping.step(BattleInput::from(input))?;
            assert_eq!(combat.target(ActorId(0)), Some(ActorId(2)));
            assert_eq!(escaping.target(ActorId(0)), Some(ActorId(2)));
            assert_eq!(escape_frame.actions, combat_frame.actions);
            assert_eq!(escaping.actors[0].position, combat.actors[0].position);
            for frame in [&combat_frame, &escape_frame] {
                assert!(!frame.cues.iter().any(|cue| matches!(
                    cue,
                    Cue::Rejected {
                        actor: ActorId(0),
                        ..
                    }
                )));
            }
            assert_eq!(action_cues(&escape_frame), action_cues(&combat_frame));
            if let Some(action) = started_action(&escape_frame, ActorId(0)) {
                assert_eq!(escaping.sequence(&action).unwrap().target, ActorId(2));
                started = true;
            }
        }
        assert!(
            started,
            "the live jump command was cancelled by an absent target"
        );
    }
    Ok(())
}

#[test]
fn escape_refreshes_an_auto_characters_committed_airborne_target() -> Result<()> {
    let mut prepared = prepared_two_enemy_fixture(Control::Auto)?;
    let action = prepared.resources.actions.insert(ActionDefinition {
        normal: None,
        execution: crate::ActionExecution::Attack(crate::tests::attack(90)),
        tp_cost: 0,
    });
    prepared.resources.actor_setup[0].techniques = vec![crate::PreparedTechnique {
        capabilities: crate::TechniqueCapabilities {
            regal_family: Some(crate::RegalArteFamily::Aerial),
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        },
        ..crate::tests::technique(action, 185)
    }];
    let mut battle = prepared.finish()?;
    battle.runtime[0].set_target(ActorId(1));
    set_launched_jump(&mut battle);
    assert!(battle.queue_technique(ActorId(0), action)?);
    battle.step(BattleInput::default())?;
    assert_eq!(battle.activity(ActorId(0)), Activity::Approaching);
    recognize(&mut battle)?;
    battle.actors[1].availability = crate::ActorAvailability::Absent;

    for _ in 0..4 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(2)));
    assert!(
        battle
            .sequences()
            .any(|(_, sequence)| { sequence.action == action && sequence.target == ActorId(2) })
    );
    Ok(())
}

#[test]
fn escape_raw_attack_does_not_start_ground_nonjump_or_forced_work() -> Result<()> {
    for case in 0..7 {
        let mut battle = fixture(Control::Manual)?;
        let task = match case {
            0 => ActorTask::None,
            1 => ActorTask::Mobility(crate::mobility::Mobility::Jump { launched: false }),
            2 => ActorTask::Mobility(crate::mobility::Mobility::backstep()),
            3 => ActorTask::Mobility(crate::mobility::Mobility::Landing {
                ticks: 2,
                breakfall: false,
            }),
            4 => ActorTask::Taunt { remaining: 30 },
            5 => ActorTask::Mobility(crate::mobility::Mobility::Breakfall),
            6 => ActorTask::Mobility(crate::mobility::Mobility::Jump { launched: true }),
            _ => unreachable!(),
        };
        battle.set_task(0, task);
        if case != 0 {
            battle.actors[0].position[1] = 100.;
            battle.actors[0].movement.vertical = -2.;
        }
        battle.recognize_escape(case == 6)?;
        assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
        for tick in 0..8 {
            let mut input = ControlInput::neutral(ActorId(0));
            input.attack.pressed = tick == 0;
            let frame = battle.step(BattleInput::from(input))?;
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Started { .. }))
            );
            if matches!(case, 0 | 6) {
                assert_eq!(battle.activity(ActorId(0)), Activity::Escaping);
            }
        }
    }
    Ok(())
}

#[test]
fn forced_departure_cancels_an_active_action_without_repaying_or_completing_it() -> Result<()> {
    let mut battle = fixture(Control::Manual)?;
    Arc::make_mut(&mut battle.prepared.actions.entries[0]).tp_cost = 5;
    let frame = battle.step(BattleInput::from(crate::ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: crate::ActionKey(0),
    }))?;
    let action = started_action(&frame, ActorId(0)).expect("attack started");
    let tp = battle.actors[0].tp;
    battle.recognize_escape(true)?;
    assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
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
        crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: crate::ActionKey(0)
        },
        ActorId(1),
        crate::ApproachParameters {
            minimum: 0.,
            maximum: 20.,
            motion: Some(MotionBinding {
                model: ResourceId(7),
                clip: 19
            }),
            motion_rate: 0.5,
            speed: 6.,
            turn_ticks: 8,
        }
    )?);
    let tp = battle.actors[0].tp;
    let uses = battle.technique_counts(ActorId(0)).cloned();
    recognize(&mut battle)?;
    let frame = battle.step(BattleInput::from(crate::ActionRequest {
        actor: ActorId(0),
        target: ActorId(1),
        action: crate::ActionKey(0),
    }))?;
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

    prepared.resources.actor_setup[1].enemy_decision =
        Some(crate::tests::enemy_decision(vec![crate::EnemyChoice {
            return_to_formation: true,
            target_policy: Some(crate::TargetPolicy::Nearest),
            ..crate::tests::enemy_choice(crate::ActionKey(0))
        }]));

    prepared.resources.actor_setup[1].decision = Some(DecisionDefinition {
        idle_ticks: 0,
        idle_variation: 0,
    });
    prepared.finish()
}

#[test]
fn escape_finishes_the_enemys_current_attack_without_followup_decisions() -> Result<()> {
    let mut battle = enemy_fixture()?;
    crate::tests::attack_mut(Arc::make_mut(&mut battle.prepared.actions.entries[0])).chain_at =
        Some(3);
    let choices = &mut battle.prepared.actor_setup[1]
        .enemy_decision
        .as_mut()
        .unwrap()
        .choices;
    choices[0].chain_group = 1;
    choices[0].followup_chance = 100;
    let mut followup = choices[0].clone();
    followup.chain_group = 2;
    choices.push(followup);
    let frame = battle.step(BattleInput::from(crate::ActionRequest {
        actor: ActorId(1),
        target: ActorId(0),
        action: crate::ActionKey(0),
    }))?;
    let action = started_action(&frame, ActorId(1)).expect("enemy attack started");
    battle.runtime[1].enemy_choice = Some(crate::EnemyChoiceId(0));
    recognize(&mut battle)?;
    let mut completed = false;
    let mut followup_window_visited = false;
    for _ in 0..64 {
        // Entering departure and returning to idle each resample ordinary
        // guard chance. The live action's visits must not sample followups.
        let action_visit =
            battle.activity(ActorId(0)) == Activity::Escaping && battle.sequence(&action).is_some();
        followup_window_visited |= action_visit
            && battle
                .sequence(&action)
                .is_some_and(|sequence| sequence.age >= 3);
        let random = battle.random_state();
        let frame = battle.step(BattleInput::default())?;
        assert!(!frame.cues.contains(&Cue::Interrupted { action }));
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        if action_visit {
            assert_eq!(battle.random_state(), random);
        }
        completed |= frame.cues.contains(&Cue::Completed { action });
    }
    assert!(completed);
    assert!(followup_window_visited);
    Ok(())
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
        crate::ActionRequest {
            actor: ActorId(1),
            target: ActorId(0),
            action: crate::ActionKey(0)
        },
        ActorId(0),
        crate::ApproachParameters {
            minimum: 0.,
            maximum: 200.,
            motion: Some(MotionBinding {
                model: ResourceId(7),
                clip: 0
            }),
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
