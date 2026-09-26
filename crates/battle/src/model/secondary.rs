//! Original model composition runs chain dynamics after ordinary bone sampling.
use crate::Actor;
use anyhow::Result;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use resonance_content::{
    animation::{Matrix, Motion, matrix_rotation},
    secondary_motion::{Chain, Environment, Simulation, UpAxis},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct Placement {
    pub world: Matrix,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Placement {
    /// 8006C6E0 starts at the origin; 1D068/31FE8 supply -90 X and unit scale.
    /// Source watch04 P0 retains this composition until the first actor visit.
    pub fn initial() -> Self {
        Self {
            world: [
                [1., 0., 0., 0.],
                [0., 0., -1., 0.],
                [0., 1., 0., 0.],
                [0., 0., 0., 1.],
            ],
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            scale: Vec3::ONE,
        }
    }

    pub fn actor(actor: &Actor) -> Self {
        Self {
            world: super::world(actor),
            rotation: Quat::from_rotation_y(actor.heading.to_radians())
                * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(actor.body.scale),
        }
    }
}

pub(super) fn apply(
    chains: &[Chain],
    motion: &Motion,
    simulations: &mut [Simulation],
    bones: &mut [Matrix],
    placement: Placement,
    advance: bool,
    acceleration: [f32; 3],
) -> Result<()> {
    if simulations.is_empty() {
        return Ok(());
    }
    let world = Mat4::from_cols_array_2d(&placement.world);
    let authored: Vec<_> = bones
        .iter()
        .map(|matrix| world * Mat4::from_cols_array_2d(matrix))
        .collect();
    for (chain, simulation) in chains.iter().zip(simulations) {
        let targets: Vec<_> = chain
            .joints
            .iter()
            .map(|joint| authored[usize::from(joint.node)].w_axis.truncate())
            .collect();
        let animated = motion
            .tracks
            .iter()
            .any(|track| track.bone == chain.joints[0].node && track.times.len() > 2);
        let plane = chain.collision_plane.as_ref().map(|plane| {
            let matrix = authored[usize::from(plane.anchor)];
            let normal = Vec3::from_array(plane.normal);
            let tangent = normal.any_orthonormal_vector();
            let normal = matrix
                .transform_vector3(tangent)
                .cross(matrix.transform_vector3(normal.cross(tangent)))
                .normalize_or_zero();
            (normal, plane.offset, plane.strength)
        });
        simulation.advance(
            chain,
            &targets,
            plane,
            if animated { 0.2 } else { chain.attraction },
            u32::from(advance),
            Environment {
                up: UpAxis::Y,
                acceleration: Vec3::from_array(acceleration),
                floor: Some(5.),
            },
        );
        deform(chain, &authored, simulation.positions(), bones, placement)?;
    }
    Ok(())
}

fn deform(
    chain: &resonance_content::secondary_motion::Chain,
    authored: &[Mat4],
    positions: &[Vec3],
    bones: &mut [Matrix],
    placement: Placement,
) -> Result<()> {
    let inverse = Mat4::from_cols_array_2d(&placement.world).inverse();
    let angles = |direction: Vec3| {
        let d = placement.rotation.inverse() * direction.normalize_or_zero();
        Vec2::new((-d.y).clamp(-1., 1.).asin(), d.x.atan2(d.z))
    };
    for (index, joint) in chain.joints.iter().enumerate().take(chain.joints.len() - 1) {
        let bone = usize::from(joint.node);
        let mut rotation = Quat::from_array(matrix_rotation(authored[bone].to_cols_array_2d())?);
        if !chain.preserve_rotation {
            let mut delta = angles(positions[index] - positions[index + 1])
                - angles(
                    authored[usize::from(joint.node)].w_axis.truncate()
                        - authored[usize::from(chain.joints[index + 1].node)]
                            .w_axis
                            .truncate(),
                );
            if chain.rotation_locks[0] {
                delta.x = 0.;
            }
            if chain.rotation_locks[1] {
                delta.y = 0.;
            }
            rotation *= Quat::from_euler(EulerRot::ZYX, 0., delta.y, delta.x);
        }
        let driven =
            Mat4::from_scale_rotation_translation(placement.scale, rotation, positions[index]);
        // The original overwrites driven world matrices only. Terminal guides
        // and other children keep the ordinary sampled world pose.
        bones[bone] = (inverse * driven).to_cols_array_2d();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::secondary_motion::Definition;

    #[test]
    fn driven_matrices_match_original_chain_outputs() -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Sample {
            tick: u32,
            position: [f32; 3],
            heading: f32,
            scale: f32,
            after: Vec<Vec<[u32; 16]>>,
            matrices_before: Vec<[u32; 12]>,
            matrices_after: Vec<[u32; 12]>,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            definition: Definition,
            names: Vec<String>,
            samples: Vec<Sample>,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../content/src/secondary_motion/lloyd-secondary.json"
        ))?;
        let chains = fixture.definition.prepare(&fixture.names)?;
        let matrix = |bits: [u32; 12]| {
            Mat4::from_cols_array_2d(&std::array::from_fn(|column| {
                std::array::from_fn(|row| {
                    if row == 3 {
                        if column == 3 { 1. } else { 0. }
                    } else {
                        f32::from_bits(bits[row * 4 + column])
                    }
                })
            }))
        };
        let mut maximum = 0f32;
        for sample in fixture.samples {
            let mut actor = crate::tests::actor(crate::Side::Party);
            actor.position = sample.position;
            actor.heading = sample.heading;
            actor.body.scale = sample.scale;
            let world = Mat4::from_cols_array_2d(&crate::model::world(&actor));
            let authored: Vec<_> = sample.matrices_before.iter().copied().map(matrix).collect();
            let mut bones: Vec<_> = authored
                .iter()
                .map(|m| (world.inverse() * m).to_cols_array_2d())
                .collect();
            for (chain, native) in chains.iter().zip(&sample.after) {
                let positions: Vec<_> = native
                    .iter()
                    .map(|row| {
                        Vec3::new(
                            f32::from_bits(row[0]),
                            f32::from_bits(row[1]),
                            f32::from_bits(row[2]),
                        )
                    })
                    .collect();
                deform(
                    chain,
                    &authored,
                    &positions,
                    &mut bones,
                    Placement::actor(&actor),
                )?;
            }
            for (index, (&bone, &expected)) in bones.iter().zip(&sample.matrices_after).enumerate()
            {
                let actual = (world * Mat4::from_cols_array_2d(&bone)).to_cols_array();
                let expected = matrix(expected).to_cols_array();
                let error = actual
                    .into_iter()
                    .zip(expected)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0f32, f32::max);
                maximum = maximum.max(error);
                assert!(error < 0.001, "tick {} bone {index}: {error}", sample.tick);
            }
        }
        eprintln!("maximum driven matrix error: {maximum}");
        Ok(())
    }
}
