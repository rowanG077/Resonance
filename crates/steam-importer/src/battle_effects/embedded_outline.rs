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
    use resonance_content::animation::{
        Channel, Interpolation, NodeTrack, Skeleton, Track, Transform,
    };

    fn translating_clip() -> Result<Vec<u8>> {
        let mut node = NodeTrack::new(Transform::default());
        node.translation = Some(Channel {
            interpolation: Interpolation::Step,
            times: vec![0., 1.],
            values: vec![[0.; 3], [1., 0., 0.]],
        });
        Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![node],
            }],
        }
        .encode_nodes(&Skeleton::single("moving-root"))
    }

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
                    nodes: vec![node.clone()],
                })
                .collect(),
        };
        assert!(same_frame(&model, &[motion.clone()], Some(0), Some(1)));
        motion.tracks[1].nodes[0].rotation.as_mut().unwrap().values[1] =
            glam::Quat::from_rotation_x(0.5).to_array();
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

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; reads only enemy78 source and its already-published missile model"]
    fn installed_murder_missile_keeps_static_source_shell_without_rewriting_media() -> Result<()> {
        use sha2::{Digest, Sha256};
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let package = source.require(monsters::package(&image, 78)?)?;
        ensure!(
            preview_model::reference(&package, 0x198)?.is_none(),
            "missile acquired a separate source outline"
        );
        let descriptor =
            preview_model::reference(&package, 0x180)?.context("missing Murder missile model")?;
        let bytes = source.require(&descriptor)?;
        let dependencies = crate::package::dependencies(&bytes)?;
        let mut native = model::Model::read(&bytes, &source.require(&dependencies[0])?)?;
        ensure!(
            !present(&native),
            "fixture no longer requires the distinct-node proof"
        );
        let path = library.join("battle/enemies/murder-0078/effects/effects.json");
        let mut effects: Definition = serde_json::from_slice(&std::fs::read(path)?)?;
        let before = serde_json::to_value(&effects)?;
        let missile_value = effects
            .artwork
            .as_mut()
            .context("missing effects artwork")?
            .models
            .get_mut(&0)
            .context("missing missile slot")?;
        let mut missile: resonance_content::battle_model::document::Model =
            serde_json::from_value(missile_value.clone())?;
        let scene = &missile.layers[0].scene;
        let media = std::iter::once(scene.mesh.clone())
            .chain(scene.clips.iter().map(|clip| clip.motion.clone()))
            .chain(scene.textures.iter().cloned())
            .map(|path| {
                let hash = Sha256::digest(std::fs::read(library.join(&path))?);
                Ok((path, hash))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            present_with_clips(&native, &library, scene)?,
            "source missile shell no longer has coincident static roots"
        );

        let old_rest = native.rig.nodes[1].rest;
        native.rig.nodes[1].rest.translation[0] += 1.;
        ensure!(
            !present_with_clips(&native, &library, scene)?,
            "separated source nodes must retain the generated layer"
        );
        native.rig.nodes[1].rest = old_rest;
        let temporary = tempfile::tempdir()?;
        std::fs::write(temporary.path().join("moving.glb"), translating_clip()?)?;
        let mut moving = scene.clone();
        moving.clips.truncate(1);
        ensure!(!moving.clips.is_empty(), "fixture lost its source clip");
        moving.clips[0].motion = "moving.glb".into();
        ensure!(
            !present_with_clips(&native, temporary.path(), &moving)?,
            "a moving clip must retain the generated layer"
        );

        missile
            .layers
            .retain(|part| part.scene.outline_color.is_none());
        ensure!(
            missile.layers.len() == 1,
            "source body/shell must remain together"
        );
        *missile_value = serde_json::to_value(missile)?;
        let after = serde_json::to_value(&effects)?;
        ensure!(before["programs"] == after["programs"]);
        ensure!(before["templates"] == after["templates"]);
        for (path, hash) in media {
            ensure!(
                Sha256::digest(std::fs::read(library.join(path))?) == hash,
                "shell selection must not rewrite model, clip or texture bytes"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; reads only enemy168 source and its already-published skull model"]
    fn installed_skull_groups_material_parts_and_requires_exact_animated_shell_tracks() -> Result<()>
    {
        use sha2::{Digest, Sha256};
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let package = source.require(monsters::package(&image, 168)?)?;
        ensure!(preview_model::reference(&package, 0x198)?.is_none());
        let descriptor =
            preview_model::reference(&package, 0x180)?.context("missing source skull")?;
        let bytes = source.require(&descriptor)?;
        let dependencies = crate::package::dependencies(&bytes)?;
        let mut native = model::Model::read(&bytes, &source.require(&dependencies[0])?)?;
        ensure!(
            !present(&native),
            "fixture no longer needs grouped coverage"
        );
        ensure!(
            native.meshes[0].indices.len() + native.meshes[1].indices.len()
                == native.meshes[3].indices.len(),
            "source skull head material split changed"
        );
        let path = library.join("battle/enemies/man-eater-0168/effects/effects.json");
        let mut effects: Definition = serde_json::from_slice(&std::fs::read(path)?)?;
        let before = serde_json::to_value(&effects)?;
        let skull_value = effects
            .artwork
            .as_mut()
            .context("missing artwork")?
            .models
            .get_mut(&0)
            .context("missing skull slot")?;
        let mut skull: resonance_content::battle_model::document::Model =
            serde_json::from_value(skull_value.clone())?;
        let scene = &skull.layers[0].scene;
        ensure!(scene.clips.len() == 1, "source skull clip set changed");
        let media = std::iter::once(scene.mesh.clone())
            .chain(scene.clips.iter().map(|clip| clip.motion.clone()))
            .chain(scene.textures.iter().cloned())
            .map(|path| {
                let hash = Sha256::digest(std::fs::read(library.join(&path))?);
                Ok((path, hash))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            present_with_clips(&native, &library, scene)?,
            "animated source skull shell must cover its grouped material parts"
        );

        let skeleton = Skeleton::from_glb(&std::fs::read(library.join(&scene.mesh))?)?;
        let mut motion = Motion::decode_for_skeleton(
            &std::fs::read(library.join(&scene.clips[0].motion))?,
            &skeleton,
        )?;
        let shell = motion
            .tracks
            .iter_mut()
            .find(|track| track.bone == 3)
            .context("missing animated skull head shell")?;
        let rotation = shell.nodes[0]
            .rotation
            .as_mut()
            .context("source head shell is no longer animated")?;
        ensure!(rotation.values.len() > 1);
        *rotation.values.last_mut().unwrap() = glam::Quat::from_rotation_x(0.5).to_array();
        let temporary = tempfile::tempdir()?;
        std::fs::write(
            temporary.path().join("divergent.glb"),
            motion.encode_nodes(&skeleton)?,
        )?;
        let mut divergent = scene.clone();
        divergent.clips[0].motion = "divergent.glb".into();
        ensure!(
            !present_with_clips(&native, temporary.path(), &divergent)?,
            "one divergent shell key must retain the generated outline"
        );

        native.meshes.push(model::Mesh {
            node: Some(0),
            flags: 1,
            material: 0,
            name: "uncovered-coloured-part".into(),
            positions: vec![[100., 0., 0.], [100., 1., 0.], [100., 0., 1.]],
            normals: vec![[1., 0., 0.]; 3],
            colors: vec![[1.; 4]; 3],
            texcoords: Vec::new(),
            joints: None,
            weights: None,
            indices: vec![0, 1, 2],
        });
        ensure!(
            !present_with_clips(&native, &library, scene)?,
            "an uncovered material part must retain the generated outline"
        );
        native.meshes.pop();

        skull
            .layers
            .retain(|part| part.scene.outline_color.is_none());
        ensure!(skull.layers.len() == 1);
        *skull_value = serde_json::to_value(skull)?;
        let after = serde_json::to_value(&effects)?;
        ensure!(before["programs"] == after["programs"]);
        ensure!(before["templates"] == after["templates"]);
        for (path, hash) in media {
            ensure!(Sha256::digest(std::fs::read(library.join(path))?) == hash);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; publishes only enemy12/13 effects"]
    fn installed_projectiles_keep_their_source_embedded_outline_only() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        for id in [12, 13] {
            let package = source.require(monsters::package(&image, id)?)?;
            for slot in 0..2 {
                ensure!(
                    preview_model::reference(&package, 0x198 + slot * 4)?.is_none(),
                    "source has a separate outline"
                );
                let descriptor = preview_model::reference(&package, 0x180 + slot * 4)?
                    .context("missing model")?;
                let bytes = source.require(&descriptor)?;
                let payload = crate::package::dependencies(&bytes)?;
                let native = model::Model::read(&bytes, &source.require(&payload[0])?)?;
                ensure!(
                    present(&native),
                    "{descriptor}: missing expanded reversed black shell"
                );
                eprintln!(
                    "{descriptor}: source shell volume {}, colour surface volume {}",
                    volume(&native.meshes[0]),
                    volume(&native.meshes[1])
                );
            }
        }
        super::super::cook_enemies(&source, &library, Some(&[12, 13]))?;
        for id in [12, 13] {
            let path = library.join(format!(
                "battle/enemies/{}/effects/effects.json",
                monsters::symbol(id)
            ));
            let cooked: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            for slot in ["0", "1"] {
                ensure!(
                    cooked["artwork"]["models"][slot]["layers"]
                        .as_array()
                        .context("missing layers")?
                        .len()
                        == 1,
                    "duplicate outline layer"
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; reads only already-published one-layer effect models"]
    fn installed_published_shell_models_cover_every_coloured_part() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let mut checked = 0;
        let mut wings = 0;
        for id in 0..monsters::count(&image)? as u16 {
            let path = library.join(format!(
                "battle/enemies/{}/effects/effects.json",
                monsters::symbol(id)
            ));
            let cooked: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            let Some(models) = cooked["artwork"]["models"].as_object() else {
                continue;
            };
            for (slot, model) in models {
                if model["layers"]
                    .as_array()
                    .is_none_or(|layers| layers.len() != 1)
                {
                    continue;
                }
                let slot = slot.parse::<usize>()?;
                let package = source.require(monsters::package(&image, id)?)?;
                ensure!(
                    preview_model::reference(&package, 0x198 + slot * 4)?.is_none(),
                    "one-layer model has a separate outline"
                );
                let descriptor = preview_model::reference(&package, 0x180 + slot * 4)?
                    .context("missing published source model")?;
                if slot == 0 && matches!(id, 215 | 235 | 239) {
                    let metadata = monsters::metadata(&package)?;
                    let attachment = preview_model::member(&package, 0x1d8)?
                        .context("layered wing record missing")?;
                    let scene: resonance_content::model_preview::PreviewPart =
                        serde_json::from_value(model["layers"][0].clone())?;
                    ensure!(
                        metadata[0x1f2] == 1
                            && attachment[0] == 0
                            && attachment[3] == 0x70
                            && matches!(
                                descriptor.to_ascii_lowercase().as_str(),
                                "bh00_obj003.tomdlb_d"
                                    | "bh22_obj004.tomdlb_d"
                                    | "ba03_obj001.tomdlb_d"
                            )
                            && scene.scene.outline_color.is_none()
                            && scene.scene.textures.len() == 2
                            && scene.scene.textures[0] == scene.scene.textures[1]
                            && scene.scene.materials.iter().all(|material| {
                                material
                                    .color
                                    .as_ref()
                                    .is_some_and(|binding| binding.texture == 1)
                                    && material.multiply.is_none()
                            }),
                        "one-layer wing lost its source recipe or shared palette slot"
                    );
                    wings += 1;
                    continue;
                }
                let bytes = source.require(&descriptor)?;
                let payload = crate::package::dependencies(&bytes)?;
                let native = model::Model::read(&bytes, &source.require(&payload[0])?)?;
                let scene: resonance_content::model_preview::PreviewPart =
                    serde_json::from_value(model["layers"][0].clone())?;
                ensure!(
                    present_with_clips(&native, &library, &scene.scene)?,
                    "new coverage guard changes published enemy{id} model{slot}: {descriptor}"
                );
                checked += 1;
            }
        }
        eprintln!(
            "{checked} already-published source shell models remain covered; {wings} source-declared wings retain their separate recipe; no outputs written"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; reads only enemy23 source and its published snowball"]
    fn installed_bigfoot_snowball_preserves_its_source_shell() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let package = source.require(monsters::package(&image, 23)?)?;
        ensure!(preview_model::reference(&package, 0x198)?.is_none());
        let descriptor =
            preview_model::reference(&package, 0x180)?.context("missing Bigfoot snowball")?;
        let bytes = source.require(&descriptor)?;
        let dependencies = crate::package::dependencies(&bytes)?;
        let mut native = model::Model::read(&bytes, &source.require(&dependencies[0])?)?;
        let path = library.join("battle/enemies/bigfoot-0023/effects/effects.json");
        let effects: Definition = serde_json::from_slice(&std::fs::read(path)?)?;
        let value = effects.artwork.context("missing artwork")?.models[&0].clone();
        let snowball: resonance_content::battle_model::document::Model =
            serde_json::from_value(value)?;
        ensure!(native.meshes.len() == 2);
        ensure!(native.meshes.iter().all(|mesh| !black_shell(mesh)));
        ensure!(native.meshes[0].indices.len() == native.meshes[1].indices.len());
        let scene = &snowball.layers[0].scene;
        ensure!(
            present_with_clips(&native, &library, scene)?,
            "source snowball shell must prevent a second darkening outline layer"
        );

        // A dark material alone is insufficient: opacity, uniform colour,
        // complete coverage, source frame and reverse winding remain required.
        let shell = native
            .meshes
            .iter()
            .position(|mesh| volume(mesh) < 0.)
            .context("missing reversed source snow shell")?;
        let original_colors = native.meshes[shell].colors.clone();
        for color in &mut native.meshes[shell].colors {
            color[3] = 0.5;
        }
        ensure!(!present_with_clips(&native, &library, scene)?);
        native.meshes[shell].colors = original_colors.clone();
        native.meshes[shell].colors[0][0] += 0.01;
        ensure!(!present_with_clips(&native, &library, scene)?);
        native.meshes[shell].colors = original_colors;
        let original_node = native.meshes[shell].node;
        native.meshes[shell].node = None;
        ensure!(!present_with_clips(&native, &library, scene)?);
        native.meshes[shell].node = original_node;
        for triangle in native.meshes[shell].indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
        ensure!(!present_with_clips(&native, &library, scene)?);
        for triangle in native.meshes[shell].indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
        let mut extra = model::Model::read(&bytes, &source.require(&dependencies[0])?)?;
        native.meshes.push(extra.meshes.swap_remove(1 - shell));
        ensure!(
            !present_with_clips(&native, &library, scene)?,
            "an extra coloured part must not inherit the two-mesh proof"
        );
        Ok(())
    }
}
