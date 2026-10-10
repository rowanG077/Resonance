use super::*;

#[test]
fn elapsed_time_and_trails_continue_past_short_clocks_until_optional_retirement() {
    for lifetime in [None, Some(40_000)] {
        let definition = Arc::new(ProjectileDefinition {
            lifetime,
            velocity: [0.; 3],
            acceleration: [0.; 3],
            offset: [0.; 3],
            clamp_ground: false,
            active: Some([33_000, 40_000]),
            birth: None,
            motion: Default::default(),
            effects: ProjectileEffects {
                trail: Some((
                    EffectAppearance {
                        resource: 1,
                        member: 2,
                    },
                    10_000,
                )),
                ..Default::default()
            },
            contact: None,
        });
        definition.validate().unwrap();
        let mut invalid = (*definition).clone();
        invalid.lifetime = Some(0);
        assert!(invalid.validate().is_err());
        let mut projectile = Projectile::new(
            definition,
            ActionId(1),
            ProjectileFrame {
                id: ProjectileId(1),
                owner: ActorId(0),
                target: ActorId(1),
                position: [0.; 3],
                heading: 0.,
                age: 0,
                contact_active: false,
                disarmed: false,
                shadow: None,
            },
        );
        let mut emissions = Vec::new();
        for age in 0..=40_000 {
            projectile.step().unwrap();
            assert_eq!(projectile.frame.age, age);
            assert_eq!(projectile.retiring, lifetime.is_some_and(|end| age >= end));
            if !projectile.effects().is_empty() {
                emissions.push(age);
            }
        }
        assert_eq!(emissions, [0, 10_000, 20_000, 30_000, 40_000]);
    }
}
