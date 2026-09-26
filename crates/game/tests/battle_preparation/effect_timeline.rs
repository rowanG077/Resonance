use super::actor;
use resonance_battle::{
    ActionDefinition, ActionPhase, ActionRequest, ActorId, Battle, BattleInput, Cue,
    PreparedBattle, ResourceBinding,
};
use resonance_content::battle_effect::Record;
use resonance_game::battle::effect_timeline::prepare;
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::authored::Module;

fn record(age: i16, command: u8, argument: u8, operand: u16) -> Record {
    Record {
        age,
        command,
        argument,
        operand,
    }
}

fn commands(records: &[Record]) -> (Module, Vec<Option<u16>>) {
    // Test witnesses identify dispatched records; they do not implement particles.
    let mut source = String::from(
        "script battle; use battle; asset effect: battle::Effect = \"effects/test.json\"; ",
    );
    for (i, _) in records.iter().enumerate().filter(|(_, r)| r.command < 254) {
        source.push_str(&format!(
            "pub fn command_{i}() {{ battle::show(effect, {i}, battle::owner()); }}\n"
        ));
    }
    source.push_str("pub fn unused() {}\n");
    let compiled = symphonia_script_compiler::compile(
        "commands",
        &BTreeMap::from([("commands".into(), source)]),
        &resonance_battle::native_declarations(),
    )
    .unwrap();
    let module = compiled.program.authored().unwrap().clone();
    let bindings = records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            (record.command < 254).then(|| {
                module
                    .functions
                    .iter()
                    .position(|f| f.name == format!("commands::command_{i}"))
                    .unwrap() as u16
            })
        })
        .collect();
    (module, bindings)
}

fn battle(records: &[Record]) -> (Battle, ActorId) {
    let (module, bindings) = commands(records);
    let (program, entry) = prepare(records, &bindings, module).unwrap();
    let prepared = Arc::new(
        PreparedBattle::new(
            vec![actor()],
            vec![ActionDefinition {
                id: 1,
                phase: ActionPhase::Actor,
                program,
                entry,
                duration: i16::MAX as u16,
                tp_cost: 0,
                resources: vec![ResourceBinding::Effect(0)],
            }],
            1,
            vec![],
            vec![empty_effects(0, 0..records.len() as u16)],
        )
        .unwrap(),
    );
    let actor = prepared.actor_ids().next().unwrap();
    (Battle::new(prepared), actor)
}

fn step((battle, id): &mut (Battle, ActorId), start: bool) -> (Vec<usize>, bool) {
    let input = BattleInput {
        actions: if start {
            vec![ActionRequest {
                actor: *id,
                action: 1,
                target: *id,
            }]
        } else {
            vec![]
        },
        ..Default::default()
    };
    let frame = battle.step(input).unwrap();
    (
        frame
            .cues
            .iter()
            .filter_map(|c| match c {
                Cue::Effect { member, .. } => Some(*member as usize),
                _ => None,
            })
            .collect(),
        frame
            .cues
            .iter()
            .any(|c| matches!(c, Cue::Completed { .. })),
    )
}

#[test]
fn source_order_repeats_and_end_share_the_existing_task_scheduler() {
    let records = [
        record(0, 255, 9, 2),
        record(300, 1, 0, 0),
        record(0, 255, 9, 1),
        record(-1, 2, 0, 0),
        record(0, 3, 0, 0),
        record(2, 4, 0, 0),
        record(1, 5, 0, 0),
        record(4, 254, 0, 0),
    ];
    let mut battle = battle(&records);
    for (age, expected) in [vec![4, 1, 3], vec![3], vec![5, 6, 1, 3], vec![3], vec![]]
        .into_iter()
        .enumerate()
    {
        let (actual, completed) = step(&mut battle, age == 0);
        assert_eq!(actual, expected, "age {age}");
        assert_eq!(completed, age == 4);
    }
    assert!(step(&mut battle, false).0.is_empty());
}

#[test]
fn nonpositive_intervals_zero_count_and_same_visit_end_match_native_control_flow() {
    for interval in [0, u16::MAX] {
        let records = [
            record(-1, 255, 3, interval),
            record(300, 1, 0, 0),
            record(0, 255, 0, 1),
            record(300, 2, 0, 0),
            record(1, 254, 0, 0),
        ];
        let mut battle = battle(&records);
        assert_eq!(step(&mut battle, true), (vec![1, 1, 1, 3], false));
        assert_eq!(step(&mut battle, false), (vec![], true));
    }
    let records = [
        record(0, 255, 3, 0),
        record(0, 1, 0, 0),
        record(0, 2, 0, 0),
        record(0, 254, 0, 0),
    ];
    assert_eq!(step(&mut battle(&records), true), (vec![2], true));
}

#[test]
fn pause_and_cancellation_do_not_restart_pending_repeats() {
    let records = [
        record(0, 255, 3, 2),
        record(0, 1, 0, 0),
        record(8, 254, 0, 0),
    ];
    let mut battle = battle(&records);
    assert_eq!(step(&mut battle, true).0, [1]);
    for _ in 0..3 {
        assert!(
            battle
                .0
                .step(BattleInput {
                    menu_open: true,
                    ..Default::default()
                })
                .unwrap()
                .cues
                .is_empty()
        );
    }
    assert!(step(&mut battle, false).0.is_empty());
    assert_eq!(step(&mut battle, false).0, [1]);
    let frame = battle.0.step(BattleInput::default()).unwrap();
    let id = frame.actions[0].0;
    let frame = battle
        .0
        .step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.cues, [Cue::Interrupted { action: id }]);
    assert!(step(&mut battle, false).0.is_empty());
}

#[test]
fn malformed_records_and_unbound_commands_fail_preparation() {
    let records = [record(0, 1, 0, 0), record(8, 254, 0, 0)];
    let (module, mut bindings) = commands(&records);
    assert!(prepare(&records[..1], &bindings[..1], module.clone()).is_err());
    assert!(prepare(&records, &bindings[..1], module.clone()).is_err());
    bindings[0] = None;
    assert!(prepare(&records, &bindings, module.clone()).is_err());
    bindings[0] = Some(u16::MAX);
    assert!(prepare(&records, &bindings, module.clone()).is_err());
    for records in [
        vec![record(0, 255, 2, 1)],
        vec![record(0, 255, 2, 1), record(8, 254, 0, 0)],
        vec![record(0, 254, 0, 0), record(8, 254, 0, 0)],
    ] {
        assert!(prepare(&records, &vec![None; records.len()], module.clone()).is_err());
    }
}

#[test]
fn original_effect_dispatches_match_dolphin_visits() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/effect-timelines.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let records: Vec<Record> = serde_json::from_value(case["records"].clone()).unwrap();
        let mut battle = battle(&records);
        for (age, visit) in case["visits"].as_array().unwrap().iter().enumerate() {
            let expected: Vec<usize> = serde_json::from_value(visit["commands"].clone()).unwrap();
            assert_eq!(
                step(&mut battle, age == 0),
                (expected, !visit["active"].as_bool().unwrap()),
                "case {} age {age}",
                case["program"]
            );
        }
    }
}

fn empty_effects(
    resource: u32,
    members: impl IntoIterator<Item = u16>,
) -> resonance_battle::EffectBank {
    let source = resonance_content::battle_effect::ProgramSource {
        models: Default::default(),
        records: vec![resonance_content::battle_effect::Record {
            age: 0,
            command: 254,
            argument: 0,
            operand: 0,
        }],
        particles: Default::default(),
        modifiers: Default::default(),
    };
    resonance_battle::EffectBank {
        models: Default::default(),
        resource,
        members: members
            .into_iter()
            .map(|id| {
                (
                    id,
                    Arc::new(
                        resonance_game::battle::effect_program::prepare(
                            &source,
                            resource,
                            id,
                            &mut super::effect_runtime::no_sound,
                        )
                        .unwrap(),
                    ),
                )
            })
            .collect(),
    }
}
