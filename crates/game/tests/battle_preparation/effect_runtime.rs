use super::actor;
use resonance_battle::{
    ActionDefinition, ActionPhase, ActionRequest, ActorId, Battle, BattleFrame, BattleInput, Cue,
    EffectBank, ParticleGeometry, PreparedBattle, ResourceBinding, Side,
};
use resonance_content::battle_effect::ProgramSource;

pub(super) fn no_sound(_: u16) -> anyhow::Result<resonance_battle::SoundBinding> {
    anyhow::bail!("unexpected sound dependency")
}
use resonance_game::battle::effect_program;
use std::{collections::BTreeMap, sync::Arc};

fn sources() -> Vec<ProgramSource> {
    serde_json::from_str(include_str!("../fixtures/casting-particle-sources.json")).unwrap()
}

pub(super) fn compiled(source: &str) -> (Arc<symphonia_script::Program>, u32) {
    let compiled = symphonia_script_compiler::compile(
        "test",
        &BTreeMap::from([("test".into(), source.into())]),
        &resonance_battle::native_declarations(),
    )
    .unwrap();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == "test::run")
        .unwrap()
        .entry;
    (Arc::new(compiled.program), entry)
}

pub(super) fn battle(
    body: &str,
    definitions: BTreeMap<u16, Arc<ActionDefinition>>,
    seed: u32,
) -> (Battle, ActorId) {
    battle_from_actor(body, definitions, seed, actor())
}

pub(super) fn battle_from_actor(
    body: &str,
    definitions: BTreeMap<u16, Arc<ActionDefinition>>,
    seed: u32,
    owner: resonance_battle::Actor,
) -> (Battle, ActorId) {
    let (program, entry) = compiled(&format!(
        "script battle; use battle; asset effect: battle::Effect = \"test/common\"; pub task run() {{ {body} }}"
    ));
    let mut enemy = actor();
    enemy.side = Side::Enemy;
    let prepared = Arc::new(
        PreparedBattle::new(
            vec![owner, enemy],
            vec![ActionDefinition {
                id: 1,
                phase: ActionPhase::Actor,
                program,
                entry,
                duration: 100,
                tp_cost: 0,
                resources: vec![ResourceBinding::Effect(77)],
            }],
            seed,
            vec![],
            vec![EffectBank {
                models: Default::default(),
                resource: 77,
                members: definitions,
            }],
        )
        .unwrap(),
    );
    let id = prepared.actor_ids().next().unwrap();
    (Battle::new(prepared), id)
}

pub(super) fn definitions() -> BTreeMap<u16, Arc<ActionDefinition>> {
    [3, 5, 7, 8]
        .into_iter()
        .zip(sources())
        .map(|(id, source)| {
            (
                id,
                Arc::new(effect_program::prepare(&source, 77, id, &mut no_sound).unwrap()),
            )
        })
        .collect()
}

#[test]
fn effect_sounds_use_the_effect_origin_and_share_timeline_ownership_and_pauses() {
    use resonance_battle::SoundBinding;
    use resonance_content::battle_effect::Record;
    let mut source = sources().remove(0);
    let particle = *source.particles.keys().next().unwrap();
    source.records = vec![
        Record {
            age: 0,
            command: 252,
            argument: 0,
            operand: 0xffff,
        },
        Record {
            age: 0,
            command: 252,
            argument: 92,
            operand: 0x12c,
        },
        Record {
            age: 0,
            command: particle,
            argument: 0,
            operand: 0,
        },
        Record {
            age: 2,
            command: 255,
            argument: 2,
            operand: 1,
        },
        Record {
            age: 0,
            command: 252,
            argument: 77,
            operand: 0x102,
        },
        Record {
            age: 5,
            command: 254,
            argument: 0,
            operand: 0,
        },
    ];
    let mut dependencies = vec![];
    let definition = effect_program::prepare(&source, 77, 28, &mut |id| {
        dependencies.push(id);
        Ok(SoundBinding {
            resource: 3,
            index: id,
        })
    })
    .unwrap();
    assert_eq!(dependencies, [92, 77]); // Zero is silent and needs no audio resource.
    for cancel_effect in [false, true] {
        let mut owner = actor();
        owner.position = [20., 0., 30.];
        owner.movement.direction = [1., 0., 0.];
        let (mut battle, owner) = battle_from_actor(
            "battle::show(effect, 28, battle::owner()); battle::forward_speed(5.0, false); await battle::wait_ticks(ticks(30));",
            BTreeMap::from([(28, Arc::new(definition.clone()))]),
            1,
            owner,
        );
        let first = battle
            .step(BattleInput {
                actions: vec![ActionRequest {
                    actor: owner,
                    target: owner,
                    action: 1,
                }],
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            first.cues[2],
            Cue::Sound {
                sound: SoundBinding { index: 92, .. },
                priority: 44,
                position: [20., 0., 30.],
                ..
            }
        ));
        let Cue::ParticleStarted {
            action: Some(effect),
            ..
        } = first.cues[3]
        else {
            panic!("sound must precede particle")
        };
        let paused = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert!(paused.cues.is_empty());
        assert_eq!(paused.update, first.update);
        // A missing audio dependency cannot replace this active generation.
        assert!(effect_program::prepare(&source, 77, 28, &mut no_sound).is_err());
        let mut sounds = vec![];
        for update in 1..=6 {
            let frame = battle
                .step(BattleInput {
                    interrupt: if update == 1 {
                        vec![first.actions[0].0]
                    } else if cancel_effect && update == 2 {
                        vec![effect]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                })
                .unwrap();
            for cue in frame.cues {
                if let Cue::Sound {
                    actor,
                    sound,
                    priority,
                    position,
                } = cue
                {
                    assert_ne!(frame.actors[owner.index()].position, position);
                    assert_eq!(
                        (actor, sound, priority, position),
                        (
                            owner,
                            SoundBinding {
                                resource: 3,
                                index: 77
                            },
                            2,
                            [20., 0., 30.]
                        )
                    );
                    sounds.push(update);
                }
            }
        }
        assert_eq!(sounds, if cancel_effect { vec![1] } else { vec![1, 2] });
        assert_eq!(battle.random_state(), 1);
    }
}

pub(super) fn start((battle, actor): &mut (Battle, ActorId)) -> BattleFrame {
    battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: *actor,
                target: *actor,
                action: 1,
            }],
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn immediate_effect_visit_precedes_group_update_and_particles_outlive_both_sequences() {
    let mut battle = battle(
        "battle::show(effect, 3, battle::owner()); battle::finish();",
        definitions(),
        1,
    );
    let frame = start(&mut battle);
    assert!(frame.actions.is_empty());
    assert_eq!(
        frame.particles.iter().map(|p| p.member).collect::<Vec<_>>(),
        [34, 35, 36]
    );
    let effect = frame
        .cues
        .iter()
        .find_map(|c| match c {
            Cue::ParticleStarted { action, .. } => *action,
            _ => None,
        })
        .unwrap();
    assert_eq!(battle.0.action_age(effect), Some(2));
    let paused = battle
        .0
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.particles, frame.particles);
    assert_eq!(paused.update, frame.update);
    let mut births = vec![0, 0, 0];
    let mut deaths = vec![];
    for update in 1..=18 {
        let frame = battle.0.step(BattleInput::default()).unwrap();
        for cue in frame.cues {
            match cue {
                Cue::ParticleStarted { .. } => births.push(update),
                Cue::ParticleExpired { .. } => deaths.push(update),
                _ => {}
            }
        }
        if update == 8 {
            assert_eq!(battle.0.action_age(effect), None);
            assert_eq!(frame.particles.len(), 6);
        }
        if update == 18 {
            assert!(frame.particles.is_empty());
        }
    }
    assert_eq!(births, [0, 0, 0, 1, 3, 5]);
    assert_eq!(deaths, [13, 13, 14, 16, 17, 18]);
}

#[test]
fn late_effects_follow_each_visit_and_new_particles_wait_for_the_next_group() {
    let mut owner = actor();
    owner.movement.direction = [1., 0., 0.];
    let mut battle = battle_from_actor(
        "battle::show_following(effect, 3, battle::owner(), 1.0, true, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
         battle::forward_speed(5.0, false); await battle::wait_ticks(ticks(30));",
        definitions(),
        1,
        owner,
    );
    let first = start(&mut battle);
    assert_eq!(first.particles.len(), 3);
    assert!(first.particles.iter().all(|p| p.origin == [0.; 3]));
    let mut births = vec![0, 0, 0];
    let mut previous_origin = first.actors[0].position;
    let mut last_count = 3;
    for update in 1..=6 {
        let frame = battle.0.step(BattleInput::default()).unwrap();
        assert_eq!(frame.particles[0].origin, [0.; 3]);
        for cue in &frame.cues {
            if matches!(cue, Cue::ParticleStarted { .. }) {
                births.push(update);
            }
        }
        if frame.particles.len() > last_count {
            assert_eq!(frame.particles.last().unwrap().origin, previous_origin);
            assert_ne!(previous_origin, [0.; 3]);
        }
        last_count = frame.particles.len();
        previous_origin = frame.actors[0].position;
    }
    assert_eq!(births, [0, 0, 0, 2, 4, 6]);
}

#[test]
fn attached_particles_follow_each_visit_after_the_emitting_effect_is_cancelled() {
    use resonance_content::battle_effect::Record;
    for late in [false, true] {
        for follow in [false, true] {
            let mut source = sources().remove(0);
            let member = *source.particles.keys().next().unwrap();
            let data = source.particles.get_mut(&member).unwrap();
            data.follow_origin = follow;
            data.late = late;
            source.records = vec![
                Record {
                    age: 0,
                    command: member,
                    argument: 0,
                    operand: 0,
                },
                Record {
                    age: 10,
                    command: 254,
                    argument: 0,
                    operand: 0,
                },
            ];
            let definition = effect_program::prepare(&source, 77, 3, &mut no_sound).unwrap();
            if follow {
                let (mut invalid, owner) = battle(
                    "battle::show(effect, 3, battle::owner());",
                    BTreeMap::from([(3, Arc::new(definition.clone()))]),
                    1,
                );
                assert!(
                    invalid
                        .step(BattleInput {
                            actions: vec![ActionRequest {
                                actor: owner,
                                target: owner,
                                action: 1
                            }],
                            ..Default::default()
                        })
                        .unwrap_err()
                        .to_string()
                        .contains("origin attachment")
                );
            }
            let mut owner = actor();
            owner.position = [20., 0., 30.];
            owner.movement.direction = [1., 0., 0.];
            let mut battle = battle_from_actor(
                "battle::show_following(effect, 3, battle::owner(), 1.0, true, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
                 battle::forward_speed(5.0, false); await battle::wait_ticks(ticks(30));",
                BTreeMap::from([(3, Arc::new(definition))]), 1, owner,
            );
            let first = start(&mut battle);
            let effect = first
                .cues
                .iter()
                .find_map(|cue| match cue {
                    Cue::ParticleStarted { action, .. } => *action,
                    _ => None,
                })
                .unwrap();
            assert_eq!(first.particles.len(), 1);
            for update in 1..=5 {
                let frame = battle
                    .0
                    .step(BattleInput {
                        interrupt: if update == 1 { vec![effect] } else { vec![] },
                        ..Default::default()
                    })
                    .unwrap();
                assert_ne!(frame.actors[0].position, first.actors[0].position);
                assert_eq!(
                    frame.particles[0].origin,
                    if follow {
                        frame.actors[0].position
                    } else {
                        first.particles[0].origin
                    }
                );
                assert_eq!(frame.particles[0].heading, first.particles[0].heading);
                let paused = battle
                    .0
                    .step(BattleInput {
                        menu_open: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(paused.particles, frame.particles);
            }
        }
    }
}

#[test]
fn centered_effects_and_particles_observe_the_sample_before_actor_movement() {
    use resonance_content::battle_effect::Record;
    let mut source = sources().remove(0);
    let member = *source.particles.keys().next().unwrap();
    source.particles.get_mut(&member).unwrap().follow_origin = true;
    source.records = vec![
        Record {
            age: 0,
            command: member,
            argument: 0,
            operand: 0,
        },
        Record {
            age: 10,
            command: 254,
            argument: 0,
            operand: 0,
        },
    ];
    let definition = effect_program::prepare(&source, 77, 3, &mut no_sound).unwrap();
    let mut owner = actor();
    owner.body.center_offset = [10., 80., -5.];
    owner.body.scale = 2.;
    owner.position = [20., 0., 30.];
    owner.movement.direction = [1., 0., 0.];
    let mut battle = battle_from_actor(
        "let tint = battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 };
         battle::show_following(effect, 3, battle::owner(), 1.0, true, tint);
         battle::show_centered(effect, 3, battle::owner(), 1.0, true, tint);
         battle::forward_speed(5.0, false); await battle::wait_ticks(ticks(30));",
        BTreeMap::from([(3, Arc::new(definition))]),
        1,
        owner,
    );
    let first = start(&mut battle);
    assert_eq!(first.particles.len(), 2);
    assert_eq!(first.particles[1].origin, [40., 160., 20.]);
    let mut previous = first.actors[0].position;
    for _ in 0..5 {
        let frame = battle.0.step(BattleInput::default()).unwrap();
        let center = [previous[0] + 20., previous[1] + 160., previous[2] - 10.];
        assert_eq!(frame.actors[0].body.center, center);
        assert_eq!(frame.particles[0].origin, frame.actors[0].position);
        assert_eq!(frame.particles[1].origin, center);
        assert_ne!(frame.particles[0].origin[0] + 20., center[0]);
        previous = frame.actors[0].position;
    }
}

#[test]
fn late_particles_follow_effects_in_the_same_update_and_keep_creation_order() {
    use resonance_content::battle_effect::Record;
    for late in [false, true] {
        let mut source = sources().remove(0);
        source.records = vec![
            Record {
                age: 1,
                command: 36,
                argument: 0,
                operand: 0,
            },
            Record {
                age: 2,
                command: 254,
                argument: 0,
                operand: 0,
            },
        ];
        let particle = source.particles.get_mut(&36).unwrap();
        particle.late = late;
        particle.lifetime = 1;
        let definition = Arc::new(effect_program::prepare(&source, 77, 3, &mut no_sound).unwrap());
        let mut run = battle(
            "battle::show_following(effect, 3, battle::owner(), 1.0, true, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
             battle::show_following(effect, 3, battle::target(), 1.0, true, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
             battle::finish();",
            BTreeMap::from([(3, definition)]),
            1,
        );
        let first = start(&mut run);
        assert_eq!(first.particles.len(), if late { 2 } else { 0 });
        let born = if late {
            first
        } else {
            run.0.step(BattleInput::default()).unwrap()
        };
        // The second effect is prepended; its particle is appended first.
        let parents: Vec<_> = born
            .cues
            .iter()
            .filter_map(|c| match c {
                Cue::ParticleStarted { action, .. } => *action,
                _ => None,
            })
            .collect();
        assert_eq!(parents.len(), 2);
        assert!(parents[0] > parents[1]);
        let starts: Vec<_> = born
            .cues
            .iter()
            .filter_map(|c| match c {
                Cue::ParticleStarted { particle, .. } => Some(*particle),
                _ => None,
            })
            .collect();
        assert_eq!(
            starts,
            born.particles.iter().map(|p| p.id).collect::<Vec<_>>()
        );
        assert!(born.particles.iter().all(|p| p.age == 0));
        let next = run.0.step(BattleInput::default()).unwrap();
        assert!(next.particles.iter().all(|p| p.age == 1));
        let retired = run.0.step(BattleInput::default()).unwrap();
        assert!(retired.particles.is_empty());
        assert_eq!(
            retired
                .cues
                .iter()
                .filter_map(|c| match c {
                    Cue::ParticleExpired { particle } => Some(*particle),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            starts
        );
    }
}

#[test]
fn effect_scale_follows_modifiers_and_preserves_unscaled_motion_and_quad_fields() {
    let mut unit = battle(
        "battle::show(effect, 5, battle::owner()); battle::finish();",
        definitions(),
        17,
    );
    let mut scaled = battle(
        "battle::show_following(effect, 5, battle::owner(), 2.0, false, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 }); battle::finish();",
        definitions(),
        17,
    );
    let unit_frame = start(&mut unit);
    let scaled_frame = start(&mut scaled);
    assert_eq!(unit.0.random_state(), scaled.0.random_state());
    for (unit, scaled) in unit_frame.particles.iter().zip(&scaled_frame.particles) {
        assert_eq!(unit.state.velocity, scaled.state.velocity);
        assert_eq!(unit.state.angles, scaled.state.angles);
        let ParticleGeometry::Size { value, .. } = unit.state.geometry else {
            panic!("size particle")
        };
        let ParticleGeometry::Size { value: scaled, .. } = scaled.state.geometry else {
            panic!("size particle")
        };
        assert_eq!(scaled, value.map(|v| v * 2.));
    }
    let mut original = sources().remove(0);
    let quad = &mut original.particles.get_mut(&36).unwrap().state;
    quad.orbit = [3., 4., 5.];
    quad.offset = [1., 2., 3.];
    quad.velocity = [0.5, 0., 0.];
    original.particles.get_mut(&36).unwrap().orbit_velocity = [1.; 3];
    let prepared = Arc::new(effect_program::prepare(&original, 77, 3, &mut no_sound).unwrap());
    let mut scaled = battle(
        "battle::show_following(effect, 3, battle::owner(), 2.0, false, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 }); battle::finish();",
        BTreeMap::from([(3, prepared)]),
        17,
    );
    let frame = start(&mut scaled);
    let quad = &frame.particles[2].state;
    assert_eq!(quad.orbit, [7., 9., 11.]);
    assert_eq!(quad.offset, [2.5, 4., 6.]);
    let ParticleGeometry::Quad { vertices, .. } = quad.geometry else {
        panic!("quad particle")
    };
    let ParticleGeometry::Quad {
        vertices: original,
        velocity,
    } = original.particles[&36].state.geometry
    else {
        panic!("quad particle")
    };
    assert_eq!(vertices[3], original[3]);
    assert_eq!(
        &vertices[..3],
        original[..3]
            .iter()
            .zip(velocity)
            .map(|(v, speed)| std::array::from_fn::<_, 3, _>(|i| v[i] * 2. + speed[i]))
            .collect::<Vec<_>>()
    );
}

#[test]
fn canceling_the_caster_keeps_effects_and_canceling_an_effect_keeps_its_particles() {
    let mut battle = battle(
        "battle::show(effect, 3, battle::owner()); await battle::wait_ticks(ticks(40));",
        definitions(),
        1,
    );
    let first = start(&mut battle);
    let effect = first
        .cues
        .iter()
        .find_map(|c| match c {
            Cue::ParticleStarted { action, .. } => *action,
            _ => None,
        })
        .unwrap();
    let next = battle
        .0
        .step(BattleInput {
            interrupt: vec![first.actions[0].0],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(next.particles.len(), 4);
    let random = battle.0.random_state();
    let next = battle
        .0
        .step(BattleInput {
            interrupt: vec![effect],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(next.particles.len(), 4);
    for _ in 0..4 {
        battle.0.step(BattleInput::default()).unwrap();
    }
    assert_eq!(battle.0.random_state(), random);
}

#[test]
fn float_registers_apply_real_signed_random_modifiers_in_source_order() {
    let mut battle = battle(
        "battle::show(effect, 5, battle::owner()); battle::finish();",
        definitions(),
        0xdead_beef,
    );
    let mut random = 0xdead_beef_u32;
    let mut draw = |modulus: i32| {
        random = random.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687);
        (i32::from((random >> 16) as i16) % modulus) as f32 * 0.1
    };
    let expected: Vec<_> = (0..3).map(|_| [draw(240), draw(10), draw(1800)]).collect();
    let first = start(&mut battle);
    assert_eq!(
        first.particles.iter().map(|p| p.member).collect::<Vec<_>>(),
        [37, 39, 38]
    );
    for (p, values, speed, turn) in [
        (&first.particles[0], expected[0], 5., 1.),
        (&first.particles[2], expected[1], 7., 0.),
    ] {
        assert_eq!(p.state.velocity[1], speed + values[1]);
        assert_eq!(p.state.angles[1], values[2] + turn);
        let ParticleGeometry::Size { value, .. } = p.state.geometry else {
            panic!("size particle")
        };
        assert_eq!(value[2], 104. + values[0]);
    }
    for _ in 0..2 {
        assert!(
            !battle
                .0
                .step(BattleInput::default())
                .unwrap()
                .cues
                .iter()
                .any(|c| matches!(c, Cue::ParticleStarted { .. }))
        );
    }
    let repeated = battle.0.step(BattleInput::default()).unwrap();
    assert_eq!(
        repeated.particles.last().unwrap().state.angles[1],
        expected[2][2]
    );
    assert_eq!(battle.0.random_state(), random);
}

#[test]
fn exhausted_object_pool_skips_modifiers_without_consuming_random() {
    let mut battle = battle(
        "for i in 0..256 { battle::show(effect, 3, battle::owner()); } battle::finish();",
        definitions(),
        1,
    );
    let first = start(&mut battle);
    assert_eq!(
        first
            .cues
            .iter()
            .filter(|c| matches!(c, Cue::Effect { .. }))
            .count(),
        104
    );
    assert_eq!(first.particles.len(), 310);
    let mut expected = 1_u32;
    for _ in 0..103 {
        expected = expected.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687);
    }
    assert_eq!(battle.0.random_state(), expected);
    battle.0.step(BattleInput::default()).unwrap();
    assert_eq!(battle.0.random_state(), expected);
}

#[test]
fn malformed_modifier_preparation_cannot_replace_a_running_generation() {
    let mut battle = battle(
        "battle::show(effect, 3, battle::owner()); battle::finish();",
        definitions(),
        1,
    );
    let before = start(&mut battle);
    let mut source = sources().remove(0);
    source.modifiers.get_mut(&0x7918).unwrap()[1] = 0x5d;
    assert!(
        effect_program::prepare(&source, 77, 3, &mut no_sound)
            .unwrap_err()
            .to_string()
            .contains("destination")
    );
    let after = battle
        .0
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(before.particles, after.particles);
    assert_eq!(before.update, after.update);
}

#[test]
fn natural_casting_particle_bindings_random_yaw_and_rng_match_dolphin() {
    compare_casting_particles(definitions());
}

fn compare_casting_particles(definitions: BTreeMap<u16, Arc<ActionDefinition>>) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/particle-emissions.json")).unwrap();
    for row in fixture["constructors"].as_array().unwrap() {
        let mut owner = actor();
        let particles = row["particles"].as_array().unwrap();
        owner.position = std::array::from_fn(|i| {
            f32::from_bits(particles[0]["origin_bits"][i].as_u64().unwrap() as u32)
        });
        owner.heading = f32::from_bits(particles[0]["heading_bits"].as_u64().unwrap() as u32);
        let mut battle = battle_from_actor(
            "battle::show_following(effect, 3, battle::owner(), 1.0, true, battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 }); battle::finish();",
            definitions.clone(),
            row["random_before"].as_u64().unwrap() as u32,
            owner,
        );
        let frame = start(&mut battle);
        assert_eq!(
            battle.0.random_state(),
            row["random_after"].as_u64().unwrap() as u32
        );
        assert_eq!(frame.particles.len(), particles.len());
        for (particle, expected) in frame.particles.iter().zip(particles) {
            assert_eq!(
                u64::from(particle.member),
                expected["member"].as_u64().unwrap()
            );
            assert_eq!(
                particle.state.velocity[1].to_bits(),
                expected["velocity_y_bits"].as_u64().unwrap() as u32
            );
            if particle.member == 36 {
                // This quad has zero angular velocity: group initialization does
                // not change the yaw observed immediately after its modifier.
                assert_eq!(
                    particle.state.angles[1].to_bits(),
                    expected["yaw_bits"].as_u64().unwrap() as u32
                );
            }
        }
    }
}

#[test]
fn authored_casting_runs_original_pulses_through_release_and_interruption() {
    use resonance_battle::{ActionPhase, Control, ModelDefinition, MotionBinding, Playback};
    use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
    let source = r#"
        script battle; use battle; use battle::casting;
        asset common: battle::Effect = "test/common";
        asset release_pose: battle::Motion = "test/release";
        asset spell: battle::Spell = "test/spell";
        asset release_sound: battle::Sound = "test/sound";
        asset voice: battle::Voice = "test/voice";
        pub task run() {
            // Observe release and its particle tail while recovery is pending;
            // source 2B18C's later guard draw is outside this component trace.
            await casting::ordinary(spell, ticks(17), ticks(90),
                casting::ReleaseMotion { motion: release_pose, rate: 1.0,
                    blend: ticks(0), loop_start: 0.0, repeat: false },
                casting::Effects { effect: common, pulse_member: 3, release_member: 7, release_sound: release_sound, scale: 1.0, tint: battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 } },
                casting::Voices { chant: voice, fallback: voice, release: voice });
            battle::finish();
        }
        pub task released() {}
    "#;
    let compilation = symphonia_script_compiler::compile(
        "test",
        &BTreeMap::from([
            ("test".into(), source.into()),
            (
                "battle::casting".into(),
                include_str!("../../../../scripts/battle/casting.sym").into(),
            ),
        ]),
        &resonance_battle::native_declarations(),
    )
    .unwrap();
    let resources: Vec<_> = compilation
        .assets
        .iter()
        .map(|a| match a.path.as_str() {
            "test/common" => ResourceBinding::Effect(77),
            "test/voice" => ResourceBinding::Voice(vec![None; 2]),
            "test/sound" => ResourceBinding::Sound(resonance_battle::SoundBinding {
                resource: 7,
                index: 123,
            }),
            "test/release" => ResourceBinding::Motion(MotionBinding { model: 7, clip: 12 }),
            "test/spell" => ResourceBinding::Spell(2),
            _ => panic!("unexpected asset"),
        })
        .collect();
    let program = Arc::new(compilation.program);
    let entry = |name| {
        program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .entry
    };
    let actions = [
        (1, ActionPhase::Casting, "test::run", 7),
        (2, ActionPhase::Resident, "test::released", 0),
    ]
    .into_iter()
    .map(|(id, phase, name, tp_cost)| ActionDefinition {
        id,
        phase,
        program: program.clone(),
        entry: entry(name),
        duration: 30,
        tp_cost,
        resources: resources.clone(),
    })
    .collect();
    let model = Arc::new(ModelDefinition {
        secondary_motion: vec![],
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        resource: 7,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind: Transform::default(),
                bind_channels: TransformChannels(8),
            }],
        },
        motions: [0, 12]
            .into_iter()
            .map(|clip| {
                (
                    clip,
                    Motion {
                        duration_frames: 3.,
                        tracks: vec![],
                    },
                )
            })
            .collect(),
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 1.,
            repeat: true,
        },
        anchors: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![],
        suppress_root_translation: [false; 3],
    });
    let mut owner = actor();
    owner.control = Control::Manual;
    let mut enemy = actor();
    enemy.side = Side::Enemy;
    let prepared = Arc::new(
        PreparedBattle::new(
            vec![owner, enemy],
            actions,
            1,
            vec![Some(model), None],
            vec![EffectBank {
                models: Default::default(),
                resource: 77,
                members: definitions(),
            }],
        )
        .unwrap(),
    );
    let owner = prepared.actor_ids().next().unwrap();
    for interrupt in [false, true] {
        let mut battle = Battle::new(prepared.clone());
        let mut caster = None;
        let mut pulses = vec![];
        let mut particles = vec![];
        let mut released = false;
        for age in 0..=40 {
            let frame = battle
                .step(BattleInput {
                    actions: if age == 0 {
                        vec![ActionRequest {
                            actor: owner,
                            target: owner,
                            action: 1,
                        }]
                    } else {
                        vec![]
                    },
                    interrupt: if interrupt && age == 18 {
                        vec![caster.unwrap()]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                })
                .unwrap();
            for cue in &frame.cues {
                match cue {
                    Cue::Started { action, .. } => caster = Some(*action),
                    Cue::Effect { member: 3, .. } => pulses.push(age),
                    Cue::ParticleStarted { particle, .. } => {
                        if frame
                            .particles
                            .iter()
                            .any(|p| p.id == *particle && (34..=36).contains(&p.member))
                        {
                            particles.push(age);
                        }
                    }
                    Cue::Released { .. } => released = true,
                    _ => {}
                }
            }
        }
        assert_eq!(pulses, [1, 9, 17]);
        assert_eq!(
            particles,
            [
                1, 1, 1, 3, 5, 7, 9, 9, 9, 11, 13, 15, 17, 17, 17, 19, 21, 23
            ]
        );
        assert_eq!(released, !interrupt);
        assert_eq!(battle.actors()[0].tp, if interrupt { 40 } else { 33 });
        // Three pulse groups consume twelve draws. An interrupted chant's
        // final request lasts through its inclusive zero visit at age 25.
        // Successful release re-arms the source385A0 recovery timer to 90,
        // keeping both actor-common draws active through this trace's age 40.
        let last_jitter_update = if interrupt { 25 } else { 40 };
        let expected = (0..12 + 2 * last_jitter_update).fold(1_u32, |random, _| {
            random.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687)
        });
        assert_eq!(battle.random_state(), expected);
    }
}

#[test]
fn release_bursts_keep_source_modifiers_and_outlive_the_emitting_task() -> anyhow::Result<()> {
    for member in [7, 8] {
        let mut active = battle(
            &format!(
                "battle::show_following(effect, {member}, battle::owner(), 1.0, true, battle::EffectTint {{ enabled: false, palette: 0, red: 0, green: 0, blue: 0 }}); battle::finish();"
            ),
            definitions(),
            1,
        );
        let first = start(&mut active);
        assert!(first.actions.is_empty());
        let mut births = Vec::new();
        for age in 1..=60 {
            let frame = active.0.step(BattleInput::default())?;
            for cue in &frame.cues {
                if let Cue::ParticleStarted { particle, .. } = cue {
                    let p = frame
                        .particles
                        .iter()
                        .find(|p| p.id == *particle)
                        .expect("new particle");
                    births.push((age, p.member));
                    assert_eq!(p.draw_after, Some(active.1));
                    if p.member == 12 {
                        assert_eq!(p.state.geometry_count, if member == 7 { 2 } else { 4 });
                        assert_eq!(
                            p.state.uv,
                            if member == 7 {
                                [0, 385, 32, 62]
                            } else {
                                [288, 64, 32, 32]
                            }
                        );
                        let ParticleGeometry::Size { value, .. } = &p.state.geometry else {
                            panic!("size particle")
                        };
                        assert_eq!(*value, [0., if member == 7 { 64. } else { 48. }, 80.75]);
                    }
                }
            }
            if (16..=24).contains(&age) {
                let p = frame.particles.last().expect("modified release particle");
                let ParticleGeometry::Size {
                    value, velocity, ..
                } = &p.state.geometry
                else {
                    panic!("size particle")
                };
                assert_eq!(value[2], 32.);
                assert_eq!(velocity[2], 0.);
                assert_eq!(p.state.brighten, [0, 0, 0, 16]);
                assert_eq!(p.state.brighten_until, 8);
                let alpha = if age < 24 { (age - 15) * 16 } else { 112 };
                assert_eq!([p.state.colors[0][3], p.state.colors[1][3]], [alpha; 2]);
            }
            if age == 60 {
                assert!(frame.particles.is_empty());
            }
        }
        assert_eq!(births, [(4, 12), (4, 13), (12, 14), (16, 14)]);
        assert_eq!(active.0.random_state(), 1);
    }
    Ok(())
}

#[test]
fn natural_release_particle_updates_match_dolphin() -> anyhow::Result<()> {
    use resonance_content::battle_effect::{ParticleState, Tints};
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/release-particles.json"))?;
    let particles = fixture["particles"].as_array().expect("observed particles");
    let release = fixture["release_tick"].as_u64().expect("release tick");
    let tints: Tints = serde_json::from_str(include_str!("../fixtures/effect-tints.json"))?;
    let element = fixture["element"].as_u64().expect("element") as usize;
    let [red, green, blue, _] = tints.colors[element];
    let palette = tints.palettes[element];
    let mut owner = actor();
    owner.position = serde_json::from_value::<[u32; 3]>(particles[0]["origin_bits"].clone())?
        .map(f32::from_bits);
    owner.heading = f32::from_bits(serde_json::from_value(
        particles[0]["heading_bits"].clone(),
    )?);
    let mut active = battle_from_actor(
        &format!("battle::show_following(effect, 7, battle::owner(), 0.9, true,
            battle::EffectTint {{ enabled: true, palette: {palette}, red: {red}, green: {green}, blue: {blue} }});
            battle::finish();"), definitions(), 1, owner,
    );
    let first = start(&mut active);
    assert!(first.actions.is_empty());
    let mut ids = vec![];
    let mut observations = 0;
    for age in 1..=46 {
        let frame = active.0.step(BattleInput::default())?;
        for cue in &frame.cues {
            if let Cue::ParticleStarted { particle, .. } = cue {
                ids.push(*particle);
            }
        }
        for (index, expected) in particles.iter().enumerate() {
            let update = expected["updates"]
                .as_array()
                .expect("particle updates")
                .iter()
                .find(|row| row["combat_tick"].as_u64() == Some(release + age));
            if let Some(update) = update {
                let actual = frame
                    .particles
                    .iter()
                    .find(|p| p.id == ids[index])
                    .expect("live observed particle");
                assert_eq!(
                    u64::from(actual.member),
                    expected["member"].as_u64().unwrap()
                );
                assert_eq!(i64::from(actual.age), update["age"].as_i64().unwrap());
                assert_eq!(actual.draw_after, Some(active.1));
                assert_eq!(
                    actual.origin.map(f32::to_bits),
                    serde_json::from_value::<[u32; 3]>(expected["origin_bits"].clone())?
                );
                assert_eq!(
                    actual.heading.to_bits(),
                    serde_json::from_value::<u32>(expected["heading_bits"].clone())?
                );
                let state: ParticleState = serde_json::from_value(update["state"].clone())?;
                assert_eq!(
                    actual.state, state,
                    "member {} at age {}",
                    actual.member, actual.age
                );
                // Keep exact floating-point evidence, including signed zero.
                let bits = |s: &ParticleState| {
                    let ParticleGeometry::Size {
                        value,
                        velocity,
                        acceleration,
                    } = s.geometry
                    else {
                        panic!("size geometry")
                    };
                    [
                        s.offset,
                        s.velocity,
                        s.acceleration,
                        s.angles,
                        s.angular_velocity,
                        s.orbit,
                        value,
                        velocity,
                        acceleration,
                    ]
                    .map(|v| v.map(f32::to_bits))
                };
                assert_eq!(bits(&actual.state), bits(&state));
                observations += 1;
            }
        }
        assert_eq!(active.0.random_state(), 1);
        if age == 46 {
            assert!(frame.particles.is_empty());
        }
    }
    assert_eq!(ids.len(), 4);
    assert_eq!(observations, 104);
    Ok(())
}

#[test]
fn unsupported_integer_selectors_and_misaligned_writes_fail_preparation() {
    let mut source = sources().remove(2);
    for words in [
        vec![0, 0x1f, 0, 0, 0xffff],
        vec![10, 0x30, 0x7ffc, 0, 0xffff],
        vec![10, 0x31, 1, 0, 0xffff],
    ] {
        source.modifiers.insert(31012, words);
        assert!(effect_program::prepare(&source, 77, 7, &mut no_sound).is_err());
    }
}

#[test]
fn integer_modifiers_keep_signed_colors_and_byte_narrowing() -> anyhow::Result<()> {
    let mut source = sources().remove(2);
    source.records = vec![
        resonance_content::battle_effect::Record {
            age: 0,
            command: 14,
            argument: 0,
            operand: 31012,
        },
        resonance_content::battle_effect::Record {
            age: 1,
            command: 254,
            argument: 0,
            operand: 0,
        },
    ];
    source.modifiers.insert(
        31012,
        vec![
            0, 0x1e, 0xffff, 0, // Signed alpha -1.
            10, 0x2b, 0x1ff, 0, // Brightening narrows to 255.
            10, 0x30, 1, 0, 0xffff,
        ],
    );
    let definition = Arc::new(effect_program::prepare(&source, 77, 7, &mut no_sound)?);
    let mut active = battle(
        "battle::show(effect, 7, battle::owner()); battle::finish();",
        BTreeMap::from([(7, definition)]),
        1,
    );
    let frame = start(&mut active);
    let state = &frame.particles[0].state;
    assert_eq!(state.brighten[3], 255);
    assert_eq!(state.brighten_until, 1);
    assert_eq!([state.colors[0][3], state.colors[1][3]], [254, 255]);
    assert_eq!(active.0.random_state(), 1);
    Ok(())
}

#[test]
fn expired_particle_handles_fault_and_recursive_activation_is_bounded() {
    let (program, entry) = compiled("script battle; use battle; asset speck: battle::ParticleTemplate = \"test/speck\"; pub task run() {
        let p = battle::spawn_particle(speck);
        await battle::at_age(ticks(4));
        battle::set_particle_angles(p, battle::ground_point(battle::owner(), 0.0, 0.0));
        battle::finish(); }");
    let mut data = sources().remove(0).particles.remove(&36).unwrap();
    data.lifetime = 1;
    let effect = Arc::new(ActionDefinition {
        id: 3,
        phase: ActionPhase::Effect,
        program,
        entry,
        duration: 0,
        tp_cost: 0,
        resources: vec![ResourceBinding::Particle(Arc::new(
            resonance_battle::ParticleDefinition {
                model: None,
                resource: 77,
                member: 36,
                data,
            },
        ))],
    });
    let mut run = battle(
        "battle::show(effect, 3, battle::owner()); battle::finish();",
        BTreeMap::from([(3, effect)]),
        1,
    );
    start(&mut run);
    run.0.step(BattleInput::default()).unwrap();
    assert!(
        run.0
            .step(BattleInput::default())
            .unwrap()
            .particles
            .is_empty()
    );
    assert!(
        run.0
            .step(BattleInput::default())
            .unwrap_err()
            .to_string()
            .contains("stale particle handle")
    );
    assert!(
        run.0
            .step(BattleInput::default())
            .unwrap_err()
            .to_string()
            .contains("faulted")
    );

    let (program, entry) = compiled(
        "script battle; use battle; asset cycle: battle::Effect = \"test/common\"; pub task run() { battle::show(cycle, 3, battle::owner()); }",
    );
    let effect = Arc::new(ActionDefinition {
        id: 3,
        phase: ActionPhase::Effect,
        program,
        entry,
        duration: 0,
        tp_cost: 0,
        resources: vec![ResourceBinding::Effect(77)],
    });
    let (mut run, actor) = battle(
        "battle::show(effect, 3, battle::owner());",
        BTreeMap::from([(3, effect)]),
        1,
    );
    let error = run
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor,
                target: actor,
                action: 1,
            }],
            ..Default::default()
        })
        .unwrap_err();
    assert!(error.to_string().contains("activation limit exceeded"));
    assert!(run.step(BattleInput::default()).is_err());
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cooked_original_effect_sources_load_and_match_the_dolphin_particle_observations()
-> anyhow::Result<()> {
    use resonance_content::{
        battle_effect::{COMMON_PATH, SourceBank, TECHNIQUES_PATH},
        prepared::{Cache, Files},
    };
    let files = Files::load(
        &super::common::asset_root(),
        &["fields/map-340.preload.json"],
        &mut Cache::default(),
        || false,
    )?;
    let common: SourceBank = files.json(COMMON_PATH)?;
    let techniques: SourceBank = files.json(TECHNIQUES_PATH)?;
    let nurse: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/nurse-transition-effect.json"))?;
    let nurse_source: SourceBank = serde_json::from_value(nurse["source"].clone())?;
    // The captured fixture contains the ef1 source, before publication added
    // its presentation dependencies. Both original discs contain this same
    // BTLusual.dat; member2's hash is retained in the source comparison below.
    let mut original_source = common.clone();
    let art = original_source
        .art
        .take()
        .expect("ordinary effect artwork was not published");
    assert_eq!(
        art.source_sha256,
        "daf67bba141841c6a317c4bad950dc7dcfc5d3a90cfc7659d76b37e277af5d50"
    );
    assert!(
        serde_json::to_value(&original_source)? == serde_json::to_value(&nurse_source)?,
        "original common effect source differs from the Dolphin fixture"
    );
    let nurse = effect_program::load(&files, COMMON_PATH, 77, &[37], &mut no_sound)?;
    assert_eq!(nurse.members.len(), 1);
    assert_eq!((common.programs.len(), common.actors.len()), (52, 87));
    assert_eq!(
        (techniques.programs.len(), techniques.actors.len()),
        (138, 142)
    );
    for (member, expected) in [3, 5, 7, 8].into_iter().zip(sources()) {
        assert_eq!(
            serde_json::to_value(common.program(member)?)?,
            serde_json::to_value(expected)?
        );
    }
    let star = common.particle(19)?;
    assert_eq!(star.state.uv, [1, 65, 30, 30]);
    assert_eq!(star.state.palettes, [17, 0]);
    assert_eq!(star.uv_track, common.uv);
    let bank = effect_program::load(&files, COMMON_PATH, 77, &[3, 5, 7, 8], &mut no_sound)?;
    compare_casting_particles(bank.members);
    // Common hit effect 1 includes kinds 4 and 5, sharing 40E40/403F4.
    let bank = effect_program::load(&files, COMMON_PATH, 77, &[1], &mut no_sound)?;
    let trace: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/effect-timelines.json"))?;
    let case = trace["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["bank"] == "Common" && case["program"] == 1)
        .unwrap();
    assert_eq!(serde_json::to_value(&common.programs[1])?, case["records"]);
    let seed = case["visits"][0]["random_before"].as_u64().unwrap() as u32;
    let mut hit = battle(
        "battle::show(effect, 1, battle::owner()); battle::finish();",
        bank.members,
        seed,
    );
    let mut frame = start(&mut hit);
    // This test emits in actor dispatch: constructor visit 0 and ordinary visit
    // 1 both precede the first particle pass. The fixture records individual
    // visits, not particle initialization or drawing frames.
    for (age, visit) in case["visits"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .skip(1)
    {
        if age != 1 {
            frame = hit.0.step(BattleInput::default())?;
        }
        let commands = case["visits"][0]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|_| age == 1)
            .chain(visit["commands"].as_array().unwrap());
        let expected: Vec<_> = commands
            .map(|index| common.programs[1][index.as_u64().unwrap() as usize].command as u16)
            .collect();
        let observed: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| {
                let Cue::ParticleStarted { particle, .. } = cue else {
                    return None;
                };
                Some(
                    frame
                        .particles
                        .iter()
                        .find(|p| p.id == *particle)
                        .unwrap()
                        .member,
                )
            })
            .collect();
        assert_eq!(observed, expected, "common-1 visit {age}");
        assert_eq!(visit["random_before"], visit["random_after"]);
        assert_eq!(hit.0.random_state(), seed);
    }
    assert_eq!(frame.particles.len(), 2); // both particles outlive the timeline
    // The entire batch fails when one requested member needs an unimplemented
    // controller. Other declarations in a bank are nevertheless valid source.
    // Guard-break member0 is supported now. Original member36 references
    // declaration55's still-unprepared kind19 controller.
    assert_eq!(common.actors[55].prefix.kind, 19);
    assert!(common.programs[36].iter().any(|row| row.command == 55));
    let error = effect_program::load(&files, COMMON_PATH, 77, &[3, 36], &mut no_sound).unwrap_err();
    assert!(
        format!("{error:#}").contains("particle controller 19 is not prepared"),
        "{error:#}"
    );
    assert!(effect_program::load(&files, COMMON_PATH, 77, &[3, 999], &mut no_sound).is_err());
    assert!(effect_program::load(&files, COMMON_PATH, 77, &[3, 3], &mut no_sound).is_err());
    assert!(effect_program::load(&Files::default(), COMMON_PATH, 77, &[3], &mut no_sound).is_err());
    Ok(())
}
