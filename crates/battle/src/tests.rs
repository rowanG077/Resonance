use super::*;
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script_compiler::compile;

#[path = "diagnostic_tests.rs"]
mod diagnostic_tests;

pub(super) fn actor(side: Side) -> Actor {
    Actor {
        side,
        control: Default::default(),
        activity: Default::default(),
        availability: Default::default(),
        overlimit: 0,
        overlimit_active: false,
        guard: Default::default(),
        hp: 50,
        max_hp: 100,
        tp: 40,
        max_tp: 40,
        hud: Default::default(),
        luck: 0,
        stats: crate::CombatStats::default(),
        elements: Default::default(),
        affinities: [crate::Affinity::Normal; 9],
        attack_power: 100,
        physical_arte_boost: false,
        recovery: RecoveryTraits::default(),
        petrified: false,
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        framing: Default::default(),
        movement: Default::default(),
        hit_stop: 0,
        reaction: Default::default(),
        body: Body::default(),
    }
}

pub(super) fn actor_tints() -> ResourceBinding {
    let tints: resonance_content::battle_effect::Tints =
        serde_json::from_str(include_str!("../../game/tests/fixtures/effect-tints.json")).unwrap();
    ResourceBinding::ActorTints(tints.actors)
}

pub(super) fn effect_binding(resource: u32, members: impl IntoIterator<Item = u16>) -> EffectBank {
    let sources = BTreeMap::from([(
        "empty".into(),
        "script battle; use battle; pub task run() { battle::finish(); }".into(),
    )]);
    let compiled = compile("empty", &sources, &native_declarations()).unwrap();
    let entry = compiled.program.authored().unwrap().functions[0].entry;
    let program = Arc::new(compiled.program);
    EffectBank {
        models: Default::default(),
        resource,
        members: members
            .into_iter()
            .map(|id| {
                (
                    id,
                    Arc::new(ActionDefinition {
                        id,
                        phase: ActionPhase::Effect,
                        program: program.clone(),
                        entry,
                        duration: 0,
                        tp_cost: 0,
                        resources: vec![],
                    }),
                )
            })
            .collect(),
    }
}

fn projectile_effects<'a>(
    definitions: impl IntoIterator<Item = &'a ProjectileDefinition>,
) -> Vec<EffectBank> {
    let mut members = BTreeMap::<u32, Vec<u16>>::new();
    for definition in definitions {
        for effect in definition
            .birth
            .into_iter()
            .chain(definition.contact.as_ref().and_then(|c| c.clash_effect))
        {
            members
                .entry(effect.resource)
                .or_default()
                .push(effect.member);
        }
    }
    members
        .into_iter()
        .map(|(resource, members)| effect_binding(resource, members))
        .collect()
}

pub(super) fn prepared(source: &str, actors: Vec<Actor>, duration: u16) -> Arc<PreparedBattle> {
    prepared_entry(source, actors, duration, "run")
}

fn prepared_entry(
    source: &str,
    actors: Vec<Actor>,
    duration: u16,
    entry: &str,
) -> Arc<PreparedBattle> {
    let sources = BTreeMap::from([(
        "test".into(),
        format!("script battle; use battle; {source}"),
    )]);
    let compiled = compile("test", &sources, &native_declarations()).unwrap();
    let resources = compiled
        .assets
        .iter()
        .map(|asset| match asset.kind.as_str() {
            "battle::ActorTints" => actor_tints(),
            "battle::Voice" => ResourceBinding::Voice(
                actors
                    .iter()
                    .enumerate()
                    .map(|(i, actor)| {
                        (actor.side == Side::Party).then_some(VoiceLine {
                            sound: SoundBinding {
                                resource: 1,
                                index: 43 + i as u16,
                            },
                            duration: 0,
                        })
                    })
                    .collect(),
            ),
            // Callers replace other placeholder bindings with their fixture data.
            _ => ResourceBinding::Effect(37),
        })
        .collect();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == format!("test::{entry}"))
        .unwrap()
        .entry;
    Arc::new(
        PreparedBattle::new(
            actors,
            vec![ActionDefinition {
                phase: ActionPhase::Resident,
                id: 99,
                program: Arc::new(compiled.program),
                entry,
                duration,
                tp_cost: 28,
                resources,
            }],
            1,
            vec![],
            vec![effect_binding(37, [1, 6])],
        )
        .unwrap(),
    )
}

fn request(actor: u8) -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(actor),
            action: 99,
            target: ActorId(actor),
        }],
        ..Default::default()
    }
}
fn recovery(frame: &BattleFrame) -> Vec<(ActorId, i16, i32)> {
    frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Recovered {
                actor,
                nominal,
                applied,
            } => Some((*actor, *nominal, *applied)),
            _ => None,
        })
        .collect()
}

#[test]
fn effect_target_is_its_recipient_even_when_the_emitting_action_targets_an_enemy() {
    for call in [
        "battle::show(effect, 1, battle::owner());",
        "battle::show_following(effect, 1, battle::owner(), 1.0, true, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });",
    ] {
        let mut prepared = prepared(&format!(
            "asset effect: battle::Effect = \"test/effect\";
             pub task run() {{ {call} }}
             pub task recipient() {{ battle::heal_percent(battle::target(), 10); battle::finish(); }}"
        ), vec![actor(Side::Party), actor(Side::Enemy)], 10);
        let definitions = Arc::get_mut(&mut prepared).unwrap();
        let mut effect = definitions.actions[0].clone();
        effect.phase = ActionPhase::Effect;
        effect.tp_cost = 0;
        effect.entry = effect
            .program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == "test::recipient")
            .unwrap()
            .entry;
        definitions
            .effects
            .get_mut(&37)
            .unwrap()
            .members
            .insert(1, Arc::new(effect));
        let mut battle = Battle::new(prepared);
        let mut input = request(0);
        input.actions[0].target = ActorId(1);
        let frame = battle.step(input).unwrap();
        assert_eq!(frame.actors[0].hp, 60);
        assert_eq!(frame.actors[1].hp, 50);
    }
}
fn started(frame: &BattleFrame) -> ActionId {
    frame
        .cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap()
}

#[test]
fn maintained_nurse_callback_matches_controlled_dolphin_recovery_and_retirement() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/nurse-recovery.json")).unwrap();
    let observations = fixture["actors"].as_array().expect("observed roster");
    let actors = observations
        .iter()
        .map(|row| {
            let mut actor = actor(if row["side"] == 0 {
                Side::Party
            } else {
                Side::Enemy
            });
            actor.hp = row["before_hp"].as_i64().unwrap() as i32;
            actor.max_hp = row["max_hp"].as_i64().unwrap() as i32;
            actor
        })
        .collect();
    let source = include_str!("../../../scripts/battle/nurse.sym")
        .replace("script battle;", "")
        .replace("use battle;", "");
    let mut battle = Battle::new(prepared_entry(&source, actors, 250, "recover"));
    let owner = fixture["owner"].as_u64().unwrap() as u8;
    let first = fixture["initial_tick"].as_u64().unwrap();
    let heal = fixture["heal_tick"].as_u64().unwrap();
    assert_eq!(heal - first, fixture["heal_age"].as_u64().unwrap() + 1);
    let frame = battle.step(request(owner)).unwrap();
    let id = started(&frame);
    assert!(recovery(&frame).is_empty());
    for tick in first + 1..heal {
        assert!(
            recovery(&battle.step(BattleInput::default()).unwrap()).is_empty(),
            "tick {tick}"
        );
    }
    assert_eq!(battle.action_age(id), Some(120));
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        recovery(&frame),
        [
            (ActorId(0), 131, 131),
            (ActorId(1), 68, 68),
            (ActorId(2), 165, 165),
        ]
    );
    for (actual, row) in battle.actors().iter().zip(observations) {
        assert_eq!(i64::from(actual.hp), row["after_hp"].as_i64().unwrap());
    }
    assert_eq!(battle.action_age(id), Some(121));
    assert!(recovery(&battle.step(BattleInput::default()).unwrap()).is_empty());
    let boundaries = fixture["resident_boundaries"].as_array().unwrap();
    let cleanup = boundaries.last().unwrap()["combat_tick"].as_u64().unwrap();
    for tick in heal + 2..=cleanup {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert!(recovery(&frame).is_empty(), "tick {tick}");
        if let Some(row) = boundaries.iter().find(|row| row["combat_tick"] == tick) {
            let after = &row["after"];
            let expected_age = (after["mode"] != 0).then(|| after["age"].as_u64().unwrap() as u32);
            assert_eq!(battle.action_age(id), expected_age, "tick {tick}");
            if let Some(sequence) = battle.sequences.get(&id) {
                let resident = sequence.resident.as_ref().unwrap();
                let phase = match resident.phase {
                    crate::script::ResidentPhase::Initializing => 0,
                    crate::script::ResidentPhase::Active => 1,
                    crate::script::ResidentPhase::Retiring => 2,
                };
                assert_eq!(after["phase"], phase, "tick {tick}");
            }
        }
        assert_eq!(
            frame.cues,
            if tick == cleanup {
                vec![Cue::Completed { action: id }]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn nurse_callback_uses_live_eligible_roster_at_120_and_preserves_resident_action() {
    let source = include_str!("../../../scripts/battle/nurse.sym");
    let source = source
        .replace("script battle;", "")
        .replace("use battle;", "");
    let mut actors = vec![actor(Side::Party); 4];
    actors[0].hp = 80;
    actors[1].hp = 75;
    actors[1].recovery.weak = true;
    actors[2].hp = 0;
    actors[3].petrified = true;
    actors.push(actor(Side::Enemy));
    let mut battle = Battle::new(prepared_entry(&source, actors, 250, "recover"));
    let first = battle.step(request(0)).unwrap();
    let id = started(&first);
    assert_eq!(battle.actors()[0].tp, 40); // Released callbacks do not pay the caster's TP.
    assert!(recovery(&battle.step(BattleInput::default()).unwrap()).is_empty()); // Active age zero.
    for _ in 1..120 {
        assert!(recovery(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
    for _ in 0..4 {
        let paused = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(paused.update, 121);
        assert_eq!(battle.action_age(id), Some(120));
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        recovery(&frame),
        [(ActorId(0), 40, 20), (ActorId(1), 40, 0)]
    );
    assert_eq!(battle.random_state(), 1); // Neither recipient has Lucky Healing.
    assert_eq!(
        battle.actors().iter().map(|a| a.hp).collect::<Vec<_>>(),
        [100, 75, 0, 50, 50]
    );
    assert!(matches!(
        battle.step(BattleInput::default()).unwrap().cues.as_slice(),
        [Cue::Voice {
            actor: ActorId(1),
            sound: SoundBinding {
                resource: 1,
                index: 44
            },
            ..
        }]
    ));
    for _ in 122..250 {
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    }
    assert_eq!(battle.action_age(id), Some(250));
    assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    assert_eq!(battle.action_age(id), Some(251));
    assert_eq!(
        battle.step(BattleInput::default()).unwrap().cues,
        [Cue::Completed { action: id }]
    );
    assert_eq!(battle.action_age(id), None);
}

#[test]
fn retained_resident_stops_callbacks_at_retirement_and_can_be_interrupted_before_cleanup() {
    for interrupt in [false, true] {
        let mut battle = Battle::new(prepared(
            "pub task run() {
                battle::retain_resident();
                await battle::at_age(ticks(2));
                battle::heal_percent(battle::owner(), 5);
                await battle::next_update();
                battle::heal_percent(battle::owner(), 20);
            }",
            vec![actor(Side::Party)],
            2,
        ));
        let id = started(&battle.step(request(0)).unwrap());
        for _ in 0..2 {
            battle.step(BattleInput::default()).unwrap();
        }
        let last = battle.step(BattleInput::default()).unwrap();
        assert_eq!(recovery(&last), [(ActorId(0), 5, 5)]);
        assert_eq!(battle.action_age(id), Some(3));
        assert!(battle.spell_active(ActorId(0), SpellSlot::Primary));
        let input = BattleInput {
            interrupt: if interrupt { vec![id] } else { vec![] },
            ..Default::default()
        };
        let expected = if interrupt {
            Cue::Interrupted { action: id }
        } else {
            Cue::Completed { action: id }
        };
        assert_eq!(battle.step(input).unwrap().cues, [expected]);
        assert!(!battle.spell_active(ActorId(0), SpellSlot::Primary));
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
        assert_eq!(battle.actors()[0].hp, 55);
    }
}

#[test]
fn retaining_a_resident_requires_its_initialization_dispatch() {
    for (source, phase) in [
        ("battle::retain_resident();", ActionPhase::Actor),
        (
            "await battle::next_update(); battle::retain_resident();",
            ActionPhase::Resident,
        ),
    ] {
        let mut definition = prepared(
            &format!("pub task run() {{ {source} }}"),
            vec![actor(Side::Party)],
            2,
        );
        Arc::get_mut(&mut definition).unwrap().actions[0].phase = phase;
        let mut battle = Battle::new(definition);
        let result = battle.step(request(0));
        let result = if phase == ActionPhase::Resident {
            result.unwrap();
            battle.step(BattleInput::default())
        } else {
            result
        };
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires resident initialization")
        );
    }
}

#[test]
fn immediate_waits_and_next_update_have_distinct_resume_boundaries() {
    let mut battle = Battle::new(prepared(
        r#"
        pub task run() {
            await battle::at_age(ticks(0));
            await battle::wait_ticks(ticks(0));
            battle::heal_percent(battle::owner(), 1);
            await battle::next_update();
            await battle::at_age(ticks(0));
            battle::heal_percent(battle::owner(), 2);
            await battle::wait_ticks(ticks(2));
            battle::heal_percent(battle::owner(), 3);
            battle::finish();
        }
    "#,
        vec![actor(Side::Party)],
        10,
    ));
    let first = battle.step(request(0)).unwrap();
    let id = started(&first);
    assert_eq!(recovery(&first), [(ActorId(0), 1, 1)]);
    assert_eq!(
        recovery(&battle.step(BattleInput::default()).unwrap()),
        [(ActorId(0), 2, 2)]
    );
    assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    let last = battle.step(BattleInput::default()).unwrap();
    assert_eq!(recovery(&last), [(ActorId(0), 3, 3)]);
    assert_eq!(last.cues.last(), Some(&Cue::Completed { action: id }));
}

#[test]
fn child_results_resume_once_in_stable_order_and_unjoined_children_are_cancelled() {
    let mut battle = Battle::new(prepared(
        r#"
        pub task run() {
            let child = spawn work();
            let amount = await child;
            battle::heal_percent(battle::owner(), amount);
            spawn abandoned();
        }
        task work() -> i32 {
            await battle::next_update();
            return 7;
        }
        task abandoned() { battle::heal_percent(battle::owner(), 40); }
    "#,
        vec![actor(Side::Party)],
        5,
    ));
    let first = battle.step(request(0)).unwrap();
    let id = started(&first);
    assert!(recovery(&first).is_empty());
    assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    assert_eq!(
        recovery(&battle.step(BattleInput::default()).unwrap()),
        [(ActorId(0), 7, 7)]
    );
    assert_eq!(battle.action_age(id), Some(2));
    for _ in 3..=5 {
        assert!(recovery(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
}

#[test]
fn interruption_cancels_pending_descendants_and_rejects_late_handles_before_mutation() {
    let mut battle = Battle::new(prepared(
        r#"
        pub task run() { let child = spawn work(); await child; }
        task work() { await battle::at_age(ticks(3)); battle::heal_percent(battle::owner(), 40); }
    "#,
        vec![actor(Side::Party)],
        10,
    ));
    let id = started(&battle.step(request(0)).unwrap());
    let frame = battle
        .step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.cues, [Cue::Interrupted { action: id }]);
    assert!(
        battle
            .step(BattleInput {
                interrupt: vec![id],
                ..Default::default()
            })
            .is_err()
    );
    for _ in 0..5 {
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    }
    assert_eq!(battle.actors()[0].hp, 50);
}

#[test]
fn insufficient_tp_and_busy_admission_do_not_repeat_costs() {
    let mut actors = vec![actor(Side::Party); 2];
    actors[1].tp = 27;
    let mut p = prepared(
        "pub task run() { battle::pay_tp(battle::tp_cost()); await battle::at_age(ticks(5)); }",
        actors,
        5,
    );
    Arc::get_mut(&mut p).unwrap().actions[0].phase = ActionPhase::Actor;
    let mut battle = Battle::new(p);
    let mut input = request(0);
    input.actions.extend(request(0).actions);
    input.actions.extend(request(1).actions);
    let frame = battle.step(input).unwrap();
    assert_eq!(
        &frame.cues[1..],
        &[
            Cue::Rejected {
                actor: ActorId(0),
                reason: Rejection::Busy
            },
            Cue::Rejected {
                actor: ActorId(1),
                reason: Rejection::InsufficientTp
            },
        ]
    );
    assert_eq!((battle.actors()[0].tp, battle.actors()[1].tp), (12, 27));
}

#[test]
fn scripted_sounds_follow_action_pauses_and_interruption_without_consuming_rng() {
    let mut prepared = prepared(
        r#"asset swing: battle::Sound = "test/swing";
        pub task run() {
            battle::sound(swing, 1);
            await battle::at_age(ticks(2));
            battle::sound(swing, 2);
            await battle::at_age(ticks(4));
            battle::sound(swing, 3);
        }"#,
        vec![actor(Side::Party)],
        10,
    );
    let sound = SoundBinding {
        resource: 7,
        index: 60,
    };
    let action = &mut Arc::get_mut(&mut prepared).unwrap().actions[0];
    action.phase = ActionPhase::Actor;
    action.resources = vec![ResourceBinding::Sound(sound)];
    let mut battle = Battle::new(prepared);
    let random = battle.random.0;
    let first = battle.step(request(0)).unwrap();
    let id = started(&first);
    assert!(first.cues.contains(&Cue::Sound {
        actor: ActorId(0),
        sound,
        position: [0.; 3],
        priority: 1,
    }));
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert!(paused.cues.is_empty());
    battle.actors[0].hit_stop = 2;
    for _ in 0..2 {
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    }
    assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    battle.actors[0].position = [9., 0., 4.];
    let second = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        second.cues,
        [Cue::Sound {
            actor: ActorId(0),
            sound,
            position: [9., 0., 4.],
            priority: 2,
        }]
    );
    assert_eq!(battle.random.0, random);
    assert_eq!(
        battle
            .step(BattleInput {
                interrupt: vec![id],
                ..Default::default()
            })
            .unwrap()
            .cues,
        [Cue::Interrupted { action: id }]
    );
    for _ in 0..5 {
        assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
    }
}

#[test]
fn invalid_calls_runaway_code_and_child_faults_end_the_battle_with_source_diagnostics() {
    for (source, expected) in [
        (
            "pub task run() { battle::ally(99); }",
            "invalid battle roster index",
        ),
        ("pub task run() { while true {} }", "budget"),
        (
            "asset sound: battle::Sound = \"test/missing\"; pub task run() { battle::sound(sound, 1); }",
            "unbound battle sound",
        ),
        (
            "pub task run() { let c = spawn work(); await c; } task work() { battle::heal_percent(battle::owner(), 101); }",
            "recovery percent",
        ),
        (
            "pub task run() { let c = spawn work(); await c; await c; } task work() {}",
            "stale or already joined",
        ),
    ] {
        let mut battle = Battle::new(prepared(source, vec![actor(Side::Party)], 10));
        let error = match battle.step(request(0)) {
            Err(error) => error,
            Ok(_) => battle.step(BattleInput::default()).unwrap_err(),
        }
        .to_string();
        assert!(
            error.contains(expected) && error.contains("test::"),
            "{error}"
        );
        assert!(
            battle
                .step(BattleInput::default())
                .unwrap_err()
                .to_string()
                .contains("ended or faulted")
        );
    }
}

#[test]
fn actor_tint_selection_validates_table_and_index_without_consuming_randomness() {
    for selector in [-1, 0, 11, 12] {
        let source = format!(
            r#"asset tints: battle::ActorTints = "test/tints";
            pub task run() {{ battle::tint_actor(battle::owner(), tints, {selector}); }}"#
        );
        let mut battle = Battle::new(prepared(&source, vec![actor(Side::Party)], 5));
        let result = battle.step(request(0));
        if selector == 0 || selector == 11 {
            let frame = result.unwrap();
            assert_eq!(
                frame.actors[0].body.tint,
                if selector == 0 {
                    [40, 112, 40, 255]
                } else {
                    [128, 128, 128, 255]
                }
            );
        } else {
            assert!(result.unwrap_err().to_string().contains("actor tint index"));
        }
        assert_eq!(battle.random.0, 1);
    }
    let mut prepared = prepared(
        r#"asset tints: battle::ActorTints = "test/tints";
        pub task run() { battle::tint_actor(battle::owner(), tints, 0); }"#,
        vec![actor(Side::Party)],
        5,
    );
    Arc::get_mut(&mut prepared).unwrap().actions[0]
        .resources
        .clear();
    assert!(
        Battle::new(prepared)
            .step(request(0))
            .unwrap_err()
            .to_string()
            .contains("unbound actor tint table")
    );
}

#[test]
fn invalid_sound_priorities_fault_instead_of_wrapping() {
    for priority in [-1, 256] {
        let source = format!(
            r#"asset sound: battle::Sound = "test/sound";
            pub task run() {{ battle::sound(sound, {priority}); }}"#
        );
        let mut p = prepared(&source, vec![actor(Side::Party)], 5);
        Arc::get_mut(&mut p).unwrap().actions[0].resources =
            vec![ResourceBinding::Sound(SoundBinding {
                resource: 1,
                index: 60,
            })];
        let mut battle = Battle::new(p);
        let error = battle.step(request(0)).unwrap_err().to_string();
        assert!(error.contains("invalid battle sound priority") && error.contains("test::"));
    }
}

#[test]
fn concurrent_recovery_consumes_random_in_actor_order_and_replays_identically() {
    let mut actors = vec![actor(Side::Party); 2];
    actors[0].recovery.lucky = true;
    actors[1].recovery.lucky = true;
    let prepared = prepared(
        "pub task run() { battle::heal_percent(battle::owner(), 40); }",
        actors,
        1,
    );
    let mut a = Battle::new(prepared.clone());
    let mut b = Battle::new(prepared);
    for battle in [&mut a, &mut b] {
        let mut input = request(1);
        input.actions.extend(request(0).actions);
        let frame = battle.step(input).unwrap();
        assert_eq!(
            recovery(&frame).iter().map(|r| r.0).collect::<Vec<_>>(),
            [ActorId(0), ActorId(1)]
        );
    }
    assert_eq!(
        a.step(BattleInput::default()).unwrap(),
        b.step(BattleInput::default()).unwrap()
    );
    assert_eq!(a.random_state(), 0xbb81_ea6b);
}

#[test]
fn recovery_preserves_source_narrowing_boost_order_and_nominal_feedback() {
    let mut recipient = actor(Side::Party);
    recipient.max_hp = 100_000;
    recipient.hp = 50_000;
    recipient.recovery.boost = true;
    let mut battle = Battle::new(prepared(
        "pub task run() { battle::heal_percent(battle::owner(), 40); }",
        vec![recipient],
        1,
    ));
    let frame = battle.step(request(0)).unwrap();
    // 40 becomes 48 before max-HP multiplication, then narrows to a signed halfword.
    assert_eq!(recovery(&frame), [(ActorId(0), -17_536, -17_536)]);
    assert_eq!(battle.actors()[0].hp, 32_464);
}

#[test]
fn prepared_effects_emit_in_order_and_unknown_members_fault() {
    for (member, succeeds) in [(6, true), (9, false)] {
        let source = format!(
            r#"asset bank: battle::Effect = "test/effect";
            pub task run() {{ battle::show(bank, {member}, battle::owner()); }}"#
        );
        let mut battle = Battle::new(prepared(&source, vec![actor(Side::Party)], 1));
        let frame = battle.step(request(0));
        assert_eq!(frame.is_ok(), succeeds);
        if let Ok(frame) = frame {
            assert!(matches!(
                frame.cues[1],
                Cue::Effect {
                    resource: 37,
                    member: 6,
                    ..
                }
            ));
        }
    }
}

#[test]
fn recognition_keeps_stepping_until_the_owner_finishes_once() {
    let mut enemy = actor(Side::Enemy);
    enemy.hp = 0;
    let mut battle = Battle::new(prepared(
        "pub task run() { await battle::at_age(ticks(4)); }",
        vec![actor(Side::Party), enemy],
        8,
    ));
    let frame = battle.step(request(0)).unwrap();
    assert_eq!(frame.recognized_result, Some(BattleResult::Victory));
    assert!(frame.outcome.is_none());
    assert!(battle.step(BattleInput::default()).is_ok());
    let frame = battle.finish_result().unwrap();
    assert_eq!(frame.outcome.unwrap().result, BattleResult::Victory);
    assert!(frame.actions.is_empty());
    assert!(battle.finish_result().is_err());
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn invalid_external_input_is_atomic_and_does_not_fault_the_running_battle() {
    let mut battle = Battle::new(prepared("pub task run() {}", vec![actor(Side::Party)], 3));
    let mut input = request(0);
    input.actions.push(ActionRequest {
        actor: ActorId(0),
        target: ActorId(0),
        action: 1234,
    });
    assert!(battle.step(input).is_err());
    assert_eq!(battle.actors()[0].tp, 40);
    assert_eq!(battle.step(BattleInput::default()).unwrap().update, 1);
}

#[test]
fn task_pool_exhaustion_is_a_fault_instead_of_a_timing_change() {
    let mut battle = Battle::new(prepared(
        r#"
        pub task run() {
            for index in 0 .. 64 { spawn work(); }
            await battle::next_update();
        }
        task work() { await battle::at_age(ticks(5)); }
    "#,
        vec![actor(Side::Party)],
        10,
    ));
    assert!(
        battle
            .step(request(0))
            .unwrap_err()
            .to_string()
            .contains("battle task limit exceeded")
    );
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn random_stream_matches_pinned_dolphin_observations() {
    let observations: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/opening-random.json")).unwrap();
    let mut random = crate::state::Random(
        observations["origin"]["battle_random_state_word"]
            .as_u64()
            .unwrap() as u32,
    );
    for transition in observations["transitions"].as_array().unwrap() {
        assert_eq!(random.0, transition["before"].as_u64().unwrap() as u32);
        let mut output = 0;
        for _ in 0..transition["draws"].as_u64().unwrap() {
            output = random.next();
        }
        let expected = transition["after"].as_u64().unwrap() as u32;
        assert_eq!(random.0, expected, "VI {}", transition["vi"]);
        assert_eq!(output, (expected >> 16) as u16);
    }
}

#[test]
fn petrified_actors_cannot_begin_an_action_or_pay_its_cost() {
    let mut caster = actor(Side::Party);
    caster.petrified = true;
    let mut battle = Battle::new(prepared(
        "pub task run() { battle::heal_percent(battle::owner(), 40); }",
        // A living ally keeps this an admission test rather than all-party defeat.
        vec![caster, actor(Side::Party), actor(Side::Enemy)],
        1,
    ));
    assert_eq!(
        battle.step(request(0)).unwrap().cues,
        [Cue::Rejected {
            actor: ActorId(0),
            reason: Rejection::Petrified
        }]
    );
    assert_eq!((battle.actors()[0].hp, battle.actors()[0].tp), (50, 40));
}

fn projectile_definition() -> ProjectileDefinition {
    let row: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/lightning-projectile.json")).unwrap();
    let vector = |name: &str| std::array::from_fn(|i| row[name][i].as_f64().unwrap() as f32);
    ProjectileDefinition {
        motion: Default::default(),
        effects: Default::default(),
        lifetime: row["lifetime"].as_u64().unwrap() as u16,
        velocity: vector("velocity"),
        acceleration: vector("acceleration"),
        offset: vector("offset"),
        clamp_ground: row["clamp_ground"].as_bool().unwrap(),
        active: (row["active_duration"].as_u64().unwrap() != 0).then(|| {
            [
                row["active_start"].as_u64().unwrap() as u16,
                (row["active_start"].as_u64().unwrap() + row["active_duration"].as_u64().unwrap())
                    as u16,
            ]
        }),
        birth: Some(EffectAppearance {
            resource: 37,
            member: row["birth_effect"].as_u64().unwrap() as u16,
        }),
        contact: None,
    }
}

pub(super) fn projectile_battle(
    source: &str,
    actors: Vec<Actor>,
    duration: u16,
    projectile: ProjectileDefinition,
) -> Battle {
    let source = source
        .replace("script battle;", "")
        .replace("use battle;", "");
    let mut prepared = Arc::try_unwrap(prepared(&source, actors, duration)).unwrap();
    let effects = projectile_effects([&projectile]);
    prepared.actions[0].resources = vec![ResourceBinding::Projectile(Arc::new(projectile))];
    Battle::new(Arc::new(
        PreparedBattle::new(
            prepared.actors,
            prepared.actions,
            prepared.random_seed,
            vec![],
            effects,
        )
        .unwrap(),
    ))
}

#[test]
fn lightning_retains_origin_uses_live_heading_and_emits_after_the_projectile_group() {
    let source =
        include_str!("../../../scripts/battle/lightning.sym").replace("task release", "task run");
    let mut actors = vec![actor(Side::Party), actor(Side::Enemy)];
    actors[1].position = [30., 8., 40.];
    let mut battle = projectile_battle(&source, actors, 90, projectile_definition());
    let mut input = request(0);
    input.actions[0].target = ActorId(1);
    let id = started(&battle.step(input).unwrap());
    battle.actors[1].position = [300., 0., 400.];
    battle.actors[0].heading = 180.;
    for _ in 1..=21 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert!(frame.projectiles.is_empty());
        assert!(frame.cues.is_empty());
    }
    // Once emitted, the projectile owns its lifetime even if the caster is interrupted.
    let frame = battle
        .step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.projectiles.len(), 1);
    let projectile = &frame.projectiles[0];
    assert_eq!(projectile.position, [29.4, 0., 39.2]);
    assert_eq!(projectile.heading, 180.);
    assert_eq!(projectile.age, 0);
    assert!(!projectile.contact_active);
    assert!(matches!(
        frame.cues.as_slice(),
        [
            Cue::Interrupted { .. },
            Cue::ProjectileStarted { .. },
            Cue::Effect { member: 28, .. }
        ]
    ));
    for age in 1..=20 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.projectiles[0].age, age);
        assert!(frame.projectiles[0].contact_active);
        assert!(frame.cues.is_empty());
    }
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.projectiles[0].age, 20);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(frame.projectiles.is_empty());
    assert_eq!(
        frame.cues,
        [Cue::ProjectileExpired {
            projectile: projectile.id
        }]
    );
    assert_eq!(battle.random_state(), 1);
}

#[test]
fn lightning_replaces_unavailable_targets_once_in_same_side_roster_order() {
    let source =
        include_str!("../../../scripts/battle/lightning.sym").replace("task release", "task run");
    for owner_side in [Side::Party, Side::Enemy] {
        let target_side = if owner_side == Side::Party {
            Side::Enemy
        } else {
            Side::Party
        };
        for (dead, stone, no_candidate, expected) in [
            (false, false, false, 3),
            (true, false, false, 2),
            (false, true, false, 2),
            (true, false, true, 3),
        ] {
            let mut actors = vec![actor(owner_side)];
            for x in [100., 200., 300.] {
                let mut target = actor(target_side);
                target.position = [x, 0., 0.];
                actors.push(target);
            }
            actors[1].petrified = true;
            actors[2].petrified = no_candidate;
            actors[3].hp = if dead { 0 } else { actors[3].hp };
            actors[3].petrified = stone;
            let origin = [actors[expected].position[0] - 1., 0., 0.];
            let mut battle = projectile_battle(&source, actors, 90, projectile_definition());
            let mut input = request(0);
            input.actions[0].target = ActorId(3);
            let first = battle.step(input).unwrap();
            if no_candidate {
                // 2108 recognizes the unavailable side before new actions are
                // admitted. This fixture cannot launch a fresh release after
                // recognition merely to exercise the no-target fallback.
                assert_eq!(
                    first.recognized_result,
                    Some(if owner_side == Side::Party {
                        BattleResult::Victory
                    } else {
                        BattleResult::Defeat
                    })
                );
                assert_eq!(
                    first.cues,
                    [Cue::Rejected {
                        actor: ActorId(0),
                        reason: Rejection::BattleEnding
                    }]
                );
                assert!(first.projectiles.is_empty());
                assert_eq!(battle.random_state(), 1);
                continue;
            }
            let id = started(&first);
            assert_eq!(battle.sequences[&id].target, ActorId(expected as u8));
            // Selection belongs to resident initialization. Later movement or
            // defeat does not recapture the origin or select another target.
            battle.actors[expected].position[0] += 500.;
            battle.actors[expected].hp = 0;
            for _ in 1..=21 {
                assert!(
                    battle
                        .step(BattleInput::default())
                        .unwrap()
                        .projectiles
                        .is_empty()
                );
            }
            let frame = battle.step(BattleInput::default()).unwrap();
            assert_eq!(frame.projectiles[0].target, ActorId(expected as u8));
            assert_eq!(frame.projectiles[0].position, origin);
            assert_eq!(battle.random_state(), 1);
        }
    }
}

#[test]
fn queued_resident_keeps_unavailable_target_after_terminal_recognition() {
    let source =
        include_str!("../../../scripts/battle/lightning.sym").replace("task release", "task run");
    let mut target = actor(Side::Enemy);
    target.hp = 0;
    target.position = [300., 0., 0.];
    let mut battle = projectile_battle(
        &source,
        vec![actor(Side::Party), target],
        90,
        projectile_definition(),
    );
    // A previously released spell already occupies the resident list before
    // this outer recognition visit; this is not a new action admission.
    let (id, sequence) = battle.allocate_sequence(0, ActorId(0), ActorId(1)).unwrap();
    battle.sequences.insert(id, sequence);
    let first = battle.step(BattleInput::default()).unwrap();
    assert_eq!(first.recognized_result, Some(BattleResult::Victory));
    assert_eq!(battle.sequences[&id].target, ActorId(1));
    for _ in 1..=21 {
        assert!(
            battle
                .step(BattleInput::default())
                .unwrap()
                .projectiles
                .is_empty()
        );
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.projectiles[0].target, ActorId(1));
    assert_eq!(frame.projectiles[0].position, [299., 0., 0.]);
    assert_eq!(battle.random_state(), 1);
}

#[test]
fn pending_projectiles_survive_task_and_action_completion_in_newest_first_order() {
    let source = r#"asset bolt: battle::Projectile = "test/projectile";
        pub task run() {
            battle::emit(bolt, battle::ground_point(battle::owner(), 10.0, 0.0));
            battle::emit(bolt, battle::ground_point(battle::owner(), 20.0, 0.0));
            battle::finish();
        }"#;
    let mut definition = projectile_definition();
    definition.velocity = [0., -2., 0.];
    definition.acceleration = [0., -1., 0.];
    definition.active = Some([2, 3]);
    let mut battle = projectile_battle(source, vec![actor(Side::Party)], 10, definition);
    let first = battle.step(request(0)).unwrap();
    assert!(first.projectiles.is_empty());
    assert!(first.actions.is_empty());
    let birth = battle.step(BattleInput::default()).unwrap();
    let heights: Vec<_> = birth
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Effect { position, .. } => Some(position[1]),
            _ => None,
        })
        .collect();
    assert_eq!(heights, [20., 10.]); // Birth captures precede age-zero movement.
    assert_eq!(
        birth
            .projectiles
            .iter()
            .map(|p| p.position[1])
            .collect::<Vec<_>>(),
        [8., 18.]
    );
    let one = battle.step(BattleInput::default()).unwrap();
    assert_eq!(one.projectiles[0].position[1], 5.);
    assert!(!one.projectiles[0].contact_active);
    for _ in 2..=3 {
        assert!(battle.step(BattleInput::default()).unwrap().projectiles[0].contact_active);
    }
    let four = battle.step(BattleInput::default()).unwrap();
    assert!(!four.projectiles[0].contact_active);
    assert_eq!(four.projectiles[0].position[1], 0.);
}

#[test]
fn interrupted_emission_never_runs_and_invalid_projectile_data_fails_preparation() {
    let source =
        include_str!("../../../scripts/battle/lightning.sym").replace("task release", "task run");
    let mut battle = projectile_battle(
        &source,
        vec![actor(Side::Party)],
        90,
        projectile_definition(),
    );
    let id = started(&battle.step(request(0)).unwrap());
    battle
        .step(BattleInput {
            interrupt: vec![id],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..24 {
        assert!(
            battle
                .step(BattleInput::default())
                .unwrap()
                .projectiles
                .is_empty()
        );
    }
    let mut definition = projectile_definition();
    definition.velocity[0] = f32::NAN;
    assert!(definition.validate().is_err());
    definition.velocity[0] = 0.;
    definition.active = Some([5, 2]);
    assert!(definition.validate().is_err());
}

fn clash_battle(
    party: ProjectileDefinition,
    enemy: ProjectileDefinition,
    enemy_position: [f32; 3],
) -> Battle {
    let source = r#"
        asset projectile: battle::Projectile = "test/projectile";
        pub task run() {
            battle::emit(projectile, battle::ground_point(battle::owner(), 0.0, 0.0));
            battle::finish();
        }
    "#;
    let mut actors = vec![actor(Side::Enemy), actor(Side::Party)];
    actors[0].position = enemy_position;
    let mut prepared = Arc::try_unwrap(prepared(source, actors, 0)).unwrap();
    let effects = projectile_effects([&party, &enemy]);
    prepared.actions[0].resources = vec![ResourceBinding::Projectile(Arc::new(party))];
    let mut second = prepared.actions[0].clone();
    second.id = 100;
    second.resources = vec![ResourceBinding::Projectile(Arc::new(enemy))];
    prepared.actions.push(second);
    let mut battle = Battle::new(Arc::new(
        PreparedBattle::new(prepared.actors, prepared.actions, 1, vec![], effects).unwrap(),
    ));
    // Enemy-first input and actor ordering must not change party-first collision.
    let first = battle
        .step(BattleInput {
            actions: vec![
                ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: 100,
                },
                ActionRequest {
                    actor: ActorId(1),
                    target: ActorId(0),
                    action: 99,
                },
            ],
            ..Default::default()
        })
        .unwrap();
    assert!(first.actions.is_empty());
    let birth = battle.step(BattleInput::default()).unwrap();
    assert!(
        birth
            .projectiles
            .iter()
            .all(|p| p.age == 0 && !p.contact_active && !p.disarmed)
    );
    battle
}

fn contact_projectile(clashes: bool, survives_contact: bool) -> ProjectileDefinition {
    let mut definition = projectile_definition();
    definition.birth = None;
    definition.contact = Some(ProjectileContact {
        hit: crate::HitRule {
            impact: None,
            arte: false,
            reaction: Default::default(),
            kind: crate::DamageKind::Slash,
            power: crate::Power::Normal,
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            guard: crate::GuardRule::default(),
        },
        cooldown: 120,
        repeat_limit: 0,
        radius: 5.,
        height: 5.,
        shape: HitShape::Sphere,
        offset: [0.; 3],
        radius_growth: 0.,
        height_growth: 0.,
        survives_contact,
        clash_effect: clashes.then_some(EffectAppearance {
            resource: 19,
            member: 11,
        }),
    });
    definition
}

fn clashes(frame: &BattleFrame) -> Vec<(ProjectileId, ProjectileId, [f32; 3])> {
    frame
        .cues
        .iter()
        .filter_map(|c| match c {
            Cue::ProjectileClashed {
                projectile,
                other: ContactSource::Projectile(other),
                position,
            } => Some((*projectile, *other, *position)),
            _ => None,
        })
        .collect()
}

#[test]
fn simultaneous_clash_disarms_only_the_party_projectile_then_retires_on_later_updates() {
    let mut battle = clash_battle(
        contact_projectile(true, false),
        contact_projectile(true, true),
        [6., 0., 0.],
    );
    let frame = battle.step(BattleInput::default()).unwrap();
    // Resident dispatch emits the party projectile first even with enemy-first actor storage.
    assert_eq!(
        clashes(&frame),
        [(ProjectileId(1), ProjectileId(2), [3., 0., 0.])]
    );
    assert!(matches!(
        frame.cues.as_slice(),
        [
            Cue::ProjectileClashed { .. },
            Cue::Effect {
                resource: 19,
                member: 11,
                heading: 0.,
                ..
            }
        ]
    ));
    assert!(frame.projectiles[0].disarmed);
    assert!(!frame.projectiles[1].disarmed);
    assert_eq!(battle.random_state(), 1);
    assert_eq!(
        battle.actors().iter().map(|a| a.hp).collect::<Vec<_>>(),
        [50, 50]
    );
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.projectiles, frame.projectiles);
    assert_eq!(paused.update, frame.update);
    assert!(paused.cues.is_empty());
    let last = battle.step(BattleInput::default()).unwrap();
    assert_eq!(last.projectiles.len(), 2);
    assert!(!last.projectiles[0].contact_active);
    assert!(last.cues.is_empty());
    let expired = battle.step(BattleInput::default()).unwrap();
    assert_eq!(expired.projectiles.len(), 1);
    assert_eq!(
        expired.cues,
        [Cue::ProjectileExpired {
            projectile: ProjectileId(1)
        }]
    );
}

#[test]
fn clash_survivor_keeps_moving_without_repeated_contacts() {
    let mut party = contact_projectile(true, true);
    party.lifetime = 4;
    party.velocity = [1., 0., 0.];
    let mut battle = clash_battle(party, contact_projectile(false, false), [5., 0., 0.]);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        clashes(&frame),
        [(ProjectileId(1), ProjectileId(2), [3.5, 0., 0.])]
    );
    for age in 2..=4 {
        let frame = battle.step(BattleInput::default()).unwrap();
        let party = &frame.projectiles[0];
        assert_eq!(party.age, age);
        assert_eq!(party.position, [f32::from(age + 1), 0., 0.]);
        assert!(party.disarmed && !party.contact_active);
        assert!(frame.cues.is_empty());
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        frame.cues,
        [Cue::ProjectileExpired {
            projectile: ProjectileId(1)
        }]
    );
    assert_eq!(frame.projectiles[0].id, ProjectileId(2));
}

#[test]
fn clash_uses_three_dimensions_world_offset_and_post_submission_radius_growth() {
    let mut party = contact_projectile(true, true);
    let mut enemy = contact_projectile(false, true);
    party.contact.as_mut().unwrap().offset = [0., 12., 0.];
    // At age 1 the contact's live radius has grown twice (including birth).
    enemy.contact.as_mut().unwrap().radius_growth = 1.1;
    let mut battle = clash_battle(party, enemy, [0.; 3]);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        clashes(&frame),
        [(ProjectileId(1), ProjectileId(2), [0., 6., 0.])]
    );
    let mut high = contact_projectile(true, true);
    high.contact.as_mut().unwrap().offset = [0., 11., 0.];
    let mut battle = clash_battle(high, contact_projectile(false, true), [0.; 3]);
    assert!(clashes(&battle.step(BattleInput::default()).unwrap()).is_empty());
}

#[test]
fn clash_checks_strict_overlap_and_requires_both_contact_windows() {
    let mut battle = clash_battle(
        contact_projectile(true, true),
        contact_projectile(false, true),
        [10., 0., 0.],
    );
    assert!(clashes(&battle.step(BattleInput::default()).unwrap()).is_empty());
    let mut enemy = contact_projectile(false, true);
    enemy.active = Some([2, 3]);
    let mut battle = clash_battle(contact_projectile(true, true), enemy, [0.; 3]);
    assert!(clashes(&battle.step(BattleInput::default()).unwrap()).is_empty());
    assert_eq!(
        clashes(&battle.step(BattleInput::default()).unwrap()).len(),
        1
    );
}

#[test]
fn sdk_distance_rounding_is_preserved_at_the_clash_boundary() {
    let mut party = contact_projectile(true, true);
    let mut enemy = contact_projectile(false, true);
    party.contact.as_mut().unwrap().radius = 0.5;
    enemy.contact.as_mut().unwrap().radius = 0.5;
    let mut battle = clash_battle(party, enemy, [1., 0., 0.]);
    // fn_800FE6F8's estimate/refinement gives the float immediately below 1.
    // A host sqrt (or comparison of squared distances) would miss this clash.
    assert_eq!(crate::distance::length([1., 0., 0.]).to_bits(), 0x3f7fffff);
    assert_eq!(
        clashes(&battle.step(BattleInput::default()).unwrap()).len(),
        1
    );
}

#[test]
fn terminal_lifetime_update_can_still_clash() {
    let mut party = contact_projectile(true, false);
    party.lifetime = 1;
    let mut battle = clash_battle(party, contact_projectile(false, true), [0.; 3]);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.projectiles[0].age, 1);
    assert_eq!(clashes(&frame).len(), 1);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        frame.cues,
        [Cue::ProjectileExpired {
            projectile: ProjectileId(1)
        }]
    );
}

#[test]
fn contacts_keep_newest_first_order_and_drop_submissions_beyond_forty_per_side() {
    let mut battle = Battle::new(Arc::new(
        PreparedBattle::new(
            vec![actor(Side::Party), actor(Side::Enemy)],
            vec![],
            1,
            vec![],
            vec![effect_binding(19, [11])],
        )
        .unwrap(),
    ));
    let party = Arc::new(contact_projectile(false, true));
    let enemy = Arc::new(contact_projectile(true, true));
    // Oldest party projectile overlaps; forty newer ones are out of range.
    battle
        .emit(party.clone(), ActionId(1), ActorId(0), ActorId(1), [0.; 3])
        .unwrap();
    for _ in 0..40 {
        battle
            .emit(
                party.clone(),
                ActionId(1),
                ActorId(0),
                ActorId(1),
                [100., 0., 0.],
            )
            .unwrap();
    }
    battle
        .emit(enemy.clone(), ActionId(2), ActorId(1), ActorId(0), [0.; 3])
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    assert!(clashes(&battle.step(BattleInput::default()).unwrap()).is_empty());
    // A newer overlapping contact is admitted; the enemy picks it first.
    battle
        .emit(party, ActionId(1), ActorId(0), ActorId(1), [2., 0., 0.])
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        clashes(&frame),
        [(ProjectileId(42), ProjectileId(43), [1., 0., 0.])]
    );
}

#[test]
fn invalid_contact_parameters_fail_preparation_and_overflow_faults_the_battle() {
    let valid = contact_projectile(true, true);
    for value in [f32::NAN, f32::INFINITY, -1.] {
        let mut definition = valid.clone();
        definition.contact.as_mut().unwrap().radius = value;
        assert!(definition.validate().is_err());
    }
    let mut party = valid;
    party.contact.as_mut().unwrap().radius_growth = f32::MAX;
    let mut battle = clash_battle(party, contact_projectile(false, true), [0.; 3]);
    assert_eq!(
        battle.step(BattleInput::default()).unwrap_err().to_string(),
        "projectile contact growth overflow"
    );
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn projectile_hits_first_eligible_pose_and_can_hit_another_target_before_retiring() {
    let source = r#"
        asset projectile: battle::Projectile = "test/projectile";
        pub task run() { battle::emit(projectile, battle::ground_point(battle::owner(), 0.0, 0.0)); }
    "#;
    let mut actors = vec![
        actor(Side::Party),
        actor(Side::Enemy),
        actor(Side::Party),
        actor(Side::Enemy),
    ];
    actors[0].body.scale = 2.;
    for actor in &mut actors[1..] {
        actor.body = Body {
            scale: 0.5,
            anchors: Vec::new(),
            points: vec![HurtPoint {
                center: [0., 1., 0.],
                radius: 2.,
            }],
            ..Default::default()
        };
    }
    actors[1].body.points.insert(
        0,
        HurtPoint {
            center: [100., 0., 0.],
            radius: 2.,
        },
    );
    actors[1].body.points.push(HurtPoint {
        center: [0., 0., 0.],
        radius: 2.,
    });
    let mut projectile = contact_projectile(false, false);
    projectile.contact.as_mut().unwrap().shape = HitShape::Box;
    projectile.contact.as_mut().unwrap().height = 1.;
    projectile.active = Some([1, 2]);
    let mut battle = projectile_battle(source, actors, 0, projectile);
    let first = battle.step(request(0)).unwrap();
    assert!(hits(&first).is_empty());
    let birth = battle.step(BattleInput::default()).unwrap();
    assert!(hits(&birth).is_empty());
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(1), 1)]);
    assert_eq!(frame.actors[1].hp, 49);
    // Contacts follow the shared HUD visit: the new number draws its birth
    // pulse without consuming a display update on the admission frame.
    let number = frame.actors[1].hud.floating[0];
    assert_eq!((number.value, number.alpha, number.pulse), (1, 255, 12));
    assert_eq!(frame.actors[3].hp, 50);
    let mut expected_random = state::Random(1);
    for _ in 0..4 {
        expected_random.next();
    }
    assert_eq!(battle.random_state(), expected_random.0);
    let paused = battle
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert!(hits(&paused).is_empty());
    assert_eq!(paused.update, frame.update);
    assert_eq!(paused.actors[1].hud.floating[0], number);
    battle.actors[1].body.points.clear();
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(3), 0)]);
    assert_eq!(frame.actors[3].hp, 49);
    assert_eq!(frame.actors[1].hud.floating[0].pulse, 10);
    assert_eq!(frame.actors[3].hud.floating[0].pulse, 12);
    assert!(hits(&battle.step(BattleInput::default()).unwrap()).is_empty());
}

#[test]
fn clash_consumption_precedes_actor_geometry_but_does_not_consume_the_opposite_record() {
    let mut battle = clash_battle(
        contact_projectile(true, true),
        contact_projectile(false, true),
        [0.; 3],
    );
    for actor in &mut battle.actors {
        actor.body.points = vec![HurtPoint {
            center: [0.; 3],
            radius: 1.,
        }];
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(clashes(&frame).len(), 1);
    assert_eq!(hits(&frame), [(ProjectileId(2), ActorId(1), 0)]);
}

#[test]
fn contact_height_grows_before_the_actor_geometry_phase() {
    let mut party = contact_projectile(false, true);
    let contact = party.contact.as_mut().unwrap();
    contact.shape = HitShape::Cylinder;
    contact.height = 0.;
    contact.height_growth = 1.;
    let mut battle = clash_battle(party, contact_projectile(false, true), [20., 0., 0.]);
    battle.actors[0].body.points = vec![HurtPoint {
        center: [0., 2., 0.],
        radius: 0.,
    }];
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(0), 0)]);
}

#[test]
fn invalid_hurt_poses_and_shapes_fail_before_activation() {
    for scale in [0., -1., f32::NAN, f32::INFINITY] {
        let mut actor = actor(Side::Party);
        actor.body.scale = scale;
        assert!(PreparedBattle::new(vec![actor], vec![], 1, vec![], vec![]).is_err());
    }
    for point in [
        HurtPoint {
            center: [f32::NAN, 0., 0.],
            radius: 1.,
        },
        HurtPoint {
            center: [0.; 3],
            radius: -1.,
        },
    ] {
        let mut actor = actor(Side::Party);
        actor.body.points.push(point);
        assert!(PreparedBattle::new(vec![actor], vec![], 1, vec![], vec![]).is_err());
    }
    let mut definition = contact_projectile(false, true);
    definition.contact.as_mut().unwrap().shape = HitShape::Ring { width: f32::NAN };
    assert!(definition.validate().is_err());
    definition.contact.as_mut().unwrap().shape = HitShape::Box;
    definition.contact.as_mut().unwrap().height = f32::INFINITY;
    assert!(definition.validate().is_err());
}

fn hits(frame: &BattleFrame) -> Vec<(ProjectileId, ActorId, u8)> {
    frame
        .cues
        .iter()
        .filter_map(|cue| match cue {
            Cue::Hit {
                source: ContactSource::Projectile(projectile),
                actor,
                hurt_point,
                ..
            } => Some((*projectile, *actor, *hurt_point)),
            _ => None,
        })
        .collect()
}

fn vulnerable(side: Side) -> Actor {
    let mut actor = actor(side);
    actor.body.points.push(HurtPoint {
        center: [0.; 3],
        radius: 1.,
    });
    actor
}

fn firing_battle(source: &str, actors: Vec<Actor>, projectile: ProjectileDefinition) -> Battle {
    let mut battle = projectile_battle(source, actors, 30, projectile);
    battle.step(request(0)).unwrap();
    battle.step(BattleInput::default()).unwrap();
    battle
}

const FIRE: &str = r#"
    asset projectile: battle::Projectile = "test/projectile";
    pub task run() { battle::emit(projectile, battle::ground_point(battle::owner(), 0.0, 0.0)); }
"#;

#[test]
fn newest_contact_hits_first_target_and_older_contact_uses_the_next_target() {
    let source = r#"
        asset projectile: battle::Projectile = "test/projectile";
        pub task run() {
            battle::emit(projectile, battle::ground_point(battle::owner(), 0.0, 0.0));
            battle::emit(projectile, battle::ground_point(battle::owner(), 0.0, 0.0));
        }
    "#;
    let mut battle = firing_battle(
        source,
        vec![
            actor(Side::Party),
            vulnerable(Side::Enemy),
            vulnerable(Side::Enemy),
        ],
        contact_projectile(false, true),
    );
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        hits(&frame),
        [
            (ProjectileId(2), ActorId(1), 0),
            (ProjectileId(1), ActorId(2), 0)
        ]
    );
    assert_eq!((frame.actors[1].hp, frame.actors[2].hp), (49, 49));
    // Each projectile has a separate target cache: the next update swaps recipients.
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(
        hits(&frame),
        [
            (ProjectileId(2), ActorId(2), 0),
            (ProjectileId(1), ActorId(1), 0)
        ]
    );
    assert!(hits(&battle.step(BattleInput::default()).unwrap()).is_empty());
}

#[test]
fn projectile_cooldowns_repeat_limits_and_pause_control_rng_and_hp() {
    let mut projectile = contact_projectile(false, true);
    let contact = projectile.contact.as_mut().unwrap();
    contact.cooldown = 3;
    contact.repeat_limit = 2;
    let mut battle = firing_battle(
        FIRE,
        vec![actor(Side::Party), vulnerable(Side::Enemy)],
        projectile,
    );
    assert_eq!(hits(&battle.step(BattleInput::default()).unwrap()).len(), 1);
    let random = battle.random_state();
    for _ in 0..4 {
        assert!(
            hits(
                &battle
                    .step(BattleInput {
                        menu_open: true,
                        ..Default::default()
                    })
                    .unwrap()
            )
            .is_empty()
        );
    }
    assert_eq!(battle.random_state(), random);
    for age in 2..=10 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(hits(&frame).len(), usize::from(age == 4), "age {age}");
    }
    assert_eq!(battle.actors[1].hp, 48);
    let mut expected = state::Random(1);
    // Each physical hit draws three times for damage and once for stun selection.
    // Each completed hurt recovery also resets auto-guard, even on an enemy.
    for _ in 0..10 {
        expected.next();
    }
    assert_eq!(battle.random_state(), expected.0);
}

#[test]
fn zero_cooldown_bypasses_repeat_count_through_inclusive_lifetime() {
    let mut projectile = contact_projectile(false, true);
    projectile.lifetime = 4;
    let contact = projectile.contact.as_mut().unwrap();
    contact.cooldown = 0;
    contact.repeat_limit = 1;
    let mut battle = firing_battle(
        FIRE,
        vec![actor(Side::Party), vulnerable(Side::Enemy)],
        projectile,
    );
    for _ in 1..=4 {
        assert_eq!(hits(&battle.step(BattleInput::default()).unwrap()).len(), 1);
    }
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(hits(&frame).is_empty());
    assert!(frame.projectiles.is_empty());
    assert_eq!(frame.actors[1].hp, 46);
}

#[test]
fn dead_and_petrified_targets_are_skipped_without_drawing_randomness() {
    let mut dead = vulnerable(Side::Enemy);
    dead.hp = 0;
    let mut stone = vulnerable(Side::Enemy);
    stone.petrified = true;
    let mut battle = firing_battle(
        FIRE,
        vec![actor(Side::Party), dead, stone, vulnerable(Side::Enemy)],
        contact_projectile(false, true),
    );
    assert_eq!(
        hits(&battle.step(BattleInput::default()).unwrap()),
        [(ProjectileId(1), ActorId(3), 0)]
    );
    let random = battle.random_state();
    assert!(hits(&battle.step(BattleInput::default()).unwrap()).is_empty());
    assert_eq!(random, battle.random_state());
}

#[test]
fn death_cancels_pending_tasks_before_their_next_emission_and_delivers_one_outcome() {
    let source = r#"
        asset projectile: battle::Projectile = "test/projectile";
        pub task run() {
            await battle::at_age(ticks(3));
            battle::emit(projectile, battle::ground_point(battle::owner(), 0.0, 0.0));
        }
    "#;
    let mut projectile = contact_projectile(false, true);
    projectile.contact.as_mut().unwrap().hit.power = Power::Fixed(100);
    let mut battle = projectile_battle(
        source,
        vec![actor(Side::Party), vulnerable(Side::Enemy)],
        30,
        projectile.clone(),
    );
    Arc::get_mut(&mut battle.prepared).unwrap().actions[0].phase = ActionPhase::Actor;
    battle.step(request(1)).unwrap();
    battle
        .emit(
            Arc::new(projectile),
            ActionId(77),
            ActorId(0),
            ActorId(1),
            [0.; 3],
        )
        .unwrap();
    battle.step(BattleInput::default()).unwrap();
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(hits(&frame), [(ProjectileId(1), ActorId(1), 0)]);
    assert!(frame.cues.contains(&Cue::Interrupted {
        action: ActionId(1)
    }));
    assert!(
        !frame
            .cues
            .iter()
            .any(|c| matches!(c, Cue::ProjectileStarted { .. }))
    );
    assert!(frame.outcome.is_none());
    assert_eq!(frame.recognized_result, None);
    let frame = battle.step(BattleInput::default()).unwrap();
    assert_eq!(frame.recognized_result, Some(BattleResult::Victory));
    assert!(frame.outcome.is_none());
    battle.retire_combat().unwrap();
    let frame = battle.finish_result().unwrap();
    let outcome = frame.outcome.unwrap();
    assert_eq!(outcome.result, BattleResult::Victory);
    assert_eq!(outcome.actors[1].hp, 0);
    assert!(frame.actions.is_empty() && frame.projectiles.is_empty());
    assert!(battle.step(BattleInput::default()).is_err());
}

#[test]
fn projectile_retains_combo_power_after_owner_changes_or_is_interrupted() {
    let mut owner = actor(Side::Party);
    owner.stats.slash = 200;
    owner.stats.accuracy = 100;
    owner.attack_power = 50;
    let mut battle = firing_battle(
        FIRE,
        vec![owner, vulnerable(Side::Enemy)],
        contact_projectile(false, true),
    );
    battle.actors[0].attack_power = 10;
    let frame = battle
        .step(BattleInput {
            interrupt: vec![ActionId(1)],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frame.actors[1].hp, 0); // 100 * 50% * 102% = 51, before HP clamp.
    assert!(matches!(
        frame.cues.iter().find(|c| matches!(c, Cue::Hit { .. })),
        Some(Cue::Hit {
            result: HitResult { amount: 51, .. },
            ..
        })
    ));
}

#[test]
fn surviving_projectile_inherits_the_live_owners_element_at_contact() {
    for (element, expected) in [
        (HitElement::Inherited, Affinity::Absorb),
        (HitElement::Neutral, Affinity::Normal),
        (HitElement::Element(Element::Lightning), Affinity::Weak),
    ] {
        let mut owner = actor(Side::Party);
        owner.elements.base = Some(Element::Fire);
        let mut target = vulnerable(Side::Enemy);
        target.affinities[Element::Fire as usize + 1] = Affinity::Immune;
        target.affinities[Element::Water as usize + 1] = Affinity::Absorb;
        target.affinities[Element::Lightning as usize + 1] = Affinity::Weak;
        let mut projectile = contact_projectile(false, true);
        projectile.contact.as_mut().unwrap().hit.element = element;
        let mut battle = firing_battle(FIRE, vec![owner, target], projectile);
        battle.actors[0].elements.enchantment = Some(Element::Water);
        let frame = battle
            .step(BattleInput {
                interrupt: vec![ActionId(1)],
                ..Default::default()
            })
            .unwrap();
        let result = frame
            .cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Hit { result, .. } => Some(result),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.affinity, expected);
        assert_eq!(result.hp_change > 0, expected == Affinity::Absorb);
    }
}

#[test]
fn guard_uses_post_acceleration_velocity_and_preserves_ordinary_projectile_retirement() {
    for (acceleration, expected_guard, hp) in [
        (
            0.3,
            GuardResult::Blocked {
                first: true,
                special: false,
            },
            45,
        ),
        (0.4, GuardResult::Broken, 30),
    ] {
        let mut target = vulnerable(Side::Enemy);
        target.guard = Guard {
            active: true,
            break_pressure: 10,
            reduction: 75,
            ..Default::default()
        };
        let mut projectile = contact_projectile(false, false);
        projectile.acceleration = [0., 0., acceleration];
        let hit = &mut projectile.contact.as_mut().unwrap().hit;
        hit.power = Power::Fixed(20);
        hit.guard.pressure = 1;
        let mut battle = firing_battle(FIRE, vec![actor(Side::Party), target], projectile);
        let paused = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            (paused.actors[1].hp, paused.actors[1].guard.pressure),
            (50, 0)
        );
        assert_eq!(battle.random_state(), 1);
        let frame = battle.step(BattleInput::default()).unwrap();
        let Some(Cue::Hit { result, .. }) =
            frame.cues.iter().find(|c| matches!(c, Cue::Hit { .. }))
        else {
            panic!("missing contact")
        };
        assert_eq!(result.guard, expected_guard);
        assert_eq!(frame.actors[1].hp, hp);
        assert!(!frame.projectiles[0].disarmed);
        // A guard result does not itself deflect/disarm an ordinary projectile.
        let frame = battle.step(BattleInput::default()).unwrap();
        assert_eq!(frame.projectiles.len(), 1);
        assert!(frame.projectiles[0].contact_active);
        assert!(!frame.projectiles[0].disarmed);
        assert!(hits(&frame).is_empty());
        assert!(
            battle
                .step(BattleInput::default())
                .unwrap()
                .projectiles
                .is_empty()
        );
    }
}

#[test]
fn exhausted_repeat_counter_preserves_the_original_byte_wrap() {
    let mut projectile = contact_projectile(false, true);
    projectile.lifetime = 0;
    let contact = projectile.contact.as_mut().unwrap();
    contact.cooldown = 1;
    contact.repeat_limit = 1;
    let mut battle = firing_battle(
        FIRE,
        vec![actor(Side::Party), vulnerable(Side::Enemy)],
        projectile,
    );
    assert_eq!(hits(&battle.step(BattleInput::default()).unwrap()).len(), 1);
    for _ in 2..=256 {
        assert!(hits(&battle.step(BattleInput::default()).unwrap()).is_empty());
    }
    assert_eq!(hits(&battle.step(BattleInput::default()).unwrap()).len(), 1);
    assert_eq!(battle.actors[1].hp, 48);
}
