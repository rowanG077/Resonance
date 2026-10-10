use crate::Actor;
use anyhow::{Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use resonance_content::{
    animation::Matrix,
    secondary_motion::{Chain, Environment, Simulation, UpAxis},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct Placement {
    pub world: Matrix,
    pub rotation: Quat,
}

impl Placement {
    pub fn actor(actor: &Actor) -> Self {
        Self {
            world: super::world(actor),
            rotation: Quat::from_rotation_y(actor.heading.to_radians())
                * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        }
    }
}

pub(super) fn apply(
    chains: &[Chain],
    simulations: &mut [Simulation],
    bones: &mut [Matrix],
    placement: Placement,
    advance: bool,
) -> Result<()> {
    if simulations.is_empty() {
        return Ok(());
    }
    let world = Mat4::from_cols_array_2d(&placement.world);
    ensure!(world.is_finite(), "invalid secondary model placement");
    let Some(inverse) = (world.determinant() != 0.)
        .then(|| world.inverse())
        .filter(|inverse| inverse.is_finite())
    else {
        // A size animation may collapse the model. Keep its authored pose and
        // restart dynamics at the new placement when it becomes visible again.
        simulations.fill_with(Simulation::default);
        return Ok(());
    };
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
            chain.attraction,
            u32::from(advance),
            Environment {
                up: UpAxis::Y,
                acceleration: Vec3::ZERO,
                floor: Some(5.),
            },
        );
        deform(
            chain,
            &authored,
            simulation.positions(),
            bones,
            placement,
            inverse,
        )?;
    }
    Ok(())
}

fn deform(
    chain: &resonance_content::secondary_motion::Chain,
    authored: &[Mat4],
    positions: &[Vec3],
    bones: &mut [Matrix],
    placement: Placement,
    inverse: Mat4,
) -> Result<()> {
    let angles = |direction: Vec3| {
        let d = placement.rotation.inverse() * direction.normalize_or_zero();
        Vec2::new((-d.y).clamp(-1., 1.).asin(), d.x.atan2(d.z))
    };
    for (index, joint) in chain.joints.iter().enumerate().take(chain.joints.len() - 1) {
        let bone = usize::from(joint.node);
        let matrix = authored[bone];
        ensure!(
            matrix.is_finite(),
            "secondary bone {bone} has an invalid transform"
        );
        let mut driven = matrix;
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
            // Rotate the authored basis rigidly, preserving its scale and shear.
            // Locks describe actor axes, so bring the correction into world space.
            let correction = placement.rotation
                * Quat::from_euler(EulerRot::ZYX, 0., delta.y, delta.x)
                * placement.rotation.inverse();
            driven = Mat4::from_quat(correction) * driven;
        }
        driven.w_axis = positions[index].extend(1.);
        // Dynamics overwrite driven joints only. Terminal guides keep the
        // composed target matrix; other children keep their ordinary pose.
        bones[bone] = (inverse * driven).to_cols_array_2d();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_targets_keep_the_composed_pose_and_authored_attraction() -> Result<()> {
        use resonance_content::secondary_motion::Joint;
        let chain = Chain {
            joints: (0..3)
                .map(|node| Joint {
                    node,
                    gravity: 0.,
                    damping: 0.,
                })
                .collect(),
            attraction: 0.03,
            preserve_rotation: true,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        let placement = Placement {
            world: Mat4::IDENTITY.to_cols_array_2d(),
            rotation: Quat::IDENTITY,
        };
        let mut simulations = vec![Simulation::default()];
        let mut expected = Simulation::default();
        for offset in [Vec3::ZERO, Vec3::X * 3., Vec3::Y * 2.] {
            let targets: Vec<_> = (0..3)
                .map(|i| Vec3::new(10., 20. + i as f32 * 8., 5.) + offset)
                .collect();
            let mut bones: Vec<_> = targets
                .iter()
                .map(|&position| Mat4::from_translation(position).to_cols_array_2d())
                .collect();
            let guide = bones[2];
            expected.advance(
                &chain,
                &targets,
                None,
                chain.attraction,
                1,
                Environment {
                    up: UpAxis::Y,
                    acceleration: Vec3::ZERO,
                    floor: Some(5.),
                },
            );
            apply(
                std::slice::from_ref(&chain),
                &mut simulations,
                &mut bones,
                placement,
                true,
            )?;
            assert_eq!(simulations[0].positions(), expected.positions());
            assert_eq!(simulations[0].positions()[0], targets[0]);
            assert_eq!(bones[2], guide);
            assert!(bones.iter().flatten().flatten().all(|v| v.is_finite()));
        }
        Ok(())
    }

    #[test]
    fn deformed_joints_follow_simulation_without_moving_guides_or_unrelated_bones() -> Result<()> {
        use resonance_content::secondary_motion::Joint;
        let mut actor = crate::tests::actor(crate::Side::Party);
        actor.position = [12., 5., -3.];
        actor.heading = 30.;
        actor.body.scale = 2.;
        let placement = Placement::actor(&actor);
        let world = Mat4::from_cols_array_2d(&placement.world);
        for scale in [Vec3::new(2., 3., 4.), Vec3::new(0., 3., 4.)] {
            let local: Vec<_> = (0..4)
                .map(|index| {
                    let mut bone = Mat4::from_scale(scale);
                    bone.y_axis.x = 0.75;
                    bone.w_axis = (Vec3::Y * index as f32 * 8.).extend(1.);
                    bone
                })
                .collect();
            let authored: Vec<_> = local.iter().map(|matrix| world * matrix).collect();
            let positions = [
                authored[0].w_axis.truncate(),
                authored[1].w_axis.truncate() + Vec3::X * 2.,
                authored[2].w_axis.truncate() + Vec3::Y * 3.,
            ];
            for preserve_rotation in [false, true] {
                let chain = Chain {
                    joints: (0..3)
                        .map(|node| Joint {
                            node,
                            gravity: 0.,
                            damping: 0.,
                        })
                        .collect(),
                    attraction: 0.,
                    preserve_rotation,
                    rotation_locks: [false; 2],
                    collision_plane: None,
                };
                let mut bones: Vec<_> = local.iter().map(Mat4::to_cols_array_2d).collect();
                deform(
                    &chain,
                    &authored,
                    &positions,
                    &mut bones,
                    placement,
                    world.inverse(),
                )?;
                for (index, bone) in bones[..2].iter().enumerate() {
                    let driven = world * Mat4::from_cols_array_2d(bone);
                    assert!(driven.is_finite());
                    let basis = glam::Mat3::from_mat4(driven);
                    let original = glam::Mat3::from_mat4(authored[index]);
                    assert!(
                        (basis.transpose() * basis)
                            .abs_diff_eq(original.transpose() * original, 0.001)
                    );
                    assert!(
                        driven
                            .w_axis
                            .truncate()
                            .abs_diff_eq(positions[index], 0.001)
                    );
                    if preserve_rotation {
                        assert!(driven.x_axis.abs_diff_eq(authored[index].x_axis, 0.001));
                        assert!(driven.y_axis.abs_diff_eq(authored[index].y_axis, 0.001));
                    }
                }
                assert_eq!(bones[2], local[2].to_cols_array_2d());
                assert_eq!(bones[3], local[3].to_cols_array_2d());
            }
        }
        Ok(())
    }
}
