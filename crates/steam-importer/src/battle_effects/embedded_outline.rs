//! Some rigid effect models already contain their expanded, inward-facing black shell.
use super::*;
use glam::Vec3;

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
                    || body.node != shell.node
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

#[cfg(test)]
mod tests {
    use super::*;

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
                ensure!(
                    present(&native),
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
