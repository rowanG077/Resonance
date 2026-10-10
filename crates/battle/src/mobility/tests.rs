use super::*;
use crate::Activity;
use crate::conditions::{Condition, ConditionSet};
use crate::{
    ActionDefinition, ActorSetup, BattleInput, ButtonInput, ControlDefinition, DecisionDefinition,
    NormalControl, PreparedBattle, Side, tests::actor,
};
use std::sync::Arc;

#[path = "backstep_guard_tests.rs"]
mod backstep_guard_tests;

pub(crate) fn battle() -> Battle {
    prepared().finish().unwrap()
}

pub(crate) fn prepared() -> PreparedBattle {
    let actions: Vec<_> = (0..7)
        .map(|index| ActionDefinition {
            normal: Some(crate::NormalAttack::ALL[index]),
            tp_cost: 0,
            execution: crate::ActionExecution::Attack(crate::PreparedAttack {
                chain_at: None,
                end_at: 90,
                opening: None,
                events: vec![],
                recovery: 0,
            }),
        })
        .collect();
    let mut owner = actor(Side::Party);
    owner.control = Control::SemiAuto;
    owner.movement.direction = [1., 0., 0.];
    owner.movement.target_direction = owner.movement.direction;
    owner.facing_direction = owner.movement.direction;
    let mut target = actor(Side::Enemy);
    target.position = [1000., 0., 0.];
    let setup = vec![
        ActorSetup {
            control: Some(Arc::new(ControlDefinition {
                walk_speed: 5.,
                run_speed: 10.,
                turn_ticks: 8,
                motions: None,
                shortcuts: [0; 4],
                normals: std::array::from_fn(|index| NormalControl {
                    action: crate::ActionKey(index),

                    reach: 120.,
                    minimum_reach: 0.,
                }),
            })),
            decision: Some(DecisionDefinition {
                idle_ticks: 0,
                idle_variation: 0,
            }),
            ..Default::default()
        },
        ActorSetup::default(),
    ];
    PreparedBattle::new(
        vec![owner, target].into_iter().zip(setup).collect(),
        actions.into(),
        0x13572468,
    )
    .unwrap()
}

fn input(stick: [i8; 2], guard: bool, edge: i8) -> BattleInput {
    BattleInput {
        controllers: vec![ControlInput {
            stick,
            horizontal_pressed: edge,
            guard: ButtonInput {
                held: guard,
                ..Default::default()
            },
            ..ControlInput::neutral(ActorId(0))
        }],
        ..Default::default()
    }
}

#[test]
fn jump_and_landing_recover_without_artwork() {
    {
        let mut prepared = prepared();
        prepared.actors[0].control = Control::Manual;
        let mut battle = prepared.finish().unwrap();
        battle.set_diagnostics(Default::default());
        for _ in 0..JUMP_CHARGE_UPDATES {
            battle.step(input([0, 80], false, 0)).unwrap();
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
        assert_eq!(battle.actors[0].position[1], JUMP_SPEED);
        let airborne = battle.actors[0].position;
        battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.actors[0].position, airborne);
        let mut landed = false;
        for _ in 0..100 {
            battle.step(BattleInput::default()).unwrap();
            landed |= battle.activity(ActorId(0)) == Activity::Recovering;
            if battle.activity(ActorId(0)) == Activity::Idle {
                break;
            }
        }
        assert!(landed);
        assert_eq!(battle.activity(ActorId(0)), Activity::Idle);
        assert_eq!(battle.actors[0].position[1], 0.);
        assert!(!battle.is_diagnostic());
    }
}

#[test]
fn jump_charge_requires_six_qualifying_visits_and_preserves_unguarded_up() {
    let mut charge = JumpCharge::default();
    for _ in 0..12 {
        assert!(!charge.sample(Control::SemiAuto, 48, true, false));
    }
    for _ in 0..5 {
        assert!(!charge.sample(Control::SemiAuto, 49, true, false));
    }
    for _ in 0..9 {
        assert!(!charge.sample(Control::SemiAuto, 70, false, false));
    }
    assert!(charge.sample(Control::SemiAuto, 49, true, false));
    for _ in 0..5 {
        assert!(!charge.sample(Control::Manual, 70, false, false));
    }
    assert!(charge.sample(Control::Manual, 70, false, false));
    charge.sample(Control::SemiAuto, 70, true, false);
    charge.sample(Control::SemiAuto, 70, true, true);
    assert_eq!(charge, JumpCharge(0));
    for _ in 0..9 {
        assert!(!charge.sample(Control::Auto, 70, true, false));
    }
}

#[test]
fn jump_can_be_replaced_by_its_prepared_aerial_normal() {
    let mut battle = battle();
    battle.actors[0].control = Control::Manual;
    for _ in 0..7 {
        battle.step(input([0, 70], false, 0)).unwrap();
    }
    assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
    let mut attack = input([0, 0], false, 0);
    attack.controllers[0].attack.pressed = true;
    battle.step(attack).unwrap();
    for _ in 0..3 {
        if matches!(battle.activity(ActorId(0)), Activity::Action) {
            break;
        }
        battle.step(BattleInput::default()).unwrap();
    }
    assert!(matches!(battle.activity(ActorId(0)), Activity::Action));
    assert!(battle.actors[0].movement.airborne_action);
    assert!(battle.runtime[0].task().mobility().is_none());
    assert!(
        battle
            .sequences()
            .map(|(_, sequence)| sequence)
            .any(|sequence| matches!(
                &sequence.definition.execution,
                crate::ActionExecution::Attack(_)
            ) && sequence.action == crate::ActionKey(5))
    );
}

#[test]
fn heavy_condition_blocks_jump_and_backstep() {
    let mut battle = battle();
    battle.actors[0].conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        intrinsic: ConditionSet::of(&[Condition::Heavy]),
        ..Default::default()
    });
    battle.step(input([0, 0], true, 0)).unwrap();
    battle.step(input([-70, 0], true, -1)).unwrap();
    assert_eq!(battle.activity(ActorId(0)), Activity::Guarding);
    for _ in 0..6 {
        battle.step(input([0, 70], true, 0)).unwrap();
    }
    assert_eq!(battle.activity(ActorId(0)), Activity::Guarding);
    assert_eq!(
        battle.runtime[0].control.as_ref().unwrap().jump_charge,
        JumpCharge(0)
    );
}

#[test]
fn changing_control_mode_preserves_active_jump_movement() -> Result<()> {
    let setup = || -> Result<Battle> {
        let mut prepared = prepared();
        let actors = &mut prepared.resources.actor_setup;
        actors[0].companion = Some(crate::CompanionDefinition {
            initial_policy: [0; 3],
            defaults: [1, 5, 2],
            limits: [crate::PolicyLimits::default(); 9],
            level: 1,
            level_difference: 0,
        });
        prepared.finish()
    };
    let mut changed = setup()?;
    let mut reference = setup()?;
    for _ in 0..7 {
        changed.step(input([0, 70], true, 0))?;
        reference.step(input([0, 70], true, 0))?;
    }
    assert_eq!(changed.activity(ActorId(0)), Activity::Jumping);
    changed.set_control_mode(ActorId(0), Control::Auto)?;
    let mut landed = false;
    for _ in 0..80 {
        changed.step(BattleInput::default())?;
        reference.step(BattleInput::default())?;
        assert_eq!(
            changed.actors[0].position[1],
            reference.actors[0].position[1]
        );
        assert_eq!(
            changed.actors[0].movement.vertical,
            reference.actors[0].movement.vertical
        );
        if changed.activity(ActorId(0)) == Activity::Idle {
            landed = true;
            break;
        }
    }
    assert!(landed);
    Ok(())
}

#[path = "contact_recovery_tests.rs"]
mod contact_recovery_tests;

#[path = "aerial_command_tests.rs"]
mod aerial_command_tests;

#[test]
fn landing_trait_shortens_recovery_and_menu_pause_holds_it() {
    let recover = |quick| {
        let mut battle = battle();
        battle.actors[0].equipment.combo_traits.landing = quick;
        for _ in 0..JUMP_CHARGE_UPDATES {
            battle.step(input([0, 70], true, 0)).unwrap();
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Jumping);
        for _ in 0..100 {
            battle.step(BattleInput::default()).unwrap();
            if battle.activity(ActorId(0)) == Activity::Recovering {
                break;
            }
        }
        assert_eq!(battle.activity(ActorId(0)), Activity::Recovering);
        let position = battle.actors[0].position;
        for _ in 0..20 {
            battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(battle.actors[0].position, position);
            assert_eq!(battle.activity(ActorId(0)), Activity::Recovering);
        }
        for ticks in 1..100 {
            battle.step(BattleInput::default()).unwrap();
            if battle.activity(ActorId(0)) == Activity::Idle {
                return ticks;
            }
        }
        panic!("landing never completed");
    };
    assert!(recover(true) < recover(false));
}
