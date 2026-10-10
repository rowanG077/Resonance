use super::*;
use resonance_content::battle_effect::*;

fn frame() -> BattleFrame {
    BattleFrame {
        actors: [crate::Side::Party, crate::Side::Enemy]
            .map(|side| crate::ActorFrame {
                state: crate::tests::actor(side),
                activity: crate::Activity::Idle,
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
        ],
        false,
    );
    let mut frame = frame();
    frame.actors[0].state.position = [30., 20., 50.];
    frame.actors[0].state.body.collider = Some(crate::Collider::standing(30., 160.));
    frame.actors[0].state.body.scale = 2.;
    frame.cues = vec![Cue::PoisonPulse { actor: ActorId(0) }];
    assert!(effects.advance(&frame, false)?.is_empty());
    let first = effects.frames();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].origin, [30., 340., 50.]);
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
    for _ in 0..8 {
        assert!(effects.advance(&frame, false)?.is_empty());
    }
    assert!(effects.frames().is_empty());
    frame.cues = vec![Cue::Effect(request())];
    effects.advance(&frame, false)?;
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
fn projectile_retirement_removes_attachments_and_pending_emission_only() -> Result<()> {
    for followed in [false, true] {
        let mut effects = effects(
            vec![emit(0, Default::default()), emit(4, Default::default())],
            followed,
        );
        let mut frame = frame();
        let parent = crate::ProjectileId(1);
        frame.projectiles.push(crate::ProjectileFrame {
            id: parent,
            owner: ActorId(0),
            target: ActorId(1),
            position: [5., 6., 7.],
            heading: 0.,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        });
        let mut spawn = request();
        spawn.follow = Some(EffectFollow::Projectile(parent));
        frame.cues = vec![Cue::Effect(spawn)];
        effects.advance(&frame, false)?;
        let first = effects.frames()[0].clone();
        assert_eq!(first.origin, frame.projectiles[0].position);
        frame.cues.clear();
        frame.projectiles[0].position[0] += 10.;
        effects.advance(&frame, false)?;
        assert_eq!(
            effects.frames()[0].origin[0],
            if followed { 15. } else { 5. }
        );
        frame.projectiles.clear();
        effects.advance(&frame, true)?;
        assert!(effects.emitters.is_empty());
        assert_eq!(effects.frames().len(), usize::from(!followed));
        for _ in 0..10 {
            effects.advance(&frame, false)?;
        }
        assert!(effects.frames().is_empty());
    }
    Ok(())
}

#[test]
fn birth_variation_faults_and_pool_pressure_do_not_damage_other_feedback() -> Result<()> {
    let range = ValueRange {
        min: -10.,
        max: 10.,
        step: 0.,
    };
    let birth = ParticleBirth {
        offset: [Some(range), Some(range), None],
        size: [
            Some(ValueRange {
                min: 10.,
                max: 20.,
                step: 0.,
            }),
            Some(ValueRange {
                min: 20.,
                max: 40.,
                step: 0.,
            }),
            None,
        ],
        palette: Some(ValueRange::fixed(19.)),
        ..Default::default()
    };
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
