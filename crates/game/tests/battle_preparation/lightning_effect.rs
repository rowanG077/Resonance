use super::effect_runtime::{battle, start};
use resonance_battle::{BattleInput, Cue, ParticleGeometry, SoundBinding};
use resonance_content::battle_effect::{ProgramSource, Record};
use resonance_game::battle::effect_program;
use std::{collections::BTreeMap, sync::Arc};

fn source() -> ProgramSource {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lightning-effect-source.json")).unwrap();
    serde_json::from_value(fixture["program"].clone()).unwrap()
}

fn definition(source: &ProgramSource) -> BTreeMap<u16, Arc<resonance_battle::ActionDefinition>> {
    let mut sounds = vec![];
    let definition = effect_program::prepare(source, 77, 28, &mut |index| {
        sounds.push(index);
        Ok(SoundBinding { resource: 3, index })
    })
    .unwrap();
    assert_eq!(sounds, [92]);
    BTreeMap::from([(28, Arc::new(definition))])
}

// Original Techniques/28 records, rather than a handwritten Lightning controller.
// This is source-derived execution coverage; Dolphin observations are separate.
#[test]
fn original_lightning_emits_modified_ribbons_and_particles_through_their_full_lifetimes() {
    let source = source();
    for interrupt_owner in [false, true] {
        let mut run = battle(
            "battle::show(effect, 28, battle::owner()); await battle::wait_ticks(ticks(60));",
            definition(&source),
            0xdead_beef,
        );
        let mut frame = start(&mut run);
        let owner_action = frame.actions[0].0;
        let sound = frame
            .cues
            .iter()
            .position(|c| matches!(c, Cue::Sound { .. }))
            .unwrap();
        let first_particle = frame
            .cues
            .iter()
            .position(|c| matches!(c, Cue::ParticleStarted { .. }))
            .unwrap();
        assert!(sound < first_particle);
        assert!(matches!(
            frame.cues[sound],
            Cue::Sound {
                sound: SoundBinding { index: 92, .. },
                priority: 0,
                ..
            }
        ));
        let mut births = vec![];
        let mut phases = vec![];
        let mut offsets = vec![];
        let mut random = 0xdead_beef_u32;
        let mut draw = |modulus: i32| {
            random = random.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687);
            (i32::from((random >> 16) as i16) % modulus) as f32 * 0.1
        };
        // Repeated commands run in registration order at each visit (420C4).
        let expected_offsets: Vec<_> = [480, 480, 480, 160, 480, 160, 480]
            .into_iter()
            .map(|modulus| [draw(modulus), 0., draw(modulus)])
            .collect();
        for update in 0..=44 {
            if update != 0 {
                frame = run
                    .0
                    .step(BattleInput {
                        interrupt: if interrupt_owner && update == 1 {
                            vec![owner_action]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    })
                    .unwrap();
                assert!(!frame.cues.iter().any(|c| matches!(c, Cue::Sound { .. })));
            }
            for p in &frame.particles {
                if p.age == 0 {
                    births.push((update, p.member));
                    if let ParticleGeometry::Ribbon { phase, .. } = p.state.geometry {
                        phases.push((p.state.geometry_count, phase));
                    }
                    if (p.member == 44 && update > 0) || (p.member == 43 && update >= 8) {
                        offsets.push(p.state.offset);
                    }
                }
                if let ParticleGeometry::Ribbon {
                    length,
                    width,
                    jitter,
                    ..
                } = p.state.geometry
                {
                    assert_eq!([length, width, jitter], [900., 176., 104.]);
                    assert_eq!(p.state.colors[0][3], (255 - 8 * (p.age + 1)).max(0));
                }
            }
            if update == 8 {
                let paused = run
                    .0
                    .step(BattleInput {
                        menu_open: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(paused.particles, frame.particles);
                assert_eq!(paused.update, frame.update);
                assert!(paused.cues.is_empty());
            }
            if update == 18 {
                assert!(!frame.particles.is_empty());
                assert!(frame.particles.iter().all(|p| p.age > 0));
            }
        }
        assert_eq!(
            births,
            [
                (0, 44),
                (0, 43),
                (0, 45),
                (2, 43),
                (2, 44),
                (5, 43),
                (5, 45),
                (5, 44),
                (8, 44),
                (8, 43),
                (11, 45),
                (11, 44),
                (11, 43),
                (14, 44),
            ]
        );
        assert_eq!(phases, [(4, 6), (5, 22), (6, 30), (4, 44), (4, 48)]);
        assert_eq!(offsets, expected_offsets);
        assert_eq!(run.0.random_state(), random);
        assert!(frame.particles.is_empty());
    }
}

#[test]
fn controlled_lightning_particles_and_sound_dispatch_match_dolphin() -> anyhow::Result<()> {
    use resonance_content::battle_effect::ParticleState;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lightning-observations.json"))?;
    let steps = fixture["steps"].as_array().expect("effect visits");
    let particles = fixture["particles"].as_array().expect("observed particles");
    let mut body = String::from("battle::show(effect, 28, battle::owner());");
    // Replay other actors' RNG draws at the observed component boundary. This
    // tests this effect's consumption and modifiers, not whole-battle scheduling.
    assert_eq!(steps[0]["external_draws"], 0);
    for step in steps.iter().skip(1) {
        body.push_str("await battle::next_update();");
        for _ in 0..step["external_draws"].as_u64().expect("draw count") {
            body.push_str("battle::random_signed();");
        }
    }
    body.push_str("await battle::wait_ticks(ticks(60));");
    let origin: [u32; 3] = serde_json::from_value(fixture["origin_bits"].clone())?;
    let heading: u32 = serde_json::from_value(fixture["heading_bits"].clone())?;
    let mut owner = super::actor();
    owner.position = origin.map(f32::from_bits);
    owner.heading = f32::from_bits(heading);
    let mut run = super::effect_runtime::battle_from_actor(
        &body,
        definition(&source()),
        serde_json::from_value(fixture["seed"].clone())?,
        owner,
    );
    let mut frame = start(&mut run);
    let mut ids = vec![];
    let mut observations = 0;
    let mut sounds = 0;
    for update in 0..=44 {
        if update > 0 {
            frame = run.0.step(BattleInput::default())?;
        }
        for cue in &frame.cues {
            match cue {
                Cue::ParticleStarted { particle, .. } => {
                    assert_eq!(particles[ids.len()]["born"], update);
                    ids.push(*particle);
                }
                Cue::Sound {
                    actor,
                    sound,
                    priority,
                    position,
                } => {
                    assert_eq!(*actor, run.1);
                    assert_eq!(update, 0);
                    assert_eq!((sound.index, *priority), (92, 0));
                    assert_eq!(position.map(f32::to_bits), origin);
                    sounds += 1;
                }
                _ => {}
            }
        }
        if let Some(step) = steps.get(update as usize) {
            assert_eq!(run.0.random_state(), step["after"].as_u64().unwrap() as u32);
        }
        for (index, expected) in particles.iter().enumerate() {
            let observation = expected["updates"]
                .as_array()
                .expect("particle updates")
                .iter()
                .find(|row| row["update"] == update);
            if let Some(observation) = observation {
                let actual = frame
                    .particles
                    .iter()
                    .find(|p| p.id == ids[index])
                    .expect("live particle");
                assert_eq!(
                    u64::from(actual.member),
                    expected["member"].as_u64().unwrap()
                );
                assert_eq!(i64::from(actual.age), observation["age"].as_i64().unwrap());
                assert_eq!(actual.origin.map(f32::to_bits), origin);
                assert_eq!(actual.heading.to_bits(), heading);
                assert_eq!(actual.draw_after, None);
                let state: ParticleState = serde_json::from_value(observation["state"].clone())?;
                assert_eq!(actual.state, state, "particle {index} at update {update}");
                let bits = |s: &ParticleState| {
                    let mut vectors = vec![
                        s.offset,
                        s.velocity,
                        s.acceleration,
                        s.angles,
                        s.angular_velocity,
                        s.orbit,
                    ];
                    match s.geometry {
                        ParticleGeometry::Size {
                            value,
                            velocity,
                            acceleration,
                        } => {
                            vectors.extend([value, velocity, acceleration]);
                        }
                        ParticleGeometry::Ribbon {
                            length,
                            width,
                            jitter,
                            ..
                        } => {
                            vectors.push([length, width, jitter]);
                        }
                        _ => panic!("unexpected Lightning geometry"),
                    }
                    vectors
                        .into_iter()
                        .map(|v| v.map(f32::to_bits))
                        .collect::<Vec<_>>()
                };
                assert_eq!(bits(&actual.state), bits(&state));
                observations += 1;
            }
        }
    }
    assert_eq!((ids.len(), observations, sounds), (14, 366, 1));
    assert!(frame.particles.is_empty());
    Ok(())
}

#[test]
fn effect_integer_cells_wrap_as_halfwords_and_remain_private_to_each_effect() {
    let mut source = source();
    source.records = vec![
        source.records[0], // Original sound dependency.
        Record {
            age: 0,
            command: 43,
            argument: 0,
            operand: 1,
        },
        Record {
            age: 2,
            command: 43,
            argument: 0,
            operand: 2,
        },
        Record {
            age: 3,
            command: 254,
            argument: 0,
            operand: 0,
        },
    ];
    source.modifiers = BTreeMap::from([
        (
            1,
            vec![
                12, 0x13, 0x7ffc, 0, // All four signed scratch cells begin at zero.
                12, 0x13, 0x7ffd, 0, 12, 0x13, 0x7ffe, 0, 12, 0x13, 0x7fff, 0, 0, 0x7ffc, 32759, 0,
                3, 0x7ffc, 8, 0, 3, 0x7ffc, 1, 0, 0, 0x7ffd, 0x7ffc, 0, 3, 0x7ffd, 0xffff, 0,
                0xffff,
            ],
        ),
        (
            2,
            vec![
                10, 0x13, 0x7ffc, 0, 12, 0x13, 0x7ffd, 0, 12, 0x12, 0xfffe, 0, 0xffff,
            ],
        ),
    ]);
    let mut run = battle(
        "battle::show(effect, 28, battle::owner()); await battle::wait_ticks(ticks(1)); battle::show(effect, 28, battle::owner()); battle::finish();",
        definition(&source),
        1,
    );
    let mut frame = start(&mut run);
    let mut phases = vec![];
    for update in 0..4 {
        if update > 0 {
            frame = run.0.step(BattleInput::default()).unwrap();
        }
        for particle in frame.particles.iter().filter(|p| p.age == 0) {
            let ParticleGeometry::Ribbon { phase, .. } = particle.state.geometry else {
                panic!("ribbon")
            };
            phases.push((particle.state.geometry_count, phase));
        }
    }
    assert_eq!(phases, [(4, 6), (4, 6), (2, 255), (2, 255)]);
    assert_eq!(run.0.random_state(), 1);
}

#[test]
fn phase_modifiers_reject_other_geometry_and_unprepared_float_selectors() {
    let mut source = source();
    for (particle, words) in [
        (44, vec![10, 0x13, 1, 0, 0xffff]),
        (43, vec![12, 0x13, 0x7ff8, 0, 0xffff]),
        (43, vec![0, 0x7ffc, 0x7ffb, 0, 0xffff]),
        (43, vec![3, 0x7ffb, 1, 0, 0xffff]),
        (43, vec![8, 0x35, 0, 0, 0, 0, 0xffff]),
    ] {
        source.records = vec![
            Record {
                age: 0,
                command: particle,
                argument: 0,
                operand: 1,
            },
            Record {
                age: 1,
                command: 254,
                argument: 0,
                operand: 0,
            },
        ];
        source.modifiers.insert(1, words);
        assert!(
            effect_program::prepare(&source, 77, 28, &mut super::effect_runtime::no_sound).is_err()
        );
    }
}
