//! Visual footprints sampled from the same vertices and pose as the body draw.
use super::*;
use bevy::mesh::{VertexAttributeValues, skinning::SkinnedMeshInverseBindposes};

pub(super) struct Geometry {
    positions: Vec<Vec3>,
    binding: Binding,
}

enum Binding {
    Rigid {
        bone: Option<usize>,
        suffix: Mat4,
    },
    Skin {
        joints: Vec<(usize, Mat4)>,
        indices: Vec<[u16; 4]>,
        weights: Vec<[f32; 4]>,
    },
}

impl Geometry {
    pub fn prepare(
        mesh: &Mesh,
        skin: Option<&SkinnedMesh>,
        inverse_bindposes: &Assets<SkinnedMeshInverseBindposes>,
        bones: &BTreeMap<Entity, usize>,
        bone: Option<usize>,
        suffix: Mat4,
    ) -> Result<Self> {
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(VertexAttributeValues::as_float3)
            .context("shadow body has no vertex positions")?
            .iter()
            .copied()
            .map(Vec3::from_array)
            .collect::<Vec<_>>();
        let binding = if let Some(skin) = skin {
            let inverse = inverse_bindposes
                .get(&skin.inverse_bindposes)
                .context("missing shadow body inverse bind poses")?;
            ensure!(
                inverse.len() == skin.joints.len(),
                "shadow skin binding differs"
            );
            let joints = skin
                .joints
                .iter()
                .zip(inverse.iter())
                .map(|(joint, inverse)| {
                    Ok((
                        *bones
                            .get(joint)
                            .context("shadow skin has an unbound bone")?,
                        *inverse,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let Some(VertexAttributeValues::Uint16x4(indices)) =
                mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
            else {
                anyhow::bail!("shadow body has no joint indices");
            };
            let Some(VertexAttributeValues::Float32x4(weights)) =
                mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
            else {
                anyhow::bail!("shadow body has no joint weights");
            };
            ensure!(
                indices.len() == positions.len() && weights.len() == positions.len(),
                "shadow skin vertex counts differ"
            );
            ensure!(
                indices
                    .iter()
                    .flatten()
                    .all(|&index| usize::from(index) < joints.len()),
                "shadow skin joint is missing"
            );
            Binding::Skin {
                joints,
                indices: indices.clone(),
                weights: weights.clone(),
            }
        } else {
            Binding::Rigid { bone, suffix }
        };
        Ok(Self { positions, binding })
    }

    pub fn radius(&self, world: Mat4, bones: &[Matrix], origin: Vec3) -> f32 {
        let mut radius = 0_f32;
        let mut include = |point: Vec3| {
            let relative = point - origin;
            radius = radius.max(relative.x.abs()).max(relative.z.abs());
        };
        match &self.binding {
            Binding::Rigid { bone, suffix } => {
                let pose = bone.map_or(Mat4::IDENTITY, |index| {
                    Mat4::from_cols_array_2d(&bones[index])
                });
                let transform = world * pose * *suffix;
                for &point in &self.positions {
                    include(transform.transform_point3(point));
                }
            }
            Binding::Skin {
                joints,
                indices,
                weights,
            } => {
                let transforms: Vec<_> = joints
                    .iter()
                    .map(|(bone, inverse)| {
                        world * Mat4::from_cols_array_2d(&bones[*bone]) * *inverse
                    })
                    .collect();
                for ((&point, indices), weights) in self.positions.iter().zip(indices).zip(weights)
                {
                    let mut position = Vec3::ZERO;
                    for (&joint, &weight) in indices.iter().zip(weights) {
                        if weight != 0. {
                            position +=
                                transforms[usize::from(joint)].transform_point3(point) * weight;
                        }
                    }
                    include(position);
                }
            }
        }
        radius
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_tracks_drawn_skin_pose_and_world_scale() {
        let geometry = Geometry {
            positions: vec![Vec3::new(2., 0., 0.)],
            binding: Binding::Skin {
                joints: vec![(0, Mat4::from_translation(Vec3::new(-2., 0., 0.)))],
                indices: vec![[0; 4]],
                weights: vec![[1., 0., 0., 0.]],
            },
        };
        let origin = Vec3::new(100., 0., -20.);
        let world = Mat4::from_translation(origin) * Mat4::from_scale(Vec3::splat(2.));
        let pose = |x, z| [Mat4::from_translation(Vec3::new(x, 90., z)).to_cols_array_2d()];
        assert_eq!(geometry.radius(world, &pose(5., 3.), origin), 10.);
        assert_eq!(geometry.radius(world, &pose(1., 8.), origin), 16.);
    }
}
