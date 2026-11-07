//! Some rigid effect models already contain an expanded, inward-facing shell.
use super::*;
use glam::Vec3;
use resonance_content::animation::Motion;

fn volume(mesh: &model::Mesh) -> f32 {
    mesh.indices
        .chunks_exact(3)
        .map(|triangle| {
            let [a, b, c] = std::array::from_fn::<_, 3, _>(|i| {
                Vec3::from_array(mesh.positions[triangle[i] as usize])
            });
            a.dot(b.cross(c))
        })
        .sum()
}

fn bounds(mesh: &model::Mesh) -> ([f32; 3], [f32; 3]) {
    (
        std::array::from_fn(|axis| {
            mesh.positions
                .iter()
                .map(|p| p[axis])
                .fold(f32::INFINITY, f32::min)
        }),
        std::array::from_fn(|axis| {
            mesh.positions
                .iter()
                .map(|p| p[axis])
                .fold(f32::NEG_INFINITY, f32::max)
        }),
    )
}

fn black_shell(mesh: &model::Mesh) -> bool {
    !mesh.colors.is_empty() && mesh.colors.iter().all(|c| c[..3] == [0.; 3] && c[3] == 1.)
}

/// Every coloured part must have its actual paired shell before dropping a whole generated layer.
pub(super) fn present(model: &model::Model) -> bool {
    covered(model, |body, shell| body.node == shell.node)
        || colored_pair(model, |body, shell| body.node == shell.node)
}

fn paired_surface(body: &model::Mesh, shell: &model::Mesh, same_frame: bool) -> bool {
    if shell.joints.is_some()
        || shell.positions.is_empty()
        || !same_frame
        || body.joints.is_some()
        || body.indices.len() != shell.indices.len()
        || !body.colors.iter().any(|c| c[..3] != [0.; 3])
    {
        return false;
    }
    let shell_volume = volume(shell);
    let (shell_min, shell_max) = bounds(shell);
    let body_volume = volume(body);
    let (body_min, body_max) = bounds(body);
    shell_volume < 0.
        && body_volume > 0.
        && (-shell_volume / body_volume) > 1.
        && (0..3).all(|axis| {
            let low = body_min[axis] - shell_min[axis];
            let high = shell_max[axis] - body_max[axis];
            (0.001..=2.).contains(&low) && (0.001..=2.).contains(&high)
        })
}

fn uniform_opaque_color(mesh: &model::Mesh) -> Option<[f32; 3]> {
    let color = *mesh.colors.first()?;
    (color[3] == 1. && mesh.colors.iter().all(|other| *other == color))
        .then_some([color[0], color[1], color[2]])
}

/// Some rigid source pairs use a uniform grey shell instead of black. Restrict
/// this extra proof to exactly two uniformly opaque meshes: one coloured body
/// and its darker expanded reverse surface, with no unpaired material parts.
fn colored_pair(
    model: &model::Model,
    same_frame: impl Fn(&model::Mesh, &model::Mesh) -> bool,
) -> bool {
    let [first, second] = model.meshes.as_slice() else {
        return false;
    };
    [(first, second), (second, first)]
        .into_iter()
        .any(|(body, shell)| {
            let (Some(body_color), Some(shell_color)) =
                (uniform_opaque_color(body), uniform_opaque_color(shell))
            else {
                return false;
            };
            (0..3).all(|channel| shell_color[channel] < body_color[channel])
                && paired_surface(body, shell, same_frame(body, shell))
        })
}

fn covered(model: &model::Model, same_frame: impl Fn(&model::Mesh, &model::Mesh) -> bool) -> bool {
    let mut bodies = model
        .meshes
        .iter()
        .filter(|mesh| !black_shell(mesh))
        .peekable();
    if bodies.peek().is_none() {
        return false;
    }
    bodies.all(|body| {
        model
            .meshes
            .iter()
            .filter(|mesh| black_shell(mesh))
            .any(|shell| paired_surface(body, shell, same_frame(body, shell)))
    })
}

fn same_frame(
    model: &model::Model,
    motions: &[Motion],
    first: Option<u16>,
    second: Option<u16>,
) -> bool {
    if first == second {
        return true;
    }
    let (Some(first), Some(second)) = (first, second) else {
        return false;
    };
    let (Some(a), Some(b)) = (
        model.rig.nodes.get(usize::from(first)),
        model.rig.nodes.get(usize::from(second)),
    ) else {
        return false;
    };
    a.visible
        && b.visible
        && a.rest == b.rest
        && a.global == b.global
        && motions.iter().all(|motion| {
            let track = |bone| motion.tracks.iter().find(|track| track.bone == bone);
            match (track(first), track(second)) {
                (Some(a), Some(b)) => !a.nodes.is_empty() && a.nodes == b.nodes,
                _ => false,
            }
        })
}

fn grouped_coverage(model: &model::Model, motions: &[Motion]) -> bool {
    let mut groups: Vec<Vec<&model::Mesh>> = Vec::new();
    for body in model.meshes.iter().filter(|mesh| !black_shell(mesh)) {
        if body.joints.is_some() || body.positions.is_empty() || body.indices.is_empty() {
            return false;
        }
        if let Some(group) = groups
            .iter_mut()
            .find(|group| same_frame(model, motions, group[0].node, body.node))
        {
            group.push(body);
        } else {
            groups.push(vec![body]);
        }
    }
    !groups.is_empty()
        && groups.iter().all(|group| {
            let indices = group.iter().map(|body| body.indices.len()).sum::<usize>();
            let body_volume = group.iter().map(|body| volume(body)).sum::<f32>();
            let body_min = std::array::from_fn::<_, 3, _>(|axis| {
                group
                    .iter()
                    .flat_map(|body| &body.positions)
                    .map(|position| position[axis])
                    .fold(f32::INFINITY, f32::min)
            });
            let body_max = std::array::from_fn::<_, 3, _>(|axis| {
                group
                    .iter()
                    .flat_map(|body| &body.positions)
                    .map(|position| position[axis])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            model
                .meshes
                .iter()
                .filter(|mesh| black_shell(mesh))
                .any(|shell| {
                    if shell.joints.is_some()
                        || shell.positions.is_empty()
                        || indices != shell.indices.len()
                        || !same_frame(model, motions, group[0].node, shell.node)
                    {
                        return false;
                    }
                    let shell_volume = volume(shell);
                    let (shell_min, shell_max) = bounds(shell);
                    shell_volume < 0.
                        && body_volume > 0.
                        && -shell_volume / body_volume > 1.
                        && (0..3).all(|axis| {
                            (0.001..=2.).contains(&(body_min[axis] - shell_min[axis]))
                                && (0.001..=2.).contains(&(shell_max[axis] - body_max[axis]))
                        })
                })
        })
}

/// A source shell may cover several material parts, including animated rigid
/// nodes. Different nodes must have identical source frames and exact imported
/// world-node track chains in every clip; no pose sampling or tolerance is used.
pub(super) fn present_with_clips(
    model: &model::Model,
    library: &Path,
    scene: &resonance_content::ScenePart,
) -> Result<bool> {
    if present(model) {
        return Ok(true);
    }
    if !model.meshes.iter().any(black_shell) && model.meshes.len() != 2 {
        return Ok(false);
    }
    let names = model.rig.joint_names();
    let mut motions = Vec::new();
    for clip in &scene.clips {
        let bytes = std::fs::read(library.join(&clip.motion))?;
        let Ok(motion) = Motion::decode_world_nodes(&bytes, &names) else {
            // An unbound or unsupported clip cannot prove shell correspondence.
            return Ok(false);
        };
        motions.push(motion);
    }
    Ok(covered(model, |body, shell| {
        same_frame(model, &motions, body.node, shell.node)
    }) || grouped_coverage(model, &motions)
        || colored_pair(model, |body, shell| {
            same_frame(model, &motions, body.node, shell.node)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::{Channel, Interpolation, NodeTrack, Track, Transform};

    #[test]
    fn animated_frames_require_equal_complete_tracks() {
        let model = model::Model {
            rig: model::Rig {
                nodes: (0..2)
                    .map(|index| model::Node {
                        binding: index,
                        name: format!("root-{index}"),
                        flags: 0x100,
                        parent: None,
                        rest: Transform::default(),
                        visible: true,
                        global: glam::Mat4::IDENTITY.to_cols_array(),
                        inverse_bind: glam::Mat4::IDENTITY.to_cols_array(),
                    })
                    .collect(),
                animation_bindings: Vec::new(),
            },
            meshes: Vec::new(),
            materials: Default::default(),
        };
        let mut node = NodeTrack::new(Transform::default());
        node.rotation = Some(Channel {
            interpolation: Interpolation::Linear,
            times: vec![0., 1.],
            values: vec![
                Transform::default().rotation,
                glam::Quat::from_rotation_x(0.25).to_array(),
            ],
        });
        let mut motion = Motion {
            duration_frames: 30.,
            tracks: (0..2)
                .map(|bone| Track {
                    bone,
                    nodes: vec![node.clone().into()],
                })
                .collect(),
        };
        assert!(same_frame(&model, &[motion.clone()], Some(0), Some(1)));
        std::sync::Arc::make_mut(&mut motion.tracks[1].nodes[0])
            .rotation
            .as_mut()
            .unwrap()
            .values[1] = glam::Quat::from_rotation_x(0.5).to_array();
        assert!(!same_frame(&model, &[motion.clone()], Some(0), Some(1)));
        motion.tracks.pop();
        assert!(!same_frame(&model, &[motion], Some(0), Some(1)));
    }

    #[test]
    fn black_material_alone_does_not_replace_a_generated_outline() {
        let make_mesh = |black: bool, expanded: bool, reversed: bool| {
            let mut mesh = model::Mesh {
                node: Some(0),
                flags: 1,
                material: 0,
                name: "surface".into(),
                positions: vec![[1., 1., 1.], [-1., -1., 1.], [-1., 1., -1.], [1., -1., -1.]],
                normals: vec![[0.; 3]; 4],
                colors: vec![if black { [0., 0., 0., 1.] } else { [1.; 4] }; 4],
                texcoords: Vec::new(),
                joints: None,
                weights: None,
                indices: vec![0, 1, 2, 0, 3, 1, 0, 2, 3, 1, 3, 2],
            };
            if volume(&mesh) < 0. {
                for triangle in mesh.indices.chunks_exact_mut(3) {
                    triangle.swap(1, 2);
                }
            }
            if expanded {
                for p in &mut mesh.positions {
                    for value in p {
                        *value *= 1.05;
                    }
                }
            }
            if reversed {
                for triangle in mesh.indices.chunks_exact_mut(3) {
                    triangle.swap(1, 2);
                }
            }
            mesh
        };
        let make_model = |expanded, reversed| model::Model {
            rig: model::Rig {
                nodes: Vec::new(),
                animation_bindings: Vec::new(),
            },
            meshes: vec![
                make_mesh(true, expanded, reversed),
                make_mesh(false, false, false),
            ],
            materials: Default::default(),
        };
        assert!(present(&make_model(true, true)));
        assert!(!present(&make_model(true, false)));
        assert!(!present(&make_model(false, true)));
        let mut mixed = make_model(true, true);
        let mut unpaired = make_mesh(false, false, false);
        unpaired.node = Some(1);
        mixed.meshes.push(unpaired);
        assert!(
            !present(&mixed),
            "an unpaired coloured part must retain the generated outline"
        );
    }
}
