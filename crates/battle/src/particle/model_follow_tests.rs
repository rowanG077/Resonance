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

fn effects() -> Result<Effects> {
    let model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
        resource: 41,
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
    }))?;
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
fn frame() -> BattleFrame {
    BattleFrame {
        projectiles: vec![crate::ProjectileFrame {
            id: ProjectileId(1),
            owner: ActorId(0),
            target: ActorId(1),
            position: [10., 20., 30.],
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
    let particle = effects
        .spawn_particle(
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
        )?
        .unwrap();
    effects.particles.get_mut(&particle).unwrap().follow =
        Some(EffectFollow::Projectile(ProjectileId(1)));
    Ok(particle)
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
