use super::*;
use glam::{EulerRot, Mat4, Quat, Vec3};
use resonance_content::secondary_motion::{Definition, Simulation};
use serde::Deserialize;

#[derive(Deserialize)]
struct ObservedClock {
    frame_bits: u32,
    state: u32,
    finished: bool,
}

#[derive(Deserialize)]
struct Observation {
    model: u32,
    tick: u32,
    before: ObservedClock,
    after: ObservedClock,
    translation_bits: [u32; 3],
    rotation_bits: [u32; 3],
    scale_bits: [u32; 3],
    chain_positions: Vec<Vec<[u32; 3]>>,
    matrices_before: Option<Vec<[u32; 12]>>,
    matrices_after: Option<Vec<[u32; 12]>>,
}

#[derive(Deserialize)]
struct Fixture {
    skeleton: Skeleton,
    secondary_motion: Definition,
    observations: Vec<Observation>,
}

fn check_clock(clock: &Clock, observed: &ObservedClock) {
    assert_eq!(clock.frame.to_bits(), observed.frame_bits);
    assert_eq!(clock.repeat, observed.state & 8 == 0);
    assert_eq!(clock.stopped, observed.state & 2 != 0);
    assert_eq!(clock.finished, observed.finished);
}

fn compare_matrices(bones: &[Matrix], world: Mat4, original: &[[u32; 12]]) -> f32 {
    assert_eq!(bones.len(), original.len());
    bones
        .iter()
        .zip(original)
        .map(|(matrix, expected)| {
            let actual = (world * Mat4::from_cols_array_2d(matrix)).to_cols_array_2d();
            (0..12)
                .map(|i| (actual[i % 4][i / 4] - f32::from_bits(expected[i])).abs())
                .fold(0., f32::max)
        })
        .fold(0., f32::max)
}

#[test]
fn nurse_scene_models_share_playback_and_match_original_poses() -> Result<()> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../tests/fixtures/nurse-scene-models.json"))?;
    let motion = Motion::decode(include_bytes!("../../tests/fixtures/nurse.motion"))?;
    fixture.skeleton.validate()?;
    motion.validate(&fixture.skeleton)?;
    let names: Vec<_> = fixture
        .skeleton
        .bones
        .iter()
        .map(|b| b.name.clone())
        .collect();
    let chains = fixture.secondary_motion.prepare(&names)?;
    let mut models = BTreeMap::new();
    let mut maximum_pose = 0f32;
    let mut maximum_chain = 0f32;
    for row in &fixture.observations {
        let (animation, simulations, tick) = match models.entry(row.model) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert((
                AnimatedPose::new(
                    &fixture.skeleton,
                    &motion,
                    Playback {
                        clip: 0,
                        frame: 0.,
                        rate: 0.5,
                        repeat: true,
                    },
                )?,
                vec![Simulation::default(); chains.len()],
                row.tick,
            )),
        };
        assert_eq!(*tick, row.tick);
        *tick += 1;
        check_clock(&animation.clock, &row.before);
        animation.advance(&fixture.skeleton, &motion)?;
        check_clock(&animation.clock, &row.after);
        let (mut pose, _) = animation.pose(&fixture.skeleton, [false; 3])?;
        let [x, y, z] = row.rotation_bits.map(|v| f32::from_bits(v).to_radians());
        let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
        let scale = Vec3::from_array(row.scale_bits.map(f32::from_bits));
        let world = Mat4::from_scale_rotation_translation(
            scale,
            rotation,
            Vec3::from_array(row.translation_bits.map(f32::from_bits)),
        );
        if let Some(expected) = &row.matrices_before {
            let error = compare_matrices(&pose.global, world, expected);
            maximum_pose = maximum_pose.max(error);
            assert!(
                error < 0.001,
                "ordinary pose {:x} tick {}: {error}",
                row.model,
                row.tick
            );
        }
        secondary::apply(
            &chains,
            &motion,
            simulations,
            &mut pose.global,
            secondary::Placement {
                world: world.to_cols_array_2d(),
                rotation,
                scale,
            },
            true,
            [0.; 3],
        )?;
        assert_eq!(simulations.len(), row.chain_positions.len());
        for (simulation, expected) in simulations.iter().zip(&row.chain_positions) {
            assert_eq!(simulation.positions().len(), expected.len());
            for (actual, expected) in simulation.positions().iter().zip(expected) {
                let error = (*actual - Vec3::from_array(expected.map(f32::from_bits)))
                    .abs()
                    .max_element();
                maximum_chain = maximum_chain.max(error);
                assert!(
                    error < 0.001,
                    "chain {:x} tick {}: {error}",
                    row.model,
                    row.tick
                );
            }
        }
        if let Some(expected) = &row.matrices_after {
            let error = compare_matrices(&pose.global, world, expected);
            maximum_pose = maximum_pose.max(error);
            assert!(
                error < 0.001,
                "driven pose {:x} tick {}: {error}",
                row.model,
                row.tick
            );
        }
    }
    assert_eq!(fixture.observations.len(), 531);
    assert_eq!(models.len(), 3);
    assert_eq!(
        fixture
            .observations
            .iter()
            .filter(|row| row.matrices_before.is_some() && row.matrices_after.is_some())
            .count(),
        21
    );
    eprintln!(
        "Nurse scene: 531 model visits; max pose error {maximum_pose}, chain error {maximum_chain}"
    );
    Ok(())
}
