use super::*;
use resonance_battle::ParticleGeometry;
use resonance_content::battle_effect::{ProgramSource, Record, declaration::Declaration};
use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
struct LocalParticles {
    records: Vec<Record>,
    declarations: BTreeMap<u8, Declaration>,
    modifiers: BTreeMap<u16, Vec<u16>>,
}

#[test]
fn ray_thrust_local_velocity_modifiers_keep_each_particles_original_motion_and_rng() -> Result<()> {
    // Original member30's two repeated local-motion emissions, actors135/136.
    // The cold martial test separately prepares the complete program/model bank.
    let fixture: LocalParticles =
        serde_json::from_str(include_str!("../fixtures/ray-thrust-local-particles.json"))?;
    let particles = fixture
        .declarations
        .iter()
        .map(|(&id, declaration)| {
            assert_eq!(declaration.prefix.kind, 5);
            let particle = declaration.particle(&[])?;
            assert!(particle.linear_orbit);
            Ok((id, particle))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let source = ProgramSource {
        records: fixture.records,
        particles,
        modifiers: fixture.modifiers,
        models: Default::default(),
    };
    let definition = Arc::new(battle::effect_program::prepare(
        &source,
        77,
        30,
        &mut super::effect_runtime::no_sound,
    )?);
    let (mut active, id) = super::effect_runtime::battle(
        "battle::show(effect, 30, battle::owner()); battle::finish();",
        [(30, definition)].into(),
        1,
    );
    let mut random = 1_u32;
    let mut draw = |divisor| {
        random = random.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687);
        (i32::from((random >> 16) as i16) % divisor) as f32 * 0.1
    };
    let rays: Vec<_> = (0..2).map(|_| [draw(1800), draw(2000), draw(80)]).collect();
    let sparks: Vec<_> = (0..4)
        .map(|_| [draw(120), draw(120), draw(40), draw(40)])
        .collect();
    let first = active.step(BattleInput {
        actions: vec![ActionRequest {
            actor: id,
            target: id,
            action: 1,
        }],
        ..Default::default()
    })?;
    assert_eq!(
        first.particles.iter().map(|p| p.member).collect::<Vec<_>>(),
        [135, 135, 136, 136, 136, 136]
    );
    for (particle, expected) in first.particles[..2].iter().zip(&rays) {
        assert_eq!(particle.state.orbit, [0., 128. + (18. + expected[2]), 0.]);
        assert_eq!(particle.state.angles[2], expected[0]);
        let ParticleGeometry::Size { value, .. } = particle.state.geometry else {
            panic!("ray geometry")
        };
        assert_eq!(value, [400. + expected[1], 4., 0.]);
    }
    for (particle, expected) in first.particles[2..].iter().zip(&sparks) {
        assert_eq!(particle.state.orbit, [expected[0], expected[1], 0.]);
        let ParticleGeometry::Size { value, .. } = particle.state.geometry else {
            panic!("spark geometry")
        };
        assert_eq!(value, [80. + expected[2], 80. + expected[3], 0.]);
    }
    assert_eq!(active.random_state(), random);
    let second = active.step(BattleInput::default())?;
    let velocities = rays
        .iter()
        .map(|ray| [0., 18. + ray[2], 0.])
        .chain(sparks.iter().map(|spark| [spark[0], spark[1], 0.]));
    for ((before, after), velocity) in first
        .particles
        .iter()
        .zip(&second.particles)
        .zip(velocities)
    {
        assert_eq!(after.id, before.id);
        for (axis, velocity) in velocity.into_iter().enumerate() {
            assert_eq!(after.state.orbit[axis], before.state.orbit[axis] + velocity);
        }
    }
    assert_eq!(active.random_state(), random);
    // Runtime modifiers are per instance, never mutations of the shared template.
    assert_eq!(source.particles[&135].orbit_velocity, [0., 18., 0.]);
    assert_eq!(source.particles[&136].orbit_velocity, [0.; 3]);
    Ok(())
}
