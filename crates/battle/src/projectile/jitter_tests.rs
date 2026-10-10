use super::*;
use crate::Random;
use std::sync::Arc;

fn definition() -> Arc<ProjectileDefinition> {
    Arc::new(ProjectileDefinition {
        lifetime: Some(20),
        velocity: [0., 8., 16.],
        acceleration: [0.; 3],
        offset: [0., 32., 90.],
        clamp_ground: false,
        active: None,
        birth: None,
        motion: ProjectileMotion {
            velocity_jitter: [4., 4., 2.],
            ..Default::default()
        },
        effects: Default::default(),
        contact: None,
    })
}

fn projectile(definition: Arc<ProjectileDefinition>, heading: f32) -> Projectile {
    Projectile::new(
        definition,
        ActionId(1),
        ProjectileFrame {
            id: ProjectileId(1),
            owner: ActorId(0),
            target: ActorId(1),
            position: [0.; 3],
            heading,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        },
    )
}

#[test]
fn jitter_is_deterministic_and_bounded_on_fractional_and_negative_axes() {
    let mut definition = definition();
    let value = Arc::get_mut(&mut definition).unwrap();
    value.velocity = [0.; 3];
    value.motion.velocity_jitter = [0.25, -1.5, 0.];
    let mut samples = vec![];
    for seed in [1, 7, 31] {
        let mut first = projectile(definition.clone(), 0.);
        let mut second = projectile(definition.clone(), 0.);
        first.initialize(&mut vec![], &mut Random::new(seed));
        second.initialize(&mut vec![], &mut Random::new(seed));
        assert_eq!(first.velocity, second.velocity);
        assert!(first.velocity[0].abs() <= 0.25);
        assert!(first.velocity[1].abs() <= 1.5);
        assert_eq!(first.velocity[2], 0.);
        samples.push(first.velocity);
    }
    assert!(samples.windows(2).any(|pair| pair[0] != pair[1]));
}

#[test]
fn jitter_rotates_with_the_projectile() {
    let mut local = projectile(definition(), 0.);
    let mut rotated = projectile(definition(), 90.);
    for instance in [&mut local, &mut rotated] {
        instance.initialize(&mut vec![], &mut Random::new(7));
    }
    crate::tests::assert_vector_close(rotated.velocity, rotate(local.velocity, 90.), 0.0001);
}

#[test]
fn jitter_rejects_nonfinite_values_and_accepts_fractional_amplitudes() {
    for value in [f32::NAN, f32::INFINITY, -f32::INFINITY] {
        let mut definition = definition();
        Arc::get_mut(&mut definition)
            .unwrap()
            .motion
            .velocity_jitter[0] = value;
        assert!(definition.validate().is_err());
    }
    for value in [0., 0.25, -1.5, 4.] {
        let mut definition = definition();
        Arc::get_mut(&mut definition)
            .unwrap()
            .motion
            .velocity_jitter[0] = value;
        assert!(definition.validate().is_ok());
    }
}
