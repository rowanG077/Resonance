use super::effect_runtime::{battle_from_actor, no_sound, start};
use anyhow::{Result, ensure};
use resonance_battle::{BattleInput, ParticleGeometry};
use resonance_content::battle_effect::{ProgramSource, Record, declaration::Declaration};
use resonance_game::battle::effect_program;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Deserialize)]
pub(super) struct Snapshot {
    data: String,
    age: i16,
}
impl Snapshot {
    pub(super) fn bytes(&self) -> Result<Vec<u8>> {
        (0..self.data.len())
            .step_by(2)
            .map(|i| Ok(u8::from_str_radix(&self.data[i..i + 2], 16)?))
            .collect()
    }
}
#[derive(Deserialize)]
pub(super) struct Visit {
    pub(super) combat_tick: u32,
    pub(super) random_before: u32,
    pub(super) random_after: u32,
    pub(super) before: Snapshot,
    pub(super) after: Snapshot,
}
#[derive(Deserialize)]
struct Trail {
    modifier: u16,
    visits: Vec<Visit>,
}
#[derive(Deserialize)]
struct Fixture {
    declaration: Declaration,
    modifier: Vec<u16>,
    observations: Vec<Trail>,
}

fn vector(data: &[u8], offset: usize) -> [u32; 3] {
    std::array::from_fn(|i| {
        let at = offset + i * 4;
        u32::from_be_bytes(data[at..at + 4].try_into().unwrap())
    })
}

#[test]
fn nurse_billboard_trails_match_both_original_variants_through_expiry() -> Result<()> {
    let fixture: Fixture = serde_json::from_str(include_str!("../fixtures/nurse-trails.json"))?;
    let particle = fixture.declaration.particle(&[])?;
    assert!(particle.late);
    for trail in &fixture.observations {
        let source = ProgramSource {
            models: Default::default(),
            records: vec![
                Record {
                    age: 0,
                    command: 63,
                    argument: 0,
                    operand: trail.modifier,
                },
                Record {
                    age: 1,
                    command: 254,
                    argument: 0,
                    operand: 0,
                },
            ],
            particles: BTreeMap::from([(63, particle.clone())]),
            modifiers: BTreeMap::from([(32948, fixture.modifier.clone())]),
        };
        let definition = effect_program::prepare(&source, 77, 37, &mut no_sound)?;
        let initial = trail.visits.first().expect("captured trail");
        let bytes = initial.before.bytes()?;
        let mut owner = super::actor();
        owner.position = vector(&bytes, 328).map(f32::from_bits);
        owner.heading = f32::from_be_bytes(bytes[344..348].try_into()?);
        let mut run = battle_from_actor(
            "battle::show(effect, 37, battle::owner()); battle::finish();",
            BTreeMap::from([(37, Arc::new(definition))]),
            initial.random_before,
            owner,
        );
        let mut frame = start(&mut run);
        for (age, visit) in trail.visits.iter().enumerate() {
            if age != 0 {
                frame = run.0.step(BattleInput::default())?;
            }
            ensure!(frame.particles.len() == 1, "expected one live trail");
            let actual = &frame.particles[0];
            assert_eq!(actual.age, age as i16);
            assert_eq!(actual.age, visit.before.age);
            assert_eq!(visit.combat_tick, initial.combat_tick + age as u32);
            assert_particle(actual, visit)?;
            assert_eq!(visit.random_before, visit.random_after);
            assert_eq!(run.0.random_state(), initial.random_before);
        }
        assert_eq!(trail.visits.len(), 41);
        assert!(run.0.step(BattleInput::default())?.particles.is_empty());
    }
    Ok(())
}

#[test]
fn trail_segment_steps_are_independent_of_time_motion_and_emitter_scale() -> Result<()> {
    use resonance_content::source::FloatOperand::Value;
    let mut fixture: Fixture = serde_json::from_str(include_str!("../fixtures/nurse-trails.json"))?;
    fixture
        .declaration
        .prefix
        .acceleration_change_or_segment_offset = [Value(1.), Value(2.), Value(3.)];
    fixture.declaration.prefix.acceleration = [Value(0.), Value(1.), Value(0.)];
    let source = ProgramSource {
        models: Default::default(),
        records: vec![
            Record {
                age: 0,
                command: 63,
                argument: 0,
                operand: 0,
            },
            Record {
                age: 1,
                command: 254,
                argument: 0,
                operand: 0,
            },
        ],
        particles: BTreeMap::from([(63, fixture.declaration.particle(&[])?)]),
        modifiers: BTreeMap::new(),
    };
    let definition = effect_program::prepare(&source, 77, 37, &mut no_sound)?;
    let mut run = battle_from_actor(
        "battle::show_following(effect, 37, battle::owner(), 2.0, true,
            battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
         battle::finish();",
        BTreeMap::from([(37, Arc::new(definition))]),
        1,
        super::actor(),
    );
    let mut frame = start(&mut run);
    for (age, y) in [40., 41., 43.].into_iter().enumerate() {
        if age != 0 {
            frame = run.0.step(BattleInput::default())?;
        }
        let state = &frame.particles[0].state;
        assert_eq!(state.offset, [0., y, 0.]);
        assert_eq!(state.acceleration, [0., 1., 0.]);
        let ParticleGeometry::BillboardTrail {
            size,
            radius,
            radius_velocity,
            segment_size_step,
            segment_offset,
            segment_angle_step,
            ..
        } = state.geometry
        else {
            panic!("expected billboard trail")
        };
        assert_eq!(size, [96.; 2]);
        assert_eq!(radius, 128. + 16. * (age + 1) as f32);
        assert_eq!(radius_velocity, 16.);
        assert_eq!(segment_size_step, [7., 7., 0.]);
        assert_eq!(segment_offset, [1., 2., 3.]);
        assert_eq!(segment_angle_step, 5.);
    }
    // Original offset 0x84 has a meaning only for this prepared geometry.
    let mut invalid = source;
    invalid.particles.get_mut(&63).unwrap().state.geometry = ParticleGeometry::Size {
        value: [1.; 3],
        velocity: [0.; 3],
        acceleration: [0.; 3],
    };
    invalid.records[0].operand = 32948;
    invalid.modifiers.insert(32948, fixture.modifier);
    assert!(effect_program::prepare(&invalid, 77, 37, &mut no_sound).is_err());
    Ok(())
}

pub(super) fn assert_particle(
    actual: &resonance_battle::ParticleFrame,
    visit: &Visit,
) -> Result<()> {
    assert_eq!(
        actual.origin.map(f32::to_bits),
        vector(&visit.after.bytes()?, 328)
    );
    assert_particle_state(actual, visit)
}

pub(super) fn assert_particle_state(
    actual: &resonance_battle::ParticleFrame,
    visit: &Visit,
) -> Result<()> {
    let after = visit.after.bytes()?;
    for (value, offset) in [
        (actual.state.offset, 52),
        (actual.state.velocity, 64),
        (actual.state.acceleration, 76),
        (actual.state.angles, 88),
        (actual.state.angular_velocity, 100),
        (actual.state.orbit, 152),
    ] {
        assert_eq!(
            value.map(f32::to_bits),
            vector(&after, offset),
            "tick {}, age {}, field {offset}",
            visit.combat_tick,
            actual.age
        );
    }
    match actual.state.geometry {
        ParticleGeometry::Size {
            value,
            velocity,
            acceleration,
        } => {
            for (value, offset) in [(value, 176), (velocity, 188), (acceleration, 200)] {
                assert_eq!(value.map(f32::to_bits), vector(&after, offset));
            }
        }
        ParticleGeometry::BillboardTrail {
            size,
            radius,
            radius_velocity,
            segment_size_step,
            segment_offset,
            segment_angle_step,
            steps_per_segment,
        } => {
            assert_eq!(
                [size[0], size[1], radius].map(f32::to_bits),
                vector(&after, 176)
            );
            assert_eq!(radius_velocity.to_bits(), vector(&after, 188)[2]);
            assert_eq!(segment_size_step.map(f32::to_bits), vector(&after, 200));
            assert_eq!(segment_offset.map(f32::to_bits), vector(&after, 112));
            assert_eq!(
                segment_angle_step.to_bits(),
                u32::from_be_bytes(after[132..136].try_into()?)
            );
            assert_eq!(steps_per_segment, after[19]);
        }
        _ => panic!("unexpected Nurse transition geometry"),
    }
    assert_eq!(actual.state.geometry_count, after[18]);
    for (i, color) in actual.state.colors.iter().flatten().enumerate() {
        let at = 24 + i * 2;
        assert_eq!(*color, i16::from_be_bytes(after[at..at + 2].try_into()?));
    }
    assert_eq!(actual.state.palettes, [after[3], after[4]]);
    assert_eq!(
        actual.state.cull_back,
        u32::from_be_bytes(after[20..24].try_into()?) & 0x1000_0000 != 0
    );
    assert_eq!(
        actual.heading.to_bits(),
        u32::from_be_bytes(after[344..348].try_into()?)
    );
    for (i, value) in actual.state.uv.iter().enumerate() {
        let at = 8 + i * 2;
        assert_eq!(*value, i16::from_be_bytes(after[at..at + 2].try_into()?));
    }
    Ok(())
}

#[test]
fn complete_nurse_transition_matches_original_emissions_modifiers_pauses_and_particle_tails()
-> Result<()> {
    use resonance_battle::{
        ActionDefinition, ActionPhase, ActionRequest, Battle, EffectBank, PreparedBattle,
        ResourceBinding,
    };
    use resonance_content::battle_effect::SourceBank;
    #[derive(Deserialize)]
    struct ObservedParticle {
        member: u16,
        visits: Vec<Visit>,
    }
    #[derive(Deserialize)]
    struct Transition {
        source: SourceBank,
        observations: Vec<ObservedParticle>,
    }
    let fixture: Transition =
        serde_json::from_str(include_str!("../fixtures/nurse-transition-effect.json"))?;
    let source = fixture.source.program(37)?;
    let mut invalid = source.clone();
    // This modifier can request culling, but it cannot silently enable an
    // attachment/controller that was not prepared for this generation.
    invalid.modifiers.get_mut(&32812).expect("culling modifier")[2] = 0x2000;
    assert!(effect_program::prepare(&invalid, 77, 37, &mut no_sound).is_err());
    let effect = effect_program::prepare(&source, 77, 37, &mut no_sound)?;
    let compiled = symphonia_script_compiler::compile(
        "test",
        &BTreeMap::from([(
            "test".into(),
            r#"
            script battle;
            use battle;
            asset effect: battle::Effect = "test/common";
            asset spell: battle::Spell = "test/spell";
            pub task run() {
                battle::begin_scene(spell, ticks(60));
                battle::show_centered(effect, 37, battle::owner(), 1.0, true,
                    battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
                await battle::next_update();
                while battle::scene_remaining() > ticks(0) { await battle::next_update(); }
                battle::activate_scene();
                await battle::next_update();
                battle::finish();
            }
            pub task resident() { await battle::wait_ticks(ticks(250)); }
        "#
            .into(),
        )]),
        &resonance_battle::native_declarations(),
    )?;
    let resources = compiled
        .assets
        .iter()
        .map(|a| match a.path.as_str() {
            "test/common" => ResourceBinding::Effect(77),
            "test/spell" => ResourceBinding::Spell(237),
            _ => panic!("unexpected binding"),
        })
        .collect::<Vec<_>>();
    let program = Arc::new(compiled.program);
    let actions = [
        (1, "test::run", ActionPhase::Casting),
        (237, "test::resident", ActionPhase::Resident),
    ]
    .into_iter()
    .map(|(id, name, phase)| ActionDefinition {
        id,
        phase,
        entry: program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .entry,
        program: program.clone(),
        duration: 250,
        tp_cost: 0,
        resources: resources.clone(),
    })
    .collect();
    let initial = fixture.observations[0].visits[0].before.bytes()?;
    let mut owner = super::actor();
    owner.position = [-700., 0., 150.];
    owner.body.center_offset = [0., 80., 0.];
    owner.heading = f32::from_be_bytes(initial[344..348].try_into()?);
    let mut enemy = super::actor();
    enemy.side = resonance_battle::Side::Enemy;
    let prepared = Arc::new(PreparedBattle::new(
        vec![owner, enemy],
        actions,
        1,
        vec![],
        vec![EffectBank {
            models: Default::default(),
            resource: 77,
            members: BTreeMap::from([(37, Arc::new(effect))]),
        }],
    )?);
    let owner = prepared.actor_ids().next().expect("prepared caster");
    let mut battle = Battle::new(prepared);
    let mut bindings = BTreeMap::new();
    let mut visits = 0;
    let mut effect_action = None;
    for tick in 316..=418 {
        let frame = battle.step(BattleInput {
            actions: if tick == 316 {
                vec![ActionRequest {
                    actor: owner,
                    target: owner,
                    action: 1,
                }]
            } else {
                vec![]
            },
            ..Default::default()
        })?;
        if tick == 316 {
            effect_action = frame.cues.iter().find_map(|cue| match cue {
                resonance_battle::Cue::Effect { action, .. } => Some(*action),
                _ => None,
            });
        }
        let expected = fixture
            .observations
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                p.visits
                    .iter()
                    .find(|v| v.combat_tick == tick)
                    .map(|v| (i, p.member, v))
            })
            .collect::<Vec<_>>();
        assert_eq!(frame.particles.len(), expected.len(), "tick {tick}");
        // Frames list creation order; the original trace visits ordinary and
        // late groups separately. Match each instance by member and age.
        for actual in &frame.particles {
            let &(index, _, visit) = expected
                .iter()
                .find(|(_, member, visit)| {
                    *member == actual.member && visit.before.age == actual.age
                })
                .expect("observed particle instance");
            if actual.age == 0 {
                assert!(bindings.insert(index, actual.id).is_none());
            }
            assert_eq!(bindings.get(&index), Some(&actual.id));
            assert_eq!(actual.age, visit.before.age, "tick {tick}");
            assert_particle(actual, visit)?;
            assert_eq!(visit.random_before, visit.random_after);
            visits += 1;
        }
        assert_eq!(battle.random_state(), 1);
        if tick >= 389 {
            assert!(
                !frame
                    .actions
                    .iter()
                    .any(|(id, _, _)| Some(*id) == effect_action)
            );
        }
        if tick == 386 {
            let paused = battle.step(BattleInput {
                menu_open: true,
                ..Default::default()
            })?;
            assert_eq!(paused.particles, frame.particles);
            assert_eq!(paused.update, frame.update);
        }
    }
    assert!(effect_action.is_some());
    assert_eq!(bindings.len(), 16);
    assert_eq!(visits, 491);
    Ok(())
}
