use super::*;
use crate::{Side, effect::Follow, tests::actor};
use resonance_content::battle_effect::declaration::Declaration;

#[test]
fn projectile_followed_particles_are_removed_with_the_parent_without_reusing_handles() -> Result<()>
{
    use crate::{ProjectileDefinition, ProjectileFrame, ProjectileId, projectile::Projectile};
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/nurse-followed-particle.json"
    ))?;
    let declaration: Declaration = serde_json::from_value(fixture["declaration"].clone())?;
    let mut battle = Battle::new(crate::tests::prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        100,
    ));
    let owner = ActorId(0);
    let projectile = ProjectileId(1);
    let definition = Arc::new(ProjectileDefinition {
        motion: Default::default(),
        effects: Default::default(),
        lifetime: 1,
        velocity: [0.; 3],
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: false,
        active: None,
        birth: None,
        contact: None,
    });
    let mut frame = ProjectileFrame {
        shadow: None,
        id: projectile,
        owner,
        target: owner,
        position: [5., 6., 7.],
        heading: 0.,
        age: 0,
        contact_active: false,
        disarmed: false,
    };
    battle.projectiles.insert(
        projectile,
        Projectile::new(definition.clone(), ActionId(1), frame.clone()),
    );
    let particle = battle
        .spawn_particle(
            Arc::new(ParticleDefinition {
                model: None,
                resource: 1,
                member: 61,
                data: declaration.particle(&[])?,
            }),
            Some(ActionId(2)),
            owner,
            owner,
            [0.; 3],
            0.,
        )?
        .unwrap();
    battle.particles.get_mut(&particle).unwrap().follow = Some(Follow::Projectile(projectile));
    let mut cues = Vec::new();
    battle.advance_particles(true, &mut cues)?;
    assert_eq!(battle.particles[&particle].frame.origin, [5., 6., 7.]);
    battle
        .projectiles
        .get_mut(&projectile)
        .unwrap()
        .frame
        .position = [8., 9., 10.];
    battle.advance_particles(true, &mut cues)?;
    assert_eq!(battle.particles[&particle].frame.origin, [8., 9., 10.]);
    battle.expire_projectile(projectile, &mut cues);
    assert!(!battle.particles.contains_key(&particle));
    assert!(cues.contains(&Cue::ParticleExpired { particle }));
    frame.id = ProjectileId(2);
    battle
        .projectiles
        .insert(frame.id, Projectile::new(definition, ActionId(3), frame));
    battle.advance_particles(true, &mut cues)?;
    assert!(!battle.particles.contains_key(&particle));
    Ok(())
}

#[test]
fn nurse_transition_particle_matches_original_motion_colors_origin_and_lifetime() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/nurse-followed-particle.json"
    ))?;
    let declaration: Declaration = serde_json::from_value(fixture["declaration"].clone())?;
    let data = declaration.particle(&[])?;
    assert!(data.follow_origin && data.late && data.draw_after_target);
    let visits = fixture["visits"].as_array().unwrap();
    let bytes = |row: &serde_json::Value| -> Vec<u8> {
        let hex = row["data"].as_str().unwrap();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    };
    let words = |data: &[u8], at| -> [u32; 3] {
        std::array::from_fn(|i| {
            u32::from_be_bytes(data[at + 4 * i..at + 4 * i + 4].try_into().unwrap())
        })
    };
    let before = bytes(&visits[0]["before"]);
    let origin = words(&before, 328).map(f32::from_bits);
    let heading = f32::from_be_bytes(before[344..348].try_into()?);
    let mut battle = Battle::new(crate::tests::prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        100,
    ));
    battle.actors[0].position = origin;
    let id = battle
        .spawn_particle(
            Arc::new(ParticleDefinition {
                model: None,
                resource: 1,
                member: 61,
                data,
            }),
            Some(ActionId(1)),
            ActorId(0),
            ActorId(0),
            origin,
            heading,
        )?
        .unwrap();
    battle.particles.get_mut(&id).unwrap().follow = Some(Follow::Actor(ActorId(0)));
    let mut cues = Vec::new();
    for visit in visits {
        battle.advance_particles(true, &mut cues)?;
        let frame = &battle.particles[&id].frame;
        let after = bytes(&visit["after"]);
        assert_eq!(
            frame.age,
            visit["combat_tick"].as_i64().unwrap() as i16 - 325
        );
        for (actual, offset) in [
            (frame.origin, 328),
            (frame.state.offset, 52),
            (frame.state.velocity, 64),
            (frame.state.acceleration, 76),
            (frame.state.angles, 88),
            (frame.state.angular_velocity, 100),
            (frame.state.orbit, 152),
        ] {
            assert_eq!(
                actual.map(f32::to_bits),
                words(&after, offset),
                "age {} field {offset}",
                frame.age
            );
        }
        let ParticleGeometry::Size {
            value,
            velocity,
            acceleration,
        } = frame.state.geometry
        else {
            panic!("expected size geometry");
        };
        for (actual, offset) in [(value, 176), (velocity, 188), (acceleration, 200)] {
            assert_eq!(actual.map(f32::to_bits), words(&after, offset));
        }
        for color in 0..2 {
            for component in 0..4 {
                let at = 24 + 8 * color + 2 * component;
                assert_eq!(
                    frame.state.colors[color][component],
                    i16::from_be_bytes(after[at..at + 2].try_into()?)
                );
            }
        }
        assert_eq!(visit["random_before"], visit["random_after"]);
    }
    assert_eq!(visits.len(), 36);
    assert!(battle.particles[&id].retiring);
    battle.advance_particles(true, &mut cues)?;
    assert!(battle.particles.is_empty());
    assert_eq!(
        cues,
        [
            Cue::ParticleStarted {
                particle: id,
                action: Some(ActionId(1))
            },
            Cue::ParticleExpired { particle: id }
        ]
    );
    Ok(())
}
