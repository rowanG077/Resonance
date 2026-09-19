use super::*;
use crate::scene::recovered::RecoveredModels;
use crate::{
    model_preview::{Layer, PointerMembers, layers, layers_with_clips},
    read::{f32 as float, u16 as half},
};
use resonance_content::model_preview::ModelPreview;

fn attachment_models<T>(members: &[Option<T>], slot: usize) -> (Option<&T>, Option<&T>) {
    // The menu independently admits a missing primary and a first-slot extra.
    (
        members.get(1).and_then(Option::as_ref),
        (slot == 0)
            .then(|| members.get(5).and_then(Option::as_ref))
            .flatten(),
    )
}

pub(super) fn menu(mut model: ModelPreview, metadata: &[u8]) -> Result<ModelPreview> {
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
    Ok(model)
}

pub(crate) fn from_package(
    bytes: &[u8],
    metadata: &[u8],
    id: u8,
    clips: &[u16],
    output: &Path,
    recovered: Option<&RecoveredModels>,
) -> Result<ModelPreview> {
    let members = PointerMembers::new(bytes, 0x18..0x1e8)?;
    let animations = clips
        .iter()
        .map(|&slot| -> Result<_> {
            Ok((
                slot,
                members
                    .animation(0x20 + usize::from(slot) * 4)?
                    .context("missing enemy animation")?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let bound = animations
        .iter()
        .map(|(slot, animation)| crate::character::Clip {
            slot: *slot,
            resource: None,
            animation,
        })
        .collect::<Vec<_>>();
    let name = &format!("monsters/{id:03}");
    let clips = &bound;
    let mut parts = Vec::new();
    let idle = if clips.is_empty() {
        members.animation(0x20)?
    } else {
        None
    };
    layers_with_clips(
        Layer {
            recovered,
            model: members
                .model(0x18)?
                .context("missing enemy primary model")?,
            outline: members.model(0x1c)?,
            animation: idle.as_ref(),
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
        let idle = if clips.is_empty() {
            members.animation(0x20 + usize::from(metadata[0xbd]) * 4)?
        } else {
            None
        };
        layers_with_clips(
            Layer {
                recovered,
                model: members
                    .model(0x180)?
                    .context("missing enemy secondary model")?,
                outline: members.model(0x198)?,
                animation: idle.as_ref(),
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
        let package = members
            .model(0x160 + i * 4)?
            .context("missing preview attachment package")?;
        let models = crate::field::sections(package)?
            .into_iter()
            .map(|range| range.map(|range| &package[range]))
            .collect::<Vec<_>>();
        let (primary, additional) = attachment_models(&models, i);
        if let Some(model) = primary {
            layers(
                Layer {
                    recovered,
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
        extra = extra.or_else(|| additional.cloned());
    }
    if let Some(model) = extra {
        layers(
            Layer {
                recovered,
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
        behavior: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::u32 as word;

    #[test]
    fn shared_menu_keeps_null_primary_extra_without_a_combat_visual() -> Result<()> {
        use resonance_content::{SceneClip, ScenePart, model_preview::PreviewPart};

        let members = [None, None, None, None, None, Some("extra")];
        let (primary, extra) = attachment_models(&members, 0);
        assert!(primary.is_none() && extra.is_some());
        let part = |name: &str, attached: bool| PreviewPart {
            scene: ScenePart {
                resource: u16::from(attached),
                mesh: format!("meshes/{name}.glb"),
                textures: vec![format!("textures/{name}.ktx2")],
                materials: vec![],
                appearance: None,
                translation: [0.; 3],
                clips: vec![SceneClip {
                    motion: "clips/test.motion".into(),
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
            behavior: None,
        };
        let bound = menu(model.clone(), &[])?;
        assert!(model.parts.iter().all(|p| p.animation.is_none()));
        assert_eq!((bound.scale, bound.elevation), (2., 3.));
        assert_eq!(bound.parts.len(), 2);
        assert_eq!(bound.parts[0].animation, Some(0));
        let extra = &bound.parts[1];
        assert_eq!(extra.scene.mesh, "meshes/extra.glb");
        assert_eq!(extra.scene.textures, ["textures/extra.ktx2"]);
        assert_eq!(extra.attached_to.as_deref(), Some("kk00"));
        assert!(extra.additive && extra.animation.is_none() && !extra.scene.autoplay);
        Ok(())
    }

    #[test]
    fn optional_attachment_primary_keeps_first_slot_extra() {
        let mut members = [None, None, None, None, None, Some("extra")];
        assert_eq!(attachment_models(&members, 0), (None, Some(&"extra")));
        assert_eq!(attachment_models(&members, 1), (None, None));
        members[1] = Some("primary");
        assert_eq!(
            attachment_models(&members, 0),
            (Some(&"primary"), Some(&"extra"))
        );
        assert_eq!(
            attachment_models(&members[..5], 0),
            (Some(&"primary"), None)
        );
    }

    #[test]
    #[ignore = "requires both original discs; reads bounded enemy packages without conversion"]
    fn original_declared_attachments_all_have_battle_primaries() -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let extracted = root.join(disc);
            let sources = crate::source_assets::Sources::read(&extracted)?;
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
                crate::source_assets::physical_ranges(&offsets, 0, archive.metadata()?.len())?;
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
                    let offset = word(&bytes, 0x160 + slot * 4)? as usize;
                    ensure!(offset != 0, "missing original attachment package");
                    let record = bytes
                        .get(offset..)
                        .context("original attachment exceeds package")?;
                    let members = crate::field::sections(record)?;
                    let (primary, extra) = attachment_models(&members, slot);
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
