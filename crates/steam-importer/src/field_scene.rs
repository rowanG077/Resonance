//! Scenery layers, panorama and cameras written in the same field scene format as GC.
use crate::{
    animation::Animation, field_overlays, model, output, package::Package, pe, source::Source,
    texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    animation::Skeleton,
    field::{DrawStage, document::Scene},
    library::{FIELDS, Mappings},
};
use std::{collections::BTreeMap, fs, path::Path};

const LAYERS: [usize; 4] = [0, 10, 12, 2];

/// GC scenery submits genuinely translucent surfaces after the room geometry.
/// Steam's descriptor order loses that priority: early glass can otherwise
/// write depth before the exterior is drawn. Preserve mesh/script identities
/// and source order within each pass; binary cutouts stay with opaque draws.
fn scenery_materials(
    source: &Source,
    library: &Path,
    descriptor: &str,
    part: &mut resonance_content::ScenePart,
    native: &model::Model,
) -> Result<()> {
    let stem = descriptor
        .rsplit_once('.')
        .context("field model has no extension")?
        .0;
    let parameters = source.require(&format!("{stem}.mxd"))?;
    let (tints, _, _) = model::material_parameters(&parameters, &native.materials)?;
    let cutoffs = model::material_alpha_cutoffs(&parameters, &native.materials)?;
    let alpha = part
        .textures
        .iter()
        .map(|path| -> Result<[bool; 256]> {
            let levels = resonance_asset_writer::texture::ktx2(&fs::read(library.join(path))?)?;
            let mut present = [false; 256];
            for pixel in levels[0].rgba.chunks_exact(4) {
                present[usize::from(pixel[3])] = true;
            }
            Ok(present)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut order = Vec::new();
    for (index, (mesh, material)) in native
        .meshes
        .iter()
        .filter(|mesh| !mesh.indices.is_empty())
        .zip(&mut part.materials)
        .enumerate()
    {
        let slot = usize::from(mesh.material);
        let cutoff = cutoffs[slot];
        material.alpha_cutoff = cutoff;
        let vertex_alpha = |value: f32| value * tints[slot][3];
        let vertex_blend = mesh.colors.iter().any(|color| vertex_alpha(color[3]) < 1.);
        let textures = &native.materials.records[slot].textures;
        material.blend = vertex_blend
            || material.multiply.is_some()
            || textures.iter().any(|binding| {
                alpha[usize::from(binding.texture)][..255]
                    .iter()
                    .any(|&present| present)
            });
        let translucent = mesh.colors.iter().any(|color| {
            let opacity = vertex_alpha(color[3]);
            opacity > 0. && opacity < 1. && opacity * 255. >= f32::from(cutoff)
        }) || textures.iter().any(|binding| {
            alpha[usize::from(binding.texture)][usize::from(cutoff.max(1))..255]
                .iter()
                .any(|&present| present)
        });
        order.push((translucent, material.draw_order, index));
    }
    order.sort_by_key(|&(translucent, priority, _)| (translucent, priority));
    for (rank, (_, _, index)) in order.into_iter().enumerate() {
        part.materials[index].draw_order = rank as u32;
    }
    Ok(())
}

pub(crate) fn cook(source: &Source, library: &Path, selected: Option<u32>) -> Result<()> {
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    if let Some(id) = selected {
        fields.target(id)?;
    }
    let image = pe::Image::parse(&source.executable)?;
    for (id, name) in fields
        .iter()
        .filter(|(id, _)| selected.is_none_or(|only| only == *id))
    {
        eprintln!("  Field scenery: {name} ({id})");
        let pointer = pe::word(image.slice(0x8cb908 + id * 24, 4)?, 0)?;
        let bytes = source.require(image.text(pointer)?)?;
        let package = Package::parse(&bytes)?;
        let root = format!("fields/{name}");
        let mut parts = Vec::new();
        for section in LAYERS {
            let Some(descriptor) = package.reference(section)? else {
                continue;
            };
            if !source.contains(descriptor)? {
                eprintln!("    Source scenery is absent: {descriptor}");
                continue;
            }
            let label = format!("part-{}", parts.len());
            let (mut part, model) = model::cook(source, library, descriptor, &root, &label)
                .with_context(|| format!("field {id} scenery section {section} ({descriptor})"))?;
            scenery_materials(source, library, descriptor, &mut part, &model)?;
            if let Some(name) = package.reference(section + 1)? {
                let skeleton = Skeleton::from_glb(&fs::read(library.join(&part.mesh))?)?;
                let bytes = Animation::read(&source.require(name)?)?
                    .bind(&model.rig, &skeleton)?
                    .encode(&skeleton)
                    .with_context(|| format!("field {id} scenery motion {name}"))?;
                let path = format!("{root}/animations/{label}-motion-0.glb");
                output::write(&library.join(&path), &bytes)?;
                part.clips.push(SceneClip {
                    motion: path,
                    joints: Default::default(),
                    resource_slot: 0,
                    duration_seconds: 0.,
                    animation_resource: None,
                    pose_chains: Vec::new(),
                });
                part.autoplay = true;
            }
            part.resource = section as u16;
            let stage = DrawStage::scenery(section as u16).context("unknown field layer")?;
            for material in &mut part.materials {
                material.draw_order += stage.offset();
                material.depth_write = section != 2;
            }
            parts.push(part);
        }
        let panorama = match package.reference(15)? {
            Some(name) => match source.read(name)? {
                Some(bytes) => {
                    let descriptor = texture::Descriptor::read(&bytes)?;
                    let image = descriptor.decode(&source.require(&descriptor.payload)?)?;
                    let path = format!("{root}/textures/panorama.ktx2");
                    output::write(&library.join(&path), &image.ktx2()?)?;
                    Some(path)
                }
                None => {
                    eprintln!("    Source panorama is absent: {name}");
                    None
                }
            },
            None => None,
        };
        let cameras = (16..package.len())
            .filter_map(|slot| match package.reference(slot) {
                Ok(Some(name)) if name.to_ascii_lowercase().ends_with(".cam") => {
                    Some(Ok((slot, name)))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut camera_tracks = BTreeMap::new();
        for (ordinal, &(slot, name)) in cameras.iter().enumerate() {
            let Some(bytes) = source.read(name)? else {
                eprintln!("    Source camera is absent: {name}");
                continue;
            };
            let bytes = camera(&bytes).with_context(|| format!("field {id} camera {name}"))?;
            let path = if cameras.len() == 1 {
                format!("{root}/camera.glb")
            } else {
                format!("{root}/camera-{}.glb", ordinal + 1)
            };
            output::write(&library.join(&path), &bytes)?;
            camera_tracks.insert(0xffee_0000 + (slot - 16) as u32, path);
        }
        let scene = Scene {
            parts,
            panorama,
            water: None,
            environment: Default::default(),
            camera_tracks,
            texture_animations: resonance_importer::field_texture_animations(id)?,
            unbound_geometry: Vec::new(),
            contact_shadow: None,
            toon_ramp: None,
            effects: "fields/settings.json".into(),
            blink: None,
            particles: None,
            overlays: field_overlays::cook(source, library, &package, id, &root)?,
            save_point: None,
        };
        output::json(&library.join(&root).join("scene.json"), &scene)?;
    }
    Ok(())
}

pub(crate) fn camera(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.starts_with(b"CAMM") && bytes.len().is_multiple_of(4),
        "unsupported field camera"
    );
    let mut bytes = bytes.to_vec();
    // CAMM's payload consists of 32-bit scalar fields. Reuse the GC converter
    // after changing numeric storage; its output already has the common clock.
    for word in bytes.chunks_exact_mut(4).skip(1) {
        word.reverse();
    }
    resonance_importer::convert_camera(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires installed Steam and RESONANCE_TEST_LIBRARY; optional RESONANCE_SCENERY_REFRESH_FIELDS=220,221 or369, RESONANCE_SCENERY_MATERIAL_OUTPUT"]
    fn installed_selected_scenery_refresh_uses_existing_translucent_priorities() -> Result<()> {
        let source = Source::open(
            Path::new(
                &std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let fields = std::env::var("RESONANCE_SCENERY_REFRESH_FIELDS")
            .unwrap_or_else(|_| "220,221".into())
            .split(',')
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for field in fields {
            let (name, expected) = match field {
                220 => ("triet-ruins-0220", Some([(12, 10), (10, 11), (11, 12)])),
                221 => ("triet-ruins-0221", Some([(7, 5), (5, 9), (6, 10)])),
                369 => ("tower-of-mana-0369", None),
                _ => anyhow::bail!("selected scenery probe only covers fields220/221/369"),
            };
            let relative = format!("fields/{name}/scene.json");
            let original = fs::read(library.join(&relative))?;
            let mut scene: Scene = serde_json::from_slice(&original)?;
            let pointer = pe::word(image.slice(0x8cb908 + field * 24, 4)?, 0)?;
            let package_bytes = source.require(image.text(pointer)?)?;
            let package = Package::parse(&package_bytes)?;
            for part in &mut scene.parts {
                let descriptor = package
                    .reference(usize::from(part.resource))?
                    .context("Triet scenery has no native model")?;
                let bytes = source.require(descriptor)?;
                let dependencies = crate::package::dependencies(&bytes)?;
                let native = model::Model::read(&bytes, &source.require(&dependencies[0])?)?;
                ensure!(
                    part.materials.len()
                        == native
                            .meshes
                            .iter()
                            .filter(|mesh| !mesh.indices.is_empty())
                            .count(),
                    "Triet source mesh/material count changed"
                );
                scenery_materials(&source, &library, descriptor, part, &native)?;
                let stage = DrawStage::scenery(part.resource).context("unknown Triet layer")?;
                for material in &mut part.materials {
                    material.draw_order += stage.offset();
                    material.depth_write = part.resource != 2;
                }
                if part.resource == 0
                    && let Some(expected) = expected
                {
                    eprintln!(
                        "field {field} all main priorities {:?}",
                        part.materials
                            .iter()
                            .map(|m| m.draw_order)
                            .collect::<Vec<_>>()
                    );
                    for (mesh, priority) in expected {
                        ensure!(
                            part.materials[mesh].draw_order == priority,
                            "field {field} mesh {mesh} no longer has source-derived scenery priority {priority}"
                        );
                    }
                    eprintln!(
                        "field {field}: existing scenery recipe entrance priorities {expected:?}"
                    );
                }
            }
            let original_value: serde_json::Value = serde_json::from_slice(&original)?;
            let mut override_value = original_value.clone();
            let normalized = serde_json::to_value(&scene)?;
            let parts = override_value["parts"]
                .as_array_mut()
                .context("Triet has no parts")?;
            let normalized_parts = normalized["parts"]
                .as_array()
                .context("normalized parts missing")?;
            ensure!(
                parts.len() == normalized_parts.len(),
                "Triet part count changed"
            );
            for (original, normalized) in parts.iter_mut().zip(normalized_parts) {
                original["materials"] = normalized["materials"].clone();
            }
            let without_materials = |mut value: serde_json::Value| {
                for part in value["parts"].as_array_mut().unwrap() {
                    part.as_object_mut().unwrap().remove("materials");
                }
                value
            };
            ensure!(
                without_materials(original_value) == without_materials(override_value.clone()),
                "Triet material refresh changed other scene fields"
            );
            if let Some(destination) = std::env::var_os("RESONANCE_SCENERY_MATERIAL_OUTPUT")
                .or_else(|| std::env::var_os("RESONANCE_TRIET_SCENERY_OUTPUT"))
            {
                output::json(&Path::new(&destination).join(relative), &override_value)?;
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam and RESONANCE_TEST_LIBRARY; optionally writes a field522 scene override to RESONANCE_NIGHT_SCENERY_OUTPUT"]
    fn installed_night_foliage_rejects_native_cutout_edges() -> Result<()> {
        let source = Source::open(
            Path::new(
                &std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let relative = "fields/field-0522/scene.json";
        let original = fs::read(library.join(relative))?;
        let mut scene: Scene = serde_json::from_slice(&original)?;
        let image = pe::Image::parse(&source.executable)?;
        let pointer = pe::word(image.slice(0x8cb908 + 522 * 24, 4)?, 0)?;
        let package_bytes = source.require(image.text(pointer)?)?;
        let package = Package::parse(&package_bytes)?;
        for part in &mut scene.parts {
            let descriptor = package
                .reference(usize::from(part.resource))?
                .context("night scenery has no native model")?;
            let bytes = source.require(descriptor)?;
            let dependencies = crate::package::dependencies(&bytes)?;
            let native = model::Model::read(&bytes, &source.require(&dependencies[0])?)?;
            scenery_materials(&source, &library, descriptor, part, &native)?;
            let cutoffs = part
                .materials
                .iter()
                .map(|m| m.alpha_cutoff)
                .collect::<Vec<_>>();
            ensure!(
                cutoffs == [129, 129, 129, 129, 129, 129, 129, 1, 129],
                "night scenery alpha comparisons changed: {cutoffs:?}"
            );
        }
        // This probe only changes material state. Keep script, animation,
        // geometry and every other scene field from the existing library.
        let mut override_value: serde_json::Value = serde_json::from_slice(&original)?;
        let normalized = serde_json::to_value(&scene)?;
        for (original, normalized) in override_value["parts"]
            .as_array_mut()
            .context("night scenery has no parts")?
            .iter_mut()
            .zip(
                normalized["parts"]
                    .as_array()
                    .context("normalized parts missing")?,
            )
        {
            original["materials"] = normalized["materials"].clone();
        }
        if let Some(destination) = std::env::var_os("RESONANCE_NIGHT_SCENERY_OUTPUT") {
            output::json(&Path::new(&destination).join(relative), &override_value)?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam; optionally publishes only field340 to RESONANCE_CLASSROOM_OUTPUT"]
    fn installed_classroom_nodes_keep_script_visibility_targets() -> Result<()> {
        let installation =
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?;
        let source = Source::open(Path::new(&installation), crate::Language::English)?;
        let temporary = tempfile::tempdir()?;
        crate::field_directory::cook(&source, temporary.path())?;
        cook(&source, temporary.path(), Some(340))?;
        let scene: Scene = serde_json::from_slice(&fs::read(
            temporary.path().join("fields/iselia-0340/scene.json"),
        )?)?;
        let mut room = scene
            .parts
            .into_iter()
            .find(|part| part.resource == 0)
            .context("classroom has no room geometry")?;
        room.prepare_bones(&fs::read(temporary.path().join(&room.mesh))?)?;
        // The source scenario toggles these exact labels. Stable indexed nodes
        // must retain their geometry bindings through the shared GC contract.
        for (name, node, mesh) in [
            ("hide01", 7, 7),
            ("hide02", 4, 4),
            ("hide03", 5, 5),
            ("hide04_a", 6, 6),
            ("world2", 10, 10),
        ] {
            ensure!(
                room.bone_names
                    .iter()
                    .position(|candidate| candidate == name)
                    == Some(node),
                "script target {name} is missing or changed its node identity"
            );
            ensure!(
                room.material_nodes[mesh].contains(&(node as u16)),
                "script target {name} no longer controls its source geometry"
            );
        }
        ensure!(
            room.materials.iter().all(|material| material.depth_write),
            "classroom windows lost the GC scenery depth-write rule"
        );
        ensure!(
            room.materials[1].draw_order > room.materials[10].draw_order,
            "classroom glass is drawn before the exterior behind it"
        );
        ensure!(
            room.materials[1].blend && room.materials[1].alpha_cutoff == 1,
            "classroom glass lost its accepted translucent alpha"
        );
        ensure!(
            !room.materials[10].blend && room.materials[10].alpha_cutoff == 129,
            "classroom exterior lost its opaque material pass"
        );
        if let Some(output) = std::env::var_os("RESONANCE_CLASSROOM_OUTPUT") {
            cook(&source, Path::new(&output), Some(340))?;
        }
        Ok(())
    }

    #[test]
    fn little_endian_camera_loads_with_shared_camera_reader() -> Result<()> {
        let mut bytes = b"CAMM".to_vec();
        bytes.extend([72u32, 2].into_iter().flat_map(u32::to_le_bytes));
        for time in [0f32, 10.] {
            bytes.extend(
                [time, time, 2., 3., 0., 0., 0., 1., 0.]
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
        for time in [0f32, 10.] {
            bytes.extend(
                [time, 0., 0., 0., 0.]
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
        let bytes = camera(&bytes)?;
        let motion = resonance_content::camera_motion::CameraMotion::decode(&bytes)?;
        assert_eq!(motion.duration_frames(), 10.);
        let key = motion.sample(5.)?;
        for (actual, expected) in key.position.into_iter().zip([5., 2., 3.]) {
            assert!((actual - expected).abs() < 0.00001);
        }
        Ok(())
    }
}
