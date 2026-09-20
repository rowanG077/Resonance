use super::*;
use crate::{
    model_preview::{Layer, layers, layers_with_clips},
    read::{f32 as float, u16 as half, u32 as word},
};
use resonance_content::model_preview::{ModelPreview, NodeScale};

fn resource(bytes: &[u8], offset: usize) -> Result<&[u8]> {
    ensure!(offset > 0, "missing preview resource");
    bytes
        .get(offset..)
        .context("preview resource exceeds package")
}

fn optional(bytes: &[u8], offset: usize) -> Result<Option<&[u8]>> {
    (offset != 0).then(|| resource(bytes, offset)).transpose()
}

type AttachmentModels<'a> = (Option<&'a [u8]>, Option<&'a [u8]>);

fn attachment_models(record: &[u8], slot: usize) -> Result<AttachmentModels<'_>> {
    // The menu independently admits a missing primary and a first-slot extra.
    let primary = optional(record, word(record, 8)? as usize)?;
    let extra = if slot == 0 && word(record, 0)? >= 6 {
        optional(record, word(record, 0x18)? as usize)?
    } else {
        None
    };
    Ok((primary, extra))
}

pub(super) fn bind(source: &Source<'_>, id: u8) -> Result<ModelPreview> {
    let (directories, bytes) =
        source.candidates(&format!("battle/all/enemy-{id}/menu-preview.json"))?;
    let mut model: ModelPreview = serde_json::from_slice(&bytes)?;
    for part in &mut model.parts {
        for path in std::iter::once(&mut part.scene.mesh).chain(&mut part.scene.textures) {
            source.verify_file(&directories, path)?;
            *path = format!("{}/{path}", directories[0]);
        }
    }
    model.validate()?;
    Ok(model)
}

/// Retain the menu layers before combat adds required rigs, outlines and trails.
/// The record shares their existing meshes and textures; no second conversion.
pub(crate) fn publish(model: &ModelPreview, metadata: &[u8], id: u8, output: &Path) -> Result<()> {
    let mut model = model.clone();
    let mut body = 0;
    for part in &mut model.parts {
        if part
            .attached_to
            .as_ref()
            .is_some_and(|bone| bone.starts_with("kk"))
        {
            part.animation = None;
            part.scene.autoplay = false;
        } else {
            if part.scene.outline_color.is_none() {
                body += 1;
            }
            let slot = if body == 1 {
                0
            } else {
                ensure!(
                    body == 2 && half(metadata, 0xb4)? & 0x8400 != 0,
                    "unexpected monster preview body"
                );
                u16::from(
                    *metadata
                        .get(0xbd)
                        .context("missing secondary preview clip")?,
                )
            };
            part.animation = part
                .scene
                .clips
                .iter()
                .any(|clip| clip.resource_slot == slot)
                .then_some(slot);
            part.scene.autoplay = part.animation.is_some();
        }
    }
    model.validate()?;
    write_atomic(
        &output.join(format!("battle/all/enemy-{id}/menu-preview.json")),
        &serde_json::to_vec(&model)?,
    )
}

pub(crate) fn with_clips(
    bytes: &[u8],
    metadata: &[u8],
    id: u8,
    name: &str,
    clips: &[crate::scene::SourceClip<'_>],
    output: &Path,
) -> Result<ModelPreview> {
    let at = |offset| -> Result<&[u8]> { resource(bytes, word(bytes, offset)? as usize) };
    let maybe =
        |offset| -> Result<Option<&[u8]>> { optional(bytes, word(bytes, offset)? as usize) };
    let mut parts = Vec::new();
    layers_with_clips(
        Layer {
            model: at(0x18)?,
            outline: maybe(0x1c)?,
            animation: if clips.is_empty() { maybe(0x20)? } else { None },
            attached_to: None,
            additive: false,
        },
        &mut parts,
        name,
        clips,
        output,
    )?;
    let bones = parts[0].scene.bone_names.clone();
    let bone = |prefix: &str| -> Result<String> {
        bones
            .iter()
            .find(|name| name.starts_with(prefix))
            .cloned()
            .with_context(|| format!("missing preview attachment bone {prefix}"))
    };
    let flags = half(metadata, 0xb4)?;
    if flags & 0x8400 != 0 {
        layers_with_clips(
            Layer {
                model: at(0x180)?,
                outline: maybe(0x198)?,
                animation: if clips.is_empty() {
                    maybe(0x20 + usize::from(metadata[0xbd]) * 4)?
                } else {
                    None
                },
                attached_to: (flags & 0x8000 != 0).then(|| bone("pa00")).transpose()?,
                additive: false,
            },
            &mut parts,
            name,
            clips,
            output,
        )?;
    }
    let count = usize::from(metadata[0x1e4]);
    ensure!(count <= 8, "too many enemy attachments");
    let mut extra = None;
    for i in 0..count {
        let record = at(0x160 + i * 4)?;
        let (primary, additional) = attachment_models(record, i)?;
        if let Some(model) = primary {
            layers(
                Layer {
                    model,
                    outline: None,
                    animation: None,
                    attached_to: Some(bone(&format!("kk0{i}"))?),
                    additive: metadata[0x124 + i * 24] & 0x10 != 0,
                },
                &mut parts,
                name,
                output,
            )?;
        }
        extra = extra.or(additional);
    }
    if let Some(model) = extra {
        layers(
            Layer {
                model,
                outline: None,
                animation: None,
                attached_to: Some(bone("kk00")?),
                additive: true,
            },
            &mut parts,
            name,
            output,
        )?;
    }
    let mut node_scales = Vec::new();
    match id {
        236..=238 => node_scales.push(NodeScale {
            bone: bone("kk00")?,
            scale: [0.; 3],
            unless_flag: None,
        }),
        208..=210 => node_scales.push(NodeScale {
            bone: bone("kk06_Hane")?,
            scale: [0.5; 3],
            unless_flag: None,
        }),
        191 => {
            for (index, flag) in [(0x46, 0x93), (0x20, 0x94), (0x3f, 0x94)] {
                node_scales.push(NodeScale {
                    bone: bones
                        .get(index)
                        .context("missing Sword Dancer body part")?
                        .clone(),
                    scale: [0.; 3],
                    unless_flag: Some(flag),
                });
            }
        }
        _ => {}
    }
    Ok(ModelPreview {
        scale: float(metadata, 0x11c)?,
        elevation: float(metadata, 0x120)?,
        parts,
        hidden_geometry: bones
            .into_iter()
            .filter(|name| {
                let prefix = name.get(..2).unwrap_or("");
                prefix.eq_ignore_ascii_case("kk") || prefix.eq_ignore_ascii_case("pa")
            })
            .collect(),
        node_scales,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_menu_keeps_null_primary_extra_without_a_combat_visual() -> Result<()> {
        use resonance_content::{SceneClip, ScenePart, model_preview::PreviewPart};

        let root = crate::temporary_path(&std::env::temp_dir().join("menu-attachments"));
        fs::create_dir(&root)?;
        let result = (|| -> Result<()> {
            let mut record = vec![0; 29];
            record[..4].copy_from_slice(&6_u32.to_be_bytes());
            record[24..28].copy_from_slice(&28_u32.to_be_bytes());
            let (primary, extra) = attachment_models(&record, 0)?;
            assert!(primary.is_none() && extra.is_some());
            let part = |name: &str, attached: bool| PreviewPart {
                scene: ScenePart {
                    resource: u16::from(attached),
                    mesh: format!("{name}.glb"),
                    textures: vec![format!("{name}.ktx2")],
                    materials: vec![],
                    appearance: None,
                    translation: [0.; 3],
                    clips: vec![SceneClip {
                        resource_slot: 0,
                        duration_seconds: 1.,
                        animation_resource: None,
                        secondary_pose_nodes: vec![],
                    }],
                    autoplay: false,
                    texture_animations: vec![],
                    bone_names: vec!["kk00".into()],
                    material_nodes: vec![],
                    outline_color: None,
                    secondary_motion: Default::default(),
                },
                animation: None,
                attached_to: attached.then(|| "kk00".into()),
                additive: attached,
                uv_offsets: vec![],
            };
            let model = ModelPreview {
                scale: 2.,
                elevation: 3.,
                parts: std::iter::once(part("body", false))
                    .chain(primary.map(|_| part("primary", true)))
                    .chain(extra.map(|_| part("extra", true)))
                    .collect(),
                hidden_geometry: vec!["kk00".into()],
                node_scales: vec![],
            };
            let directory = root.join("assets/shared");
            for part in &model.parts {
                for path in std::iter::once(&part.scene.mesh).chain(&part.scene.textures) {
                    write_atomic(&directory.join(path), b"existing converted resource")?;
                }
            }
            publish(&model, &[], 0, &directory)?;
            assert!(model.parts.iter().all(|p| p.animation.is_none()));
            write_atomic(
                &root.join("sources.json"),
                &serde_json::to_vec(&serde_json::json!({
                    "disc1/renamed/enemies.bin": ["assets/shared"],
                    "disc2/renamed/enemies.bin": ["assets/shared"]
                }))?,
            )?;
            for disc in [1, 2] {
                let source = Source::open(&root, disc, "renamed/enemies.bin")?;
                let bound = bind(&source, 0)?;
                assert_eq!((bound.scale, bound.elevation), (2., 3.));
                assert_eq!(bound.parts.len(), 2);
                assert_eq!(bound.parts[0].animation, Some(0));
                let extra = &bound.parts[1];
                assert_eq!(extra.scene.mesh, "assets/shared/extra.glb");
                assert_eq!(extra.scene.textures, ["assets/shared/extra.ktx2"]);
                assert_eq!(extra.attached_to.as_deref(), Some("kk00"));
                assert!(extra.additive && extra.animation.is_none() && !extra.scene.autoplay);
                assert!(!directory.join("battle/all/visuals/enemy-0.json").exists());
            }
            fs::remove_file(directory.join("extra.glb"))?;
            assert!(bind(&Source::open(&root, 1, "renamed/enemies.bin")?, 0).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn optional_attachment_primary_keeps_first_slot_extra_and_rejects_bad_offsets() -> Result<()> {
        let mut record = vec![0; 28];
        record[..4].copy_from_slice(&6_u32.to_be_bytes());
        record[24..28].copy_from_slice(&28_u32.to_be_bytes());
        record.extend(b"extra model");
        assert_eq!(
            attachment_models(&record, 0)?,
            (None, Some(&b"extra model"[..]))
        );
        assert_eq!(attachment_models(&record, 1)?, (None, None));
        record[8..12].copy_from_slice(&28_u32.to_be_bytes());
        assert_eq!(attachment_models(&record, 1)?.0, Some(&b"extra model"[..]));
        record[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(attachment_models(&record, 0).is_err());
        record[8..12].fill(0);
        record[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(attachment_models(&record, 0).is_err());
        assert!(attachment_models(&record[..24], 0).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; reads bounded enemy packages without conversion"]
    fn original_declared_attachments_all_have_battle_primaries() -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let extracted = root.join(disc);
            let sources = crate::battle::all::Sources::read(&extracted)?;
            let files = extracted.join("files");
            let usual = fs::read(files.join(sources.usual))?;
            let directory = &usual[crate::field::sections(&usual)?[10]
                .clone()
                .context("enemy directory")?];
            let offsets = directory
                .chunks_exact(4)
                .map(|row| word(row, 0))
                .collect::<Result<Vec<_>>>()?;
            let mut archive = fs::File::open(files.join(sources.enemy))?;
            let ranges =
                crate::battle::all::physical_ranges(&offsets, 0, archive.metadata()?.len())?;
            let (mut packages, mut attachments, mut extras) = (0, 0, 0);
            for (id, range) in ranges {
                archive.seek(SeekFrom::Start(range.start as u64))?;
                let mut packed = vec![0; range.len()];
                archive.read_exact(&mut packed)?;
                let bytes = crate::compression::decode(&packed)?;
                let metadata = usize::from(half(&bytes, 4)?);
                let count = usize::from(
                    *bytes
                        .get(metadata + 0x1e4)
                        .context("enemy attachment count")?,
                );
                ensure!(count <= 8, "enemy {id} has too many attachments");
                for slot in 0..count {
                    let record = resource(&bytes, word(&bytes, 0x160 + slot * 4)? as usize)?;
                    let (primary, extra) = attachment_models(record, slot)?;
                    ensure!(
                        primary.is_some(),
                        "{disc} enemy {id} slot {slot} reaches menu-only null-primary branch"
                    );
                    attachments += 1;
                    extras += usize::from(extra.is_some());
                }
                packages += 1;
            }
            assert!(packages > 0 && attachments > 0 && extras > 0);
            eprintln!(
                "{disc}: {packages} enemy packages, {attachments} declared attachment primaries, {extras} menu extras, no null primaries"
            );
        }
        Ok(())
    }
}
