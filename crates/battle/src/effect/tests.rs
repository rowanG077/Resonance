use super::*;
use resonance_content::battle_effect::*;

fn frame() -> BattleFrame {
    BattleFrame {
        actors: [crate::Side::Party, crate::Side::Enemy]
            .map(|side| {
                let state = crate::tests::actor(side);
                crate::ActorFrame {
                    effect_center: state.effect_origin(),
                    state,
                    activity: crate::Activity::Idle,
                    cast_releasing: false,
                    stun_remaining: 0,
                    stun_struggling: false,
                }
            })
            .to_vec(),
        ..Default::default()
    }
}
fn request() -> EffectRequest {
    EffectRequest {
        owner: ActorId(0),
        target: ActorId(0),
        appearance: EffectAppearance {
            resource: 77,
            member: 1,
        },
        origin: [1., 2., 3.],
        heading: 0.,
        follow: None,
        scale: 1.,
        tint: Default::default(),
    }
}
fn effects(events: Vec<ScheduledEvent>, followed: bool) -> Effects {
    let mut particle = crate::tests::particle_definition(77, 0);
    particle.data.follow_origin = followed;
    particle.data.lifetime = Some(6);
    let definition = EffectDefinition {
        events,
        particles: [(0, Arc::new(particle))].into(),
        sounds: [(1, crate::Sound::Cue(1))].into(),
    };
    Effects::new(
        vec![
            crate::EffectBank::new(
                77,
                BTreeMap::new(),
                [(1, Arc::new(definition))].into(),
                &Diagnostics::new(true),
            )
            .unwrap(),
        ],
        Some(request().appearance),
        7,
        Default::default(),
    )
    .unwrap()
}
fn emit(at: u32, birth: ParticleBirth) -> ScheduledEvent {
    ScheduledEvent {
        at,
        operation: EffectOperation::Spawn {
            particle: 0,
            blend: None,
            palette: None,
            birth: Box::new(birth),
        },
    }
}

#[test]
fn cinematic_cleanup_destroys_particles_and_delayed_births_while_held() -> Result<()> {
    let mut effects = effects(
        vec![emit(0, Default::default()), emit(4, Default::default())],
        true,
    );
    let mut frame = frame();
    frame.cues.push(Cue::Effect(request()));
    effects.advance(&frame, false)?;
    assert!(!effects.particles.is_empty());
    assert!(!effects.emitters.is_empty());
    frame.clock = crate::BattleClock::Held;
    frame.cues = vec![Cue::ClearEffects];
    effects.advance(&frame, true)?;
    assert!(effects.particles.is_empty());
    assert!(effects.emitters.is_empty());
    frame.cues.clear();
    for paused in [true, false] {
        frame.clock = if paused {
            crate::BattleClock::Held
        } else {
            crate::BattleClock::Running
        };
        for _ in 0..10 {
            assert!(effects.advance(&frame, paused)?.is_empty());
            assert!(effects.particles.is_empty());
            assert!(effects.emitters.is_empty());
        }
    }
    Ok(())
}

#[test]
fn trail_birth_sizes_and_acceleration_preserve_distinct_motion_fields() -> Result<()> {
    let mut particle = crate::tests::particle_definition(77, 0);
    particle.data.state.offset = [0.; 3];
    particle.data.state.velocity = [0.; 3];
    particle.data.state.acceleration = [0.; 3];
    particle.data.state.geometry = ParticleGeometry::BillboardTrail {
        size: [112.; 2],
        radius: 144.,
        radius_velocity: 0.,
        segment_size_step: [-5., -5., 0.],
        segment_offset: [0., 6., 0.],
        segment_angle_step: 10.,
        steps_per_segment: 1,
    };
    let particle = Arc::new(particle);
    let bank = |birth| {
        EffectBank::new(
            77,
            BTreeMap::new(),
            [(
                1,
                Arc::new(EffectDefinition {
                    events: vec![emit(0, birth)],
                    particles: [(0, particle.clone())].into(),
                    sounds: BTreeMap::new(),
                }),
            )]
            .into(),
            &Diagnostics::new(true),
        )
    };
    let birth = ParticleBirth {
        acceleration: [None, Some(ValueRange::fixed(2.)), None],
        size: [80., 96., 176.].map(|v| Some(ValueRange::fixed(v))),
        size_velocity: [None, None, Some(ValueRange::fixed(3.))],
        ..Default::default()
    };
    let mut invalid = birth.clone();
    invalid.size_velocity[0] = Some(ValueRange::fixed(1.));
    assert!(bank(invalid).is_err());
    let mut invalid = birth.clone();
    invalid.size_acceleration[2] = Some(ValueRange::fixed(1.));
    assert!(bank(invalid).is_err());
    let mut effects = Effects::new(vec![bank(birth)?], None, 7, Diagnostics::new(true))?;
    let mut frame = frame();
    frame.cues.push(Cue::Effect(request()));
    effects.advance(&frame, false)?;
    let state = &effects.frames()[0].state;
    let ParticleGeometry::BillboardTrail { size, radius, .. } = state.geometry else {
        panic!("trail geometry");
    };
    assert_eq!((size, radius), ([80., 96.], 179.));
    assert_eq!(state.acceleration, [0., 2., 0.]);
    assert_eq!(state.velocity, [0., 2., 0.]);
    frame.cues.clear();
    effects.advance(&frame, false)?;
    let state = &effects.frames()[0].state;
    assert_eq!(state.offset, [0., 2., 0.]);
    assert_eq!(state.velocity, [0., 4., 0.]);
    let ParticleGeometry::BillboardTrail { size, radius, .. } = state.geometry else {
        panic!("trail geometry");
    };
    assert_eq!((size, radius), ([80., 96.], 182.));
    Ok(())
}

#[test]
fn delayed_size_motion_targets_its_birth_before_integration_and_waits_when_paused() -> Result<()> {
    let mut particle = crate::tests::particle_definition(77, 0);
    particle.data.lifetime = Some(100);
    particle.data.state.geometry = ParticleGeometry::Size {
        value: [0., 0., 150.],
        velocity: [0.; 3],
        acceleration: [0.; 3],
    };
    let particle = Arc::new(particle);
    let mut events: Vec<_> = [(12, 32.), (20, 32.), (20, 150.), (25, 64.)]
        .into_iter()
        .map(|(at, size)| {
            emit(
                at,
                ParticleBirth {
                    size: [None, None, Some(ValueRange::fixed(size))],
                    ..Default::default()
                },
            )
        })
        .collect();
    events.push(ScheduledEvent {
        at: 58,
        operation: EffectOperation::SetMotion {
            spawn: 2,
            motion: Box::new(MotionChange {
                size: [Some(32.), Some(32.), None],
                velocity: [None, Some(-0.5), None],
                size_velocity: [None, None, Some(5.)],
                size_acceleration: [None, None, Some(1.)],
                ..Default::default()
            }),
        },
    });
    let definition = |events| {
        Arc::new(EffectDefinition {
            events,
            particles: [(0, particle.clone())].into(),
            sounds: BTreeMap::new(),
        })
    };
    let bank = EffectBank::new(
        77,
        BTreeMap::new(),
        [
            (1, definition(events)),
            (2, definition(vec![emit(0, Default::default())])),
        ]
        .into(),
        &Diagnostics::new(true),
    )?;
    let mut effects = Effects::new(vec![bank], None, 7, Diagnostics::new(true))?;
    let mut frame = frame();
    let mut other = request();
    other.appearance.member = 2;
    frame.cues = vec![Cue::Effect(request()), Cue::Effect(other)];
    for age in 0..58 {
        effects.advance(&frame, false)?;
        frame.cues.clear();
        assert_eq!(effects.emitters[0].age, age + 1);
    }
    let sizes = |effects: &Effects| {
        effects
            .particles
            .values()
            .map(|p| {
                let ParticleGeometry::Size { value, .. } = p.frame.state.geometry else {
                    panic!("size particle");
                };
                value[2]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(sizes(&effects), [150., 32., 32., 150., 64.]);
    for _ in 0..3 {
        effects.advance(&frame, true)?;
        assert_eq!(sizes(&effects), [150., 32., 32., 150., 64.]);
    }
    let offsets = |effects: &Effects| {
        effects
            .particles
            .values()
            .map(|p| p.frame.state.offset[1])
            .collect::<Vec<_>>()
    };
    let before = offsets(&effects);
    effects.advance(&frame, false)?;
    let mut expected = before.clone();
    expected[3] -= 0.5;
    assert_eq!(offsets(&effects), expected);
    assert_eq!(sizes(&effects), [150., 32., 32., 155., 64.]);
    for (index, particle) in effects.particles.values().enumerate() {
        let ParticleGeometry::Size { value, .. } = particle.frame.state.geometry else {
            panic!("size particle");
        };
        assert_eq!(value[..2], if index == 3 { [32., 32.] } else { [0., 0.] });
    }
    effects.advance(&frame, false)?;
    expected[3] -= 0.5;
    assert_eq!(offsets(&effects), expected);
    assert_eq!(sizes(&effects), [150., 32., 32., 161., 64.]);
    Ok(())
}

#[test]
fn delayed_motion_preserves_rotation_and_stops_selected_spin() -> Result<()> {
    for (heading, expected) in [(0., [-1.5, -2.4, 1.5]), (90., [1.5, -2.4, 1.5])] {
        let mut effects = effects(
            vec![
                emit(
                    0,
                    ParticleBirth {
                        velocity: [30., 30., -30.].map(|value| Some(ValueRange::fixed(value))),
                        angles: [None, Some(ValueRange::fixed(10.)), None],
                        yaw_heading: -1.,
                        angular_velocity: [
                            Some(ValueRange::fixed(7.)),
                            Some(ValueRange::fixed(3.)),
                            None,
                        ],
                        ..Default::default()
                    },
                ),
                ScheduledEvent {
                    at: 2,
                    operation: EffectOperation::SetMotion {
                        spawn: 0,
                        motion: Box::new(MotionChange {
                            angular_velocity: [Some(0.), None, None],
                            velocity_scale: Some([-0.05, -0.08, -0.05]),
                            ..Default::default()
                        }),
                    },
                },
            ],
            false,
        );
        let mut frame = frame();
        frame.cues.push(Cue::Effect(EffectRequest {
            heading,
            ..request()
        }));
        for _ in 0..3 {
            effects.advance(&frame, false)?;
            frame.cues.clear();
        }
        let particle = effects.particles.values().next().unwrap();
        assert_eq!(particle.frame.state.angular_velocity[..2], [0., 3.]);
        assert_eq!(particle.frame.state.angles[1], 19. - heading);
        for (actual, expected) in particle.frame.state.velocity.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.0001);
        }
    }
    Ok(())
}

#[test]
fn delayed_forward_motion_uses_retained_heading_and_stops_acceleration() -> Result<()> {
    let change = |at, velocity_add, acceleration, forward_motion, velocity| ScheduledEvent {
        at,
        operation: EffectOperation::SetMotion {
            spawn: 0,
            motion: Box::new(MotionChange {
                velocity,
                velocity_add,
                acceleration,
                forward_motion,
                ..Default::default()
            }),
        },
    };
    let mut effects = effects(
        vec![
            emit(
                0,
                ParticleBirth {
                    offset: [Some(ValueRange::fixed(0.)); 3],
                    velocity: [Some(ValueRange::fixed(0.)); 3],
                    acceleration: [Some(ValueRange::fixed(0.)); 3],
                    ..Default::default()
                },
            ),
            change(1, Some([0., 15., 0.]), [None; 3], None, [None; 3]),
            change(
                2,
                None,
                [None; 3],
                Some(ForwardMotion {
                    velocity: Some(11.),
                    acceleration: Some(-0.25),
                }),
                [None; 3],
            ),
            change(3, None, [Some(0.); 3], None, [Some(0.); 3]),
        ],
        false,
    );
    let mut frame = frame();
    frame.cues.push(Cue::Effect(EffectRequest {
        heading: 90.,
        ..request()
    }));
    effects.advance(&frame, false)?;
    frame.cues.clear();
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames()[0].state.offset[1], 15.);
    effects.advance(&frame, false)?;
    let before = effects.frames()[0].state.clone();
    assert!((before.offset[0] - 11.).abs() < 0.0001);
    assert!((before.velocity[0] - 10.75).abs() < 0.0001);
    assert!((before.acceleration[0] + 0.25).abs() < 0.0001);
    effects.advance(&frame, false)?;
    let after = &effects.frames()[0].state;
    assert_eq!(after.offset, before.offset);
    assert_eq!(after.velocity, [0.; 3]);
    assert_eq!(after.acceleration, [0.; 3]);
    Ok(())
}

#[test]
fn ambient_births_match_recorded_arena_clock_and_world_origin() -> Result<()> {
    let mut effects = effects(
        vec![emit(0, Default::default()), emit(5, Default::default())],
        false,
    )
    .with_ambient(Some((request().appearance, 30)))?;
    let frame = frame();
    let mut births = Vec::new();
    for tick in 1..=150 {
        let before = effects.next_particle;
        effects.advance(&frame, (60..=105).contains(&tick))?;
        if effects.next_particle != before {
            let particle = &effects.particles[&ParticleId(before)];
            assert_eq!(particle.frame.origin, [0.; 3]);
            births.push((tick, particle.age));
        }
    }
    // Arena 15 supplies the thirty-update repetition and entry hold;
    // arena 14 supplies the ordinary age-one delayed emission.
    assert_eq!(
        births,
        [
            (1, 0),
            (6, 1),
            (31, 0),
            (36, 1),
            (107, 0),
            (112, 1),
            (137, 0),
            (142, 1)
        ]
    );
    let mut paused = self::effects(
        vec![emit(0, Default::default()), emit(5, Default::default())],
        false,
    )
    .with_ambient(Some((request().appearance, 30)))?;
    let mut uninterrupted = self::effects(
        vec![emit(0, Default::default()), emit(5, Default::default())],
        false,
    )
    .with_ambient(Some((request().appearance, 30)))?;
    paused.advance(&frame, false)?;
    uninterrupted.advance(&frame, false)?;
    let mut held = frame.clone();
    held.cues.push(Cue::Casting {
        actor: ActorId(0),
        action: crate::ActionKey(0),
        phase: crate::CastPhase::SceneOpening,
    });
    paused.advance(&held, false)?;
    held.cues.clear();
    held.clock = crate::BattleClock::SpellTransition;
    for _ in 0..60 {
        paused.advance(&held, false)?;
        assert_eq!(paused.frames(), uninterrupted.frames());
    }
    for _ in 0..60 {
        paused.advance(&frame, false)?;
        uninterrupted.advance(&frame, false)?;
        assert_eq!(paused.frames(), uninterrupted.frames());
    }
    Ok(())
}

#[test]
fn action_effects_pause_births_with_the_actor_but_keep_particles_running() -> Result<()> {
    for (initialized, interrupted) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut emitter = effects(
            [0, 1, 2].map(|at| emit(at, Default::default())).into(),
            false,
        );
        let mut snapshot = frame();
        let action = crate::ActionId(7);
        snapshot.actions.push((action, ActorId(0), 0));
        snapshot.cues = vec![if initialized {
            Cue::InitializedActionEffect {
                action,
                request: request(),
            }
        } else {
            Cue::ActionEffect {
                action,
                request: request(),
            }
        }];
        emitter.advance(&snapshot, false)?;
        assert_eq!(emitter.frames().len(), 2 * usize::from(initialized));
        snapshot.cues.clear();
        emitter.advance(&snapshot, false)?;
        assert_eq!(emitter.frames().len(), 1 + 2 * usize::from(initialized));
        snapshot.cues = vec![Cue::RetireActionEffects(action)];
        if interrupted {
            snapshot.actions.clear();
            snapshot.cues = vec![Cue::Interrupted { action }];
        }
        emitter.advance(&snapshot, false)?;
        let surviving = if initialized {
            3
        } else if interrupted {
            1
        } else {
            2
        };
        assert_eq!(emitter.frames().len(), surviving);
        assert!(emitter.emitters.is_empty());
        snapshot.cues.clear();
        emitter.advance(&snapshot, false)?;
        assert_eq!(emitter.frames().len(), surviving);
    }
    let mut effects = effects(
        vec![
            emit(0, Default::default()),
            emit(1, Default::default()),
            emit(4, Default::default()),
        ],
        false,
    );
    let mut frame = frame();
    let action = crate::ActionId(7);
    frame.actions.push((action, ActorId(0), 0));
    frame.actors[0].position = [10., 20., 30.];
    let mut spawn = request();
    spawn.follow = Some(EffectFollow::Actor(ActorId(0)));
    frame.cues.push(Cue::ActionEffect {
        action,
        request: spawn,
    });
    effects.advance(&frame, false)?;
    assert!(effects.frames().is_empty());

    frame.cues.clear();
    frame.actors[0].position[0] = 100.;
    // A contact starts hit-stop after this update's actor clock already ran.
    frame.actors[0].hit_stop = 2;
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames()[0].origin, [10., 20., 30.]);
    let frozen = effects.frames();
    effects.advance(&frame, true)?;
    assert_eq!(effects.frames(), frozen);
    frame.paused_actors.push(ActorId(0));
    for remaining in [1, 0] {
        frame.actors[0].hit_stop = remaining;
        effects.advance(&frame, false)?;
        assert_eq!(effects.frames().len(), 1);
    }
    assert_eq!(effects.particles.values().next().unwrap().age, 3);
    frame.paused_actors.clear();
    frame.actors[0].position[0] = 200.;
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames().len(), 2);
    assert_eq!(effects.frames()[1].origin, [100., 20., 30.]);
    let later_particle = effects.frames()[1].id;

    frame.cues = vec![Cue::PauseActionEffects(action)];
    let age = effects.particles.values().next().unwrap().age;
    for _ in 0..2 {
        effects.advance(&frame, false)?;
        assert_eq!(effects.frames().len(), 2);
    }
    assert_eq!(effects.particles.values().next().unwrap().age, age + 2);

    frame.cues = vec![Cue::RetireActionEffects(action)];
    effects.advance(&frame, false)?;
    assert!(effects.emitters.is_empty());
    // The older particle expires naturally; retirement preserves the later birth.
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(effects.frames()[0].id, later_particle);
    let immediate = crate::ActionId(8);
    frame.actions.push((immediate, ActorId(0), 0));
    frame.cues = vec![
        Cue::ActionEffect {
            action: immediate,
            request: request(),
        },
        Cue::RetireActionEffects(immediate),
    ];
    effects.advance(&frame, false)?;
    assert!(effects.emitters.is_empty());
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(effects.frames()[0].id, later_particle);
    frame.cues.clear();
    frame.actions.clear();
    for _ in 0..10 {
        effects.advance(&frame, false)?;
    }
    assert!(effects.frames().is_empty());
    Ok(())
}

#[test]
fn action_emission_matches_recorded_hit_stop_delays() -> Result<()> {
    let mut effects = effects(
        [28, 33, 38, 43]
            .map(|at| emit(at, Default::default()))
            .to_vec(),
        false,
    );
    let mut frame = frame();
    let action = crate::ActionId(7);
    frame.actions.push((action, ActorId(0), 0));
    let mut births = Vec::new();
    // Two two-update contact holds in the recorded dive attack.
    for tick in 0..55 {
        frame.paused_actors = if matches!(tick, 23 | 24 | 32 | 33) {
            vec![ActorId(0)]
        } else {
            vec![]
        };
        frame.cues = if tick == 0 {
            vec![Cue::ActionEffect {
                action,
                request: request(),
            }]
        } else {
            vec![]
        };
        effects.advance(&frame, false)?;
        if effects.particles.values().any(|particle| particle.age == 1) {
            births.push(tick);
        }
    }
    assert_eq!(births, [31, 38, 43, 48]);
    Ok(())
}

#[test]
fn scene_effects_hold_expire_and_clear_at_combat_retirement() -> Result<()> {
    let mut effects = effects(
        vec![
            emit(0, Default::default()),
            ScheduledEvent {
                at: 2,
                operation: EffectOperation::Sound { id: 1, priority: 0 },
            },
            ScheduledEvent {
                at: 2,
                operation: EffectOperation::Shake {
                    duration: 12,
                    amplitude: 8,
                },
            },
            ScheduledEvent {
                at: 2,
                operation: EffectOperation::StageTint {
                    color: [24; 3],
                    duration: 150,
                },
            },
            ScheduledEvent {
                at: 2,
                operation: EffectOperation::Notice {
                    text: "Poisonous Pollen".into(),
                },
            },
        ],
        false,
    );
    let mut frame = frame();
    frame.cues = vec![Cue::Effect(request())];
    assert!(effects.advance(&frame, false)?.is_empty());
    let first = effects.frames();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].origin, request().origin);
    frame.cues.clear();
    for _ in 0..5 {
        assert!(effects.advance(&frame, true)?.is_empty());
        assert_eq!(effects.frames(), first);
    }
    effects.advance(&frame, false)?;
    let due = effects.advance(&frame, false)?;
    assert!(
        due.iter()
            .any(|c| matches!(c, Cue::Sound { sound, .. } if *sound == crate::Sound::Cue(1)))
    );
    assert!(due.contains(&Cue::Shake {
        duration: 12,
        amplitude: 8
    }));
    assert!(due.contains(&Cue::StageTint(crate::StageTint {
        color: [24; 3],
        duration: 150,
    })));
    assert!(due.contains(&Cue::EffectNotice {
        actor: ActorId(0),
        text: "Poisonous Pollen".into(),
        duration: 90,
    }));
    for _ in 0..3 {
        assert!(effects.advance(&frame, false)?.is_empty());
    }
    assert_eq!(effects.frames().len(), 1);
    effects.advance(&frame, false)?;
    assert!(effects.frames().is_empty());
    let mut other_bank = effects.banks[&77].clone();
    other_bank.resource = 78;
    for definition in other_bank.members.values_mut() {
        for particle in Arc::make_mut(definition).particles.values_mut() {
            Arc::make_mut(particle).resource = 78;
        }
    }
    effects.banks.insert(78, other_bank);
    let mut other_owner = request();
    other_owner.owner = ActorId(1);
    let mut other_resource = request();
    other_resource.appearance.resource = 78;
    frame.cues = vec![
        Cue::Effect(request()),
        Cue::Effect(other_owner),
        Cue::Effect(other_resource),
    ];
    effects.advance(&frame, false)?;
    frame.cues = vec![Cue::ClearEffectBank {
        actor: ActorId(0),
        resource: 77,
    }];
    effects.advance(&frame, true)?;
    assert_eq!(
        effects
            .frames()
            .iter()
            .map(|p| (p.owner, p.resource))
            .collect::<Vec<_>>(),
        [(ActorId(1), 77), (ActorId(0), 78)],
    );
    assert_eq!(
        effects
            .emitters
            .iter()
            .map(|e| (e.actor, e.resource))
            .collect::<Vec<_>>(),
        [(ActorId(1), 77), (ActorId(0), 78)],
    );
    frame.cues = vec![Cue::CombatRetired];
    effects.advance(&frame, true)?;
    assert!(effects.frames().is_empty() && effects.emitters.is_empty());
    // New result feedback can still use the same prepared assets.
    frame.cues = vec![Cue::Effect(request())];
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames().len(), 1);
    Ok(())
}

#[test]
fn bone_particles_sample_current_pose_with_explicit_following() -> Result<()> {
    use resonance_content::animation::Transform;
    for (following, static_model) in [(false, false), (true, false), (false, true), (true, true)] {
        let birth = ParticleBirth {
            origin_bone: (!following || static_model).then_some(0),
            ..Default::default()
        };
        let mut effects = effects(vec![emit(1, birth)], false);
        effects.diagnostics = Diagnostics::new(true);
        let bank = effects.banks.get_mut(&77).unwrap();
        if static_model {
            bank.models.insert(
                0,
                crate::PreparedEffectModel::new(Arc::new(crate::EffectModelDefinition {
                    resource: 77,
                    skeleton: crate::tests::model_definition(77, [(0, 1.)]).skeleton,
                    motions: Default::default(),
                    secondary_motion: vec![],
                }))?,
            );
        }
        let effect = Arc::make_mut(bank.members.get_mut(&1).unwrap());
        let particle = Arc::make_mut(effect.particles.get_mut(&0).unwrap());
        particle.model = static_model.then_some(ModelBinding {
            slot: 0,
            animation: None,
            repeat: false,
            reverse_at: None,
            program_animation: false,
            attachment: None,
        });
        particle.data.follow_bone = following.then_some(0);
        particle.data.state.offset = [0., 0., 20.];
        particle.data.state.velocity = [0.; 3];
        particle.data.state.acceleration = [0.; 3];
        particle.data.jerk = [0.; 3];
        particle.data.state.angular_velocity = [0.; 3];
        particle.data.state.orbit = [0.; 3];
        particle.data.orbit_velocity = [0.; 3];
        effect.validate(&bank.models)?;
        if following && static_model {
            let mut mismatch = effect.clone();
            let EffectOperation::Spawn { birth, .. } = &mut mismatch.events[0].operation else {
                unreachable!();
            };
            birth.origin_bone = Some(1);
            assert!(mismatch.validate(&bank.models).is_err());
        }
        let mut frame = frame();
        let mut model = crate::model::Model::new(
            Arc::new(crate::tests::model_definition(7, [(0, 10.)])),
            ActorId(0),
            &frame.actors[0].state,
        )?
        .shown;
        model.world = Transform {
            translation: [100., 10., 20.],
            ..Default::default()
        }
        .matrix();
        model.particle_world = model.world;
        frame.models.push(model);
        frame.cues.push(Cue::Effect(request()));
        effects.advance(&frame, false)?;
        assert!(effects.frames().is_empty());
        frame.cues.clear();
        Arc::make_mut(&mut frame.models[0].bones)[0] = Transform {
            translation: [2., 3., 4.],
            ..Default::default()
        }
        .matrix();
        effects.advance(&frame, false)?;
        let first = effects.frames()[0].clone();
        assert_eq!(first.origin, [102., 13., 24.]);
        assert_eq!(first.model.is_some(), static_model);
        Arc::make_mut(&mut frame.models[0].bones)[0] = Transform {
            translation: [30., 40., 50.],
            rotation: [0., 1., 0., 0.],
            scale: [2., 3., 4.],
        }
        .matrix();
        frame.models[0].world[3][0] += 7.;
        effects.advance(&frame, false)?;
        let next = &effects.frames()[0];
        assert_eq!(next.id, first.id);
        assert_eq!(
            next.origin,
            if following {
                [130., 50., 70.]
            } else {
                first.origin
            }
        );
        assert_eq!(next.state.offset, first.state.offset);
        assert_eq!(next.state.angles, first.state.angles);
        if following {
            frame.models[0].particle_world = frame.models[0].world;
            effects.advance(&frame, false)?;
            assert_eq!(effects.frames()[0].origin, [137., 50., 70.]);
        }
        assert!(effects.diagnostics.entries().is_empty());
    }
    for following in [false, true] {
        let mut effects = effects(
            vec![emit(0, Default::default()), emit(0, Default::default())],
            false,
        );
        let bank = effects.banks.get_mut(&77).unwrap();
        let effect = Arc::make_mut(bank.members.get_mut(&1).unwrap());
        let EffectOperation::Spawn { birth, .. } = &mut effect.events[1].operation else {
            unreachable!();
        };
        birth.attachment_bone = Some(1);
        assert!(effect.validate(&bank.models).is_err());
        let particle = Arc::make_mut(effect.particles.get_mut(&0).unwrap());
        particle.data.follow_bone = following.then_some(0);
        particle.data.owner_bone = (!following).then_some(0);
        effect.validate(&bank.models)?;
        let mut frame = frame();
        let mut model = crate::model::Model::new(
            Arc::new(crate::tests::model_definition(7, [(0, 10.)])),
            ActorId(0),
            &frame.actors[0].state,
        )?
        .shown;
        let bone = |x| {
            Transform {
                translation: [x, 0., 0.],
                ..Default::default()
            }
            .matrix()
        };
        model.world = Transform::default().matrix();
        model.particle_world = model.world;
        model.bones = Arc::new(vec![bone(10.), bone(20.)]);
        frame.models.push(model);
        frame.cues.push(Cue::Effect(request()));
        effects.advance(&frame, false)?;
        let first = effects.frames();
        assert_eq!(first.len(), 2);
        for (index, particle) in first.iter().enumerate() {
            assert_eq!(particle.owner_bone, (!following).then_some(index as u8));
            assert_eq!(
                particle.origin,
                if following {
                    [10. + index as f32 * 10., 0., 0.]
                } else {
                    request().origin
                }
            );
        }
        assert!(Arc::ptr_eq(
            &effects.particles[&first[0].id].definition,
            &effects.particles[&first[1].id].definition,
        ));
        frame.cues.clear();
        frame.models[0].bones = Arc::new(vec![bone(30.), bone(40.)]);
        effects.advance(&frame, false)?;
        for (index, particle) in effects.frames().iter().enumerate() {
            assert_eq!(particle.owner_bone, first[index].owner_bone);
            assert_eq!(
                particle.origin,
                if following {
                    [30. + index as f32 * 10., 0., 0.]
                } else {
                    first[index].origin
                }
            );
        }
    }
    Ok(())
}

#[test]
fn following_particles_can_use_a_fixed_world_origin() -> Result<()> {
    let mut effects = effects(
        vec![emit(0, Default::default()), emit(2, Default::default())],
        true,
    );
    effects.diagnostics = Diagnostics::new(true);
    let mut frame = frame();
    let spawn = request();
    frame.cues = vec![Cue::Effect(spawn.clone())];
    effects.advance(&frame, false)?;
    frame.cues.clear();
    frame.actors[0].position = [100., 200., 300.];
    for _ in 0..2 {
        effects.advance(&frame, false)?;
    }
    assert_eq!(effects.frames().len(), 2);
    assert!(
        effects
            .frames()
            .iter()
            .all(|particle| particle.origin == spawn.origin)
    );
    assert!(effects.diagnostics.entries().is_empty());
    Ok(())
}

#[test]
fn projectile_retirement_removes_attachments_but_finishes_independent_bursts() -> Result<()> {
    for (followed, attached) in [(false, true), (true, true), (false, false)] {
        let mut effects = effects(
            vec![
                emit(0, Default::default()),
                emit(2, Default::default()),
                emit(7, Default::default()),
            ],
            followed,
        );
        if followed {
            let bank = effects.banks.get_mut(&77).unwrap();
            let effect = Arc::make_mut(bank.members.get_mut(&1).unwrap());
            let particle = Arc::make_mut(effect.particles.get_mut(&0).unwrap());
            particle.data.follow_orientation = true;
            particle.data.state.geometry = ParticleGeometry::Quad {
                vertices: [[0.; 3]; 4],
                velocity: [[0.; 3]; 4],
            };
        }
        let mut frame = frame();
        let parent = crate::ProjectileId(1);
        frame.projectiles.push(crate::ProjectileFrame {
            id: parent,
            owner: ActorId(0),
            target: ActorId(1),
            position: [5., 6., 7.],
            velocity: [1., 1., 0.],
            heading: 0.,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        });
        let mut spawn = request();
        spawn.follow = attached.then_some(EffectFollow::Projectile(parent));
        let origin = if followed {
            frame.projectiles[0].position
        } else {
            spawn.origin
        };
        frame.cues = vec![Cue::InitializedEffect(spawn)];
        effects.advance(&frame, false)?;
        assert_eq!(effects.frames().len(), 1);
        assert_eq!(effects.particles.values().next().unwrap().age, 1);
        let first = effects.frames()[0].clone();
        assert_eq!(first.origin, origin);
        if followed {
            assert!((first.state.angles[0] + 45.).abs() < 0.001);
            assert_eq!(first.state.angles[1], 90.);
        }
        frame.cues.clear();
        frame.projectiles[0].position[0] += 10.;
        effects.advance(&frame, false)?;
        assert_eq!(
            effects.frames()[0].origin[0],
            if followed { 15. } else { origin[0] }
        );
        if followed {
            assert_eq!(effects.frames()[0].state.angles[..2], [270., 90.]);
        }
        assert_eq!(effects.frames().len(), 2);
        effects.advance(&frame, false)?;
        assert_eq!(effects.frames().len(), 2);
        frame.projectiles.clear();
        effects.advance(&frame, true)?;
        assert_eq!(effects.emitters.is_empty(), attached && followed);
        assert_eq!(effects.frames().len(), 2 * usize::from(!followed));
        let last_anchor = if attached { [15., 6., 7.] } else { origin };
        for _ in 0..5 {
            effects.advance(&frame, false)?;
        }
        assert!(effects.emitters.is_empty());
        assert_eq!(effects.frames().len(), usize::from(!followed));
        if !followed {
            assert_eq!(effects.frames()[0].origin, last_anchor);
        }
        for _ in 0..10 {
            effects.advance(&frame, false)?;
        }
        assert!(effects.frames().is_empty());
        assert!(effects.emitters.is_empty());
    }
    Ok(())
}

#[test]
fn reused_buffers_keep_birth_order_and_release_live_state() -> Result<()> {
    let birth = ParticleBirth {
        random_draws: 2,
        ..Default::default()
    };
    let mut effects = effects(vec![emit(0, birth.clone()), emit(2, birth)], false);
    let mut frame = frame();
    let requests: Vec<_> = [ActorId(0), ActorId(1)]
        .into_iter()
        .map(|owner| Cue::Effect(EffectRequest { owner, ..request() }))
        .collect();
    frame.cues.clone_from(&requests);
    effects.advance(&frame, false)?;
    for active in &effects.emitters {
        assert_eq!(active.spawned.capacity(), 2);
        assert_eq!(active.samples.capacity(), 4);
    }
    frame.cues.clear();
    for _ in 0..2 {
        effects.advance(&frame, false)?;
    }
    let mut published = Vec::new();
    effects.write_frames(&mut published);
    assert_eq!(
        published.iter().map(|p| p.owner).collect::<Vec<_>>(),
        [ActorId(0), ActorId(1), ActorId(0), ActorId(1)]
    );
    let last_id = published.last().unwrap().id;
    let emitter_capacities = (
        effects.emitters.capacity(),
        effects.spare_emitters.capacity(),
    );
    let visit_capacity = effects.particle_visits.capacity();
    let publication_capacity = published.capacity();
    for _ in 0..6 {
        effects.advance(&frame, false)?;
        effects.write_frames(&mut published);
        assert_eq!(
            (
                effects.emitters.capacity(),
                effects.spare_emitters.capacity()
            ),
            emitter_capacities
        );
        assert_eq!(effects.particle_visits.capacity(), visit_capacity);
        assert!(effects.particle_visits.is_empty());
        assert_eq!(published.capacity(), publication_capacity);
    }
    assert!(effects.emitters.is_empty() && effects.spare_emitters.is_empty());
    assert!(effects.particles.is_empty() && published.is_empty());
    frame.cues = requests;
    effects.advance(&frame, false)?;
    effects.write_frames(&mut published);
    assert_eq!(
        published.iter().map(|p| p.owner).collect::<Vec<_>>(),
        [ActorId(0), ActorId(1)]
    );
    assert!(published.iter().all(|p| p.id > last_id));
    assert_eq!(published.capacity(), publication_capacity);
    Ok(())
}

#[test]
fn birth_variation_faults_and_pool_pressure_do_not_damage_other_feedback() -> Result<()> {
    for (min, max, step, choices) in [
        (0.3, 0.7, 0.1, 5.),
        (14.1, 21.9, 0.1, 79.),
        (1.2, 1.8, 0.1, 7.),
        (0., 1., 0.3, 4.),
    ] {
        let range = ValueRange {
            min,
            max,
            step,
            draw: None,
            reverse: false,
        };
        assert_eq!(range.choice_count(), choices);
        assert_eq!(
            range.sample(1. - 0.5 / choices),
            min + (choices - 1.) * step
        );
    }
    let range = ValueRange {
        min: -10.,
        max: 10.,
        step: 0.,
        draw: Some(0),
        reverse: false,
    };
    let birth = ParticleBirth {
        random_draws: 3,
        offset: [
            Some(range),
            Some(ValueRange {
                draw: Some(1),
                ..range
            }),
            Some(range),
        ],
        angular_velocity: [
            Some(ValueRange {
                reverse: true,
                ..range
            }),
            None,
            None,
        ],
        size: [
            Some(ValueRange {
                min: 10.,
                max: 20.,
                step: 0.,
                draw: Some(2),
                reverse: false,
            }),
            Some(ValueRange {
                min: 20.,
                max: 40.,
                step: 0.,
                draw: Some(2),
                reverse: false,
            }),
            None,
        ],
        palette: Some(ValueRange::fixed(19.)),
        ..Default::default()
    };
    #[cfg(feature = "test-support")]
    {
        let carried = ParticleBirth {
            offset: [Some(range), None, None],
            ..Default::default()
        };
        let mut replay = effects(
            vec![emit(0, birth.clone()), emit(1, carried.clone())],
            false,
        );
        replay.diagnostics = Diagnostics::new(true);
        let request = request();
        replay.observe_fixture_births(&[(request.owner, request.appearance, 0)])?;
        replay.fixture_birth_inputs(vec![FixtureEffectBirth {
            owner: request.owner,
            appearance: request.appearance,
            spawn: 0,
            particle: 0,
            first_draw: 0,
            birth: birth.clone(),
            samples: vec![0.25, 0.75, 0.5],
            model_ordinal: Some(1),
            heading: None,
        }])?;
        assert!(replay.finish_fixture_birth_inputs().is_err());
        let mut frame = frame();
        frame.cues.push(Cue::Effect(request.clone()));
        replay.advance(&frame, false)?;
        replay.finish_fixture_birth_inputs()?;
        let shown = replay.frames();
        assert_eq!(shown[0].state.offset, [-5., 5., -5.]);
        assert_eq!(shown[0].state.angular_velocity[0], 5.);
        replay.fixture_birth_inputs(vec![FixtureEffectBirth {
            owner: request.owner,
            appearance: request.appearance,
            spawn: 1,
            particle: 0,
            first_draw: 3,
            birth: carried,
            samples: Vec::new(),
            model_ordinal: None,
            heading: None,
        }])?;
        frame.cues.clear();
        replay.advance(&frame, false)?;
        replay.finish_fixture_birth_inputs()?;
        assert_eq!(replay.frames().last().unwrap().state.offset[0], -5.);
        frame.cues.push(Cue::Effect(request));
        assert!(
            replay.advance(&frame, false).is_err(),
            "extra recorded birth"
        );
    }
    let mut effects = effects(vec![emit(0, birth)], false);
    let mut frame = frame();
    frame.cues = vec![Cue::Effect(request()); MAX_PARTICLES + 1];
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames().len(), MAX_PARTICLES);
    assert!(effects.diagnostics.entries().is_empty());
    let held = effects.frames();
    frame.cues.clear();
    effects.advance(&frame, true)?;
    assert_eq!(effects.frames(), held);
    for _ in 0..6 {
        effects.advance(&frame, false)?;
    }
    assert!(effects.frames().is_empty());
    frame.cues = vec![Cue::Effect(request())];
    effects.advance(&frame, false)?;
    let first = effects.frames()[0].clone();
    assert_eq!(first.state.palettes[0], 19);
    assert!(first.state.finite());
    assert_eq!(first.state.offset[0], first.state.offset[2]);
    assert_ne!(first.state.offset[0], first.state.offset[1]);
    assert_eq!(first.state.offset[0], -first.state.angular_velocity[0]);
    let ParticleGeometry::Size { value, .. } = first.state.geometry else {
        panic!("size geometry")
    };
    assert_eq!(value[1], value[0] * 2.);
    let mut invalid = request();
    invalid.scale = f32::MAX;
    frame.cues = vec![Cue::Effect(invalid)];
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(effects.frames()[0].id, first.id);
    assert_eq!(effects.diagnostics.entries().len(), 1);
    frame.cues = vec![Cue::Effect(EffectRequest {
        appearance: EffectAppearance {
            resource: 999,
            member: 1,
        },
        ..request()
    })];
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(effects.diagnostics.entries().len(), 2);
    Ok(())
}

#[test]
fn passive_groups_repeat_per_owner_pause_and_retire() -> Result<()> {
    // Authored schedules: Fire Bird, Ktugach, and the immediate-only
    // Fire Element/Shadow programs. Startup advances the emitter, not particles.
    for (schedule, interval, expected) in [
        (
            vec![(0, 0), (5, 3), (10, 6), (15, 9), (20, 12)],
            13,
            vec![
                (1, 0),
                (5, 3),
                (10, 6),
                (14, 0),
                (15, 9),
                (18, 3),
                (20, 12),
                (23, 6),
                (27, 0),
            ],
        ),
        (
            vec![(0, 0), (3, 1), (6, 2), (9, 3)],
            12,
            vec![
                (1, 0),
                (3, 1),
                (6, 2),
                (9, 3),
                (13, 0),
                (15, 1),
                (18, 2),
                (21, 3),
                (25, 0),
            ],
        ),
        (vec![(0, 0)], 61, vec![(1, 0), (62, 0), (123, 0)]),
        (vec![(0, 0)], 60, vec![(1, 0), (61, 0), (121, 0)]),
        (vec![(0, 0)], 1, vec![(1, 0), (2, 0), (3, 0)]),
    ] {
        let mut particle = crate::tests::particle_definition(77, 0);
        particle.data.state.effect_group = Some(0);
        particle.data.state.offset = [0., 30., 0.];
        particle.data.lifetime = Some(32);
        let events = schedule
            .into_iter()
            .map(|(at, group)| {
                emit(
                    at,
                    ParticleBirth {
                        effect_group: Some(group),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let definition = EffectDefinition {
            events,
            particles: [(0, Arc::new(particle))].into(),
            sounds: BTreeMap::new(),
        };
        let bank = EffectBank::new(
            77,
            BTreeMap::new(),
            [(1, Arc::new(definition))].into(),
            &Diagnostics::new(true),
        )?;
        let mut effects =
            Effects::new(vec![bank], None, 1, Diagnostics::new(true))?.with_passive(vec![
                PassiveEffect {
                    actor: ActorId(0),
                    appearance: request().appearance,
                    interval,
                    idle_only: false,
                    weapon: None,
                },
                PassiveEffect {
                    actor: ActorId(1),
                    appearance: request().appearance,
                    interval,
                    idle_only: false,
                    weapon: None,
                },
            ])?;
        let mut frame = frame();
        frame.actors[0].state.body.scale = 0.9;
        frame.actors[0].state.effect_scale = 1.;
        let mut births = [Vec::new(), Vec::new()];
        for tick in 1..=2 * u32::from(interval) + 1 {
            let before = effects.next_particle;
            effects.advance(&frame, false)?;
            for id in before..effects.next_particle {
                let particle = &effects.particles[&ParticleId(id)];
                births[particle.frame.owner.index()]
                    .push((tick, particle.frame.state.effect_group.unwrap()));
                assert_eq!(particle.age, 1);
                let expected_y = if particle.frame.owner == ActorId(0) {
                    27.
                } else {
                    30.
                };
                assert!((particle.frame.state.offset[1] - expected_y).abs() < 0.00001);
            }
            let saved = effects.frames();
            effects.advance(&frame, true)?;
            assert_eq!(saved, effects.frames());
        }
        assert_eq!(births[0], births[1]);
        assert_eq!(births[0], expected);
        // A defeated body's glow continues until the body disappears.
        // Already emitted particles finish normally.
        frame.actors[0].state.hp = 0;
        frame.actors[0].state.availability = crate::ActorAvailability::Dead;
        let model = crate::model::Model::new(
            Arc::new(crate::tests::model_definition(7, [(0, 10.)])),
            ActorId(0),
            &frame.actors[0],
        )?;
        frame.models.push(model.render_frame(&frame.actors[0]));
        if interval == 1 {
            for _ in 0..3 {
                let before = effects.next_particle;
                effects.advance(&frame, false)?;
                assert_eq!(effects.next_particle - before, 2);
            }
        }
        frame.models[0].visible = false;
        if interval == 1 {
            let before = effects.next_particle;
            effects.advance(&frame, false)?;
            assert_eq!(effects.next_particle - before, 2);
            let before = effects.next_particle;
            effects.advance(&frame, false)?;
            assert_eq!(effects.next_particle - before, 1);
        }
        for _ in 0..52_u16.div_ceil(interval) * interval {
            effects.advance(&frame, false)?;
        }
        assert!(!effects.frames().is_empty());
        assert!(
            effects
                .frames()
                .iter()
                .all(|particle| particle.owner == ActorId(1))
        );
        frame.cues.push(Cue::CombatRetired);
        effects.advance(&frame, false)?;
        frame.cues.clear();
        for _ in 0..26 {
            effects.advance(&frame, false)?;
        }
        assert!(effects.frames().is_empty() && effects.emitters.is_empty());
    }
    {
        let mut idle = effects(vec![emit(0, Default::default())], false).with_passive(vec![
            PassiveEffect {
                actor: ActorId(0),
                appearance: request().appearance,
                interval: 2,
                idle_only: true,
                weapon: None,
            },
        ])?;
        let initial = idle.next_particle;
        let mut frame = frame();
        idle.advance(&frame, false)?;
        assert_eq!(idle.next_particle, initial);
        frame.combat_active = true;
        idle.advance(&frame, false)?;
        assert_eq!(idle.next_particle, initial + 1);
        frame.actors[0].activity = crate::Activity::Action;
        for _ in 0..4 {
            idle.advance(&frame, false)?;
        }
        assert_eq!(idle.next_particle, initial + 1);
        frame.actors[0].activity = crate::Activity::Idle;
        let model = crate::model::Model::new(
            Arc::new(crate::tests::model_definition(7, [(0, 10.)])),
            ActorId(0),
            &frame.actors[0],
        )?;
        let mut body = model.render_frame(&frame.actors[0]);
        body.blend_weight = 0.5;
        frame.models.push(body);
        idle.advance(&frame, false)?;
        assert_eq!(idle.next_particle, initial + 1);
        frame.models[0].blend_weight = 1.;
        idle.advance(&frame, false)?;
        assert_eq!(idle.next_particle, initial + 2);
        idle.advance(&frame, false)?;
        assert_eq!(idle.next_particle, initial + 2);
        idle.advance(&frame, false)?;
        assert_eq!(idle.next_particle, initial + 3);
    }
    let mut effects = effects(
        vec![emit(
            0,
            ParticleBirth {
                origin_bone: Some(0),
                ..Default::default()
            },
        )],
        false,
    )
    .with_passive(vec![PassiveEffect {
        actor: ActorId(0),
        appearance: request().appearance,
        interval: 2,
        idle_only: false,
        weapon: Some(100),
    }])?;
    let mut frame = frame();
    frame.weapons.push(crate::WeaponFrame {
        owner: ActorId(0),
        slot: 0,
        visible: true,
        resource: 100,
        tint: [255; 4],
        material: Default::default(),
        clip: None,
        frame: 0.,
        world: glam::Mat4::from_translation(glam::Vec3::new(20., 0., 0.)).to_cols_array_2d(),
        bones: Arc::new(vec![
            glam::Mat4::from_translation(glam::Vec3::new(0., 3., 0.)).to_cols_array_2d(),
        ]),
        links: vec![],
        texture_scroll: None,
    });
    for _ in 0..2 {
        effects.advance(&frame, false)?;
        assert!(effects.frames().is_empty());
    }
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(effects.frames()[0].origin, [20., 3., 0.]);
    frame.weapons[0].world[3][0] = 40.;
    frame.weapons[0].visible = false;
    effects.advance(&frame, false)?;
    assert_eq!(effects.frames()[0].origin, [20., 3., 0.]);
    for _ in 0..8 {
        effects.advance(&frame, false)?;
    }
    assert!(effects.frames().is_empty());
    frame.weapons[0].visible = true;
    for _ in 0..2 {
        effects.advance(&frame, false)?;
    }
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(effects.frames()[0].origin, [40., 3., 0.]);
    frame.cues.push(Cue::CombatRetired);
    effects.advance(&frame, false)?;
    frame.cues.clear();
    for _ in 0..8 {
        effects.advance(&frame, false)?;
    }
    assert!(effects.frames().is_empty());
    Ok(())
}

#[test]
fn floor_bounces_emit_prepared_children_once_per_crossing() -> Result<()> {
    let mut parent = crate::tests::particle_definition(77, 0);
    parent.data.ground_restitution = Some(0.65);
    parent.data.bounce_effect = Some(2);
    parent.data.state.offset = [0., -3., 0.];
    parent.data.state.velocity = [0., -2., 0.];
    parent.data.state.acceleration = [0.; 3];
    parent.data.jerk = [0.; 3];
    let mut child = crate::tests::particle_definition(77, 1);
    child.data.state.offset = [0., 1., 0.];
    child.data.state.velocity = [0.; 3];
    child.data.state.acceleration = [0.; 3];
    let definition = |particle, data| {
        Arc::new(EffectDefinition {
            events: vec![ScheduledEvent {
                at: 0,
                operation: EffectOperation::Spawn {
                    particle,
                    blend: None,
                    palette: None,
                    birth: Default::default(),
                },
            }],
            particles: [(particle, Arc::new(data))].into(),
            sounds: BTreeMap::new(),
        })
    };
    let parent = definition(0, parent);
    assert!(
        EffectBank::new(
            77,
            BTreeMap::new(),
            [(1, parent.clone())].into(),
            &Diagnostics::new(true)
        )
        .is_err()
    );
    let bank = EffectBank::new(
        77,
        BTreeMap::new(),
        [(1, parent), (2, definition(1, child))].into(),
        &Diagnostics::new(true),
    )?;
    let mut effects = Effects::new(vec![bank], None, 7, Diagnostics::new(true))?;
    let frame = frame();
    effects.start(&request(), &frame, None)?;
    effects.advance(&frame, false)?;
    assert_eq!(effects.particles.len(), 2);
    let parent_id = crate::ParticleId(1);
    let parent = &effects.particles[&parent_id];
    assert_eq!(parent.frame.origin[1] + parent.frame.state.offset[1], 0.);
    assert_eq!(parent.frame.state.velocity[1], 1.3);
    let child = &effects.particles[&crate::ParticleId(2)];
    assert_eq!(child.frame.origin, [1., 0., 3.]);
    assert_eq!(child.age, 1);
    effects.advance(&frame, true)?;
    effects.advance(&frame, false)?;
    assert_eq!(effects.particles.len(), 2);
    effects
        .particles
        .get_mut(&parent_id)
        .unwrap()
        .frame
        .state
        .offset[1] = -3.;
    effects
        .particles
        .get_mut(&parent_id)
        .unwrap()
        .frame
        .state
        .velocity[1] = -1.;
    effects.advance(&frame, false)?;
    assert_eq!(effects.particles.len(), 3);
    Ok(())
}

#[test]
fn holds_visit_only_pause_enabled_effects_and_menus_freeze_everything() -> Result<()> {
    let particles: BTreeMap<_, _> = [false, true]
        .into_iter()
        .enumerate()
        .map(|(member, during_pause)| {
            let mut particle = crate::tests::particle_definition(77, member as u16);
            particle.data.lifetime = Some(100);
            particle.data.during_pause = during_pause;
            (member as u8, Arc::new(particle))
        })
        .collect();
    let definition = Arc::new(EffectDefinition {
        events: [0, 4]
            .into_iter()
            .flat_map(|at| {
                [0, 1].into_iter().map(move |particle| ScheduledEvent {
                    at,
                    operation: EffectOperation::Spawn {
                        particle,
                        blend: None,
                        palette: None,
                        birth: Default::default(),
                    },
                })
            })
            .collect(),
        particles,
        sounds: BTreeMap::new(),
    });
    let bank = EffectBank::new(
        77,
        BTreeMap::new(),
        [(1, definition)].into(),
        &Diagnostics::new(true),
    )?;
    let mut effects =
        Effects::new(vec![bank], None, 1, Diagnostics::new(true))?.with_passive(vec![
            PassiveEffect {
                actor: ActorId(1),
                appearance: request().appearance,
                interval: 20,
                idle_only: false,
                weapon: None,
            },
        ])?;
    let mut frame = frame();
    frame.cues.push(Cue::Effect(request()));
    effects.advance(&frame, false)?;
    let ordinary_age = effects.emitters[0].age;
    let passive_remaining = effects.passive[0].remaining;
    frame.unison = Some(crate::UnisonFrame {
        generation: 1,
        phase: crate::UnisonPhase::Windup,
        initiator: ActorId(0),
        target: Some(ActorId(1)),
        elapsed: 1,
        blocked_slots: 0,
        selected: vec![],
        selected_actions: vec![],
        hidden: vec![],
        stage_color: None,
        completion: None,
        combination: None,
        feedback: None,
    });
    frame.cues = vec![Cue::EffectDuringPause(request())];
    effects.advance(&frame, false)?;
    frame.cues.clear();
    assert_eq!(effects.emitters[0].age, ordinary_age);
    assert_eq!(effects.passive[0].remaining, passive_remaining);
    let ages = |effects: &Effects| {
        effects
            .particles
            .values()
            .map(|p| p.age)
            .collect::<Vec<_>>()
    };
    // The ordinary particle is held; its already-born exempt sibling still runs.
    // The cinematic emitter's ordinary birth remains invisible until gameplay resumes.
    assert_eq!(ages(&effects), [1, 2, 1, 2, 0, 1]);
    frame.unison.as_mut().unwrap().phase = crate::UnisonPhase::Selecting;
    let frozen = ages(&effects);
    effects.advance(&frame, false)?;
    assert_eq!(ages(&effects), frozen);
    frame.unison.as_mut().unwrap().phase = crate::UnisonPhase::CombinationPose;
    effects.advance(&frame, false)?;
    assert_eq!(ages(&effects), [1, 3, 1, 3, 0, 2]);
    effects.advance(&frame, true)?;
    assert_eq!(ages(&effects), [1, 3, 1, 3, 0, 2]);
    frame.unison = None;
    effects.advance(&frame, false)?;
    assert_eq!(ages(&effects), [2, 4, 2, 4, 1, 3]);
    assert_eq!(effects.emitters[0].age, ordinary_age + 1);
    assert_eq!(effects.passive[0].remaining, passive_remaining - 1);
    frame.clock = crate::BattleClock::Held;
    let frozen = ages(&effects);
    effects.advance(&frame, true)?;
    assert_eq!(ages(&effects), frozen);
    let ordinary_age = effects.emitters[0].age;
    let passive_remaining = effects.passive[0].remaining;
    let births = effects.particles.len();
    for clock in [
        crate::BattleClock::TimedHold,
        crate::BattleClock::Rescue(ActorId(0)),
    ] {
        frame.clock = clock;
        let before: Vec<_> = effects
            .particles
            .iter()
            .map(|(&id, particle)| (id, particle.age, particle.definition.data.during_pause))
            .collect();
        effects.advance(&frame, true)?;
        for (id, age, advances) in before {
            assert_eq!(effects.particles[&id].age, age + u32::from(advances));
        }
        assert_eq!(effects.emitters[0].age, ordinary_age);
        assert_eq!(effects.passive[0].remaining, passive_remaining);
    }
    assert!(effects.particles.len() > births);
    frame.clock = crate::BattleClock::Running;
    frame.cues = vec![
        Cue::ClearEffects,
        Cue::Casting {
            actor: ActorId(0),
            action: crate::ActionKey(0),
            phase: crate::CastPhase::SceneOpening,
        },
        Cue::InitializedEffect(request()),
    ];
    effects.advance(&frame, false)?;
    assert_eq!(effects.emitters[0].age, 2);
    assert_eq!(ages(&effects), [0, 1]);
    assert_eq!(effects.passive[0].remaining, passive_remaining);
    frame.cues.clear();
    frame.clock = crate::BattleClock::SpellTransition;
    for _ in 0..4 {
        effects.advance(&frame, false)?;
        assert!(
            effects
                .particles
                .values()
                .all(|particle| { particle.definition.data.during_pause || particle.age == 0 })
        );
    }
    assert_eq!(effects.particles.len(), 4);
    assert_eq!(effects.passive[0].remaining, passive_remaining);
    frame.clock = crate::BattleClock::Running;
    effects.advance(&frame, false)?;
    assert!(effects.particles.values().all(|particle| particle.age > 0));

    let ordinary: Vec<_> = effects
        .particles
        .iter()
        .filter(|(_, particle)| !particle.definition.data.during_pause)
        .map(|(&id, _)| id)
        .collect();
    let ordinary_ages = |effects: &Effects| {
        ordinary
            .iter()
            .map(|id| effects.particles[id].age)
            .collect::<Vec<_>>()
    };
    let frozen = ordinary_ages(&effects);
    frame.cues = vec![Cue::HoldEffects(3)];
    effects.advance(&frame, false)?;
    frame.cues.clear();
    effects.advance(&frame, true)?;
    for _ in 0..2 {
        effects.advance(&frame, false)?;
        assert_eq!(ordinary_ages(&effects), frozen);
    }
    effects.advance(&frame, false)?;
    assert_eq!(
        ordinary_ages(&effects),
        frozen.iter().map(|age| age + 1).collect::<Vec<_>>()
    );

    let mut delayed = self::effects(
        vec![
            emit(0, Default::default()),
            emit(2, Default::default()),
            emit(4, Default::default()),
        ],
        false,
    )
    .with_passive(
        (0..2)
            .map(|actor| PassiveEffect {
                actor: ActorId(actor),
                appearance: request().appearance,
                interval: 100,
                idle_only: false,
                weapon: None,
            })
            .collect(),
    )?;
    for passive in &mut delayed.passive {
        passive.remaining = 0;
    }
    delayed.advance(&frame, false)?;
    frame.cues = vec![Cue::Casting {
        actor: ActorId(0),
        action: crate::ActionKey(0),
        phase: crate::CastPhase::SceneOpening,
    }];
    delayed.advance(&frame, false)?;
    assert_eq!(
        delayed
            .passive
            .iter()
            .map(|p| p.remaining)
            .collect::<Vec<_>>(),
        [98, 99]
    );
    frame.cues.clear();
    frame.clock = crate::BattleClock::SpellTransition;
    for _ in 0..20 {
        delayed.advance(&frame, false)?;
    }
    assert_eq!(
        delayed.emitters.iter().map(|p| p.age).collect::<Vec<_>>(),
        [2, 2]
    );
    assert_eq!(
        delayed
            .particles
            .values()
            .map(|p| p.age)
            .collect::<Vec<_>>(),
        [1, 1]
    );
    frame.clock = crate::BattleClock::Running;
    for count in [4, 4, 6] {
        delayed.advance(&frame, false)?;
        assert_eq!(delayed.particles.len(), count);
    }
    Ok(())
}
