use super::*;

fn fixture() -> Result<(Effects, BattleFrame)> {
    let effects = Effects::new(vec![], None, 7, Default::default())?
        .with_stun_particle(Some(Arc::new(crate::tests::particle_definition(77, 19))))?
        .with_head_anchors(vec![crate::HeadAnchor {
            bone: 0,
            offset: [0., 100., 0.],
        }]);
    let mut actor = crate::tests::actor(crate::Side::Enemy);
    actor.position = [10., 0., 20.];
    actor.body.collider = Some(crate::Collider::standing(20., 100.));
    let mut world = glam::Mat4::IDENTITY.to_cols_array_2d();
    world[3] = [10., 0., 20., 1.];
    let frame = BattleFrame {
        actors: vec![crate::ActorFrame {
            effect_center: actor.effect_origin(),
            state: actor,
            activity: crate::Activity::Stunned,
            cast_releasing: false,
            stun_remaining: 180,
            stun_struggling: false,
        }],
        models: vec![crate::ModelFrame {
            actor: ActorId(0),
            visible: true,
            depth_write: true,
            tint: [255; 4],
            material: Default::default(),
            texture_layers: [0; 4],
            texture_tick: 0,
            light: None,
            shadow: None,
            resource: crate::ResourceId(1),
            clip: 0,
            frame: 0.,
            blend_weight: 1.,
            root_translation: [0.; 3],
            world,
            particle_world: world,
            bones: Arc::new(vec![glam::Mat4::IDENTITY.to_cols_array_2d()]),
        }],
        ..Default::default()
    };
    Ok((effects, frame))
}

#[test]
fn stun_birds_follow_actor_and_controller_countdown_without_restarting() -> Result<()> {
    let (mut effects, mut frame) = fixture()?;
    let mut id = None;
    for (remaining, birds) in [
        (180, 4),
        (135, 4),
        (90, 4),
        (89, 2),
        (45, 2),
        (44, 1),
        (1, 1),
    ] {
        frame.actors[0].stun_remaining = remaining;
        frame.actors[0].position[0] += 3.;
        frame.models[0].world[3][0] = frame.actors[0].position[0];
        effects.advance(&frame, false)?;
        let particles = effects.frames();
        assert_eq!(particles.len(), 1);
        let particle = &particles[0];
        assert_eq!(particle.member, 19);
        assert_eq!(particle.state.geometry_count, birds);
        assert_eq!(particle.origin, [frame.actors[0].position[0], 100., 20.]);
        assert_eq!(particle.id, *id.get_or_insert(particle.id));
    }
    let frozen = effects.frames();
    effects.advance(&frame, true)?;
    assert_eq!(effects.frames(), frozen);
    frame.actors[0].activity = crate::Activity::Idle;
    frame.actors[0].stun_remaining = 0;
    effects.advance(&frame, false)?;
    assert!(effects.frames().is_empty());
    Ok(())
}

#[test]
fn stun_birds_end_on_interruption_death_results_and_paused_event_clear() -> Result<()> {
    for case in 0..4 {
        let (mut effects, mut frame) = fixture()?;
        effects.advance(&frame, false)?;
        assert_eq!(effects.frames().len(), 1);
        match case {
            0 => frame.actors[0].activity = crate::Activity::Hurt,
            1 => frame.actors[0].hp = 0,
            2 => frame.recognized_result = Some(crate::BattleResult::Victory),
            _ => frame.cues.push(Cue::ClearEffects),
        }
        effects.advance(&frame, true)?;
        assert!(effects.frames().is_empty());
        frame.cues.clear();
        effects.advance(&frame, true)?;
        assert!(
            effects.frames().is_empty(),
            "held event must not recreate birds"
        );
    }

    let (mut effects, mut frame) = fixture()?;
    frame.cues.push(Cue::Hit {
        source: crate::ContactSource::Melee {
            actor: ActorId(0),
            action: crate::ActionId(1),
        },
        owner: ActorId(0),
        actor: ActorId(0),
        position: [0.; 3],
        element: None,
        contact_sound: None,
        suppress_guard_sound: false,
        was_casting: false,
        overlimit: false,
        stunned: true,
        repeated: false,
        result: crate::HitResult {
            amount: 1,
            hp_change: -1,
            critical: false,
            boosted: false,
            affinity: crate::Affinity::Normal,
            guard: crate::GuardResult::None,
            protection: crate::HitProtection::None,
            reaction_suppressed: false,
            reaction_forced: false,
        },
    });
    let mut previous = None;
    for _ in 0..2 {
        effects.advance(&frame, false)?;
        assert!(effects.frames().is_empty());
        assert_eq!(effects.particles.len(), 1);
        let (&id, particle) = effects.particles.first_key_value().unwrap();
        assert_eq!(particle.age, 0);
        assert_ne!(previous, Some(id));
        previous = Some(id);
    }
    frame.cues.clear();
    frame.actors[0].activity = crate::Activity::Hurt;
    effects.advance(&frame, false)?;
    assert!(effects.particles.is_empty());
    Ok(())
}

#[test]
fn paralysis_does_not_spawn_stun_birds() -> Result<()> {
    let (mut effects, mut frame) = fixture()?;
    frame.actors[0].activity = crate::Activity::Hurt;
    frame.actors[0].stun_remaining = 0;
    frame.actors[0].conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
        intrinsic: crate::conditions::Condition::Paralysis.into(),
        ..Default::default()
    });
    effects.advance(&frame, false)?;
    assert!(effects.frames().is_empty());
    Ok(())
}
