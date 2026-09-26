use super::*;
use crate::{
    ActionDefinition, ActionPhase, BattleInput, ButtonInput, ControlDefinition, DecisionDefinition,
    NormalControl, PreparedBattle, ResourceBinding, Side, SoundBinding,
    tests::{actor, effect_binding},
};
use std::{collections::BTreeMap, sync::Arc};

fn battle() -> Battle {
    let sources = BTreeMap::from([
        ("control".into(), "script battle; use battle; use battle::mobility; pub task run() { while true { mobility::update(); await battle::next_update(); } } pub task normal() { await battle::at_age(ticks(90)); }".into()),
        ("battle::mobility".into(), include_str!("../../../../scripts/battle/mobility.sym").into()),
    ]);
    let compiled =
        symphonia_script_compiler::compile("control", &sources, &crate::native_declarations())
            .unwrap();
    let function = |name: &str| {
        compiled
            .program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap()
            .entry
    };
    let entry = function("control::run");
    let normal_entry = function("control::normal");
    let program = Arc::new(compiled.program);
    let mut actions = vec![ActionDefinition {
        id: 10,
        phase: ActionPhase::Decision,
        entry,
        duration: 0,
        tp_cost: 0,
        program: program.clone(),
        resources: compiled
            .assets
            .iter()
            .map(|asset| match asset.kind.as_str() {
                "battle::OptionalMotion" => ResourceBinding::OptionalMotion(vec![None; 2]),
                "battle::Effect" => ResourceBinding::Effect(42),
                "battle::Sound" => ResourceBinding::Sound(SoundBinding {
                    resource: 43,
                    index: 137,
                }),
                _ => panic!("unexpected mobility resource {}", asset.path),
            })
            .collect(),
    }];
    for id in 0..7 {
        actions.push(ActionDefinition {
            id,
            phase: ActionPhase::Actor,
            entry: normal_entry,
            duration: 90,
            tp_cost: 0,
            program: program.clone(),
            resources: actions[0].resources.clone(),
        });
    }
    let mut owner = actor(Side::Party);
    owner.control = Control::SemiAuto;
    owner.movement.direction = [1., 0., 0.];
    owner.movement.target_direction = owner.movement.direction;
    owner.facing_direction = owner.movement.direction;
    let mut target = actor(Side::Enemy);
    target.position = [1000., 0., 0.];
    let prepared = PreparedBattle::new(
        vec![owner, target],
        actions,
        0x13572468,
        vec![],
        vec![effect_binding(42, [17])],
    )
    .unwrap()
    .with_controls(vec![ControlDefinition {
        actor: ActorId(0),
        target: ActorId(1),
        combo_limit: 3,
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 8,
        motions: None,
        shortcuts: [None; 4],
        normals: std::array::from_fn(|index| NormalControl {
            action: index as u16,
            allowed_directions: 0,
            fallback: None,
            reach: 120.,
            minimum_reach: 0.,
            combo_at: [0; 2],
            buffer_until: 90,
        }),
    }])
    .unwrap()
    .with_decisions(vec![DecisionDefinition {
        actor: ActorId(0),
        target: ActorId(1),
        action: 10,
        idle_ticks: 0,
        idle_variation: 0,
        fidget_ticks: 0,
        idle_motion: None,
    }])
    .unwrap();
    Battle::new(Arc::new(prepared))
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
fn source_jump_and_backstep_vectors_cover_admission_arc_and_full_recovery() {
    // Source02, unmodified opening save, raw PAD arrival VI1834. No fitted
    // phase offsets; every row is one original VI from guard admission onward.
    // The away vector includes re-guard, neutral release and the direct idle tail.
    type SourceVisit = (u32, u8, i16, f32, f32, f32, f32);
    let vectors: BTreeMap<String, Vec<SourceVisit>> =
        serde_json::from_str(include_str!("../../tests/fixtures/opening-mobility.json")).unwrap();
    for (mode, rows) in vectors {
        let mut battle = battle();
        for (visit, callback, count, height, forward, vertical, gravity) in rows {
            let up = mode == "guard-up" && (2..10).contains(&visit);
            let away = mode == "guard-away" && visit == 2;
            battle
                .step(input(
                    [if away { -70 } else { 0 }, if up { 70 } else { 0 }],
                    visit < 50,
                    if away { -1 } else { 0 },
                ))
                .unwrap();
            let actor = &battle.actors[0];
            let actual = (
                actor.reaction.remaining,
                actor.position[1],
                actor.movement.forward,
                actor.movement.vertical,
                actor.movement.gravity,
            );
            assert_eq!(
                actual,
                (count, height, forward, vertical, gravity),
                "{mode} VI{} callback{callback}",
                1834 + visit
            );
            let activity = match callback {
                3 | 2 => Activity::Idle,
                11 => Activity::Guarding,
                12 => Activity::Jumping,
                8 => Activity::Recovering,
                17 => Activity::Evading,
                _ => panic!("unexpected callback"),
            };
            assert_eq!(actor.activity, activity, "{mode} VI{}", 1834 + visit);
            assert_eq!(actor.reaction.idle_initialization.is_some(), callback == 2);
        }
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
fn local_hit_stop_charges_jump_before_guard_callback_but_menu_holds_every_clock() {
    let mut battle = battle();
    battle.step(input([0, 0], true, 0)).unwrap();
    battle.actors[0].hit_stop = 8;
    for _ in 0..5 {
        battle.step(input([0, 70], true, 0)).unwrap();
    }
    assert_eq!(
        battle.controls[0].as_ref().unwrap().jump_charge,
        JumpCharge(5)
    );
    let before = battle.actors[0].clone();
    let rng = battle.random_state();
    for _ in 0..6 {
        let mut held = input([0, 70], true, 0);
        held.menu_open = true;
        battle.step(held).unwrap();
    }
    assert_eq!(battle.actors[0].hit_stop, before.hit_stop);
    assert_eq!(battle.random_state(), rng);
    // Hit-stop consumes this selector's sixth emission without dispatching29770.
    battle.step(input([0, 70], true, 0)).unwrap();
    assert_eq!(
        battle.controls[0].as_ref().unwrap().jump_charge,
        JumpCharge(0)
    );
    assert_eq!(battle.actors[0].activity, Activity::Guarding);
}

#[test]
fn backstep_requires_guard_fresh_away_edge_and_allows_contact_interruption() {
    let mut battle = battle();
    battle.step(input([-70, 0], true, -1)).unwrap();
    assert_eq!(battle.actors[0].activity, Activity::Guarding);
    battle.step(input([-70, 0], true, 0)).unwrap();
    assert_eq!(battle.actors[0].activity, Activity::Guarding);
    battle.step(input([0, 0], true, 0)).unwrap();
    battle.actors[0].heading = -90.;
    battle.actors[0].movement.turning_disabled = true;
    battle.step(input([-70, 0], true, -1)).unwrap();
    assert_eq!(battle.actors[0].activity, Activity::Evading);
    // 4DDB4: frsp(atan2(1,0)) * source degree factor0x42652ee4.
    assert_eq!(battle.actors[0].heading.to_bits(), 0x42b4_0003);
    assert!(!battle.actors[0].movement.turning_disabled);
    battle.actors[0].activity = Activity::Hurt;
    battle.actors[0].reaction.remaining = 10;
    battle.step(input([0, 0], false, 0)).unwrap();
    assert!(battle.controls[0].as_ref().unwrap().mobility.is_none());
    assert_eq!(battle.actors[0].activity, Activity::Hurt);
}

#[test]
fn jump_can_be_replaced_by_its_prepared_aerial_normal() {
    let mut battle = battle();
    battle.actors[0].control = Control::Manual;
    for _ in 0..7 {
        battle.step(input([0, 70], false, 0)).unwrap();
    }
    assert_eq!(battle.actors[0].activity, Activity::Jumping);
    let mut expected_rng = crate::Random::from_state(battle.random_state());
    expected_rng.next();
    battle.actors[0].heading = -90.;
    battle.actors[0].movement.turning_disabled = true;
    // Keep command18F0 at +X, with a different retained facing18E4 at +Z.
    battle.actors[0].facing_direction = [0., 0., 1.];
    let mut attack = input([0, 0], false, 0);
    attack.controllers[0].attack.pressed = true;
    battle.step(attack).unwrap();
    assert_eq!(battle.random_state(), expected_rng.state());
    // 2EEE4 snaps command heading to0x42b40003, then its24D24 tail turns
    // toward retained facing by180/8=22.5. The callback does both writes.
    assert_eq!(battle.actors[0].heading.to_bits(), 0x4287_0003);
    assert!(!battle.actors[0].movement.turning_disabled);
    assert_eq!(battle.actors[0].position[1], 40.);
    assert_eq!(battle.actors[0].movement.vertical, 18.5);
    assert_eq!(battle.actors[0].reaction.remaining, 1);
    assert!(matches!(battle.actors[0].activity, Activity::Action { .. }));
    assert!(battle.actors[0].movement.airborne_action);
    assert!(battle.controls[0].as_ref().unwrap().mobility.is_none());
}

#[test]
fn jump_and_backstep_consume_only_the_source_guard_initializer_draws() {
    let mut jump = battle();
    jump.actors[0].control = Control::Manual;
    let initial = jump.random_state();
    for _ in 0..5 {
        jump.step(input([0, 70], false, 0)).unwrap();
    }
    assert_eq!(jump.random_state(), initial);
    let mut random = crate::Random::from_state(initial);
    random.next();
    jump.step(input([0, 70], false, 0)).unwrap();
    assert_eq!(jump.random_state(), random.state());
    // Local stop does not hold2EEE4; a command-menu visit does.
    jump.actors[0].hit_stop = 3;
    let mut held = input([0, 0], false, 0);
    held.menu_open = true;
    jump.step(held).unwrap();
    assert_eq!(jump.actors[0].reaction.remaining, 0);
    jump.step(input([0, 0], false, 0)).unwrap();
    assert_eq!(jump.actors[0].reaction.remaining, 1);
    assert_eq!(jump.actors[0].position[1], 20.5);
    assert_eq!(jump.random_state(), random.state());
    for _ in 0..60 {
        jump.step(input([0, 0], false, 0)).unwrap();
    }
    random.next();
    assert_eq!(jump.random_state(), random.state());

    let mut step = battle();
    step.step(input([0, 0], true, 0)).unwrap();
    let initial = step.random_state();
    step.step(input([-70, 0], true, -1)).unwrap();
    assert_eq!(step.random_state(), initial);
    for _ in 0..36 {
        step.step(input([0, 0], false, 0)).unwrap();
    }
    let mut random = crate::Random::from_state(initial);
    random.next();
    assert_eq!(step.random_state(), random.state());
    assert_eq!(step.actors[0].activity, Activity::Idle);
    assert_eq!(step.actors[0].reaction.remaining, 29);
    assert_eq!(step.actors[0].reaction.idle_initialization, None);
}

#[test]
fn combined_condition_200_blocks_mobility_after_the_selector_charge() {
    let mut battle = battle();
    battle.actors[0].movement.mobility_blocked = true;
    battle.step(input([0, 0], true, 0)).unwrap();
    battle.step(input([-70, 0], true, -1)).unwrap();
    assert_eq!(battle.actors[0].activity, Activity::Guarding);
    for _ in 0..6 {
        battle.step(input([0, 70], true, 0)).unwrap();
    }
    assert_eq!(battle.actors[0].activity, Activity::Guarding);
    assert_eq!(
        battle.controls[0].as_ref().unwrap().jump_charge,
        JumpCharge(0)
    );
}

#[test]
fn aerial_attack_on_grounded_boundary_is_superseded_by_landing() {
    let mut battle = battle();
    battle.actors[0].control = Control::Manual;
    for _ in 0..7 {
        battle.step(input([0, 70], false, 0)).unwrap();
    }
    while battle.actors[0].position[1] > 0.1 {
        battle.step(input([0, 0], false, 0)).unwrap();
    }
    assert_eq!(battle.actors[0].activity, Activity::Jumping);
    let mut expected_rng = crate::Random::from_state(battle.random_state());
    expected_rng.next();
    let mut attack = input([0, 0], false, 0);
    attack.controllers[0].attack.pressed = true;
    battle.step(attack).unwrap();
    assert_eq!(battle.actors[0].activity, Activity::Recovering);
    assert_eq!(battle.actors[0].reaction.remaining, 17);
    assert!(
        battle
            .sequences
            .values()
            .all(|sequence| sequence.definition.phase != ActionPhase::Actor)
    );
    assert_eq!(battle.random_state(), expected_rng.state());
}
