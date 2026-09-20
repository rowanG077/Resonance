use super::*;
use crate::scene::glb::animation_samples;

#[test]
#[ignore = "requires locally cooked monster assets"]
fn catalogue_matches_oracle_statistics_and_uses_converted_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked");
    for id in 0..MONSTER_COUNT as u8 {
        let monster: Monster =
            serde_json::from_slice(&fs::read(root.join(format!("monsters/{id:03}.json"))).unwrap())
                .unwrap();
        monster.validate(528).unwrap();
        assert_eq!(monster.id, id);
        for part in &monster.preview.parts {
            assert!(part.scene.mesh.ends_with(".glb"));
            let model = fs::read(root.join(&part.scene.mesh)).unwrap();
            assert_eq!(&model[..4], b"glTF");
            for texture in &part.scene.textures {
                assert!(texture.ends_with(".ktx2"));
                assert!(root.join(texture).is_file());
            }
        }
        // Dolphin's base and repeat-battle pages, including zero-TP/zero-reward records.
        let expected: &[[u32; 6]] = match id {
            0 => &[[7480, 0, 1030, 90, 228, 321]],
            1 => &[[6390, 0, 856, 79, 183, 382]],
            3 => &[[470, 0, 140, 8, 8, 13], [687, 38, 250, 10, 0, 0]],
            36 => &[
                [800, 0, 130, 0, 8, 12],
                [700, 0, 133, 10, 8, 10],
                [2480, 0, 432, 10, 78, 102],
            ],
            _ => &[],
        };
        if !expected.is_empty() {
            assert_eq!(monster.statistics.len(), expected.len());
        }
        for (s, stats) in monster.statistics.iter().zip(expected) {
            assert_eq!(
                [
                    s.hp,
                    s.tp.into(),
                    s.attack.into(),
                    s.defense.into(),
                    s.experience,
                    s.gald
                ],
                *stats
            );
        }
    }
}

#[test]
#[cfg(unix)]
#[ignore = "requires both extracted discs, cook-all and the original prepared Monster Book; no codecs or devices"]
fn shared_monster_preparation_preserves_every_catalogue_record_and_preview() -> Result<()> {
    use crate::read::{u16 as half, u32 as word};
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let root = crate::temporary_path(&std::env::temp_dir().join("shared-monsters"));
    fs::create_dir(&root)?;
    for entry in ["assets", "data", "sources.json"] {
        std::os::unix::fs::symlink(local.join("all-assets").join(entry), root.join(entry))?;
    }
    let result = (|| -> Result<()> {
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            cook(&extracted, &root, &[])?;
            let sources = crate::battle::all::Sources::read(&extracted)?;
            let directory = fs::read(extracted.join("files").join(sources.usual))?;
            let archive = fs::read(extracted.join("files").join(sources.enemy))?;
            let table = word(&directory, 0x2c)? as usize;
            for id in 0..MONSTER_COUNT {
                let start = word(&directory, table + id * 4)? as usize;
                let end = word(&directory, table + (id + 1) * 4)? as usize;
                let package = crate::compression::decode(&archive[start..end])?;
                let metadata = &package[usize::from(half(&package, 4)?)..];
                let mut motions = Vec::new();
                for (fields, slot) in [
                    ([0x18, 0x1c], Some(0)),
                    (
                        [0x180, 0x198],
                        (half(metadata, 0xb4)? & 0x8400 != 0).then_some(metadata[0xbd]),
                    ),
                ] {
                    let Some(slot) = slot else { continue };
                    for field in fields {
                        let model = word(&package, field)? as usize;
                        if model != 0 {
                            motions.push((model, slot));
                        }
                    }
                }
                let file = format!("monsters/{id:03}.json");
                let current: Monster = serde_json::from_slice(&fs::read(root.join(&file))?)?;
                let previous: Monster =
                    serde_json::from_slice(&fs::read(local.join("cooked").join(file))?)?;
                let data = |monster: &Monster| -> Result<_> {
                    let mut value = serde_json::to_value(monster)?;
                    value.as_object_mut().unwrap().remove("preview");
                    Ok(value)
                };
                assert_eq!(data(&current)?, data(&previous)?, "monster {id} data");
                let actual = &current.preview;
                let expected = &previous.preview;
                assert_eq!(
                    (actual.scale, actual.elevation, actual.parts.len()),
                    (expected.scale, expected.elevation, expected.parts.len()),
                    "monster {id} framing/layers"
                );
                assert_eq!(
                    serde_json::to_value(&actual.node_scales)?,
                    serde_json::to_value(&expected.node_scales)?
                );
                assert_eq!(actual.hidden_geometry, expected.hidden_geometry);
                for (layer, (part, old)) in actual.parts.iter().zip(&expected.parts).enumerate() {
                    assert!(part.scene.mesh.starts_with("assets/"));
                    assert!(root.join(&part.scene.mesh).is_file());
                    assert!(
                        part.scene
                            .textures
                            .iter()
                            .all(|p| p.starts_with("assets/") && root.join(p).is_file())
                    );
                    assert_eq!(
                        (&part.attached_to, part.additive, part.scene.outline_color),
                        (&old.attached_to, old.additive, old.scene.outline_color),
                        "monster {id} layer {layer}"
                    );
                    assert_eq!(
                        serde_json::to_value(&part.scene.materials)?,
                        serde_json::to_value(&old.scene.materials)?,
                        "monster {id} materials {layer}"
                    );
                    let selected = part.selected_clip()?;
                    let native = motions
                        .get(layer)
                        .map(|&(model, slot)| -> Result<_> {
                            Ok((
                                model,
                                slot,
                                word(&package, 0x20 + usize::from(slot) * 4)? as usize,
                            ))
                        })
                        .transpose()?
                        .filter(|&(_, _, animation)| animation != 0);
                    assert_eq!(
                        selected.is_some(),
                        native.is_some(),
                        "monster {id} idle {layer}"
                    );
                    if let Some((index, clip)) = selected {
                        let (model, slot, animation) = native.unwrap();
                        assert_eq!(clip.resource_slot, u16::from(slot));
                        let current = crate::scene::glb::Glb::read(&root.join(&part.scene.mesh))?;
                        let (_, model) = crate::geometry::model_resource(&package[model..])?;
                        let bindings = crate::animation::ModelBindings::read(
                            &model[crate::geometry::skeleton_range(model)?],
                        )?;
                        let mut original = crate::scene::glb::Glb {
                            json: current.json.clone(),
                            binary: current.binary.clone(),
                        };
                        original.json["animations"] = serde_json::json!([]);
                        let duration = crate::animation::bake(
                            &package[animation..],
                            &bindings,
                            &mut original.json,
                            &mut original.binary,
                            "native-preview",
                        )?;
                        assert_eq!(
                            clip.duration_seconds, duration,
                            "monster {id} duration {layer}"
                        );
                        let actual = animation_samples(&current, index)?;
                        let expected = animation_samples(&original, 0)?;
                        assert_eq!(actual.len(), expected.len());
                        for ((target, times, samples), (old_target, old_times, old_samples)) in
                            actual.iter().zip(&expected)
                        {
                            assert_eq!(
                                (target, times),
                                (old_target, old_times),
                                "monster {id} channels {layer}"
                            );
                            assert_eq!(samples.len(), old_samples.len());
                            // Different build profiles can round normalized quaternions
                            // differently; timing and channel ownership stay exact.
                            for (sample, (&value, &old)) in
                                samples.iter().zip(old_samples).enumerate()
                            {
                                assert!(
                                    (value - old).abs() <= 4. * f32::EPSILON * old.abs().max(1.),
                                    "monster {id} animation {layer}, {target}, sample {sample}: {value} != {old}"
                                );
                            }
                        }
                    }
                }
            }
            assert!(
                root.join("monsters").read_dir()?.all(|entry| entry
                    .unwrap()
                    .file_type()
                    .unwrap()
                    .is_file())
            );
        }
        Ok(())
    })();
    fs::remove_dir_all(root)?;
    result
}
