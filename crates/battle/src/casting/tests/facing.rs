use super::*;

fn controlled(mode: Control, duration: u16) -> Battle {
    let mut prepared = super::spell_charge::charged_prepared(mode, duration);
    prepared.resources.actor_setup[0].spell_charge = None;
    Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).turn_ticks = 8;
    prepared.actors[0].movement.direction = [0., 0., 1.];
    prepared.actors[0].facing_direction = [0., 0., 1.];
    prepared.actors[1].position = [0., 0., 1000.];

    // Keep autonomous decisions inactive while testing cast controls.
    crate::companion::tests::inert_decision(&mut prepared, ActorId(0));
    prepared.resources.actor_setup[0].companion = Some(crate::CompanionDefinition {
        initial_policy: [1, 1, 1],
        defaults: [1, 1, 1],
        limits: [crate::PolicyLimits::default(); 9],
        level: 1,
        level_difference: 0,
    });
    prepared.finish().unwrap()
}

fn held(guard: bool, technique: bool) -> BattleInput {
    let mut input = crate::ControlInput::neutral(ActorId(0));
    input.guard.held = guard;
    input.technique.held = technique;
    BattleInput {
        controllers: vec![input],
        ..Default::default()
    }
}

fn cast_id(battle: &Battle) -> ActionId {
    battle
        .sequences()
        .find_map(|(&id, row)| (row.actor == ActorId(0) && row.action == CAST).then_some(id))
        .expect("actual casting sequence")
}

fn turn_right(battle: &mut Battle) {
    battle.actors[0].heading = 0.;
    battle.actors[0].facing_direction = [0., 0., 1.];
    battle.actors[0].movement.direction = [1., 0., 0.];
}

#[test]
fn unfinished_cast_turn_holds_task_and_motion() {
    {
        let mut battle = controlled(Control::Manual, 8);
        battle.step(request()).unwrap();
        battle.step(held(false, false)).unwrap();
        assert_eq!(cast_remaining(&battle), 7);
        let id = cast_id(&battle);
        let age = battle.action_age(id);
        let position = battle.actors[0].position;
        let tp = battle.actors[0].tp;
        battle.actors[0].movement.forward = 3.;
        turn_right(&mut battle);
        let mut held_updates = 0;
        for _ in 0..8 {
            let frame = battle.step(held(false, false)).unwrap();
            if battle.actors[0].heading == 90. {
                break;
            }
            held_updates += 1;
            assert_eq!(battle.action_age(id), age);
            assert_eq!(cast_remaining(&battle), 7);
            assert_eq!(battle.actors[0].position, position);
            assert_eq!(battle.actors[0].movement.forward, 3.);
            assert_eq!(battle.actors[0].tp, tp);
            assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
        }
        assert!(
            held_updates > 0,
            "turning must hold the cast before resuming"
        );
        assert_eq!(battle.actors[0].heading, 90., "turn must complete");
        assert_eq!(battle.action_age(id), age.map(|age| age + 1));
        assert_eq!(cast_remaining(&battle), 6);
        assert_eq!(battle.actors[0].position[0], position[0] + 3.);
        assert!(battle.actors[0].movement.forward > 0. && battle.actors[0].movement.forward < 3.);
    }
}

#[test]
fn released_guard_input_is_not_retained_while_turning() {
    let mut battle = controlled(Control::Manual, 20);
    battle.step(request()).unwrap();
    turn_right(&mut battle);
    battle.step(held(true, false)).unwrap();
    for _ in 0..12 {
        battle.step(held(false, false)).unwrap();
    }
    assert!(matches!(
        battle.activity(ActorId(0)),
        crate::Activity::Casting { held: false }
    ));
    assert!(!battle.cast_guard_requested(ActorId(0)));
}

#[test]
fn local_hit_stop_holds_cast_integration_but_not_its_callback() {
    {
        let mut battle = controlled(Control::Manual, 8);
        battle.step(request()).unwrap();
        let position = battle.actors[0].position;
        battle.actors[0].movement.forward = 3.;
        battle.actors[0].hit_stop = 2;
        for remaining in [7, 6] {
            battle.step(held(false, false)).unwrap();
            assert_eq!(cast_remaining(&battle), remaining);
            assert_eq!(battle.actors[0].position, position);
            assert_eq!(battle.actors[0].movement.forward, 3.);
        }
        assert_eq!(battle.actors[0].hit_stop, 0);
        battle.step(held(false, false)).unwrap();
        assert_eq!(cast_remaining(&battle), 5);
        assert_eq!(battle.actors[0].position[2], position[2] + 3.);
        assert!(battle.actors[0].movement.forward > 0. && battle.actors[0].movement.forward < 3.);
    }
}

#[test]
fn zero_direction_does_not_stall_casting_and_floor_is_constrained() {
    let mut battle = controlled(Control::Manual, 8);
    battle.actors[0].movement.direction = [0.; 3];
    battle.actors[0].position[1] = -1.;
    let frame = battle.step(request()).unwrap();

    assert_eq!(frame.actors[0].position[1], 0.);
    assert!(
        frame.actors[0]
            .position
            .iter()
            .all(|value| value.is_finite())
    );
    let remaining = cast_remaining(&battle);
    battle.step(held(false, false)).unwrap();
    assert!(cast_remaining(&battle) < remaining);
}

#[test]
fn airborne_actors_cannot_start_casts_but_existing_casts_can_continue_without_turning() {
    let mut fresh = controlled(Control::Manual, 8);
    fresh.actors[0].position[1] = 1.;
    let rejected = fresh.step(request()).unwrap();
    assert!(rejected.cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            actor: ActorId(0),
            ..
        }
    )));

    assert!(fresh.sequences().map(|(_, sequence)| sequence).all(|row| {
        row.actor != ActorId(0)
            || !matches!(
                &row.definition.execution,
                crate::ActionExecution::Casting(_)
            )
    }));
    assert_eq!(fresh.technique_uses(ActorId(0), 66), Some(49));

    for airborne in [false, true] {
        let mut battle = controlled(Control::Manual, 8);
        battle.step(request()).unwrap();

        turn_right(&mut battle);
        battle.actors[0].movement.turning_disabled = !airborne;
        battle.actors[0].position[1] = if airborne { 1. } else { 0. };
        let remaining = cast_remaining(&battle);
        battle.step(held(false, false)).unwrap();
        assert!(cast_remaining(&battle) < remaining);
        assert_eq!(battle.actors[0].heading, 0.);
        assert_eq!(battle.actors[0].movement.direction, [1., 0., 0.]);
    }
}

#[test]
fn ready_release_waits_for_facing_before_queue_retirement_and_use() {
    {
        let mut battle = controlled(Control::Manual, 1);
        battle
            .queue_technique_target(ActorId(0), CAST, ActorId(1))
            .unwrap();
        battle.step(request()).unwrap();
        battle.step(held(false, true)).unwrap();
        assert_eq!(cast_remaining(&battle), 0);
        let tp = battle.actors[0].tp;
        turn_right(&mut battle);
        for _ in 0..8 {
            battle.step(held(true, true)).unwrap();
            if battle.actors[0].heading == 90. {
                break;
            }
            assert_eq!(battle.pending_technique(ActorId(0)), Some(CAST));
            assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
            assert_eq!(battle.actors[0].tp, tp);
        }
        assert_eq!(battle.actors[0].heading, 90., "turn must complete");
        assert_eq!(
            battle.pending_technique(ActorId(0)),
            Some(CAST),
            "holding Techs still delays the ready cast after turning"
        );
        battle.step(held(false, false)).unwrap();
        assert_eq!(battle.pending_technique(ActorId(0)), None);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        assert!(battle.actors[0].tp < tp);
    }
}

#[test]
fn released_cast_recovery_uses_normal_movement_without_the_cast_turn_or_hit_stop_gate() {
    let mut battle = controlled(Control::Manual, 1);
    battle.step(request()).unwrap();
    for _ in 0..80 {
        if battle.activity(ActorId(0)) == crate::Activity::Recovering {
            break;
        }
        battle.step(held(false, false)).unwrap();
    }
    assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
    let id = cast_id(&battle);
    let remaining = battle.action_recovery_remaining(id).unwrap();
    assert!(remaining > 1);
    let position = battle.actors[0].position;
    turn_right(&mut battle);
    battle.actors[0].movement.forward = 3.;
    battle.actors[0].hit_stop = 2;
    battle.step(held(false, false)).unwrap();
    assert_eq!(battle.action_recovery_remaining(id), Some(remaining - 1));
    assert_eq!(battle.actors[0].position[0], position[0] + 3.);
    assert!(battle.actors[0].movement.forward > 0. && battle.actors[0].movement.forward < 3.);
    assert_eq!(battle.actors[0].heading, 0.);
}
