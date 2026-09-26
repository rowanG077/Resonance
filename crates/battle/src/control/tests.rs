use super::*;
use crate::{BattleInput, HurtPoint, Side, tests::actor};
use std::{collections::BTreeMap, sync::Arc};

fn battle(control: Control) -> Battle {
    let sources = BTreeMap::from([("control".into(), "script battle; use battle; pub task run() { await battle::at_age(ticks(90)); battle::finish(); }".into())]);
    let compiled =
        symphonia_script_compiler::compile("control", &sources, &crate::native_declarations())
            .unwrap();
    let entry = compiled.program.authored().unwrap().functions[0].entry;
    let program = Arc::new(compiled.program);
    let actions = (0..7)
        .map(|id| crate::ActionDefinition {
            id,
            phase: ActionPhase::Actor,
            program: program.clone(),
            entry,
            duration: 90,
            tp_cost: 0,
            resources: vec![],
        })
        .collect();
    let mut player = actor(Side::Party);
    player.control = control;
    player.heading = 90.;
    player.movement.direction = [1., 0., 0.];
    player.facing_direction = [1., 0., 0.];
    // Direct initializer tests below start with an already sampled18FC.
    player.movement.target_direction = [1., 0., 0.];
    player.movement.braking = 1.;
    player.body.approach_points.push(HurtPoint {
        center: [0.; 3],
        radius: 10.,
    });
    let mut enemy = actor(Side::Enemy);
    enemy.position = [100., 0., 0.];
    enemy.body.target_center = enemy.position;
    enemy.body.approach_points.push(HurtPoint {
        center: enemy.position,
        radius: 10.,
    });
    let mut other = enemy.clone();
    other.position[0] = 200.;
    other.body.target_center = other.position;
    other.body.approach_points[0].center = other.position;
    let prepared =
        PreparedBattle::new(vec![player, enemy, other], actions, 0, vec![], vec![]).unwrap();
    let prepared = prepared
        .with_controls(vec![ControlDefinition {
            actor: ActorId(0),
            target: ActorId(1),
            combo_limit: 3,
            walk_speed: 5.,
            run_speed: 10.,
            turn_ticks: 8,
            motions: None,
            shortcuts: [None; 4],
            normals: std::array::from_fn(|i| NormalControl {
                action: i as u16,
                allowed_directions: [14, 96, 11, 7, 0, 64, 32][i],
                fallback: [Some(4), Some(5), Some(0), Some(0), None, Some(6), Some(5)][i],
                reach: 120.,
                minimum_reach: 0.,
                combo_at: [15, 0],
                buffer_until: 60,
            }),
        }])
        .unwrap();
    Battle::new(Arc::new(prepared))
}

fn companion_battle(colette: bool) -> Battle {
    let mut battle = battle(Control::Auto);
    let source = format!(
        "script battle; use battle; use battle::companion_ai; pub task decide() {{ spawn companion_ai::combos({colette}); while true {{ battle::ai_set_idle_timer(battle::ai_idle_timer() + 1); await battle::next_update(); }} }} pub task arte() {{ battle::pay_tp(battle::tp_cost()); await battle::at_age(ticks(30)); battle::finish(); }}"
    );
    let sources = BTreeMap::from([
        ("policy".into(), source),
        (
            "battle::companion_ai".into(),
            include_str!("../../../../scripts/battle/companion_ai.sym").into(),
        ),
    ]);
    let compiled =
        symphonia_script_compiler::compile("policy", &sources, &crate::native_declarations())
            .unwrap();
    let entry = |name: &str| {
        compiled
            .program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name || f.name.ends_with(&format!("::{name}")))
            .unwrap()
            .entry
    };
    let decision_entry = entry("decide");
    let arte_entry = entry("arte");
    let program = Arc::new(compiled.program);
    let prepared = Arc::get_mut(&mut battle.prepared).unwrap();
    prepared.actions.push(crate::ActionDefinition {
        id: 7,
        phase: if colette {
            ActionPhase::Actor
        } else {
            ActionPhase::Casting
        },
        program: program.clone(),
        entry: arte_entry,
        duration: 30,
        tp_cost: 5,
        resources: vec![],
    });
    prepared.actions.push(crate::ActionDefinition {
        id: 8,
        phase: ActionPhase::Decision,
        program,
        entry: decision_entry,
        duration: 0,
        tp_cost: 0,
        resources: vec![],
    });
    prepared.companions[0] = Some(crate::CompanionDefinition {
        actor: ActorId(0),
        strategy: [3, 6, if colette { 2 } else { 5 }],
        saved_position: 0,
        level: 1,
        level_difference: 0,
        tp_limit: 30,
        healing_limit: 65,
        support_level_limit: 2,
        techniques: vec![crate::CompanionTechnique {
            action: 7,
            enabled: true,
            flags: if colette { 0x41106 } else { 0x444186 },
            cost: 5,
            learning_route: 0,
            minimum: 400.,
            maximum: 500.,
        }],
    });
    let definition = crate::DecisionDefinition {
        actor: ActorId(0),
        target: ActorId(1),
        action: 8,
        idle_ticks: 0,
        idle_variation: 0,
        fidget_ticks: 0,
        idle_motion: None,
    };
    prepared.decisions[0] = Some(definition);
    battle.decisions[0] = Some(crate::decision::Decision::new(definition));
    battle.actors[0].tp = battle.actors[0].max_tp;
    battle
        .start_automatic_normal(ActorId(0), 0, ActorId(1), &mut vec![])
        .unwrap();
    battle
}

#[test]
fn automatic_chain_requires_contact_and_does_not_revisit_the_idle_task() -> Result<()> {
    let mut whiff = companion_battle(true);
    for _ in 0..16 {
        whiff.step(BattleInput::default())?;
    }
    assert_eq!(whiff.random_state(), 0);
    assert_eq!(
        whiff.controls[0].as_ref().unwrap().normal.unwrap().0,
        ActionId(1)
    );
    assert_eq!(whiff.idle_timers[0], 16);

    let mut hit = companion_battle(true);
    for _ in 0..15 {
        hit.step(BattleInput::default())?;
    }
    hit.confirm_actor_contact(ActorId(0));
    let tp = hit.actors[0].tp;
    hit.step(BattleInput::default())?;
    let mut expected = crate::Random::from_state(0);
    expected.next(); // level-one eligibility, modulo one
    expected.next(); // one learned non-casting candidate
    assert_eq!(hit.random_state(), expected.state());
    assert_eq!(
        hit.idle_timers[0], 16,
        "combo child must not revisit main NextUpdate"
    );
    assert_eq!(hit.sequences[&ActionId(3)].definition.id, 7);
    assert_eq!(
        hit.actors[0].tp, tp,
        "new initializer runs on the next actor callback"
    );
    hit.step(BattleInput::default())?;
    assert_eq!(hit.actors[0].tp, tp - 5);
    Ok(())
}

#[test]
fn casting_only_companion_chain_keeps_all_nine_by_one_hundred_rejection_draws() -> Result<()> {
    let mut battle = companion_battle(false);
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    battle.confirm_actor_contact(ActorId(0));
    battle.step(BattleInput::default())?;
    let mut expected = crate::Random::from_state(0);
    for _ in 0..901 {
        expected.next();
    }
    let mut selected = expected.next() % 5;
    while selected == 1 {
        selected = expected.next() % 5;
    }
    assert_eq!(battle.random_state(), expected.state());
    assert_eq!(battle.controls[0].as_ref().unwrap().combo, 1);
    assert_eq!(battle.actors[0].attack_power, 85);
    assert_eq!(battle.idle_timers[0], 16);
    assert_ne!(battle.sequences[&ActionId(3)].definition.id, 7);
    Ok(())
}

fn approach_policy_battle(control: Control, colette: bool) -> Battle {
    let mut prepared = Arc::try_unwrap(battle(control).prepared).unwrap();
    prepared.actors[1].position[0] = 600.;
    prepared.actors[1].body.center = prepared.actors[1].position;
    prepared.actors[1].body.approach_points[0].center = prepared.actors[1].position;
    let source = format!(
        "script battle; use battle; use battle::companion_ai; pub task decide() {{ spawn companion_ai::approaches({colette}); while true {{ battle::ai_set_idle_timer(battle::ai_idle_timer() + 1); await battle::next_update(); }} }}"
    );
    let sources = BTreeMap::from([
        ("policy".into(), source),
        (
            "battle::companion_ai".into(),
            include_str!("../../../../scripts/battle/companion_ai.sym").into(),
        ),
    ]);
    let compiled =
        symphonia_script_compiler::compile("policy", &sources, &crate::native_declarations())
            .unwrap();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == "decide" || f.name.ends_with("::decide"))
        .unwrap()
        .entry;
    prepared.actions.push(crate::ActionDefinition {
        id: 8,
        phase: ActionPhase::Decision,
        program: Arc::new(compiled.program),
        entry,
        duration: 0,
        tp_cost: 0,
        resources: vec![],
    });
    if colette {
        prepared = prepared
            .with_companions(vec![crate::CompanionDefinition {
                actor: ActorId(0),
                strategy: [3, 6, 2],
                saved_position: 0,
                level: 1,
                level_difference: 0,
                tp_limit: 30,
                healing_limit: 65,
                support_level_limit: 2,
                techniques: vec![],
            }])
            .unwrap();
    }
    prepared = prepared
        .with_decisions(vec![crate::DecisionDefinition {
            actor: ActorId(0),
            target: ActorId(1),
            action: 8,
            idle_ticks: 0,
            idle_variation: 0,
            fidget_ticks: 0,
            idle_motion: None,
        }])
        .unwrap();
    let mut battle = Battle::new(Arc::new(prepared));
    battle
        .request_approach(
            ActorId(0),
            ActorId(1),
            0,
            crate::ApproachParameters {
                minimum: 0.,
                maximum: 120.,
                motion: None,
                stop_motion: None,
                motion_rate: 0.5,
                speed: 6.,
                turn_ticks: 8,
            },
        )
        .unwrap();
    battle
}

#[test]
fn moving_normal_reselection_crosses_exact_height_boundary_without_extra_idle_visit() -> Result<()>
{
    let mut battle = approach_policy_battle(Control::SemiAuto, false);
    battle.actors[1].position[1] = 99.999;
    battle.step(BattleInput::default())?; // Requested -> Moving; no callback yet.
    assert_eq!(battle.approach_normal(ActorId(0)), Some(0));
    let random = battle.random_state();
    battle.actors[1].position[1] = 100.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.approach_normal(ActorId(0)), Some(1));
    assert_eq!(
        battle.random_state(),
        random,
        "upward transition consumes no random draw"
    );
    assert_eq!(
        battle.idle_timers[0], 2,
        "approach callback must not revisit NextUpdate"
    );
    battle.actors[1].position[1] = 99.999;
    let mut expected = crate::Random::from_state(random);
    let mut selector = expected.next() & 3;
    while selector == 1 {
        selector = expected.next() & 3;
    }
    battle.step(BattleInput::default())?;
    assert_eq!(battle.approach_normal(ActorId(0)), Some(selector as u8));
    assert_eq!(battle.random_state(), expected.state());
    assert_eq!(battle.idle_timers[0], 3);
    Ok(())
}

#[test]
fn requested_auto_keeps_selected_range_and_only_reevaluates_immediate_admission() -> Result<()> {
    for (root_x, requested_selector) in [(110., 1), (130., 0)] {
        let mut battle = approach_policy_battle(Control::Auto, false);
        Arc::get_mut(&mut battle.prepared).unwrap().controls[0]
            .as_mut()
            .unwrap()
            .normals[1]
            .reach = 100.;
        battle.approaches[0] = None;
        battle.actors[0].activity = Activity::Idle;
        battle.actors[1].position = [root_x, 100., 0.];
        battle.actors[1].body.approach_points[0].center = [root_x, 0., 0.];
        battle.request_approach(
            ActorId(0),
            ActorId(1),
            1,
            crate::ApproachParameters {
                minimum: 0.,
                maximum: 100.,
                motion: None,
                stop_motion: None,
                motion_rate: 0.5,
                speed: 6.,
                turn_ticks: 8,
            },
        )?;
        assert_eq!(
            battle.approach_normal(ActorId(0)),
            Some(0),
            "298C0 clears the upward selector first"
        );
        battle.step(BattleInput::default())?;
        assert_eq!(battle.approach_normal(ActorId(0)), Some(requested_selector));
        // At gap110, retaining the selected100 range requires Moving. Replacing
        // it with neutral120 would incorrectly reevaluate on Requested above.
        if root_x == 130. {
            battle.actors[1].position[1] = 100.;
            battle.step(BattleInput::default())?;
            assert_eq!(battle.approach_normal(ActorId(0)), Some(1));
        }
    }
    Ok(())
}

#[test]
fn manual_mode_excludes_moving_normal_reevaluation() -> Result<()> {
    let mut battle = approach_policy_battle(Control::SemiAuto, false);
    battle.step(BattleInput::default())?;
    // Preserve an already-moving request across an ordinary control-mode change.
    battle.actors[0].control = Control::Manual;
    battle.actors[1].position[1] = 100.;
    let random = battle.random_state();
    battle.step(BattleInput::default())?;
    assert_eq!(battle.approach_normal(ActorId(0)), Some(0));
    assert_eq!(battle.random_state(), random);
    Ok(())
}

#[test]
fn colette_downward_reselection_consumes_random_before_flying_target_override() -> Result<()> {
    let mut battle = approach_policy_battle(Control::Auto, true);
    battle.actors[1].movement.flying = true;
    battle.step(BattleInput::default())?;
    battle.actors[1].position[1] = 100.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.approach_normal(ActorId(0)), Some(1));
    let mut expected = crate::Random::from_state(battle.random_state());
    while expected.next() & 3 == 1 {}
    battle.actors[1].position[1] = 99.999;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.approach_normal(ActorId(0)), Some(3));
    assert_eq!(battle.random_state(), expected.state());
    Ok(())
}

fn input(stick: [i8; 2], pressed: bool) -> BattleInput {
    BattleInput {
        controllers: vec![ControlInput {
            stick,
            attack: ButtonInput {
                held: pressed,
                pressed,
                released: false,
            },
            ..ControlInput::neutral(ActorId(0))
        }],
        ..Default::default()
    }
}

#[test]
fn directional_boundaries_preserve_vertical_priority_and_air_selection() {
    assert_eq!(normal_direction([48, 48], 0.1), 0);
    assert_eq!(normal_direction([-49, 48], 0.1), 3);
    assert_eq!(normal_direction([49, 49], 0.1), 1);
    assert_eq!(normal_direction([49, -49], 0.1), 2);
    assert_eq!(normal_direction([0, -48], 0.1001), 5);
    assert_eq!(normal_direction([0, -49], 0.1001), 6);
}

#[test]
fn only_attack_edges_start_and_buffered_neutral_uses_source_fallback() -> Result<()> {
    let mut battle = battle(Control::Manual);
    let mut held = input([0, 0], false);
    held.controllers[0].attack.held = true;
    assert!(battle.step(held)?.actions.is_empty());
    let frame = battle.step(input([0, 0], true))?;
    assert!(frame.actions.is_empty());
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.actions, [(ActionId(1), ActorId(0), 1)]);
    let after_start = battle.random_state();
    battle.step(input([0, 0], true))?;
    for _ in 0..13 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.action_age(ActionId(1)), Some(15));
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.actions, [(ActionId(2), ActorId(0), 0)]);
    assert_eq!(battle.sequences[&ActionId(2)].definition.id, 4);
    assert_eq!(frame.actors[0].attack_power, 85);
    assert_eq!(battle.random_state(), after_start);
    assert!(matches!(
        frame.cues.as_slice(),
        [
            Cue::Completed {
                action: ActionId(1)
            },
            Cue::Started {
                action: ActionId(2),
                actor: ActorId(0)
            }
        ]
    ));
    Ok(())
}

#[test]
fn permitted_directions_chain_and_limit_prevents_a_fourth_normal() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    battle.step(BattleInput::default())?;
    battle.step(input([80, 0], true))?;
    for _ in 0..14 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.sequences[&ActionId(2)].definition.id, 3);
    battle.step(input([0, 0], true))?;
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.sequences[&ActionId(3)].definition.id, 0);
    assert_eq!(battle.actors[0].attack_power, 70);
    battle.step(input([80, 0], true))?;
    for _ in 0..20 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.controls[0].as_ref().unwrap().combo, 2);
    assert!(battle.sequences.contains_key(&ActionId(3)));
    assert_eq!(battle.next_action, 4);
    Ok(())
}

#[test]
fn hit_stop_accepts_an_edge_but_menu_pause_discards_it() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    battle.step(BattleInput::default())?;
    let mut paused = input([80, 0], true);
    paused.menu_open = true;
    battle.step(paused)?;
    assert_eq!(battle.controls[0].as_ref().unwrap().buffered, None);
    battle.actors[0].hit_stop = 3;
    battle.step(input([80, 0], true))?;
    assert_eq!(battle.action_age(ActionId(1)), Some(1));
    assert_eq!(battle.controls[0].as_ref().unwrap().buffered, Some(3));
    Ok(())
}

#[test]
fn semi_auto_uses_body_gap_and_manual_attacks_at_any_distance() -> Result<()> {
    let mut semi = battle(Control::SemiAuto);
    semi.actors[1].position[0] = 400.;
    semi.actors[1].body.approach_points[0].center[0] = 400.;
    let frame = semi.step(input([0, 0], true))?;
    assert_eq!(frame.actors[0].activity, Activity::Approaching);
    assert!(frame.actions.is_empty());
    assert_eq!(
        frame.actors[0].position[0],
        crate::distance::planar_direction([400., 0., 0.], [0.; 3], [1., 0., 0.])[0] * 10.
    );
    semi.actors[1].body.approach_points[0].center[0] = 120.;
    assert!(semi.step(BattleInput::default())?.actions.is_empty());
    let frame = semi.step(BattleInput::default())?;
    assert!(matches!(frame.actors[0].activity, Activity::Action { .. }));
    assert_eq!(frame.actions.len(), 1);
    let mut manual = battle(Control::Manual);
    manual.actors[1].body.approach_points[0].center[0] = 1000.;
    assert!(manual.step(input([0, 0], true))?.actions.is_empty());
    assert_eq!(manual.step(BattleInput::default())?.actions.len(), 1);
    Ok(())
}

#[test]
fn approach_distance_uses_body_flags_scaled_radii_and_source_empty_bound() {
    let mut source = actor(Side::Party);
    let mut target = actor(Side::Enemy);
    source.body.points.push(HurtPoint {
        center: [0.; 3],
        radius: 1000.,
    });
    target.body.points = source.body.points.clone();
    assert_eq!(body_gap(&source, &target), 10000.);
    source.body.scale = 2.;
    source.body.approach_points.push(HurtPoint {
        center: [0., 90., 0.],
        radius: 10.,
    });
    target.body.approach_points.push(HurtPoint {
        center: [100., -90., 0.],
        radius: 5.,
    });
    assert_eq!(
        body_gap(&source, &target),
        crate::distance::length([100., 0., 0.]) - 25.
    );
    target.body.approach_points[0].center[0] = 20.;
    assert_eq!(body_gap(&source, &target), 0.01);
}

#[test]
fn target_tap_avoids_current_and_held_selector_freezes_actor_time() -> Result<()> {
    let mut battle = battle(Control::Manual);
    assert_eq!(battle.snapshot().actors[0].hud.target_highlight, 120);
    let mut tap = input([0, 0], false);
    tap.controllers[0].target.released = true;
    assert_eq!(battle.step(tap)?.targets[0], Some(ActorId(2)));
    for visit in 0..10 {
        let mut held = input([0, 0], false);
        held.controllers[0].target = ButtonInput {
            held: true,
            pressed: visit == 0,
            released: false,
        };
        let frame = battle.step(held)?;
        assert_eq!(frame.target_selector.is_some(), visit == 9);
        assert_eq!(frame.hud_holds, crate::HudHolds::default());
    }
    let clock = 11;
    let hud_clock = battle.hud_update;
    battle.actors[0].hp = 39;
    battle.actors[0].hud.portrait_bounce = 14;
    battle.show_recovery(ActorId(0), crate::RecoveryKind::Tp, 12)?;
    let mut select = input([0, 0], false);
    select.controllers[0].target.held = true;
    select.controllers[0].target_step = -1;
    let frame = battle.step(select)?;
    assert_eq!(frame.update, clock);
    assert_eq!(frame.hud_update, hud_clock + 1);
    assert_eq!(
        frame.hud_holds,
        crate::HudHolds {
            notices: true,
            combo_tracking: true,
            intro: false
        }
    );
    assert_eq!(frame.actors[0].hud.target_highlight, 120);
    assert_eq!(frame.actors[0].hud.hp, 39);
    assert_eq!(frame.actors[0].hud.portrait_bounce, 14);
    assert_eq!(frame.actors[0].hud.recovery[1].x_delta, 24);
    assert_eq!(frame.targets[0], Some(ActorId(1)));
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.target_selector, None);
    assert_eq!(frame.targets[0], Some(ActorId(1)));
    assert_eq!(frame.update, clock);
    assert_eq!(frame.hud_update, hud_clock + 2);
    assert_eq!(
        frame.hud_holds,
        crate::HudHolds {
            notices: true,
            combo_tracking: true,
            intro: false
        }
    );
    assert_eq!(frame.actors[0].hud.target_highlight, 120);
    assert_eq!(frame.actors[0].hud.recovery[1].x_delta, 47);
    let held = battle.step(BattleInput {
        menu_open: true,
        ..Default::default()
    })?;
    assert_eq!(held.hud_update, frame.hud_update);
    assert_eq!(held.hud_holds, frame.hud_holds);
    assert_eq!(held.actors[0].hud.target_highlight, 120);
    let resumed = battle.step(BattleInput::default())?;
    assert_eq!(resumed.hud_update, frame.hud_update + 1);
    assert_eq!(resumed.hud_holds, crate::HudHolds::default());
    assert_eq!(resumed.actors[0].hud.target_highlight, 119);
    Ok(())
}

#[test]
fn bad_controller_input_is_rejected_before_mutation() {
    let mut battle = battle(Control::Manual);
    let mut duplicate = input([0, 0], true);
    duplicate.controllers.push(duplicate.controllers[0]);
    assert!(battle.step(duplicate).is_err());
    assert_eq!(battle.random_state(), 0);
    assert_eq!(battle.next_action, 1);
    assert!(battle.step(input([0, 0], true)).is_ok());
}

#[test]
fn prepared_input_remains_valid_after_result_controller_retirement() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.recognize_escape(false)?;
    assert_eq!(
        battle.recognize_result(),
        Some(crate::BattleResult::Escaped)
    );
    battle.retire_combat()?;
    // Result construction replaces this mutable controller; its immutable
    // device binding still belongs to the same prepared generation.
    battle.controls[0] = None;
    let before = battle.snapshot();
    let frame = battle.step(input([80, 0], true))?;
    assert_eq!(frame.actors[0].position, before.actors[0].position);
    assert_eq!(battle.next_action, 1);
    assert!(battle.controls[0].is_none());
    let mut unknown = input([0, 0], false);
    unknown.controllers[0].actor = ActorId(1);
    assert!(battle.step(unknown).is_err());
    Ok(())
}

#[test]
fn run_release_reversal_and_stop_follow_the_packed_source_counter() -> Result<()> {
    let mut short = battle(Control::Manual);
    short.step(input([80, 0], false))?;
    assert_eq!(short.actors[0].movement.forward, 5.5);
    short.step(input([-80, 0], false))?;
    assert_eq!(
        short.controls[0].as_ref().unwrap().locomotion,
        Locomotion::Walk
    );
    assert!(short.actors[0].movement.direction[0] > 0.);
    assert_eq!(short.actors[0].movement.forward, 5.);
    short.step(input([-80, 0], false))?;
    assert!(short.actors[0].movement.direction[0] < 0.);

    let mut long = battle(Control::Manual);
    for _ in 0..16 {
        long.step(input([80, 0], false))?;
    }
    assert_eq!(long.controls[0].as_ref().unwrap().run_ticks, 72);
    let direction = long.actors[0].movement.direction;
    long.step(input([-80, 0], false))?;
    assert_eq!(
        long.controls[0].as_ref().unwrap().locomotion,
        Locomotion::Stop
    );
    assert_eq!(long.actors[0].movement.forward, 10. - 0.55);
    long.step(input([-80, 0], false))?;
    assert_eq!(long.actors[0].movement.direction, direction);
    assert_eq!(
        long.controls[0].as_ref().unwrap().locomotion,
        Locomotion::Stop
    );
    Ok(())
}

#[test]
fn normal_admission_turns_on_following_visits_without_moving() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.actors[0].heading = 0.;
    battle.actors[0].movement.forward = 4.;
    assert!(battle.step(input([0, 0], true))?.actions.is_empty());
    assert_eq!(battle.actors[0].position[0], 4.);
    // 4DDB4 narrows atan2 before multiplying rodata2808 (0x42652EE4).
    // Its +X heading is 90.000023, so the fourth 22.5-degree visit still
    // reports an unfinished turn; admission is on the fifth visit.
    for heading in [22.5, 45., 67.5, 90.] {
        let frame = battle.step(BattleInput::default())?;
        assert!(frame.actions.is_empty());
        assert_eq!(frame.actors[0].heading, heading);
        assert_eq!(frame.actors[0].position[0], 4.);
    }
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.actions.len(), 1);
    assert_eq!(frame.actors[0].heading.to_bits(), 0x42b4_0003);
    assert_eq!(frame.actors[0].position[0], 4.);
    Ok(())
}

#[test]
fn normal_landing_shortens_action_and_retains_the_animation_clock() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    battle.step(BattleInput::default())?;
    battle.actors[0].position[1] = 10.;
    battle.actors[0].movement.vertical = 2.;
    battle.brake_control(0)?;
    let animation_age = battle.sequences[&ActionId(1)].animation_age;
    battle.actors[0].position[1] = 0.;
    battle.actors[0].movement.vertical = -2.;
    battle.brake_control(0)?;
    let sequence = &battle.sequences[&ActionId(1)];
    assert_eq!(sequence.age, 74);
    assert_eq!(sequence.command_age, 0x400);
    assert_eq!(sequence.hit_age, 0x400);
    assert_eq!(sequence.animation_age, animation_age);
    assert!(battle.controls[0].as_ref().unwrap().landed);
    battle.step(input([80, 0], true))?;
    assert_eq!(battle.controls[0].as_ref().unwrap().buffered, None);
    Ok(())
}

#[test]
fn target_projection_keeps_perspective_depth() {
    let camera = crate::CameraPose {
        eye: [0., 0., 1000.],
        focus: [0.; 3],
        pitch: 0.,
        yaw: 0.,
        radius: 1000.,
    };
    assert_eq!(project_screen_x(camera, [0., 10., 0.]), 320.);
    assert!(project_screen_x(camera, [50., 10., 700.]) > project_screen_x(camera, [100., 10., 0.]));
}

fn player_buttons(attack: bool, technique: bool, guard: bool, stick: [i8; 2]) -> BattleInput {
    BattleInput {
        controllers: vec![ControlInput {
            attack: ButtonInput {
                pressed: attack,
                held: attack,
                released: false,
            },
            technique: ButtonInput {
                pressed: technique,
                held: technique,
                released: false,
            },
            guard: ButtonInput {
                pressed: guard,
                held: guard,
                released: false,
            },
            stick,
            ..ControlInput::neutral(ActorId(0))
        }],
        ..Default::default()
    }
}

fn shortcuts_battle(control: Control) -> Battle {
    let mut battle = battle(control);
    let compiled = symphonia_script_compiler::compile(
        "technique",
        &BTreeMap::from([("technique".into(), "script battle; use battle; pub task run() { battle::pay_tp(battle::tp_cost()); await battle::at_age(ticks(90)); battle::finish(); }".into())]),
        &crate::native_declarations(),
    ).unwrap();
    let entry = compiled.program.authored().unwrap().functions[0].entry;
    let program = Arc::new(compiled.program);
    let prepared = Arc::get_mut(&mut battle.prepared).unwrap();
    for id in 10..14 {
        prepared.actions.push(crate::ActionDefinition {
            id,
            phase: ActionPhase::Actor,
            program: program.clone(),
            entry,
            duration: 90,
            tp_cost: 4,
            resources: vec![],
        });
    }
    prepared.controls[0].as_mut().unwrap().shortcuts = std::array::from_fn(|slot| {
        Some(TechniqueControl {
            action: 10 + slot as u16,
            minimum: 0.,
            maximum: 800.,
        })
    });
    battle
}

#[test]
fn held_guard_activates_next_callback_and_neutral_release_waits_for_zero() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = battle(mode);
        let initial_random = battle.random_state();
        battle.actors[0].movement.forward = 3.;
        let first = battle.step(player_buttons(false, false, true, [0; 2]))?;
        assert_eq!(first.actors[0].activity, Activity::Guarding);
        assert!(!first.actors[0].guard.active);
        assert_eq!(first.actors[0].reaction.remaining, 30);
        assert_eq!(first.actors[0].position[0], 3.);
        assert_eq!(battle.random_state(), initial_random);
        let held = battle.step(player_buttons(false, false, true, [0; 2]))?;
        assert!(held.actors[0].guard.active);
        assert_eq!(held.actors[0].reaction.remaining, 29);
        assert_eq!(held.actors[0].position[0], 6.);
        assert_eq!(held.actors[0].movement.forward, 3. - 0.55);
        // 339F8 retains command4 after release until it observes count0.
        for remaining in (0..29).rev() {
            let frame = battle.step(BattleInput::default())?;
            assert_eq!(frame.actors[0].activity, Activity::Guarding);
            assert!(frame.actors[0].guard.active);
            assert_eq!(frame.actors[0].reaction.remaining, remaining);
            assert_eq!(frame.actors[0].guard.auto_chance, 100);
            assert_eq!(battle.random_state(), initial_random);
        }
        let released = battle.step(BattleInput::default())?;
        assert_eq!(released.actors[0].activity, Activity::Idle);
        assert!(!released.actors[0].guard.active);
        assert_eq!(released.actors[0].reaction.remaining, 29);
        assert_eq!(released.actors[0].reaction.idle_initialization, None);
        let mut random = crate::state::Random(initial_random);
        random.next();
        assert_eq!(battle.random_state(), random.0);
        // There is no second recovery callback or draw blocking the next input.
        battle.step(player_buttons(true, false, false, [0; 2]))?;
        assert_eq!(battle.actors[0].activity, Activity::Approaching);
        random.next();
        assert_eq!(battle.random_state(), random.0);
    }
    Ok(())
}

#[test]
fn guard_holds_after_countdown_and_hit_stop_defers_release() -> Result<()> {
    let mut battle = battle(Control::Manual);
    for _ in 0..40 {
        battle.step(player_buttons(false, false, true, [0; 2]))?;
    }
    assert!(battle.actors[0].guard.active);
    assert_eq!(battle.actors[0].reaction.remaining, 0);
    assert_eq!(battle.random_state(), 0);
    battle.actors[0].hit_stop = 2;
    for _ in 0..2 {
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(frame.actors[0].activity, Activity::Guarding);
        assert!(frame.actors[0].guard.active);
    }
    assert_eq!(battle.random_state(), 0);
    let released = battle.step(BattleInput::default())?;
    assert_eq!(released.actors[0].activity, Activity::Idle);
    assert_eq!(released.actors[0].reaction.remaining, 29);
    let mut random = crate::state::Random(0);
    random.next();
    assert_eq!(battle.random_state(), random.0);
    Ok(())
}

#[test]
fn guard_ignores_direction_until_release_but_attack_takes_precedence() -> Result<()> {
    let mut walking = battle(Control::Manual);
    walking.step(player_buttons(false, false, true, [0; 2]))?;
    // 32738 parameter11 bypasses command1/2 even with a horizontal stick.
    for remaining in (0..30).rev() {
        let frame = walking.step(player_buttons(false, false, false, [80, 0]))?;
        assert_eq!(frame.actors[0].activity, Activity::Guarding);
        assert!(frame.actors[0].guard.active);
        assert_eq!(frame.actors[0].reaction.remaining, remaining);
        assert_eq!(frame.actors[0].movement.forward, 0.);
        assert_eq!(walking.random_state(), 0);
    }
    let released = walking.step(player_buttons(false, false, false, [80, 0]))?;
    assert_eq!(released.actors[0].activity, Activity::Idle);
    assert_eq!(released.actors[0].movement.forward, 0.);
    let mut random = crate::state::Random(0);
    random.next();
    assert_eq!(walking.random_state(), random.0);
    let moved = walking.step(player_buttons(false, false, false, [80, 0]))?;
    assert_eq!(moved.actors[0].movement.forward, 5.);
    assert_eq!(walking.random_state(), random.0);
    let mut attack = battle(Control::Manual);
    attack.step(player_buttons(false, false, true, [0; 2]))?;
    attack.step(player_buttons(true, false, true, [0; 2]))?;
    let mut random = crate::state::Random(0);
    random.next();
    assert_eq!(attack.random_state(), random.0);
    assert_eq!(attack.actors[0].activity, Activity::Approaching);
    assert!(!attack.actors[0].guard.active);
    assert_eq!(attack.step(BattleInput::default())?.actions.len(), 1);
    Ok(())
}

#[test]
fn shortcut_edges_keep_direction_priority_and_pay_tp_only_in_the_initializer() -> Result<()> {
    for (stick, selected) in [
        ([48, 48], 10),
        ([-49, 48], 13),
        ([80, 49], 11),
        ([-80, -49], 12),
    ] {
        let mut battle = shortcuts_battle(Control::Manual);
        let first = battle.step(player_buttons(true, true, true, stick))?;
        assert!(first.actions.is_empty());
        assert_eq!(first.actors[0].tp, 40);
        assert_eq!(first.actors[0].activity, Activity::Approaching);
        let second = battle.step(BattleInput::default())?;
        assert_eq!(second.actors[0].tp, 36);
        assert_eq!(battle.sequences[&ActionId(1)].definition.id, selected);
        for _ in 0..3 {
            let mut held = player_buttons(false, false, true, [0; 2]);
            held.controllers[0].technique.held = true;
            let frame = battle.step(held)?;
            assert_eq!(frame.actors[0].tp, 36);
            assert_eq!(frame.actions.len(), 1);
        }
    }
    Ok(())
}

#[test]
fn insufficient_tp_and_empty_shortcuts_do_not_spend_or_consume_admission_rng() -> Result<()> {
    let mut battle = shortcuts_battle(Control::Manual);
    battle.actors[0].tp = 3;
    let frame = battle.step(player_buttons(false, true, true, [0; 2]))?;
    assert_eq!(frame.actors[0].activity, Activity::Guarding);
    assert_eq!(frame.actors[0].tp, 3);
    assert_eq!(battle.random_state(), 0);
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            reason: crate::Rejection::InsufficientTp,
            ..
        }
    )));
    let mut empty = shortcuts_battle(Control::Manual);
    Arc::get_mut(&mut empty.prepared).unwrap().controls[0]
        .as_mut()
        .unwrap()
        .shortcuts[0] = None;
    let frame = empty.step(player_buttons(true, true, false, [0; 2]))?;
    assert!(frame.actions.is_empty());
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert_eq!(empty.random_state(), 0);
    Ok(())
}

#[test]
fn technique_uses_player_range_and_shared_semi_approach() -> Result<()> {
    let mut near = shortcuts_battle(Control::SemiAuto);
    near.step(player_buttons(false, true, false, [0; 2]))?;
    assert_eq!(near.actors[0].position[0], 0.); // Body gap80, below Auto's700 minimum.
    assert_eq!(near.step(BattleInput::default())?.actors[0].tp, 36);
    let mut far = shortcuts_battle(Control::SemiAuto);
    far.actors[1].position[0] = 1000.;
    far.actors[1].body.approach_points[0].center[0] = 1000.;
    let frame = far.step(player_buttons(false, true, false, [0; 2]))?;
    assert_eq!(frame.actors[0].position[0], 10.);
    assert_eq!(frame.actors[0].movement.forward, 10.);
    assert_eq!(frame.actors[0].tp, 40);
    assert!(frame.actions.is_empty());
    Ok(())
}

#[test]
fn third_normal_can_buffer_a_technique_during_hit_stop_without_a_fourth_normal() -> Result<()> {
    let mut battle = shortcuts_battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    battle.step(BattleInput::default())?;
    battle.step(input([80, 0], true))?;
    for _ in 0..14 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.sequences[&ActionId(2)].definition.id, 3);
    battle.step(input([0, 0], true))?;
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.controls[0].as_ref().unwrap().combo, 2);
    assert_eq!(battle.actors[0].attack_power, 70);
    let random = battle.random_state();
    battle.actors[0].hit_stop = 2;
    battle.step(player_buttons(false, true, false, [0; 2]))?;
    assert_eq!(
        battle.controls[0].as_ref().unwrap().chained_technique,
        Some(10)
    );
    assert_eq!(battle.actors[0].tp, 40);
    for _ in 0..30 {
        battle.step(BattleInput::default())?;
        if battle
            .sequences
            .get(&ActionId(4))
            .is_some_and(|sequence| sequence.age > 0)
        {
            break;
        }
    }
    assert_eq!(battle.sequences[&ActionId(4)].definition.id, 10);
    assert_eq!(battle.actors[0].tp, 36);
    assert_eq!(battle.actors[0].attack_power, 70);
    assert_eq!(battle.random_state(), random);
    Ok(())
}

#[test]
fn technique_chain_checks_tp_at_consumption_and_keeps_the_normal_on_rejection() -> Result<()> {
    let mut battle = shortcuts_battle(Control::Manual);
    battle.actors[0].tp = 3;
    battle.step(input([0; 2], true))?;
    battle.step(BattleInput::default())?;
    let frame = battle.step(player_buttons(false, true, false, [0; 2]))?;
    assert!(
        !frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Rejected { .. }))
    );
    assert_eq!(
        battle.controls[0].as_ref().unwrap().chained_technique,
        Some(10)
    );
    let random = battle.random_state();
    for _ in 0..14 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.controls[0].as_ref().unwrap().chained_technique, None);
    assert!(battle.sequences.contains_key(&ActionId(1)));
    assert_eq!(battle.next_action, 2);
    assert_eq!(battle.actors[0].tp, 3);
    assert_eq!(battle.random_state(), random);
    Ok(())
}

#[test]
fn prepared_shortcuts_reject_unknown_actions_and_invalid_ranges() {
    for shortcut in [
        TechniqueControl {
            action: 99,
            minimum: 0.,
            maximum: 800.,
        },
        TechniqueControl {
            action: 0,
            minimum: 800.,
            maximum: 800.,
        },
        TechniqueControl {
            action: 0,
            minimum: 0.,
            maximum: f32::NAN,
        },
    ] {
        let battle = battle(Control::Manual);
        let prepared = Arc::try_unwrap(battle.prepared).unwrap();
        let mut definition = prepared.controls[0].clone().unwrap();
        definition.shortcuts[0] = Some(shortcut);
        assert!(prepared.with_controls(vec![definition]).is_err());
    }
}

#[test]
fn facing_cache_rebuild_uses_source_factor_and_keeps_movement_independent() {
    // Derived arithmetic regression, not an original-runtime trig comparison.
    assert_eq!(
        direction_from_heading(90.).map(f32::to_bits),
        [0x3f80_0000, 0, 0x34a8_885a]
    );
    let mut owner = actor(Side::Party);
    owner.heading = 0.;
    owner.movement.direction = [1., 0., 0.];
    assert!(!face_cached(&mut owner, [1., 0., 0.], 22.5));
    assert_eq!(owner.heading, 22.5);
    assert_eq!(owner.movement.direction, [1., 0., 0.]);
    assert!(owner.facing_direction[0] > 0.38 && owner.facing_direction[0] < 0.39);
    assert!(owner.facing_direction[2] > 0.92 && owner.facing_direction[2] < 0.93);
    let cached = owner.facing_direction;
    face(&mut owner, [1., 0., 0.], 22.5);
    assert_eq!(owner.heading, 45.);
    assert_eq!(
        owner.facing_direction, cached,
        "24A8C changes heading without rebuilding18E4"
    );
}

#[test]
fn grounded_manual_normal_checks_cached_facing_before_copying_target() -> Result<()> {
    for (cached, moving, copied) in [
        ([1., 0., 0.], [-1., 0., 0.], true),
        ([-1., 0., 0.], [1., 0., 0.], false),
    ] {
        let mut battle = battle(Control::Manual);
        battle.actors[0].heading = 0.;
        battle.actors[0].facing_direction = cached;
        battle.actors[0].movement.direction = moving;
        battle.start_automatic_normal(ActorId(0), 0, ActorId(1), &mut vec![])?;
        battle.face_normal_entry(ActionId(1), ActorId(0))?;
        if copied {
            let actor = &battle.actors[0];
            assert_eq!(actor.facing_direction, actor.movement.direction);
            assert!(actor.facing_direction[0] > 0.999);
            assert_eq!(
                actor.heading, 22.5,
                "the copied direction is independent of a partially turned heading"
            );
        } else {
            assert_eq!(battle.actors[0].facing_direction, cached);
            assert_eq!(battle.actors[0].movement.direction, moving);
            assert_eq!(battle.actors[0].heading, 0.);
        }
    }
    Ok(())
}

#[test]
fn normal_entry_uses_the_sampled_target_direction_even_after_roots_change() -> Result<()> {
    let mut battle = battle(Control::Auto);
    // Source04 C61's18FC refers to the centers sampled during C60 callbacks:
    // those contain C59 roots. Current C61 roots instead point at59.7838deg.
    let colette = [3276727239, 0, 3280205466].map(f32::from_bits);
    let zombie = [1132351902, 0, 3199538140].map(f32::from_bits);
    let sampled = crate::distance::planar_direction(zombie, colette, [0.; 3]);
    battle.actors[0].movement.target_direction = sampled;
    battle.actors[0].position = [3276183705, 0, 3280194849].map(f32::from_bits);
    battle.actors[1].position = [1132267946, 0, 3205701558].map(f32::from_bits);
    battle.actors[0].heading = 69.76288;
    battle.start_automatic_normal(ActorId(0), 0, ActorId(1), &mut vec![])?;
    let random = battle.random_state();
    battle.face_normal_entry(ActionId(1), ActorId(0))?;
    assert_eq!(battle.actors[0].movement.direction, sampled);
    assert_eq!(battle.actors[0].facing_direction, sampled);
    assert!((battle.actors[0].heading - 60.252773).abs() < 0.00001);
    assert_eq!(battle.random_state(), random);
    Ok(())
}

#[test]
fn aerial_normal_entry_preserves_the_last_facing_cache() -> Result<()> {
    let mut battle = battle(Control::SemiAuto);
    battle.actors[0].position[1] = 50.;
    battle.actors[0].heading = 90.;
    battle.actors[0].movement.direction = [1., 0., 0.];
    let cached = direction_from_heading(37.);
    battle.actors[0].facing_direction = cached;
    battle.start_automatic_normal(ActorId(0), 6, ActorId(1), &mut vec![])?;
    battle.face_normal_entry(ActionId(1), ActorId(0))?;
    assert_eq!(battle.actors[0].facing_direction, cached);
    assert_eq!(battle.actors[0].movement.direction, [1., 0., 0.]);
    assert_eq!(battle.actors[0].heading, 90.);
    Ok(())
}

fn active_normal() -> Result<Battle> {
    let mut battle = battle(Control::Manual);
    battle.start_automatic_normal(ActorId(0), 0, ActorId(1), &mut vec![])?;
    battle.face_normal_entry(ActionId(1), ActorId(0))?;
    battle.actors[0].heading = 0.;
    battle.actors[0].facing_direction = [0., 0., 1.];
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].movement.forward = 6.;
    let sequence = battle.sequences.get_mut(&ActionId(1)).unwrap();
    sequence.age = 10;
    sequence.command_age = 11;
    sequence.hit_age = 12;
    sequence.animation_age = 13;
    Ok(battle)
}

#[test]
fn unfinished_normal_turn_holds_streams_input_movement_and_landing() -> Result<()> {
    let mut battle = active_normal()?;
    battle.actors[0].movement.airborne_action = true;
    for heading in [22.5, 45., 67.5, 90.] {
        battle.step(input([0, 0], true))?;
        let sequence = &battle.sequences[&ActionId(1)];
        assert_eq!(
            (
                sequence.age,
                sequence.command_age,
                sequence.hit_age,
                sequence.animation_age
            ),
            (10, 11, 12, 13)
        );
        let actor = &battle.actors[0];
        assert_eq!(actor.heading, heading);
        assert_eq!(actor.facing_direction, direction_from_heading(heading));
        assert_eq!(actor.position, [0.; 3]);
        assert_eq!(actor.movement.forward, 6.);
        assert!(
            actor.movement.airborne_action,
            "3DA00 landing was not dispatched"
        );
        assert_eq!(battle.controls[0].as_ref().unwrap().buffered, None);
    }
    battle.actors[0].movement.airborne_action = false;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].heading.to_bits(), 0x42b4_0003);
    assert_eq!(battle.sequences[&ActionId(1)].age, 11);
    assert_eq!(battle.actors[0].position[0], 6.);
    Ok(())
}

#[test]
fn normal_facing_rebuild_precedes_hit_stop_and_common_still_advances() -> Result<()> {
    let mut battle = active_normal()?;
    battle.actors[0].hit_stop = 2;
    battle.actors[0].position[1] = -1.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].heading, 22.5);
    assert_eq!(
        battle.actors[0].facing_direction,
        direction_from_heading(22.5)
    );
    assert_eq!(battle.actors[0].hit_stop, 1);
    assert_eq!(battle.actors[0].position[1], 0., "30B4C still calls23AD0");
    assert_eq!(battle.sequences[&ActionId(1)].age, 10);
    Ok(())
}

#[test]
fn aerial_party_bypasses_normal_turn_but_aerial_enemy_does_not() -> Result<()> {
    for side in [Side::Party, Side::Enemy] {
        let mut battle = active_normal()?;
        battle.actors[0].side = side;
        if side == Side::Enemy {
            battle.actors[1].side = Side::Party;
            battle.actors[0].control = Control::Enemy;
            battle.controls[0] = None;
            let prepared = Arc::get_mut(&mut battle.prepared).unwrap();
            prepared.controls[0] = None;
            prepared.enemy_decisions[0] = Some(crate::EnemyDecisionDefinition {
                actor: ActorId(0),
                strategy: 1,
                difficulty: 0,
                choices: vec![crate::EnemyChoice {
                    action: 0,
                    weight: 1,
                    requirements: 0,
                    target_policy: 1,
                    guard_chance: 0,
                    combo_at: i16::MAX as u16,
                    followup_chance: 0,
                    range: [0, 0],
                    tp: 0,
                    approach_minimum: 0.,
                    approach_range: 120.,
                }],
                back_row: vec![],
                walk_speed: 5.,
                turn_ticks: 8,
                body_flags: 0,
            });
        }
        battle.actors[0].position[1] = 50.;
        battle.step(BattleInput::default())?;
        if side == Side::Party {
            assert_eq!(battle.actors[0].heading, 0.);
            assert_eq!(battle.actors[0].facing_direction, [0., 0., 1.]);
            assert_eq!(battle.sequences[&ActionId(1)].age, 11);
        } else {
            assert_eq!(battle.actors[0].heading, 22.5);
            assert_eq!(battle.sequences[&ActionId(1)].age, 10);
        }
    }
    Ok(())
}

#[test]
fn profile_turn_disable_passes_normal_gate_without_facing_writes() -> Result<()> {
    let mut battle = active_normal()?;
    battle.actors[0].movement.turning_disabled = true;
    battle.controls[0].as_mut().unwrap().entry_facing = true;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].heading, 0.);
    assert_eq!(battle.actors[0].facing_direction, [0., 0., 1.]);
    assert_eq!(battle.sequences[&ActionId(1)].age, 11);
    assert!(!face_cached(&mut battle.actors[0], [1., 0., 0.], 22.5));
    assert_eq!(battle.actors[0].heading, 0.);
    Ok(())
}

#[test]
fn short_facing_input_still_wraps_heading_and_rebuilds_cache() {
    let mut actor = actor(Side::Party);
    actor.heading = 720.;
    assert!(!face_cached(&mut actor, [0.; 3], 22.5));
    assert_eq!(actor.heading, 360.);
    assert_eq!(actor.facing_direction, direction_from_heading(360.));
}

#[test]
fn normal_recovery_does_not_revisit_the_action_facing_gate() -> Result<()> {
    let mut battle = active_normal()?;
    battle.actors[0].activity = Activity::Recovering;
    battle.sequences.get_mut(&ActionId(1)).unwrap().recovery = Some(5);
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].heading, 0.);
    assert_eq!(battle.actors[0].facing_direction, [0., 0., 1.]);
    assert_eq!(battle.sequences[&ActionId(1)].recovery, Some(4));
    assert_eq!(battle.actors[0].position[0], 6.);
    Ok(())
}

fn pending_martial(companion: bool, phase: ActionPhase) -> Result<Battle> {
    let mut battle = shortcuts_battle(if companion {
        Control::Auto
    } else {
        Control::Manual
    });
    let prepared = Arc::get_mut(&mut battle.prepared).unwrap();
    prepared
        .actions
        .iter_mut()
        .find(|a| a.id == 10)
        .unwrap()
        .phase = phase;
    if companion {
        prepared.controls[0].as_mut().unwrap().shortcuts = [None; 4];
        prepared.companions[0] = Some(crate::CompanionDefinition {
            actor: ActorId(0),
            strategy: [3, 6, 2],
            saved_position: 0,
            level: 1,
            level_difference: 0,
            tp_limit: 30,
            healing_limit: 65,
            support_level_limit: 2,
            techniques: vec![crate::CompanionTechnique {
                action: 10,
                enabled: true,
                flags: 0x41106,
                cost: 4,
                learning_route: 0,
                minimum: 400.,
                maximum: 500.,
            }],
        });
    }
    battle.actors[0].tp = 20;
    battle.actors[0].heading = 0.;
    battle.actors[0].facing_direction = [0., 0., 1.];
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].movement.forward = 6.;
    battle.start(
        ActionRequest {
            actor: ActorId(0),
            action: 10,
            target: ActorId(1),
        },
        &mut vec![],
    )?;
    Ok(battle)
}

#[test]
fn martial_shortcut_and_companion_turns_hold_initialization_and_movement() -> Result<()> {
    for companion in [false, true] {
        let mut battle = pending_martial(companion, ActionPhase::Actor)?;
        battle.step(BattleInput::default())?;
        let sequence = &battle.sequences[&ActionId(1)];
        assert_eq!(
            (
                sequence.age,
                sequence.command_age,
                sequence.hit_age,
                sequence.animation_age
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(battle.actors[0].tp, 20, "the initializer has not run");
        assert_eq!(battle.actors[0].heading, 22.5);
        assert_eq!(
            battle.actors[0].facing_direction,
            direction_from_heading(22.5)
        );
        assert_eq!(battle.actors[0].position, [0.; 3]);
        assert_eq!(battle.actors[0].movement.forward, 6.);

        battle.actors[0].heading = f32::from_bits(0x42b4_0003);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.sequences[&ActionId(1)].age, 1);
        assert_eq!(battle.actors[0].tp, 16);
        assert_eq!(battle.actors[0].position[0], 6.);
    }
    Ok(())
}

#[test]
fn martial_facing_precedes_hit_stop_and_keeps_the_floor_tail() -> Result<()> {
    let mut battle = pending_martial(false, ActionPhase::Actor)?;
    battle.actors[0].hit_stop = 2;
    battle.actors[0].position[1] = -1.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.actors[0].heading, 22.5);
    assert_eq!(
        battle.actors[0].facing_direction,
        direction_from_heading(22.5)
    );
    assert_eq!(battle.actors[0].hit_stop, 1);
    assert_eq!(battle.actors[0].position, [0.; 3]);
    assert_eq!(battle.sequences[&ActionId(1)].age, 0);
    assert_eq!(battle.actors[0].tp, 20);
    Ok(())
}

#[test]
fn casting_and_martial_recovery_do_not_use_the_active_action_facing_gate() -> Result<()> {
    for phase in [ActionPhase::Actor, ActionPhase::Casting] {
        let mut battle = pending_martial(false, phase)?;
        if phase == ActionPhase::Actor {
            battle.actors[0].activity = Activity::Recovering;
            battle.sequences.get_mut(&ActionId(1)).unwrap().recovery = Some(5);
        }
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].heading, 0.);
        assert_eq!(battle.actors[0].facing_direction, [0., 0., 1.]);
        let sequence = &battle.sequences[&ActionId(1)];
        if phase == ActionPhase::Actor {
            assert_eq!(sequence.recovery, Some(4));
        } else {
            assert_eq!(sequence.age, 1);
            assert_eq!(battle.actors[0].tp, 16);
        }
    }
    Ok(())
}

#[test]
fn player_normal_approach_ignores_the_authored_auto_minimum() -> Result<()> {
    let mut battle = battle(Control::SemiAuto);
    Arc::get_mut(&mut battle.prepared).unwrap().controls[0]
        .as_mut()
        .unwrap()
        .normals[0]
        .minimum_reach = 100.;
    battle.step(input([0, 0], true))?;
    assert_eq!(battle.actors[0].movement.forward, 0.);
    assert_eq!(battle.actors[0].position, [0.; 3]);
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(
        frame.actions.len(),
        1,
        "in-range admission does not retreat to Auto's minimum"
    );
    Ok(())
}

#[test]
fn facing_return_operand_uses_adjusted_desired_angle_before_a_partial_turn() {
    let mut actor = actor(Side::Party);
    actor.heading = -90.;
    let (ready, operand) = face_cached_with_desired(&mut actor, [0., 0., 1.], 22.5);
    assert!(!ready);
    assert_eq!(actor.heading, -67.5);
    assert_eq!(
        operand,
        Some(crate::weapon_flight::ReturnSteering::Desired([0., 0., 1.]))
    );

    actor.heading = -30.;
    let (ready, operand) = face_cached_with_desired(&mut actor, [0., 0., -1.], 22.5);
    assert!(!ready);
    assert_eq!(actor.heading, -52.5);
    let Some(crate::weapon_flight::ReturnSteering::Desired([low, desired, paired])) = operand
    else {
        panic!("wrapped facing must retain its desired angle");
    };
    assert!(desired > -180. && desired < -179.9);
    assert_eq!(paired, desired);
    assert_eq!(low.to_bits(), f64::from(desired).to_bits() as u32);
    assert_ne!(desired, actor.heading);

    assert_eq!(
        crate::weapon_flight::ReturnSteering::facing(f32::from_bits(0x42b4_0003), false),
        crate::weapon_flight::ReturnSteering::KeepDirection,
    );
}

#[test]
fn command_pause_keeps_gameplay_callbacks_held_but_advances_source_hud_visit() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.show_recovery(ActorId(0), crate::RecoveryKind::Tp, 12)?;
    let before = battle.snapshot();
    let held = battle.step(BattleInput {
        controllers: vec![ControlInput::neutral(ActorId(0))],
        command_pause: true,
        ..Default::default()
    })?;
    assert_eq!(held.update, before.update + 1);
    assert_eq!(held.hud_update, before.hud_update + 1);
    assert!(held.hud_holds.intro);
    assert!(held.hud_holds.notices);
    assert_eq!(held.actors[0].hud.phase, before.actors[0].hud.phase);
    assert_eq!(held.actors[0].hud.recovery[1].x_delta, 0);
    let resumed = battle.step(BattleInput::default())?;
    assert!(resumed.actors[0].hud.phase > held.actors[0].hud.phase);
    Ok(())
}

#[test]
fn ordinary_player_countdown_uses_shared_count_before_callback_and_holds_in_menu() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = battle(mode);
        battle.actors[0].reaction.remaining = 3;
        battle.actors[0].hit_stop = 5;
        for remaining in [2, 1, 0, 0] {
            let frame = battle.step(BattleInput::default())?;
            assert_eq!(frame.actors[0].reaction.remaining, remaining);
            assert_eq!(frame.actors[0].activity, Activity::Idle);
            assert_eq!(battle.random_state(), 0);
        }
        for _ in 0..2 {
            let frame = battle.step(player_buttons(false, false, false, [30, 0]))?;
            assert_eq!(frame.actors[0].reaction.remaining, 29);
        }
        let paused = BattleInput {
            menu_open: true,
            ..Default::default()
        };
        assert_eq!(battle.step(paused)?.actors[0].reaction.remaining, 29);
        // The first run release selects command1 and resets the shared count;
        // the next neutral ordinary callback observes that count and decrements.
        assert_eq!(
            battle.step(BattleInput::default())?.actors[0]
                .reaction
                .remaining,
            29
        );
        assert_eq!(
            battle.step(BattleInput::default())?.actors[0]
                .reaction
                .remaining,
            28
        );
    }
    Ok(())
}
