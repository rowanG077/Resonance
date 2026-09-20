use super::*;
use crate::{
    read::u32 as word,
    scene::glb::{Glb, animation_samples},
};
use std::fs;

#[test]
#[cfg(unix)]
#[ignore = "requires both extracted discs and cook-all; no codecs or devices"]
fn shared_previews_bind_every_figurine_and_original_idle() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let root = crate::temporary_path(&std::env::temp_dir().join("shared-figurines"));
    fs::create_dir(&root)?;
    for entry in ["assets", "data", "sources.json"] {
        std::os::unix::fs::symlink(local.join("all-assets").join(entry), root.join(entry))?;
    }
    let result = (|| -> Result<()> {
        let mut first = Vec::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            cook(&extracted, &root, &[])?;
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let hidden_prefix = crate::dol::text(&executable, 0x8035d3b8)?;
            let declaration = crate::dol::text(&executable, 0x801aaa18)?;
            let path =
                crate::all_assets::roles::declared_path(&extracted.join("files"), &declaration)?;
            let archive = fs::read(extracted.join("files").join(path))?;
            let catalogue = book(&root, disc)?;
            catalogue.validate()?;
            for record in &catalogue.records {
                let id = usize::from(record.id);
                let row = crate::dol::slice(&executable, 0x802280c0 + id as u32 * 80, 80)?;
                let resource = word(row, 8)?;
                let entry = if resource >= 0x20000 {
                    resource - 0x20000
                } else {
                    resource
                } as usize;
                let start = word(&archive, 4 + entry * 8)? as usize;
                let size = word(&archive, 8 + entry * 8)? as usize;
                ensure!(size > 0, "original figurine entry is empty");
                let package = &archive[start..start + size];
                let sections = crate::field::sections(package)?;
                let idle = sections
                    .iter()
                    .take(sections.len() - 1)
                    .skip(2)
                    .flatten()
                    .next();
                let animation = idle
                    .map(|range| -> Result<_> {
                        let bytes = &package[range.clone()];
                        if bytes.starts_with(&0x007b7960u32.to_be_bytes()) {
                            Ok(bytes.to_vec())
                        } else {
                            crate::compression::decode(bytes)
                        }
                    })
                    .transpose()?;
                assert_eq!(record.name, crate::dol::text(&executable, word(row, 0)?)?);
                assert_eq!(
                    record.preview.elevation,
                    if resource == 0x20049 { -80. } else { 0. }
                );
                assert_eq!(
                    record.preview.parts.len(),
                    if sections[1].is_some() { 2 } else { 1 }
                );
                let mut hidden: BTreeSet<_> = record.preview.parts[0]
                    .scene
                    .bone_names
                    .iter()
                    .filter(|name| name.starts_with(&hidden_prefix))
                    .cloned()
                    .collect();
                for offset in (16..80).step_by(4) {
                    if let Some(rule) = crate::dol::optional_text(&executable, word(row, offset)?)?
                    {
                        let prefix = rule.strip_prefix('-').unwrap_or(&rule);
                        for bone in record.preview.parts[0]
                            .scene
                            .bone_names
                            .iter()
                            .filter(|n| n.starts_with(prefix))
                        {
                            if rule.starts_with('-') {
                                hidden.insert(bone.clone());
                            } else {
                                hidden.remove(bone);
                            }
                        }
                    }
                }
                assert_eq!(
                    record.preview.hidden_geometry,
                    hidden.into_iter().collect::<Vec<_>>()
                );
                let primary = &package[sections[0].clone().context("original primary")?];
                let textures = crate::tpl::parse_tpl(&primary[word(primary, 0)? as usize..])?;
                let rows = primary[14]
                    .checked_sub(1)
                    .map(|texture| match textures[usize::from(texture)].height {
                        512 => 2,
                        1024 => 4,
                        _ => 0,
                    })
                    .unwrap_or(0);
                let variant = word(row, 12)?;
                for (layer, part) in record.preview.parts.iter().enumerate() {
                    let source = &package[sections[layer].clone().context("original layer")?];
                    let glb = Glb::read(&root.join(&part.scene.mesh))?;
                    assert!(
                        part.scene
                            .textures
                            .iter()
                            .all(|p| p.starts_with("assets/") && root.join(p).is_file())
                    );
                    let selected = part.selected_clip()?;
                    assert_eq!(
                        selected.is_some(),
                        animation.is_some(),
                        "figurine {id} idle {layer}"
                    );
                    let normalized = crate::character::texture_palette(primary, source)?;
                    let chains = crate::secondary_motion::cook(
                        &normalized,
                        &glb.json,
                        &part.scene.bone_names,
                    )?;
                    assert_eq!(
                        serde_json::to_value(chains)?,
                        serde_json::to_value(&part.scene.secondary_motion)?
                    );
                    if let Some((index, clip)) = selected {
                        let bindings = crate::animation::ModelBindings::read(
                            &normalized[crate::geometry::skeleton_range(&normalized)?],
                        )?;
                        let mut original = Glb {
                            json: glb.json.clone(),
                            binary: glb.binary.clone(),
                        };
                        original.json["animations"] = serde_json::json!([]);
                        let duration = crate::animation::bake(
                            animation.as_ref().unwrap(),
                            &bindings,
                            &mut original.json,
                            &mut original.binary,
                            "original-idle",
                        )?;
                        assert_eq!(
                            clip.duration_seconds, duration,
                            "figurine {id} duration {layer}"
                        );
                        let actual = animation_samples(&glb, index)?;
                        let expected = animation_samples(&original, 0)?;
                        assert_eq!(actual.len(), expected.len());
                        for ((target, times, values), (old_target, old_times, old_values)) in
                            actual.iter().zip(&expected)
                        {
                            assert_eq!(
                                (target, times),
                                (old_target, old_times),
                                "figurine {id} channels {layer}"
                            );
                            assert_eq!(values.len(), old_values.len());
                            for (&value, &old) in values.iter().zip(old_values) {
                                assert!(
                                    (value - old).abs() <= 4. * f32::EPSILON * old.abs().max(1.),
                                    "figurine {id} layer {layer}: {value} != {old}"
                                );
                            }
                        }
                    }
                    let selector = source[14].checked_sub(1).map(usize::from);
                    if variant == 0 || variant >= rows || selector.is_none() {
                        assert!(part.uv_offsets.is_empty());
                    } else {
                        let shift = variant as f32 / rows as f32;
                        for (offset, material) in part.uv_offsets.iter().zip(&part.scene.materials)
                        {
                            let y = |b: &Option<resonance_content::TextureBinding>| {
                                if b.as_ref().is_some_and(|b| Some(b.texture) == selector) {
                                    shift
                                } else {
                                    0.
                                }
                            };
                            assert_eq!(
                                *offset,
                                [0., y(&material.color), 0., y(&material.multiply)]
                            );
                        }
                    }
                }
            }
            let encoded = serde_json::to_vec(&catalogue)?;
            if disc == 1 {
                first = encoded;
            } else {
                assert_eq!(first, encoded, "disc identity changed shared previews");
            }
        }
        Ok(())
    })();
    fs::remove_dir_all(root)?;
    result
}
