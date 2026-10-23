use super::*;
use crate::Random;
use std::sync::Arc;

fn definition() -> Arc<ProjectileDefinition> {
    Arc::new(ProjectileDefinition {
        lifetime: Some(20),
        velocity: [0., 8., 16.],
        offset: [0., 32., 90.],
        motion: ProjectileMotion {
            velocity_jitter: [4., 4., 2.],
            ..Default::default()
        },
        ..Default::default()
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
            velocity: [0.; 3],
            heading,
            age: 0,
            contact_active: false,
            disarmed: false,
            shadow: None,
        },
        2,
    )
}

#[test]
fn jitter_is_quantized_without_activating_subunit_axes() {
    let mut definition = definition();
    let value = Arc::get_mut(&mut definition).unwrap();
    value.velocity = [0., 0., 7.];
    value.motion.velocity_jitter = [0.25, -1.5, 1.];
    let mut samples = vec![];
    for seed in [1, 7, 31] {
        let mut first = projectile(definition.clone(), 0.);
        let mut first_random = Random::new(seed);
        first
            .initialize(&mut vec![], |_, _| Ok(first_random.next_u16()), None)
            .unwrap();
        assert_eq!(first.velocity[0], 0.);
        for (axis, base, maximum) in [(1, 0., 1.4), (2, 7., 0.9)] {
            let delta = first.velocity[axis] - base;
            assert!(delta.abs() <= maximum + 0.00001);
            let tenths = delta * 10.;
            assert!((tenths - tenths.round()).abs() < 0.00001);
        }
        samples.push(first.velocity);
    }
    assert!(samples.windows(2).any(|pair| pair[0] != pair[1]));
}

#[test]
fn jitter_rotates_with_the_projectile() {
    let mut local = projectile(definition(), 0.);
    let mut rotated = projectile(definition(), 90.);
    for instance in [&mut local, &mut rotated] {
        let mut random = Random::new(7);
        instance
            .initialize(&mut vec![], |_, _| Ok(random.next_u16()), None)
            .unwrap();
    }
    crate::tests::assert_vector_close(rotated.velocity, rotate(local.velocity, 90.), 0.0001);
}
