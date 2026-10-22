use super::*;
use crate::{EffectBank, EffectFollow, EffectModelDefinition, PreparedEffectModel, ProjectileId};
use resonance_content::{
    animation::{Bone, Skeleton, Transform, TransformChannels},
    battle_effect::SourceBank,
};

fn source() -> SourceBank {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../content/tests/fixtures/projectile-model-follow.json"
    ))
    .unwrap();
    serde_json::from_value(fixture["bank"].clone()).unwrap()
}

fn model(resource: u32) -> Result<PreparedEffectModel> {
    PreparedEffectModel::new(Arc::new(EffectModelDefinition {
        resource,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            }],
        },
        motions: Default::default(),
        secondary_motion: vec![],
    }))
}

fn effects() -> Result<Effects> {
    let model = model(41)?;
    Effects::new(
        vec![EffectBank::new(
            17,
            [(0, model.clone()), (1, model)].into(),
            Default::default(),
            &Default::default(),
        )?],
        None,
        7,
        Default::default(),
    )
}

#[test]
fn thrown_weapon_uses_current_equipment_and_retains_existing_projectiles() -> Result<()> {
    let mut effects = effects()?;
    effects.banks.get_mut(&17).unwrap().bind_weapon_models(
        0,
        ActorId(0),
        0,
        [(100, model(41)?), (200, model(42)?)].into(),
    )?;
    let mut frame = frame();
    frame.weapons.push(crate::WeaponFrame {
        owner: ActorId(0),
        slot: 0,
        visible: false,
        tint: [255; 4],
        material: Default::default(),
        resource: 100,
        clip: None,
        frame: 0.,
        world: glam::Mat4::IDENTITY.to_cols_array_2d(),
        bones: Arc::new(vec![]),
        links: vec![],
        texture_scroll: None,
    });
    effects.advance(&frame, true)?;
    let first = spawn(&mut effects, 1)?;
    frame.weapons[0].resource = 200;
    effects.advance(&frame, true)?;
    let second = spawn(&mut effects, 1)?;
    assert_eq!(
        effects.particles[&first].model.as_ref().unwrap().resource(),
        41
    );
    assert_eq!(
        effects.particles[&second]
            .model
            .as_ref()
            .unwrap()
            .resource(),
        42
    );
    frame.weapons[0].resource = 300;
    assert!(effects.advance(&frame, true).is_err());
    Ok(())
}
fn frame() -> BattleFrame {
    BattleFrame {
        projectiles: vec![crate::ProjectileFrame {
            id: ProjectileId(1),
            owner: ActorId(0),
            target: ActorId(1),
            position: [10., 20., 30.],
            velocity: [0.; 3],
            heading: 90.,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        }],
        ..Default::default()
    }
}

fn spawn(effects: &mut Effects, actor: u8) -> Result<ParticleId> {
    let source = source();
    source.program(usize::from(actor) + 1)?;
    let resonance_content::battle_effect::declaration::Declaration::ModelParticle {
        binding, ..
    } = &source.actors[usize::from(actor)]
    else {
        anyhow::bail!("fixture needs model particle")
    };
    let particle = effects.spawn_particle(
        Arc::new(ParticleDefinition {
            resource: 17,
            member: u16::from(actor),
            model: Some(*binding),
            data: source.particle(usize::from(actor))?.clone(),
        }),
        ActorId(0),
        ActorId(1),
        [10., 20., 30.],
        90.,
    )?;
    effects.particles.get_mut(&particle).unwrap().follow =
        Some(EffectFollow::Projectile(ProjectileId(1)));
    Ok(particle)
}

#[test]
fn attached_model_keeps_the_complete_joint_matrix_and_expires_with_its_parent() -> Result<()> {
    let mut effects = effects()?;
    let model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
        resource: 41,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "kk00_attachment".into(),
                parent: None,
                bind_channels: TransformChannels(4),
                bind: Transform {
                    rotation: glam::Quat::from_rotation_z(0.7).to_array(),
                    ..Default::default()
                },
            }],
        },
        motions: Default::default(),
        secondary_motion: vec![],
    }))?;
    effects.banks.get_mut(&17).unwrap().models = [(0, model.clone()), (1, model)].into();
    let parent = spawn(&mut effects, 1)?;
    let child = spawn(&mut effects, 1)?;
    for id in [parent, child] {
        let particle = effects.particles.get_mut(&id).unwrap();
        particle.follow = None;
        let data = &mut Arc::make_mut(&mut particle.definition).data;
        data.lifetime = Some(2);
        data.follow_origin = false;
        data.follow_orientation = false;
        particle.frame.state.geometry = ParticleGeometry::Size {
            value: [2., 3., 4.],
            velocity: [0.; 3],
            acceleration: [0.; 3],
        };
    }
    effects
        .particles
        .get_mut(&child)
        .unwrap()
        .frame
        .state
        .offset = [400.; 3];
    assert!(effects.attach_particle_model(child, parent, 1).is_err());
    effects.attach_particle_model(child, parent, 0)?;
    effects.advance_particles(&frame())?;
    let parent = effects.particles[&parent].frame.model.as_ref().unwrap();
    let expected = glam::Mat4::from_cols_array_2d(&parent.world)
        * glam::Mat4::from_cols_array_2d(&parent.bones[0]);
    // Nonuniform parent scale and joint rotation produce shear.
    assert!(
        expected
            .x_axis
            .truncate()
            .dot(expected.y_axis.truncate())
            .abs()
            > 0.1
    );
    assert_eq!(
        effects.particles[&child]
            .frame
            .model
            .as_ref()
            .unwrap()
            .world,
        expected.to_cols_array_2d()
    );
    effects.advance_particles(&frame())?;
    effects.advance_particles(&frame())?;
    assert!(effects.frames().is_empty());

    let source = source();
    let mut data = source.particle(1)?.clone();
    data.owner_bone = Some(0);
    data.follow_origin = false;
    data.follow_orientation = false;
    data.model_elevation = None;
    data.state.offset = [2., 3., 4.];
    data.state.angles = [0., 0., 90.];
    data.state.angular_velocity = [0., 10., 0.];
    let id = effects.spawn_particle(
        Arc::new(ParticleDefinition {
            resource: 17,
            member: 1,
            model: Some(resonance_content::battle_effect::ModelBinding {
                slot: 0,
                animation: None,
                repeat: false,
                reverse_at: None,
                program_animation: true,
                attachment: None,
            }),
            data,
        }),
        ActorId(0),
        ActorId(1),
        [400.; 3],
        90.,
    )?;
    assert_eq!(effects.particles[&id].frame.state.angles, [0., 0., 90.]);
    let mut frame = frame();
    let mut owner = crate::model::Model::new(
        Arc::new(crate::tests::model_definition(7, [(0, 10.)])),
        ActorId(0),
        &crate::tests::actor(crate::Side::Enemy),
    )?
    .shown;
    owner.bones = Arc::new(vec![expected.to_cols_array_2d()]);
    frame.models.push(owner);
    for tick in 1..=2 {
        let world = glam::Mat4::from_translation(glam::Vec3::new(tick as f32 * 20., 0., 0.));
        frame.models[0].world = world.to_cols_array_2d();
        effects.advance_particles(&frame)?;
        let local = glam::Mat4::from_scale_rotation_translation(
            glam::Vec3::ONE,
            glam::Quat::from_euler(
                glam::EulerRot::ZYX,
                90_f32.to_radians(),
                (tick as f32 * 10.).to_radians(),
                0.,
            ),
            glam::Vec3::new(2., 3., 4.),
        );
        assert_eq!(
            effects.particles[&id].frame.model.as_ref().unwrap().world,
            (world * expected * local).to_cols_array_2d(),
        );
    }
    let before = effects.particles[&id].frame.clone();
    let age = effects.particles[&id].age;
    let movement = glam::Mat4::from_translation(glam::Vec3::new(0., 7., 0.));
    frame.models[0].bones = Arc::new(vec![(movement * expected).to_cols_array_2d()]);
    effects.advance_particles_filtered(&frame, true)?;
    let after = &effects.particles[&id];
    assert_eq!(after.age, age);
    assert_eq!(after.frame.state, before.state);
    assert_eq!(
        after.frame.model.as_ref().unwrap().frame,
        before.model.as_ref().unwrap().frame
    );
    assert!(
        glam::Mat4::from_cols_array_2d(&after.frame.model.as_ref().unwrap().world).abs_diff_eq(
            movement * glam::Mat4::from_cols_array_2d(&before.model.as_ref().unwrap().world),
            0.0001,
        )
    );
    Ok(())
}

#[test]
fn followed_model_uses_origin_delta_and_keeps_rotation_and_orbit_while_stationary() -> Result<()> {
    let mut effects = effects()?;
    let mut frame = frame();
    let id = spawn(&mut effects, 1)?;
    let particle = effects.particles.get_mut(&id).unwrap();
    particle.frame.state.orbit = [2., 3., 4.];
    particle.orbit_velocity = [10.; 3];
    assert_eq!(particle.frame.state.angles, [-90., 100., 10.]);
    frame.projectiles[0].position = [13., 24., 30.];
    effects.advance_particles(&frame)?;
    let particle = &effects.particles[&id];
    assert!((particle.frame.state.angles[0] - 216.8699).abs() < 0.0001);
    assert!((particle.frame.state.angles[1] - 90.).abs() < 0.0001);
    assert_eq!(particle.frame.state.angles[2], 10.);
    assert_eq!(particle.frame.state.orbit, [2., 3., 4.]);
    assert_eq!(particle.frame.state.offset[1], 25.);
    let angles = particle.frame.state.angles;
    effects.advance_particles(&frame)?;
    assert_eq!(effects.particles[&id].frame.state.angles, angles);
    let particle = effects.particles.get_mut(&id).unwrap();
    particle.frame.state.angular_velocity = [0.; 3];
    Arc::make_mut(&mut particle.definition)
        .data
        .follow_orientation = false;
    for position in [[23., 24., 30.], [23., 34., 30.]] {
        frame.projectiles[0].position = position;
        effects.advance_particles(&frame)?;
        assert_eq!(effects.particles[&id].frame.state.angles, angles);
    }
    Arc::make_mut(&mut effects.particles.get_mut(&id).unwrap().definition)
        .data
        .follow_orientation = true;
    effects.advance_particles(&frame)?;
    assert_eq!(effects.particles[&id].frame.state.angles[..2], [225., 90.]);
    Ok(())
}

#[test]
fn model_elevation_is_applied_to_the_current_follow_position() -> Result<()> {
    let mut effects = effects()?;
    let mut frame = frame();
    let id = spawn(&mut effects, 1)?;
    Arc::make_mut(&mut effects.particles.get_mut(&id).unwrap().definition)
        .data
        .model_elevation = Some(12.);
    assert!(effects.frames().is_empty());
    effects.advance_particles(&frame)?;
    let drawn = effects.frames();
    assert_eq!(drawn.len(), 1);
    assert_eq!(
        drawn[0].model.as_ref().unwrap().world[3],
        [10., 57., 30., 1.]
    );
    frame.projectiles[0].position = [13., 24., 30.];
    effects.advance_particles(&frame)?;
    assert_eq!(
        effects.frames()[0].model.as_ref().unwrap().world[3],
        [13., 61., 30., 1.]
    );
    assert_eq!(effects.particles[&id].frame.origin, [13., 24., 30.]);
    Ok(())
}

#[test]
fn ground_model_follows_horizontal_motion_without_inheriting_height() -> Result<()> {
    let mut effects = effects()?;
    let mut frame = frame();
    let id = spawn(&mut effects, 1)?;
    Arc::make_mut(&mut effects.particles.get_mut(&id).unwrap().definition)
        .data
        .model_ground_relative = true;
    effects.advance_particles(&frame)?;
    assert_eq!(
        effects.frames()[0].model.as_ref().unwrap().world[3],
        [10., 25., 30., 1.]
    );
    frame.projectiles[0].position = [13., 84., 30.];
    effects.advance_particles(&frame)?;
    assert_eq!(
        effects.frames()[0].model.as_ref().unwrap().world[3],
        [13., 25., 30., 1.]
    );
    Ok(())
}

#[test]
fn model_choices_require_all_artwork_and_reuse_the_authored_draw() -> Result<()> {
    use resonance_content::battle_effect::{
        EffectOperation, ParticleBirth, ScheduledEvent, ValueRange,
    };
    let mut effects = effects()?;
    let mut models = effects.banks[&17].models.clone();
    // Distinct resource identities make the selected model observable.
    let skeleton = crate::tests::model_definition(42, [(0, 1.)]).skeleton;
    models.insert(
        1,
        PreparedEffectModel::new(Arc::new(EffectModelDefinition {
            resource: 42,
            skeleton,
            motions: Default::default(),
            secondary_motion: vec![],
        }))?,
    );
    let range = ValueRange {
        min: 0.,
        max: 1.,
        step: 1.,
        draw: Some(0),
        reverse: false,
    };
    let definition = Arc::new(crate::EffectDefinition {
        events: vec![ScheduledEvent {
            at: 0,
            operation: EffectOperation::Spawn {
                particle: 0,
                blend: None,
                palette: None,
                birth: Box::new(ParticleBirth {
                    random_draws: 1,
                    model: Some(range),
                    offset: [Some(range), None, None],
                    ..Default::default()
                }),
            },
        }],
        particles: [(
            0,
            Arc::new(ParticleDefinition {
                resource: 17,
                member: 0,
                model: Some(resonance_content::battle_effect::ModelBinding {
                    slot: 0,
                    animation: None,
                    repeat: false,
                    reverse_at: None,
                    program_animation: false,
                    attachment: None,
                }),
                data: source().particle(1)?.clone(),
            }),
        )]
        .into(),
        sounds: Default::default(),
    });
    let members = [(1, Arc::clone(&definition))].into();
    assert!(
        EffectBank::new(
            17,
            [(0, models[&0].clone())].into(),
            members,
            &resonance_content::diagnostics::Diagnostics::new(true)
        )
        .is_err()
    );
    effects.banks.insert(
        17,
        EffectBank::new(17, models, [(1, definition)].into(), &Default::default())?,
    );
    let mut frame = frame();
    frame.actors = [crate::Side::Party, crate::Side::Enemy]
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
        .to_vec();
    frame.cues.push(crate::Cue::Effect(crate::EffectRequest {
        owner: ActorId(0),
        target: ActorId(1),
        appearance: crate::EffectAppearance {
            resource: 17,
            member: 1,
        },
        origin: [0.; 3],
        heading: 0.,
        follow: None,
        scale: 1.,
        tint: Default::default(),
    }));
    let mut selected = std::collections::BTreeSet::new();
    for _ in 0..16 {
        effects.advance(&frame, false)?;
        let particle = effects.particles.values().last().unwrap();
        let choice = particle.frame.state.offset[0] as u32;
        assert_eq!(particle.model.as_ref().unwrap().resource(), 41 + choice);
        selected.insert(choice);
    }
    assert_eq!(selected, [0, 1].into());
    Ok(())
}

#[test]
fn program_model_clip_starts_at_zero_and_loops_without_sharing_playback() -> Result<()> {
    use resonance_content::battle_effect::{
        EffectOperation, ModelBinding, ParticleBirth, ScheduledEvent, ValueRange,
    };
    let source = crate::tests::model_definition(41, [(0, 15.), (1, 4.)]);
    let model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
        resource: 41,
        skeleton: source.skeleton,
        motions: source.motions,
        secondary_motion: vec![],
    }))?;
    let mut particle = crate::tests::particle_definition(17, 0);
    particle.data.lifetime = Some(180);
    particle.data.follow_origin = true;
    particle.data.follow_orientation = true;
    particle.data.periodic_effect = Some(resonance_content::battle_effect::PeriodicEffect {
        member: 2,
        interval: 8,
    });
    particle.data.state.offset = [0.; 3];
    particle.data.state.velocity = [0.; 3];
    particle.data.state.acceleration = [0.; 3];
    particle.data.jerk = [0.; 3];
    particle.data.state.geometry = ParticleGeometry::Size {
        value: [1.; 3],
        velocity: [0.; 3],
        acceleration: [0.; 3],
    };
    particle.model = Some(ModelBinding {
        slot: 0,
        animation: None,
        repeat: false,
        reverse_at: None,
        program_animation: true,
        attachment: None,
    });
    let definition = Arc::new(crate::EffectDefinition {
        particles: [(0, Arc::new(particle))].into(),
        sounds: Default::default(),
        events: [0, 5]
            .map(|at| ScheduledEvent {
                at,
                operation: EffectOperation::Spawn {
                    particle: 0,
                    blend: None,
                    palette: None,
                    birth: Box::new(ParticleBirth {
                        model: Some(ValueRange::fixed(0.)),
                        animation: Some(resonance_content::battle_effect::ModelAnimation {
                            frame: 0.,
                            model: 0,
                            clip: 0,
                            blend_ticks: 0,
                            rate: 0.5,
                            repeat: true,
                        }),
                        ..Default::default()
                    }),
                },
            })
            .into(),
    });
    let mut child = crate::tests::particle_definition(17, 1);
    child.data.lifetime = Some(16);
    child.data.state.offset = [0.; 3];
    child.data.state.velocity = [0.; 3];
    child.data.state.acceleration = [0.; 3];
    child.data.jerk = [0.; 3];
    let child_effect = Arc::new(crate::EffectDefinition {
        particles: [(1, Arc::new(child))].into(),
        sounds: Default::default(),
        events: vec![ScheduledEvent {
            at: 0,
            operation: EffectOperation::Spawn {
                particle: 1,
                blend: None,
                palette: None,
                birth: Default::default(),
            },
        }],
    });
    assert!(
        EffectBank::new(
            17,
            [(0, model.clone())].into(),
            [(1, definition.clone())].into(),
            &resonance_content::diagnostics::Diagnostics::new(true),
        )
        .is_err()
    );
    let mut held = (*definition).clone();
    let particle = Arc::make_mut(held.particles.get_mut(&0).unwrap());
    particle.data.follow_origin = false;
    particle.data.follow_orientation = false;
    particle.data.periodic_effect = None;
    particle.data.owner_bone = Some(0);
    let binding = particle.model.as_mut().unwrap();
    binding.animation = Some(0);
    binding.repeat = true;
    binding.program_animation = false;
    for (index, event) in held.events.iter_mut().enumerate() {
        let EffectOperation::Spawn { birth, .. } = &mut event.operation else {
            unreachable!()
        };
        let animation = birth.animation.as_mut().unwrap();
        animation.frame = 10. + index as f32;
        animation.rate = 0.;
    }
    EffectBank::new(
        17,
        [(0, model.clone())].into(),
        [(1, Arc::new(held.clone()))].into(),
        &resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    for unbound in 0..3 {
        let mut invalid = held.clone();
        if unbound == 0 {
            Arc::make_mut(invalid.particles.get_mut(&0).unwrap())
                .data
                .owner_bone = None;
        } else {
            let EffectOperation::Spawn { birth, .. } = &mut invalid.events[0].operation else {
                unreachable!()
            };
            let animation = birth.animation.as_mut().unwrap();
            if unbound == 1 {
                animation.frame = 0.;
                animation.rate = 0.5;
            } else {
                animation.repeat = false;
            }
        }
        assert!(
            EffectBank::new(
                17,
                [(0, model.clone())].into(),
                [(1, Arc::new(invalid))].into(),
                &resonance_content::diagnostics::Diagnostics::new(true),
            )
            .is_err()
        );
    }
    let mut ordinary = (*definition.particles[&0]).clone();
    ordinary.data.periodic_effect = None;
    let binding = ordinary.model.as_mut().unwrap();
    binding.animation = Some(0);
    binding.repeat = true;
    binding.program_animation = false;
    let mut delayed_orientation = (*definition).clone();
    Arc::make_mut(delayed_orientation.particles.get_mut(&0).unwrap())
        .data
        .follow_orientation = false;
    let bank = EffectBank::new(
        17,
        [(0, model)].into(),
        [
            (1, definition),
            (2, child_effect),
            (3, Arc::new(delayed_orientation)),
        ]
        .into(),
        &Default::default(),
    )?;
    let mut effects = Effects::new(vec![bank], None, 7, Default::default())?;
    let mut frame = frame();
    frame.actors = [crate::Side::Party, crate::Side::Enemy]
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
        .to_vec();
    let mut request = crate::EffectRequest {
        owner: ActorId(0),
        target: ActorId(0),
        appearance: crate::EffectAppearance {
            resource: 17,
            member: 1,
        },
        origin: [0.; 3],
        heading: 0.,
        follow: Some(EffectFollow::Projectile(ProjectileId(1))),
        scale: 1.,
        tint: Default::default(),
    };
    frame.cues.push(crate::Cue::Effect(request.clone()));
    let mut births = Vec::new();
    let mut origins = std::collections::BTreeMap::new();
    for tick in 0..=31 {
        frame.projectiles[0].position = [tick as f32 * 3., 20., 30.];
        if matches!(tick, 10 | 20 | 30) {
            frame.cues.push(crate::Cue::ProjectileRebound {
                projectile: ProjectileId(1),
                first: tick != 30,
                pitch: (tick == 20).then_some(120.),
            });
        }
        effects.advance(&frame, false)?;
        frame.cues.clear();
        for parent in effects.particles.values().filter(|p| p.model.is_some()) {
            assert_eq!(parent.definition.data.follow_orientation, tick < 10);
            if tick >= 10 {
                let (spin, pitch) = if tick < 20 {
                    (-6., 264. - 6. * (tick - 10) as f32)
                } else if tick < 30 {
                    (-6., 114. - 6. * (tick - 20) as f32)
                } else {
                    (-3., 57. - 3. * (tick - 30) as f32)
                };
                assert_eq!(parent.frame.state.angular_velocity[0], spin);
                assert!((parent.frame.state.angles[0] - pitch).abs() < 0.0001);
            }
        }
        for child in effects.particles.values().filter(|p| p.frame.member == 1) {
            assert!(child.follow.is_none());
            if child.age == 1 {
                births.push(tick);
                assert_eq!(child.frame.origin, frame.projectiles[0].position);
                origins.insert(child.frame.id, child.frame.origin);
            }
            assert_eq!(child.frame.origin, origins[&child.frame.id]);
        }
        let poses: Vec<_> = effects
            .frames()
            .iter()
            .filter_map(|p| p.model.as_ref().map(|model| model.frame))
            .collect();
        assert_eq!(poses[0], if tick <= 30 { tick as f32 * 0.5 } else { 0.5 });
        if tick >= 5 {
            assert_eq!(poses[1], (tick - 5) as f32 * 0.5);
        }
        if tick == 10 {
            effects.advance(&frame, true)?;
            assert_eq!(
                effects
                    .frames()
                    .iter()
                    .filter_map(|p| p.model.as_ref().map(|model| model.frame))
                    .collect::<Vec<_>>(),
                poses
            );
        }
    }
    assert_eq!(births, [0, 5, 8, 13, 16, 21, 24, 29]);
    frame.cues.push(crate::Cue::ProjectileDeparture {
        projectile: ProjectileId(1),
        animation: resonance_content::battle_effect::ModelAnimation {
            model: 0,
            clip: 1,
            frame: 0.,
            rate: 0.5,
            repeat: true,
            blend_ticks: 4,
        },
    });
    for expected in [0., 0., 0., 0., 0., 0.5, 1.] {
        effects.advance(&frame, false)?;
        frame.cues.clear();
        let models: Vec<_> = effects
            .frames()
            .into_iter()
            .filter_map(|p| p.model)
            .collect();
        assert_eq!(models.len(), 2);
        assert!(
            models
                .iter()
                .all(|model| model.clip == Some(1) && model.frame == expected)
        );
    }
    let projectile = frame.projectiles.pop().unwrap();
    effects.advance(&frame, false)?;
    assert!(!effects.particles.is_empty());
    assert!(effects.particles.values().all(|p| p.frame.member == 1));
    let next = effects.next_particle;
    for _ in 0..16 {
        effects.advance(&frame, false)?;
    }
    assert_eq!(effects.next_particle, next);
    assert!(effects.particles.is_empty());
    for (followed, frames) in [(true, [0., 0.5]), (false, [0.5, 1.])] {
        ordinary.data.follow_origin = followed;
        let ordinary = effects.spawn_particle(
            Arc::new(ordinary.clone()),
            ActorId(0),
            ActorId(0),
            [0.; 3],
            0.,
        )?;
        for expected in frames {
            effects.advance_particles(&frame)?;
            assert_eq!(
                effects.particles[&ordinary]
                    .frame
                    .model
                    .as_ref()
                    .unwrap()
                    .frame,
                expected
            );
        }
    }
    frame.projectiles.push(projectile);
    request.appearance.member = 3;
    let creation_origin = request.origin;
    frame.cues = vec![
        crate::Cue::ClearEffects,
        crate::Cue::InitializedEffect(request.clone()),
    ];
    effects.advance(&frame, false)?;
    let particle = effects
        .particles
        .values()
        .find(|particle| particle.model.is_some())
        .unwrap();
    assert_eq!(particle.frame.origin, frame.projectiles[0].position);
    assert_eq!(particle.orientation_origin, creation_origin);

    request.appearance.member = 1;
    frame.projectiles[0].velocity = [0., 1., 0.];
    for (velocity, expected) in [([0., -1., 0.], [360., 0.]), ([1., 0., 0.], [270., 90.])] {
        frame.cues = vec![
            crate::Cue::ClearEffects,
            crate::Cue::ProjectileStarted {
                projectile: ProjectileId(1),
                action: crate::ActionId(1),
                velocity,
            },
            crate::Cue::InitializedEffect(request.clone()),
        ];
        effects.advance(&frame, false)?;
        let particle = effects
            .particles
            .values()
            .find(|p| p.model.is_some())
            .unwrap();
        assert_eq!(particle.frame.state.angles[..2], expected);
    }
    frame.cues.clear();
    frame.projectiles[0].position[1] += 2.;
    effects.advance(&frame, false)?;
    let particle = effects
        .particles
        .values()
        .find(|p| p.model.is_some())
        .unwrap();
    assert_eq!(particle.frame.state.angles[..2], [180., 0.]);
    frame.projectiles[0].position[0] += 1.;
    frame.projectiles[0].position[1] = 0.;
    frame.projectiles[0].velocity = [0.; 3];
    frame.cues.push(crate::Cue::ProjectileStopped {
        projectile: ProjectileId(1),
    });
    effects.advance(&frame, false)?;
    let particle = effects
        .particles
        .values()
        .find(|p| p.model.is_some())
        .unwrap();
    assert_eq!(particle.frame.origin, frame.projectiles[0].position);
    assert_eq!(particle.frame.state.angles[..2], [180., 0.]);
    Ok(())
}
