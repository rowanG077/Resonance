use super::*;
use crate::{BattleInput, Side, tests::actor};
use std::sync::Arc;

#[path = "combo_tests.rs"]
mod combo_tests;
#[path = "heavy_tests.rs"]
mod heavy_tests;
#[path = "normal_guard_tests.rs"]
mod normal_guard_tests;
#[path = "paralysis_tests.rs"]
mod paralysis_tests;

fn prepared(control: Control) -> PreparedBattle {
    let actions: Vec<_> = (0..7)
        .map(|index| crate::ActionDefinition {
            normal: Some(crate::NormalAttack::ALL[index]),
            execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                chain_at: Some(15),
                end_at: 90,
                opening: None,
                events: vec![],
                recovery: 0,
            }),
            tp_cost: 0,
        })
        .collect();
    let mut player = actor(Side::Party);
    player.control = control;
    player.heading = 90.;
    player.movement.direction = [1., 0., 0.];
    player.facing_direction = [1., 0., 0.];
    player.movement.target_direction = [1., 0., 0.];
    player.movement.braking = 1.;
    player.body.collider = Some(crate::Collider::sphere(10.));
    let mut enemy = actor(Side::Enemy);
    enemy.position = [100., 0., 0.];

    enemy.body.collider = Some(crate::Collider::sphere(10.));
    let mut other = enemy.clone();
    other.position[0] = 200.;

    let mut actors = vec![crate::ActorSetup::default(); 3];
    actors[0].control = Some(Arc::new(ControlDefinition {
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 8,
        motions: None,
        shortcuts: [0; 4],
        normals: std::array::from_fn(|i| NormalControl {
            action: crate::ActionKey(i),

            reach: 120.,
            minimum_reach: 0.,
        }),
    }));
    PreparedBattle::new(
        vec![player, enemy, other].into_iter().zip(actors).collect(),
        actions.into(),
        0,
    )
    .unwrap()
}

fn battle(control: Control) -> Battle {
    prepared(control).finish().unwrap()
}

fn missing_motions() -> ControlMotions {
    let motion = MotionBinding { model: 7, clip: 99 };
    ControlMotions {
        walk: motion,
        run: motion,
        stop: motion,
        landing: motion,
    }
}

#[test]
fn locomotion_updates_walk_run_acceleration_and_live_speed() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut prepared = prepared(mode);
        Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).motions =
            Some(missing_motions());
        let mut battle = prepared.finish()?;
        battle.set_diagnostics(Default::default());
        for (actor, x) in battle.actors[1..].iter_mut().zip([10_000., 20_000.]) {
            actor.position[0] = x;
        }
        battle.actors[0].equipment.speed_multiplier = 1.1;
        battle.actors[0].body.scale = 1.25;
        battle.step(input([25, 0], false))?;
        battle.step(input([30, 0], false))?;
        assert_eq!(battle.actors[0].movement.locomotion, Locomotion::Walk);
        crate::tests::assert_close(battle.actors[0].movement.forward, 6.875, 0.001);
        battle.step(input([80, 0], false))?;
        assert_eq!(battle.actors[0].movement.locomotion, Locomotion::Run);
        assert_eq!(battle.actors[0].movement.forward, 5.5);
        for _ in 0..20 {
            battle.step(input([80, 0], false))?;
        }
        assert_eq!(battle.actors[0].movement.forward, 11.);
        let position = battle.actors[0].position[0];
        battle.actors[0].equipment.speed_multiplier = 1.;
        battle.step(input([80, 0], false))?;
        assert_eq!(battle.actors[0].movement.forward, 10.);
        assert_eq!(battle.actors[0].position[0] - position, 10.);
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
fn locomotion_reversal_brakes_before_turning_and_quick_turn_releases_stop() -> Result<()> {
    let mut short_run = battle(Control::Manual);
    short_run.step(input([80, 0], false))?;
    short_run.step(input([-80, 0], false))?;
    assert_eq!(short_run.actors[0].movement.locomotion, Locomotion::Walk);
    assert!(short_run.actors[0].movement.direction[0] > 0.);
    short_run.step(input([-80, 0], false))?;
    assert!(short_run.actors[0].movement.direction[0] < 0.);

    for quick_turn in [false, true] {
        let mut battle = battle(Control::Manual);
        for (actor, x) in battle.actors[1..].iter_mut().zip([10_000., 20_000.]) {
            actor.position[0] = x;
        }
        battle.actors[0].equipment.quick_turn = quick_turn;
        for _ in 0..24 {
            battle.step(input([80, 0], false))?;
        }
        battle.step(input([-80, 0], false))?;
        assert_eq!(battle.actors[0].movement.locomotion, Locomotion::Stop);
        let position = battle.actors[0].position[0];
        battle.step(input([-80, 0], false))?;
        if quick_turn {
            assert!(battle.actors[0].position[0] < position);
            assert_eq!(battle.actors[0].movement.locomotion, Locomotion::Walk);
        } else {
            assert!(battle.actors[0].position[0] > position);
            assert_eq!(battle.actors[0].movement.locomotion, Locomotion::Stop);
            for _ in 0..20 {
                battle.step(input([0, 0], false))?;
            }
            assert_eq!(battle.actors[0].movement.locomotion, Locomotion::Idle);
        }
    }
    Ok(())
}

#[test]
fn locomotion_keeps_airborne_momentum_and_samples_input_during_guard() -> Result<()> {
    let mut airborne = battle(Control::Manual);
    airborne.actors[0].position[1] = 100.;
    airborne.actors[0].movement.forward = 3.;
    airborne.actors[0].movement.vertical = 2.;
    airborne.actors[0].movement.gravity = 0.;
    airborne.step(input([-80, 0], false))?;
    assert_eq!(airborne.actors[0].position, [3., 102., 0.]);
    assert_eq!(airborne.actors[0].movement.forward, 3.);
    assert_eq!(airborne.actors[0].movement.direction, [1., 0., 0.]);

    let mut guarded = battle(Control::Manual);
    for _ in 0..40 {
        guarded.step(player_buttons(false, false, true, [80, 0]))?;
    }
    for _ in 0..4 {
        guarded.step(input([80, 0], false))?;
    }
    assert!(!guarded.actors[0].guard.active);
    assert_eq!(guarded.actors[0].movement.locomotion, Locomotion::Walk);
    assert_eq!(guarded.actors[0].movement.forward, 5.);
    Ok(())
}

fn companion_battle(colette: bool) -> Battle {
    companion_battle_for(if colette { 2 } else { 3 })
}

fn companion_prepared_for(character: u8) -> PreparedBattle {
    let colette = character == 2;
    let mut candidate = prepared(Control::Auto);
    if colette {
        let mut action = crate::tests::action(30);
        action.tp_cost = 5;
        candidate.resources.actions.entries.push(Arc::new(action));
    } else {
        candidate
            .resources
            .actions
            .insert(crate::tests::cast(30, 5));
    }
    let prepared = &mut candidate.resources;
    prepared.actor_setup[0].companion = Some(crate::CompanionDefinition {
        initial_policy: [0, 0, 0],
        defaults: [3, 6, if colette { 2 } else { 5 }],
        limits: [crate::PolicyLimits {
            tp: 30,
            healing: 65,
            support_level: 2,
        }; 9],
        level: 1,
        level_difference: 0,
    });
    prepared.actor_setup[0].techniques = vec![crate::PreparedTechnique {
        action: crate::ActionKey(7),
        catalogue: 1,
        player_range: [400., 500.],
        ai_range: [400., 500.],
        capabilities: crate::TechniqueCapabilities {
            family: Some(crate::ArteFamily::Basic),
            spell: !colette,
            offensive: true,
            chains_without_contact: colette,
            target: crate::TechniqueTarget::Enemy,
            ..Default::default()
        },
        element: 0,
    }];
    let definition = crate::DecisionDefinition {
        idle_ticks: 0,
        idle_variation: 0,
    };
    prepared.actor_setup[0].decision = Some(definition);
    candidate.actors[0].tp = candidate.actors[0].equipment.max_tp;
    candidate
}

fn companion_battle_for(character: u8) -> Battle {
    let mut battle = companion_prepared_for(character).finish().unwrap();
    battle
        .start_actor_command(
            crate::ActionRequest {
                actor: ActorId(0),
                action: crate::ActionKey(0),
                target: ActorId(1),
            },
            &mut vec![],
        )
        .unwrap();
    battle
}

#[test]
fn automatic_chain_requires_contact_then_initializes_once() -> Result<()> {
    let mut whiff = companion_battle(true);
    for _ in 0..16 {
        whiff.step(BattleInput::default())?;
    }

    assert!(
        whiff
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|s| s.action == crate::ActionKey(0))
    );

    let mut hit = companion_battle(true);
    for _ in 0..15 {
        hit.step(BattleInput::default())?;
    }
    hit.confirm_actor_contact(ActorId(0));
    let tp = hit.actors[0].tp;
    hit.step(BattleInput::default())?;
    assert!(
        hit.sequences()
            .map(|(_, sequence)| sequence)
            .any(|s| s.action == crate::ActionKey(7))
    );
    assert_eq!(hit.actors[0].tp, tp - 5);
    for _ in 0..3 {
        hit.step(BattleInput::default())?;
    }
    assert_eq!(hit.actors[0].tp, tp - 5);
    Ok(())
}

fn approach_policy_battle(control: Control, colette: bool) -> Battle {
    let mut prepared = prepared(control);
    prepared.actors[1].position[0] = 600.;

    if colette {
        prepared.resources.actor_setup[0].companion = Some(crate::CompanionDefinition {
            initial_policy: [0; 3],
            defaults: [3, 6, 2],
            limits: [crate::PolicyLimits {
                tp: 30,
                healing: 65,
                support_level: 2,
            }; 9],
            level: 1,
            level_difference: 0,
        });
    }
    prepared.resources.actor_setup[0].decision = Some(crate::DecisionDefinition {
        idle_ticks: 0,
        idle_variation: 0,
    });
    let mut battle = prepared.finish().unwrap();
    battle
        .request_approach(
            ActorId(0),
            ActorId(1),
            crate::ActionKey(0),
            crate::ApproachParameters {
                minimum: 0.,
                maximum: 120.,
                motion: None,
                motion_rate: 0.5,
                speed: 6.,
                turn_ticks: 8,
            },
        )
        .unwrap();
    battle
}

#[test]
fn auto_adapts_to_target_height_while_semi_auto_preserves_the_selected_attack() -> Result<()> {
    for mode in [Control::Auto, Control::SemiAuto] {
        let mut battle = approach_policy_battle(mode, false);
        battle.step(BattleInput::default())?;
        battle.actors[1].position[1] = 120.;
        battle.step(BattleInput::default())?;
        assert_eq!(
            battle.approach_normal(ActorId(0)),
            Some(if mode == Control::Auto {
                NormalAttack::Rising
            } else {
                NormalAttack::Neutral
            })
        );
        battle.actors[1].position[1] = 0.;
        battle.step(BattleInput::default())?;
        assert_eq!(
            battle.approach_normal(ActorId(0)),
            Some(NormalAttack::Neutral)
        );
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

    battle.step(BattleInput::default())?;
    assert_eq!(
        battle.approach_normal(ActorId(0)),
        Some(NormalAttack::Neutral)
    );
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

fn wait_for_action(battle: &mut Battle, definition: crate::ActionKey) -> Result<ActionId> {
    for _ in 0..32 {
        if let Some((&id, _)) = battle.sequences().find(|(_, sequence)| {
            matches!(
                &sequence.definition.execution,
                crate::ActionExecution::Attack(_)
            ) && sequence.action == definition
                && sequence.age > 0
        }) {
            return Ok(id);
        }
        battle.step(BattleInput::default())?;
    }
    anyhow::bail!("action {definition} did not start")
}

#[test]
fn only_attack_edges_start_and_buffered_neutral_uses_fallback() -> Result<()> {
    let mut battle = battle(Control::Manual);
    let mut held = input([0, 0], false);
    held.controllers[0].attack.held = true;
    assert!(battle.step(held)?.actions.is_empty());
    battle.step(input([0, 0], true))?;
    let first = wait_for_action(&mut battle, crate::ActionKey(0))?;
    battle.step(input([0, 0], true))?;
    let chained = wait_for_action(&mut battle, crate::ActionKey(4))?;
    assert_ne!(first, chained);
    assert!(battle.sequence(&first).is_none());
    assert_eq!(battle.actors[0].attack_power, 85);

    Ok(())
}

#[test]
fn permitted_directions_chain_and_limit_prevents_a_fourth_normal() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    wait_for_action(&mut battle, crate::ActionKey(0))?;
    battle.step(input([80, 0], true))?;
    wait_for_action(&mut battle, crate::ActionKey(3))?;
    battle.step(input([0, 0], true))?;
    let third = wait_for_action(&mut battle, crate::ActionKey(0))?;
    assert_eq!(battle.actors[0].attack_power, 70);

    let next = battle.next_action;
    battle.step(input([80, 0], true))?;

    for _ in 0..20 {
        battle.step(BattleInput::default())?;
    }
    assert!(battle.sequence(&third).is_some());
    assert_eq!(battle.next_action, next);
    Ok(())
}

#[test]
fn hit_stop_accepts_an_edge_but_menu_pause_discards_it() -> Result<()> {
    let mut battle = battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    let action = wait_for_action(&mut battle, crate::ActionKey(0))?;
    let age = battle.action_age(action);
    let mut paused = input([80, 0], true);
    paused.paused = true;
    battle.step(paused)?;
    assert_eq!(battle.action_age(action), age);

    battle.actors[0].hit_stop = 3;
    battle.step(input([80, 0], true))?;
    assert_eq!(battle.action_age(action), age);

    Ok(())
}

#[test]
fn semi_auto_approaches_before_attacking_and_manual_can_attack_at_distance() -> Result<()> {
    let mut semi = battle(Control::SemiAuto);
    semi.actors[1].position[0] = 400.;

    let frame = semi.step(input([0, 0], true))?;
    assert_eq!(frame.actors[0].activity, Activity::Approaching);
    assert!(frame.actions.is_empty());
    assert!(frame.actors[0].position[0] > 0. && frame.actors[0].movement.forward <= 10.);
    semi.actors[1].position[0] = 100.;

    let action = wait_for_action(&mut semi, crate::ActionKey(0))?;
    assert_eq!(semi.sequence(&action).unwrap().target, ActorId(1));
    let mut manual = battle(Control::Manual);
    manual.actors[1].position[0] = 1000.;

    manual.step(input([0, 0], true))?;
    wait_for_action(&mut manual, crate::ActionKey(0))?;
    assert_eq!(manual.actors[0].position[0], 0.);
    Ok(())
}

#[test]
fn automatic_approach_can_admit_an_attack_without_optional_body_volumes() -> Result<()> {
    let mut battle = battle(Control::SemiAuto);
    for actor in &mut battle.actors {
        actor.body.collider = None;
    }
    battle.step(input([0, 0], true))?;
    wait_for_action(&mut battle, crate::ActionKey(0))?;
    assert_eq!(battle.actors[0].position, [0.; 3]);
    Ok(())
}

#[test]
fn command_target_projection_does_not_consume_controller_navigation() -> Result<()> {
    let mut battle = battle(Control::Manual);
    let owner = ActorId(0);
    let ordinary = battle.target(owner);
    battle.project_command_target(Some((owner, ActorId(2))))?;
    let update = battle.snapshot().update;
    for direction in [-1, 1, -1] {
        let mut controllers = input([0, 0], false);
        controllers.controllers[0].target_step = direction;
        let frame = battle.step(controllers)?;
        assert_eq!(frame.target_selector, Some(owner));
        assert_eq!(frame.targets[owner.index()], Some(ActorId(2)));
        assert_eq!(battle.target(owner), ordinary);
        assert_eq!(frame.update, update);
    }
    battle.project_command_target(None)?;
    assert_eq!(battle.snapshot().target_selector, None);
    assert_eq!(battle.snapshot().targets[owner.index()], ordinary);
    Ok(())
}

#[test]
fn target_navigation_cycles_live_targets_and_drops_unavailable_choices() -> Result<()> {
    let mut battle = battle(Control::Manual);
    let owner = ActorId(0);
    battle.actors[1].position[0] = 10.;
    battle.actors[2].position[0] = 20.;
    let mut selected = Some(ActorId(1));
    for expected in [ActorId(2), ActorId(1), ActorId(2)] {
        selected = battle.select_target_step(owner, selected, 1);
        assert_eq!(selected, Some(expected));
    }
    battle.actors[2].position[0] = 5.;
    assert_eq!(battle.select_target_step(owner, None, 0), Some(ActorId(2)));
    battle.actors[2].position[0] = 20.;
    assert_eq!(battle.select_target_step(owner, None, 0), Some(ActorId(1)));
    battle.actors[2].availability = crate::ActorAvailability::Dead;
    selected = battle.select_target_step(owner, selected, 0);
    assert_eq!(selected, Some(ActorId(1)));
    assert!(
        battle
            .project_command_target(Some((owner, ActorId(2))))
            .is_err()
    );
    battle.actors[1].availability = crate::ActorAvailability::Dead;
    assert_eq!(battle.select_target_step(owner, selected, 1), None);
    Ok(())
}

#[test]
fn target_tap_avoids_current_and_held_selector_freezes_actor_time() -> Result<()> {
    let mut battle = battle(Control::Manual);
    let mut tap = input([0, 0], false);
    tap.controllers[0].target.released = true;
    assert_eq!(battle.step(tap)?.targets[0], Some(ActorId(2)));
    battle.actors[0].reaction.protection.armor(90);
    for visit in 0..16 {
        let mut held = input([0, 0], false);
        held.controllers[0].target = ButtonInput {
            held: true,
            pressed: visit == 0,
            released: false,
        };
        if battle.step(held)?.target_selector.is_some() {
            break;
        }
    }
    assert_eq!(battle.snapshot().target_selector, Some(ActorId(0)));
    let clock = battle.snapshot().update;
    let remaining = battle.actors[0].reaction.protection.remaining;
    let position = battle.actors[0].position;
    battle.actors[0].hp = 39;
    let mut select = input([0, 0], false);
    select.controllers[0].target.held = true;
    select.controllers[0].target_step = -1;
    let frame = battle.step(select)?;
    assert_eq!(frame.update, clock);
    assert_eq!(frame.actors[0].reaction.protection.remaining, remaining);
    assert_eq!(frame.actors[0].position, position);
    assert_eq!(frame.targets[0], Some(ActorId(1)));
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.target_selector, None);
    assert_eq!(frame.targets[0], Some(ActorId(1)));
    assert_eq!(frame.update, clock);
    assert_eq!(frame.actors[0].reaction.protection.remaining, remaining);
    let held = battle.step(BattleInput {
        paused: true,
        ..Default::default()
    })?;
    assert_eq!(held.update, clock);
    assert_eq!(held.actors[0].reaction.protection.remaining, remaining);
    let resumed = battle.step(BattleInput::default())?;
    assert!(resumed.update > clock);
    assert!(resumed.actors[0].reaction.protection.remaining < remaining);
    Ok(())
}

#[test]
fn bad_controller_input_is_rejected_before_mutation() {
    let mut battle = battle(Control::Manual);
    let mut duplicate = input([0, 0], true);
    duplicate.controllers.push(duplicate.controllers[0]);
    assert!(battle.step(duplicate).is_err());
    assert_eq!(battle.next_action, 1);
    assert!(battle.step(input([0, 0], true)).is_ok());
}

#[test]
fn victory_keeps_target_hold_clock_without_opening_a_selector() -> Result<()> {
    let mut battle = battle(Control::SemiAuto);
    battle.terminal.result = Some(crate::BattleResult::Victory);
    for visit in 0..12 {
        let mut request = input([0, 0], false);
        request.controllers[0].target.held = true;
        request.controllers[0].target.pressed = visit == 0;
        battle.step(request)?;
        assert!(battle.target_selector.is_none());
    }
    assert_eq!(battle.runtime[0].control.as_ref().unwrap().target_ticks, 12);
    assert!(!battle.decision_ready(ActorId(0)));
    Ok(())
}

#[test]
fn victory_input_stops_at_result_controller_retirement() -> Result<()> {
    let mut battle = battle(Control::SemiAuto);
    battle.terminal.result = Some(crate::BattleResult::Victory);
    battle.retire_combat()?;
    battle.runtime[0].control = None;
    let position = battle.actors[0].position;

    let frame = battle.step(input([80, 0], true))?;
    assert_eq!(frame.actors[0].position, position);
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    assert!(frame.actions.is_empty());
    assert!(battle.runtime[0].control.is_none());
    Ok(())
}

#[test]
fn result_poses_are_not_replaced_by_companion_ai() -> Result<()> {
    let mut battle = companion_battle(false);
    let owner = ActorId(0);
    battle.terminal.result = Some(crate::BattleResult::Victory);
    battle.retire_combat()?;
    battle.reset_result_actor(owner)?;
    battle.runtime[0].idle_timer = 10;
    battle.play_victory_pose(owner, MotionBinding { model: 7, clip: 1 })?;
    for _ in 0..20 {
        battle.step(BattleInput::default())?;
    }

    assert!(battle.sequences().next().is_none());
    assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
    Ok(())
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
    battle.runtime[0].control = None;
    let before = battle.snapshot();
    let frame = battle.step(input([80, 0], true))?;
    assert_eq!(frame.actors[0].position, before.actors[0].position);
    assert_eq!(battle.next_action, 1);
    assert!(battle.runtime[0].control.is_none());
    let mut unknown = input([0, 0], false);
    unknown.controllers[0].actor = ActorId(1);
    assert!(battle.step(unknown).is_err());
    Ok(())
}

#[test]
fn landing_cancels_the_airborne_attack_and_finishes_recovery() -> Result<()> {
    for chain in [false, true] {
        let mut prepared = shortcuts_prepared(Control::Manual);
        Arc::make_mut(prepared.resources.actor_setup[0].control.as_mut().unwrap()).motions =
            Some(missing_motions());
        let owner = &mut prepared.actors[0];
        owner.position[1] = 200.;
        owner.movement.gravity = crate::movement::GRAVITY;

        owner.equipment.combo_traits.aerial_arte = true;
        prepared.resources.actor_setup[0].techniques[0]
            .capabilities
            .aerial = true;
        prepared.actors[1].body.collider = Some(crate::Collider::sphere(0.1));
        let action = Arc::make_mut(&mut prepared.resources.actions.entries[5]);
        let contact = Arc::new(crate::MeleeDefinition {
            hit: crate::HitRule {
                overlimit_pause: false,
                condition: None,
                arte: false,
                reaction: crate::ReactionRule {
                    hitstun: 20,
                    ..Default::default()
                },
                kind: crate::DamageKind::Slash,
                power: crate::Power::Fixed(5),
                element: crate::HitElement::Neutral,
                prevents_defeat: false,
                guard: Default::default(),
            },
            trail: None,
            volume: crate::MeleeVolume {
                offset: [0.; 3],
                radius: 120.,
                half_height: 0.1,
            },
        });
        action.execution = crate::ActionExecution::Attack(crate::PreparedAttack {
            chain_at: Some(1),
            events: vec![
                (
                    0,
                    crate::AttackEvent::Contact {
                        definition: contact.clone(),
                        duration: 30,
                    },
                ),
                (
                    45,
                    crate::AttackEvent::Contact {
                        definition: contact,
                        duration: 1,
                    },
                ),
            ],
            recovery: 2,
            ..crate::tests::attack(45)
        });
        let mut martial = action.clone();
        martial.normal = None;
        martial.tp_cost = 4;
        prepared.resources.actions.entries[7] = Arc::new(martial);
        let definition = prepared.resources.actor_setup[0].control.clone().unwrap();
        let mut battle = prepared.finish()?;
        let mut cues = vec![];
        battle.start_control_normal(
            ActorId(0),
            &definition,
            NormalAttack::AerialSlash,
            &mut cues,
        )?;
        let normal = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Started { action, .. } => Some(*action),
                _ => None,
            })
            .unwrap();
        let mut id = normal;
        assert!(battle.actors[0].movement.airborne_action);
        let mut landed = false;
        for update in 0..60 {
            let before = &battle.actors[0];
            let crosses_floor = before.position[1] + before.movement.vertical < 0.;
            let frame = battle.step(player_buttons(false, chain && update == 1, false, [0; 2]))?;
            for cue in &frame.cues {
                if let Cue::Started {
                    actor: ActorId(0),
                    action,
                    ..
                } = cue
                {
                    assert!(chain);
                    assert_eq!(battle.action_definition(*action), Some(crate::ActionKey(7)));
                    assert!(frame.actors[0].position[1] > crate::movement::GROUND_TOLERANCE);
                    assert!(battle.sequence(&normal).is_none());
                    id = *action;
                }
            }
            let hits = frame.cues.iter().filter(|cue| matches!(cue,
                Cue::Hit { actor: ActorId(1), source: crate::ContactSource::Melee { action, .. }, .. }
                    if *action == id)).count();
            if frame.actors[0].position[1] > 0.1 {
                assert_eq!(hits, 0);
                continue;
            }
            assert!(crosses_floor, "regression must exercise floor correction");
            assert_eq!(hits, 1, "final airborne contact was discarded");

            assert_eq!(frame.actors[1].hp, 45);
            assert_eq!(id != normal, chain, "requested aerial chain did not start");
            assert_eq!(battle.action_recovery_remaining(id).is_some(), !chain);
            assert_eq!(
                frame.actors[0].activity,
                if chain {
                    Activity::Action
                } else {
                    Activity::Recovering
                }
            );
            assert!(!frame.actors[0].movement.airborne_action);
            landed = true;
            break;
        }
        assert!(landed, "airborne normal never landed");
        let mut grounded_hits = 0;
        for _ in 0..60 {
            let frame = battle.step(input([80, 0], !chain))?;
            for cue in &frame.cues {
                if matches!(
                    cue,
                    Cue::Hit {
                        actor: ActorId(1),
                        ..
                    }
                ) {
                    assert!(chain);
                    assert_eq!(frame.actors[0].position[1], 0.);

                    grounded_hits += 1;
                }
            }
            if battle.action_age(id).is_none() {
                break;
            }
            assert_eq!(
                battle.next_action,
                id.0 + 1,
                "landing cannot chain an attack"
            );
        }
        assert!(battle.action_age(id).is_none());
        assert_eq!(grounded_hits, u8::from(chain));
    }
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

fn shortcuts_prepared(control: Control) -> PreparedBattle {
    let mut candidate = prepared(control);
    let prepared = &mut candidate.resources;
    for _ in 0..4 {
        prepared
            .actions
            .entries
            .push(Arc::new(crate::ActionDefinition {
                normal: None,
                execution: crate::ActionExecution::Attack(crate::tests::attack(90)),
                tp_cost: 4,
            }));
    }
    Arc::make_mut(prepared.actor_setup[0].control.as_mut().unwrap()).shortcuts = [1, 35, 34, 4];
    prepared.actor_setup[0].techniques = [1, 35, 34, 4]
        .into_iter()
        .enumerate()
        .map(|(index, catalogue)| crate::PreparedTechnique {
            action: crate::ActionKey(7 + index),
            catalogue,
            player_range: [0., 800.],
            ai_range: [0., 800.],
            capabilities: crate::TechniqueCapabilities {
                family: Some(crate::ArteFamily::Basic),
                uses_weapon_reach: true,
                ..Default::default()
            },
            element: 0,
        })
        .collect();
    candidate
}

fn shortcuts_battle(control: Control) -> Battle {
    shortcuts_prepared(control).finish().unwrap()
}

#[test]
fn held_guard_survives_countdown_and_hit_stop_then_allows_attack() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = battle(mode);
        battle.set_diagnostics(Default::default());
        for _ in 0..40 {
            battle.step(player_buttons(false, false, true, [0; 2]))?;
        }
        assert!(battle.actors[0].guard.active);
        let position = battle.actors[0].position;
        battle.step(player_buttons(false, false, true, [80, 0]))?;
        assert_eq!(battle.actors[0].position, position);

        battle.actors[0].hit_stop = 2;
        for _ in 0..2 {
            let held = battle.step(BattleInput::default())?;
            assert_eq!(held.actors[0].activity, Activity::Guarding);
            assert!(held.actors[0].guard.active);
        }
        let released = battle.step(BattleInput::default())?;
        assert_eq!(released.actors[0].activity, Activity::Idle);
        assert!(!released.actors[0].guard.active);
        battle.step(player_buttons(false, false, true, [0; 2]))?;
        battle.step(player_buttons(true, false, true, [0; 2]))?;
        wait_for_action(&mut battle, crate::ActionKey(0))?;
        assert!(!battle.actors[0].guard.active);
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
fn shortcut_edges_keep_direction_priority_and_pay_tp_once() -> Result<()> {
    for (stick, selected) in [
        ([48, 48], 7),
        ([-49, 48], 10),
        ([80, 49], 8),
        ([-80, -49], 9),
    ] {
        let mut battle = shortcuts_battle(Control::Manual);
        battle.step(player_buttons(true, true, true, stick))?;
        let action = wait_for_action(&mut battle, crate::ActionKey(selected))?;
        assert!(!battle.is_diagnostic());
        assert_eq!(battle.sequence(&action).unwrap().target, ActorId(1));
        assert_eq!(battle.actors[0].tp, 36);
        for _ in 0..3 {
            let mut held = player_buttons(false, false, true, [0; 2]);
            held.controllers[0].technique.held = true;
            battle.step(held)?;
            assert_eq!(battle.actors[0].tp, 36);
            assert!(battle.sequence(&action).is_some());
        }
    }
    Ok(())
}

#[test]
fn insufficient_tp_and_empty_shortcuts_do_not_admit_or_spend() -> Result<()> {
    let mut battle = shortcuts_battle(Control::Manual);
    battle.actors[0].tp = 3;
    let frame = battle.step(player_buttons(false, true, true, [0; 2]))?;
    assert_eq!(frame.actors[0].activity, Activity::Guarding);
    assert_eq!(frame.actors[0].tp, 3);
    assert!(frame.cues.iter().any(|cue| matches!(
        cue,
        Cue::Rejected {
            reason: crate::Rejection::InsufficientTp,
            ..
        }
    )));
    let mut empty = shortcuts_battle(Control::Manual);
    empty.prepare_shortcut(ActorId(0), 0, None)?.commit();
    let frame = empty.step(player_buttons(true, true, false, [0; 2]))?;
    assert!(frame.actions.is_empty());
    assert_eq!(frame.actors[0].activity, Activity::Idle);
    Ok(())
}

#[test]
fn technique_uses_player_range_and_approach_defers_payment_until_arrival() -> Result<()> {
    let mut near = shortcuts_battle(Control::SemiAuto);
    near.step(player_buttons(false, true, false, [0; 2]))?;
    wait_for_action(&mut near, crate::ActionKey(7))?;
    assert_eq!(near.actors[0].position[0], 0.);
    assert_eq!(near.actors[0].tp, 36);
    let mut far = shortcuts_battle(Control::SemiAuto);
    far.actors[1].position[0] = 1000.;

    let frame = far.step(player_buttons(false, true, false, [0; 2]))?;
    assert!(frame.actors[0].position[0] > 0.);
    assert_eq!(frame.actors[0].activity, Activity::Approaching);
    assert_eq!(frame.actors[0].tp, 40);
    assert!(frame.actions.is_empty());
    far.actors[1].position[0] = 100.;

    wait_for_action(&mut far, crate::ActionKey(7))?;
    assert_eq!(far.actors[0].tp, 36);
    Ok(())
}

#[test]
fn third_normal_can_buffer_a_technique_during_hit_stop_without_a_fourth_normal() -> Result<()> {
    let mut battle = shortcuts_battle(Control::Manual);
    battle.step(input([0, 0], true))?;
    wait_for_action(&mut battle, crate::ActionKey(0))?;
    battle.step(input([80, 0], true))?;
    wait_for_action(&mut battle, crate::ActionKey(3))?;
    battle.step(input([0, 0], true))?;
    let third = wait_for_action(&mut battle, crate::ActionKey(0))?;

    let age = battle.action_age(third);
    battle.actors[0].hit_stop = 2;
    battle.step(player_buttons(false, true, false, [0; 2]))?;
    assert_eq!(battle.action_age(third), age);

    assert_eq!(battle.actors[0].tp, 40);
    wait_for_action(&mut battle, crate::ActionKey(7))?;
    assert_eq!(battle.actors[0].tp, 36);
    assert_eq!(battle.actors[0].attack_power, 70);
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

    for _ in 0..14 {
        battle.step(BattleInput::default())?;
    }

    assert!(battle.sequence(&ActionId(1)).is_some());
    assert_eq!(battle.next_action, 2);
    assert_eq!(battle.actors[0].tp, 3);
    Ok(())
}

#[test]
fn prepared_shortcuts_reject_unknown_catalogues_and_invalid_ranges() {
    for kind in [None, Some(NormalAttack::Rising)] {
        let mut candidate = prepared(Control::Manual);
        Arc::make_mut(&mut candidate.resources.actions.entries[0]).normal = kind;
        assert!(candidate.finish().is_err());
    }

    let mut unknown = prepared(Control::Manual);
    Arc::make_mut(unknown.resources.actor_setup[0].control.as_mut().unwrap()).shortcuts[0] = 99;
    assert!(unknown.finish().is_err());
    for range in [[800., 800.], [0., f32::NAN]] {
        let mut prepared = shortcuts_prepared(Control::Manual);
        prepared.resources.actor_setup[0].techniques[0].player_range = range;
        assert!(prepared.finish().is_err());
    }
}

fn active_normal() -> Result<Battle> {
    let mut battle = battle(Control::Manual);
    assert!(battle.start_actor_command(
        crate::ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(0),
            target: ActorId(1)
        },
        &mut vec![]
    )?);
    battle.step(BattleInput::default())?;
    battle.actors[0].heading = 0.;
    battle.actors[0].facing_direction = [0., 0., 1.];
    battle.actors[0].movement.direction = [1., 0., 0.];
    battle.actors[0].movement.forward = 6.;
    Ok(battle)
}

fn pending_technique(control: Control, casting: bool) -> Result<Battle> {
    let mut candidate = shortcuts_prepared(control);
    if casting {
        candidate.resources.actions.entries[7] = Arc::new(crate::tests::cast(90, 4));
    }
    let owner = &mut candidate.actors[0];
    owner.tp = 20;
    owner.heading = 0.;
    owner.facing_direction = [0., 0., 1.];
    owner.movement.direction = [1., 0., 0.];
    owner.movement.forward = 6.;
    let mut battle = candidate.finish()?;
    battle.start(
        ActionRequest {
            actor: ActorId(0),
            action: crate::ActionKey(7),
            target: ActorId(1),
        },
        &mut vec![],
    )?;
    Ok(battle)
}

#[test]
fn admitted_actions_wait_for_facing_then_move_without_repaying() -> Result<()> {
    for (mut battle, paid_tp) in [
        (active_normal()?, 40),
        (pending_technique(Control::Manual, false)?, 16),
        (pending_technique(Control::Auto, false)?, 16),
        (pending_technique(Control::Manual, true)?, 20),
    ] {
        let age = battle.action_age(ActionId(1)).unwrap();
        let mut held = false;
        let mut resumed = false;
        for _ in 0..8 {
            battle.step(BattleInput::default())?;
            let owner = &battle.actors[0];
            assert_eq!(owner.tp, paid_tp);
            assert_eq!(owner.facing_direction, [1., 0., 0.]);
            if battle.action_age(ActionId(1)).unwrap() > age {
                assert!((owner.heading - 90.).abs() < 0.001);
                assert!(owner.position[0] > 0.);
                resumed = true;
                break;
            }
            held = true;
            assert_eq!(owner.position, [0.; 3]);
        }
        assert!(
            held && resumed,
            "an admitted action waits for the turn, then advances"
        );
    }
    Ok(())
}

#[test]
fn airborne_party_and_turn_disabled_actions_do_not_wait_for_facing() -> Result<()> {
    for (side, height, turning_disabled, advances) in [
        (Side::Party, 50., false, true),
        (Side::Enemy, 50., false, false),
        (Side::Party, 0., true, true),
    ] {
        let mut candidate = prepared(Control::Manual);
        if side == Side::Enemy {
            candidate.actors[0].side = Side::Enemy;
            candidate.actors[0].control = Control::Enemy;
            candidate.actors[1].side = Side::Party;
            let setup = &mut candidate.resources.actor_setup[0];
            setup.control = None;
            setup.enemy_decision = Some(crate::EnemyDecisionDefinition {
                strategy: crate::TargetPolicy::Nearest,
                difficulty: 0,
                choices: vec![crate::EnemyChoice {
                    action: crate::ActionKey(0),
                    weight: 1,
                    requirements: Default::default(),
                    target_policy: Some(crate::TargetPolicy::Nearest),
                    return_to_formation: true,
                    guard_chance: 0,
                    range: [0, 0],
                    tp: 0,
                    approach_minimum: 0.,
                    approach_range: 120.,
                }],
                back_row: vec![],
                walk_speed: 5.,
                walk_motion: None,
                turn_ticks: 8,
            });
        }
        let owner = &mut candidate.actors[0];
        owner.position[1] = height;
        owner.heading = 0.;
        owner.facing_direction = [0., 0., 1.];
        owner.movement.turning_disabled = turning_disabled;
        candidate.targets = [1, 0, 1].map(ActorId).to_vec();
        let mut battle = candidate.finish()?;
        battle.start(
            ActionRequest {
                actor: ActorId(0),
                action: crate::ActionKey(0),
                target: ActorId(1),
            },
            &mut vec![],
        )?;
        battle.step(BattleInput::default())?;
        assert_eq!(battle.action_age(ActionId(1)).unwrap() > 0, advances);
        if advances {
            assert_eq!(battle.actors[0].heading, 0.);
        } else {
            assert!(battle.actors[0].heading > 0. && battle.actors[0].heading < 90.);
        }
    }
    Ok(())
}

#[test]
fn hit_stop_holds_the_admitted_action_and_movement_without_repaying() -> Result<()> {
    let mut battle = pending_technique(Control::Manual, false)?;
    battle.actors[0].heading = 90.;
    battle.actors[0].hit_stop = 2;
    battle.actors[0].position[1] = -1.;
    battle.step(BattleInput::default())?;
    assert_eq!(battle.action_age(ActionId(1)), Some(0));
    assert_eq!(battle.actors[0].position, [0.; 3]);
    assert_eq!(battle.actors[0].tp, 16);
    for _ in 0..8 {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.actors[0].hit_stop, 0);
    assert!(battle.action_age(ActionId(1)).unwrap() > 0);
    assert!(battle.actors[0].position[0] > 0.);
    assert_eq!(battle.actors[0].tp, 16);
    Ok(())
}

#[test]
fn recovery_keeps_movement_and_resumes_input_without_repaying() -> Result<()> {
    for mut battle in [
        active_normal()?,
        pending_technique(Control::Manual, false)?,
        pending_technique(Control::Manual, true)?,
    ] {
        let paid_tp = battle.actors[0].tp;
        battle.sequence_mut(&ActionId(1)).unwrap().execution =
            crate::action::Execution::Recovering { remaining: 5 };
        battle.step(BattleInput::default())?;
        assert!(battle.actors[0].position[0] > 0.);
        assert_eq!(battle.actors[0].tp, paid_tp);
        for _ in 0..16 {
            battle.step(BattleInput::default())?;
            if battle.activity(ActorId(0)) == Activity::Idle {
                break;
            }
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        let position = battle.actors[0].position;
        battle.step(input([70, 0], false))?;
        assert_ne!(battle.actors[0].position, position);
        assert_eq!(battle.actors[0].tp, paid_tp);
    }
    Ok(())
}

#[test]
fn player_normal_approach_ignores_the_authored_auto_minimum() -> Result<()> {
    let mut battle = battle(Control::SemiAuto);
    Arc::make_mut(battle.prepared.actor_setup[0].control.as_mut().unwrap()).normals[0]
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
fn physical_target_holds_continue_during_pause_but_selection_waits_for_gameplay() -> Result<()> {
    for pause in 0..2 {
        let mut battle = battle(Control::Manual);
        if pause == 1 {
            battle.request_timed_hold(10, None);
        }
        for tick in 1..=10 {
            let mut held = input([0, 0], false);
            held.paused = pause == 0;
            held.controllers[0].target = ButtonInput {
                held: true,
                pressed: tick == 1,
                released: false,
            };
            assert_eq!(battle.step(held)?.target_selector, None);
            assert_eq!(battle.target_hold_counts().next().flatten(), Some(tick));
        }
        let mut resumed = input([0, 0], false);
        resumed.controllers[0].target.held = true;
        assert_eq!(battle.step(resumed)?.target_selector, Some(ActorId(0)));
        let released = battle.step(input([0, 0], false))?;
        assert_eq!(released.target_selector, None);
        assert_eq!(battle.target_hold_counts().next().flatten(), Some(0));

        // A fresh press during a menu restarts the physical duration.
        let mut pressed = input([0, 0], false);
        pressed.paused = true;
        pressed.controllers[0].target = ButtonInput {
            held: true,
            pressed: true,
            released: false,
        };
        battle.step(pressed)?;
        assert_eq!(battle.target_hold_counts().next().flatten(), Some(1));
        let mut resumed = input([0, 0], false);
        resumed.controllers[0].target.held = true;
        assert_eq!(battle.step(resumed)?.target_selector, None);
        assert_eq!(battle.target_hold_counts().next().flatten(), Some(2));
    }
    Ok(())
}

#[test]
fn target_selector_pauses_all_actors_and_actions_until_release() -> Result<()> {
    let mut candidate = prepared(Control::Manual);
    crate::tests::assign_action(&mut candidate, 1, crate::ActionKey(0));
    crate::tests::assign_action(&mut candidate, 2, crate::ActionKey(0));
    let mut battle = candidate.finish()?;
    battle.release_volley(
        Arc::new(crate::tests::volley()),
        ActorId(0),
        ActorId(1),
        crate::SpellSlot::Primary,
        None,
        &mut vec![],
    )?;
    for count in 1..=9 {
        let mut held = input([0, 0], false);
        held.controllers[0].target.held = true;
        if count == 1 {
            held.actions = vec![
                crate::ActionRequest {
                    actor: ActorId(1),
                    action: crate::ActionKey(0),
                    target: ActorId(0),
                },
                crate::ActionRequest {
                    actor: ActorId(2),
                    action: crate::ActionKey(0),
                    target: ActorId(0),
                },
            ];
        }
        assert_eq!(battle.step(held)?.target_selector, None);
    }
    let before = battle.snapshot();
    let ages = [ActionId(1), ActionId(2), ActionId(3)].map(|id| battle.action_age(id).unwrap());
    for _ in 0..4 {
        let mut held = input([70, 0], true);
        held.controllers[0].target.held = true;
        let paused = battle.step(held)?;
        assert_eq!(paused.target_selector, Some(ActorId(0)));
        assert_eq!(paused.update, before.update);
        assert_eq!(paused.actions.len(), before.actions.len());
        assert!(paused.cues.is_empty());
        for (actor, previous) in paused.actors.iter().zip(&before.actors) {
            assert_eq!(
                (actor.position, actor.activity, actor.hp),
                (previous.position, previous.activity, previous.hp)
            );
        }
        assert_eq!(
            [ActionId(1), ActionId(2), ActionId(3)].map(|id| battle.action_age(id).unwrap()),
            ages
        );
    }
    let mut released = input([0, 0], false);
    released.controllers[0].target.released = true;
    assert_eq!(battle.step(released)?.target_selector, None);
    let resumed = battle.step(BattleInput::default())?;
    assert_eq!(resumed.actions.len(), before.actions.len());
    assert_eq!(
        [ActionId(1), ActionId(2), ActionId(3)].map(|id| battle.action_age(id).unwrap()),
        ages.map(|age| age + 1)
    );
    Ok(())
}

#[test]
fn selector_release_changes_desired_facing_without_moving_or_turning_while_paused() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        let mut battle = battle(mode);
        battle.actors[0].heading = 0.;
        battle.actors[0].movement.direction = [0., 0., -1.];
        battle.actors[1].position = [200., 0., 0.];
        battle.target_selector = Some(ActorId(0));
        let before = battle.snapshot();
        let released = battle.step(BattleInput::default())?;
        assert_eq!(released.target_selector, None);
        assert_eq!(released.update, before.update);
        assert_eq!(released.actors[0].heading, before.actors[0].heading);
        assert_eq!(released.actors[0].position, before.actors[0].position);
        assert_eq!(
            released.actors[0].movement.direction,
            before.actors[0].movement.direction
        );
        assert!(released.actors[0].facing_direction[0] > 0.99);
        let held = battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(held.actors[0].heading, before.actors[0].heading);
        for _ in 0..8 {
            battle.step(BattleInput::default())?;
        }
        assert!((battle.actors[0].heading - 90.).abs() < 0.01);
    }
    Ok(())
}

#[test]
fn changing_live_target_preserves_the_committed_attack() -> Result<()> {
    let mut battle = active_normal()?;
    let committed = battle.sequence(&ActionId(1)).unwrap().target;
    battle.set_decision_target(ActorId(0), ActorId(2))?;
    assert_eq!(battle.target(ActorId(0)), Some(ActorId(2)));
    assert_eq!(battle.sequence(&ActionId(1)).unwrap().target, committed);
    assert_eq!(
        battle.runtime[0].control.as_ref().unwrap().attack_target,
        committed
    );
    Ok(())
}

#[test]
fn arte_boost_shortcuts_and_approach_check_live_cost() -> Result<()> {
    for mode in [Control::Manual, Control::SemiAuto] {
        for tp in [4, 5] {
            let mut battle = shortcuts_battle(mode);
            battle.actors[0].equipment.damage.physical_arte_boost = true;
            battle.actors[0].tp = tp;
            let frame = battle.step(player_buttons(false, true, false, [0; 2]))?;
            if tp == 4 {
                assert!(frame.cues.iter().any(|cue| matches!(
                    cue,
                    Cue::Rejected {
                        reason: crate::Rejection::InsufficientTp,
                        ..
                    }
                )));
                assert!(battle.runtime[0].task().approach().is_none());
                assert_eq!(battle.actors[0].tp, 4);
            } else {
                wait_for_action(&mut battle, crate::ActionKey(7))?;
                assert_eq!(battle.actors[0].tp, 0);
                battle.step(BattleInput::default())?;
                assert_eq!(battle.actors[0].tp, 0);
            }
        }
    }
    // Direct callers cannot bypass the shortcut's earlier affordability gate.
    for mode in [Control::Manual, Control::SemiAuto, Control::Auto] {
        let mut battle = shortcuts_battle(mode);
        battle.actors[0].equipment.damage.physical_arte_boost = true;
        battle.actors[0].tp = 4;
        let parameters = crate::ApproachParameters {
            minimum: 0.,
            maximum: 800.,
            motion: None,
            motion_rate: 0.5,
            speed: 6.,
            turn_ticks: 8,
        };
        assert!(!battle.request_approach(
            ActorId(0),
            ActorId(1),
            crate::ActionKey(7),
            parameters
        )?);
        assert!(battle.runtime[0].task().approach().is_none());
        battle.actors[0].tp = 5;
        assert!(battle.request_approach(
            ActorId(0),
            ActorId(1),
            crate::ActionKey(7),
            parameters
        )?);
        assert_eq!(battle.actors[0].tp, 5);
    }
    Ok(())
}

#[test]
fn ex134_buffered_player_chain_rechecks_quote_at_consumption() -> Result<()> {
    let mut battle = shortcuts_battle(Control::Manual);
    battle.actors[0].equipment.damage.physical_arte_boost = true;
    battle.actors[0].tp = 5;
    battle.step(input([0; 2], true))?;
    battle.step(BattleInput::default())?;
    battle.step(player_buttons(false, true, false, [0; 2]))?;

    battle.actors[0].tp = 4; // Still enough for raw cost, insufficient for quote5.

    let mut rejected = false;
    for _ in 0..14 {
        rejected |= battle.step(BattleInput::default())?.cues.iter().any(|cue| {
            matches!(
                cue,
                Cue::Rejected {
                    reason: crate::Rejection::InsufficientTp,
                    ..
                }
            )
        });
    }
    assert!(rejected);

    assert!(battle.sequence(&ActionId(1)).is_some());
    assert_eq!(battle.next_action, 2);
    assert_eq!(battle.actors[0].tp, 4);
    Ok(())
}

#[test]
fn arte_boost_companion_chain_uses_live_cost() -> Result<()> {
    {
        let mut battle = companion_battle(true);
        battle.actors[0].equipment.damage.physical_arte_boost = true;

        let action = &battle.prepared.actions[crate::ActionKey(7)];
        assert_eq!(battle.action_quote(ActorId(0), crate::ActionKey(7)), 6);
        assert_eq!(action.tp_cost, 5);
        battle.actors[0].tp = 5;
        assert!(!battle.queue_companion_chain(ActorId(0), crate::ActionKey(7))?);

        battle.actors[0].tp = 6;
        assert!(battle.queue_companion_chain(ActorId(0), crate::ActionKey(7))?);

        assert_eq!(battle.actors[0].tp, 6);
    }
    let mut battle = companion_battle(true);
    battle.actors[0].equipment.damage.physical_arte_boost = true;
    battle.actors[0].tp = 5;
    for _ in 0..15 {
        battle.step(BattleInput::default())?;
    }
    battle.confirm_actor_contact(ActorId(0));
    battle.step(BattleInput::default())?;
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .all(|sequence| sequence.action != crate::ActionKey(7))
    );
    assert_eq!(battle.actors[0].tp, 5);

    Ok(())
}

fn usage_shortcuts_battle() -> Battle {
    let prepared = shortcuts_prepared(Control::Manual)
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[1, 35, 34, 4],
            &[(1, 49)],
        )])
        .unwrap();
    prepared.finish().unwrap()
}

#[test]
fn martial_chain_counts_the_accepted_new_catalogue_at_consumption_only() -> Result<()> {
    for allowed in [true, false] {
        let mut battle = usage_shortcuts_battle();
        battle.actors[0].tp = if allowed { 40 } else { 3 };
        battle.step(input([0; 2], true))?;
        battle.step(BattleInput::default())?;
        battle.step(player_buttons(false, true, false, [0; 2]))?;
        assert_eq!(battle.technique_uses(ActorId(0), 1), Some(49));

        for _ in 0..16 {
            battle.step(BattleInput::default())?;
        }
        assert_eq!(
            battle.technique_uses(ActorId(0), 1),
            Some(if allowed { 50 } else { 49 })
        );
        assert_eq!(battle.actors[0].proficiency, if allowed { 1 } else { 0 });
        assert_eq!(battle.actors[0].tp, if allowed { 36 } else { 3 });
    }
    Ok(())
}

mod shortcut_tests;

#[test]
fn equipped_combo_limit_allows_an_extra_normal() -> Result<()> {
    for extra in [0, 1] {
        let mut battle = battle(Control::Manual);
        battle.actors[0].equipment.normal_combo_limit = 3 + extra;
        battle.step(input([0, 0], true))?;
        battle.step(BattleInput::default())?;
        battle.step(input([80, 0], true))?;
        for _ in 0..14 {
            battle.step(BattleInput::default())?;
        }
        battle.step(BattleInput::default())?;
        battle.step(input([0, 0], true))?;
        for _ in 0..14 {
            battle.step(BattleInput::default())?;
        }
        battle.step(BattleInput::default())?;

        battle.step(input([80, 0], true))?;

        for _ in 0..14 {
            battle.step(BattleInput::default())?;
        }
        assert_eq!(battle.next_action, 4 + u64::from(extra));
    }
    Ok(())
}
