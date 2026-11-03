//! Some rigid effect models already contain their expanded, inward-facing black shell.
use super::*;
use glam::Vec3;
use resonance_content::animation::{Channel, Interpolation, Motion};

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
            .any(|shell| {
                if shell.joints.is_some()
                    || shell.positions.is_empty()
                    || !same_frame(body, shell)
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
                // Native baked outlines reverse winding and expand the matching surface by one unit.
                shell_volume < 0.
                    && body_volume > 0.
                    && (-shell_volume / body_volume) > 1.
                    && (0..3).all(|axis| {
                        let low = body_min[axis] - shell_min[axis];
                        let high = shell_max[axis] - body_max[axis];
                        (0.001..=2.).contains(&low) && (0.001..=2.).contains(&high)
                    })
            })
    })
}

fn constant<const N: usize>(channel: &Option<Channel<N>>, rest: [f32; N]) -> bool {
    channel.as_ref().is_none_or(|channel| {
        channel.interpolation != Interpolation::CubicSpline
            && channel.values.iter().all(|value| *value == rest)
    })
}

fn static_clip(bytes: &[u8]) -> Result<bool> {
    Ok(Motion::decode(bytes)?.tracks.iter().all(|track| {
        track.nodes.iter().all(|node| {
            constant(&node.translation, node.bind.translation)
                && constant(&node.rotation, node.bind.rotation)
                && constant(&node.scale, node.bind.scale)
        })
    }))
}

fn coincident_roots(model: &model::Model, first: usize, second: usize) -> bool {
    let (Some(first), Some(second)) = (model.rig.nodes.get(first), model.rig.nodes.get(second))
    else {
        return false;
    };
    first.parent.is_none()
        && second.parent.is_none()
        && first.visible
        && second.visible
        && first.rest == second.rest
        && first.global == second.global
}

/// Separate rigid roots may carry one source surface/shell pair. Accept them
/// only when their frames coincide and every published model clip stays at rest.
/// Animated or hierarchically separated nodes retain the generated outline.
pub(super) fn present_with_clips(
    model: &model::Model,
    library: &Path,
    scene: &resonance_content::ScenePart,
) -> Result<bool> {
    if present(model) {
        return Ok(true);
    }
    if !covered(model, |body, shell| {
        if body.node == shell.node {
            return true;
        }
        let (Some(body), Some(shell)) = (body.node, shell.node) else {
            return false;
        };
        coincident_roots(model, usize::from(body), usize::from(shell))
    }) {
        return Ok(false);
    }
    for clip in &scene.clips {
        let bytes = std::fs::read(library.join(&clip.motion))?;
        if !static_clip(&bytes)? {
            return Ok(false);
        }
        let motion = Motion::decode_world_nodes(&bytes, &model.rig.joint_names())?;
        for first in 0..model.rig.nodes.len() {
            for second in first + 1..model.rig.nodes.len() {
                if !coincident_roots(model, first, second) {
                    continue;
                }
                let frame = |bone| -> Result<_> {
                    let track = motion
                        .tracks
                        .iter()
                        .find(|track| usize::from(track.bone) == bone)
                        .context("static source root has no clip binding")?;
                    let bind = track.nodes.first().context("empty source root track")?.bind;
                    track.sample_matrix(0., bind)
                };
                if frame(first)? != frame(second)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::{NodeTrack, Skeleton, Track, Transform};

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
    fn a_later_translation_key_is_not_a_static_shell_binding() -> Result<()> {
        assert!(!static_clip(&translating_clip()?)?);
        Ok(())
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
            "{checked} already-published source shell models remain covered by the conservative guard; no outputs written"
        );
        Ok(())
    }
}
