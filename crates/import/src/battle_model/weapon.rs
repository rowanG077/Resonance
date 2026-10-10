//! Bounded weapon model packages and attachment bindings.
use crate::{
    field::{MapArchive, sections},
    model_preview::{self, Layer},
    rel::Rel,
    scene::decoded::Package,
    source_assets::{Sources, physical_ranges, read_range},
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_model::{
    Attachment, ModelPart, TrailMaterial, TrailTexture, WEAPONS_PATH, Weapon, Weapons,
};
use std::{collections::BTreeMap, fs, path::Path};

/// Shared production publisher, also used by the temporary development cooker.
pub fn publish(extracted: &Path, output: &Path) -> Result<String> {
    publish_source(extracted, &Sources::read(extracted)?, output)
}

pub(crate) fn publish_source(extracted: &Path, sources: &Sources, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let items = crate::item::read(&executable)?;
    let owner_archive = MapArchive::open(
        &extracted
            .join("files")
            .join(owner_motion_path(extracted, &executable)?),
    )?;
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let table = module
        .at((5, 0x6e8))?
        .get(..158 * 4)
        .context("truncated weapon directory")?;
    let offsets = table
        .chunks_exact(4)
        .map(|row| crate::read::u32(row, 0))
        .collect::<Result<Vec<_>>>()?;
    let path = extracted.join("files").join(&sources.weapons);
    let length = fs::metadata(&path)?.len();
    let mut decoded = Package::default();
    let owner_motions = owner_archive
        .sections
        .iter()
        .enumerate()
        .skip(62)
        .filter_map(|(member, range)| range.as_ref().map(|range| (member, range)))
        .map(|(member, range)| {
            let bytes = &owner_archive.bytes[range.clone()];
            Ok((
                (member - 2).try_into()?,
                decoded.decode_animation(bytes, || crate::animation::read_member(bytes))?,
            ))
        })
        .collect::<Result<Vec<(u16, _)>>>()?;
    let owner_clips = owner_motions
        .iter()
        .map(|(slot, animation)| crate::character::Clip {
            slot: *slot,
            resource: None,
            animation,
        })
        .collect::<Vec<_>>();
    let mut records = BTreeMap::new();
    for (id, range) in physical_ranges(&offsets, 0, length)? {
        let archive = MapArchive::decode(&read_range(&path, range.clone())?)?;
        // DOL item category 17 is the original owner-linked kendama family.
        let linked = id < 139 && items[usize::from(id) + 135].category == 17;
        records.insert(
            range.start as u32,
            weapon(
                &archive,
                if linked { &owner_clips } else { &[] },
                output,
                &mut decoded,
            )?,
        );
    }
    let count = offsets
        .iter()
        .position(|&offset| u64::from(offset) == length)
        .context("missing weapon archive end")?;
    let mut attachments = BTreeMap::new();
    for (index, offset) in offsets[..count].iter().enumerate() {
        if index != 0 && *offset == 0 {
            continue;
        }
        let source = records.get(offset).context("missing weapon package")?;
        for (item, shield) in attachment_keys(index.try_into()?) {
            let attachment = if shield {
                Attachment::Shield(source.clone())
            } else {
                Attachment::Weapon(source.clone())
            };
            ensure!(
                attachments.insert(item, attachment).is_none(),
                "duplicate attachment item {item}"
            );
        }
    }
    for (item, definition) in items.iter().enumerate() {
        if matches!(definition.category, 32..=34) {
            ensure!(
                attachments
                    .insert(item.try_into()?, Attachment::Nonvisual)
                    .is_none(),
                "nonvisual equipment has a model"
            );
        }
    }
    let bank = Weapons {
        source_sha256: crate::media::hash_file(&path)?,
        table_sha256: crate::digest(table),
        owner_motion_sha256: owner_archive.source_sha256.clone(),
        records: attachments,
    };
    crate::write_atomic(&output.join(WEAPONS_PATH), &serde_json::to_vec(&bank)?)?;
    Ok(WEAPONS_PATH.into())
}

/// Resolve the binary directory's slots only while publishing the native catalogue.
fn attachment_keys(slot: u16) -> impl Iterator<Item = (u16, bool)> {
    let key = match slot {
        0..=138 => (slot + 135, false),
        139..=149 => (slot + 217, true),
        _ => (slot + 379, false),
    };
    std::iter::once(key).chain((slot == 149).then_some((528, false)))
}

pub(crate) fn owner_motion_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    let catalogue = crate::resource::read(executable)?;
    crate::field_resources::resolve_path(
        &extracted.join("files"),
        catalogue.party(crate::resource::PartyResource::BattleMotion, 3, 0)?,
    )
}

fn weapon(
    archive: &MapArchive,
    owner_clips: &[crate::character::Clip<'_>],
    output: &Path,
    decoded: &mut Package,
) -> Result<Weapon> {
    // Multi-part equipment wraps each layer package in a separate directory member.
    let packages = if sections(archive.section(0)?).is_ok() {
        archive
            .sections
            .iter()
            .enumerate()
            .filter_map(|(slot, range)| {
                range
                    .as_ref()
                    .map(|range| (slot, &archive.bytes[range.clone()]))
            })
            .collect::<Vec<_>>()
    } else {
        vec![(0, archive.bytes.as_slice())]
    };
    let mut parts = BTreeMap::new();
    let mut trails = BTreeMap::new();
    for (slot, package) in packages {
        let ranges = sections(package)?;
        ensure!(
            (5..=7).contains(&ranges.len()),
            "invalid weapon layer package"
        );
        let at = |index: usize| {
            ranges
                .get(index)
                .and_then(Option::as_ref)
                .map(|range| &package[range.clone()])
        };
        let metadata = at(0).context("missing weapon metadata")?;
        trails.insert(slot.try_into()?, trail_material(metadata)?);
        let primary = at(1).context("missing weapon body")?;
        let mut layers = Vec::new();
        let mut layer_skeletons = BTreeMap::new();
        for (independent, model, outline, motion) in [
            (false, Some(primary), at(2), at(3)),
            (true, at(5), None, at(6)),
        ] {
            let Some(model) = model else { continue };
            let animation = motion
                .map(|bytes| {
                    decoded.decode_animation(bytes, || crate::animation::read_member(bytes))
                })
                .transpose()?;
            ensure!(
                owner_clips.is_empty() || motion.is_none(),
                "owner-linked weapon also has local animation"
            );
            let first = layers.len();
            model_preview::layers_with_clips(
                Layer {
                    model,
                    outline,
                    animation: animation.as_deref(),
                    attached_to: None,
                    additive: false,
                },
                &mut layers,
                &format!("battle/weapons/{}/{slot}", archive.source_sha256),
                owner_clips,
                output,
                decoded,
            )?;
            if independent {
                let skeleton = super::rig(model, super::RigKind::Effect)?.skeleton;
                for layer in &layers[first..] {
                    layer_skeletons.insert(layer.scene.resource, skeleton.clone());
                }
            }
        }
        parts.insert(
            slot.try_into()?,
            ModelPart {
                rig: super::rig(primary, super::RigKind::Weapon)?,
                layer_skeletons,
                layers,
            },
        );
    }
    Ok(Weapon {
        source_sha256: archive.source_sha256.clone(),
        files: super::files(
            parts
                .values()
                .flat_map(|part| part.layers.iter().map(|layer| &layer.scene)),
            output,
        )?,
        parts,
        trails,
    })
}

pub(super) fn trail_material(bytes: &[u8]) -> Result<TrailMaterial> {
    let bytes = bytes
        .get(..0x1c)
        .context("truncated weapon trail metadata")?;
    ensure!(bytes[0x0f] <= 1, "unsupported weapon trail blend");
    Ok(TrailMaterial {
        texture: match bytes[0x0d] as i8 {
            ..0 => None,
            2 => Some(TrailTexture::Enemy { slot: 2 }),
            slot => Some(TrailTexture::Common { slot: slot as u8 }),
        },
        palette: bytes[0x0e],
        additive: bytes[0x0f] == 1,
        color: bytes[0x10..0x13].try_into()?,
        uv: std::array::from_fn(|i| i16::from_be_bytes([bytes[0x14 + i * 2], bytes[0x15 + i * 2]])),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trail_material_decodes_texture_ownership_and_rejects_unknown_blends() -> Result<()> {
        let mut bytes = [0; 0x1c];
        for (source, expected) in [
            (255, None),
            (0, Some(TrailTexture::Common { slot: 0 })),
            (2, Some(TrailTexture::Enemy { slot: 2 })),
        ] {
            bytes[0x0d] = source;
            for blend in [0, 1] {
                bytes[0x0f] = blend;
                let material = trail_material(&bytes)?;
                assert_eq!(material.texture, expected);
                assert_eq!(material.additive, blend == 1);
            }
        }
        for blend in [2, 3, 4, 255] {
            bytes[0x0f] = blend;
            assert!(trail_material(&bytes).is_err());
        }
        assert!(trail_material(&bytes[..27]).is_err());
        Ok(())
    }

    fn attachment_model(attachment: &Attachment) -> Result<&Weapon> {
        match attachment {
            Attachment::Weapon(model) | Attachment::Shield(model) => Ok(model),
            Attachment::Nonvisual => anyhow::bail!("attachment has no model"),
        }
    }

    #[test]
    fn published_attachment_keys_preserve_shields_and_carried_prop_alias() {
        assert_eq!(attachment_keys(0).collect::<Vec<_>>(), [(135, false)]);
        assert_eq!(attachment_keys(138).collect::<Vec<_>>(), [(273, false)]);
        assert_eq!(attachment_keys(139).collect::<Vec<_>>(), [(356, true)]);
        assert_eq!(
            attachment_keys(149).collect::<Vec<_>>(),
            [(366, true), (528, false)]
        );
        assert_eq!(attachment_keys(150).collect::<Vec<_>>(), [(529, false)]);
    }

    #[test]
    #[ignore = "requires both extracted original discs; publishes shared weapon models"]
    fn weapon_packages_publish_layers_and_attachment_items() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let path = publish(&local.join(format!("disc{disc}")), output.path())?;
            let bytes = fs::read(output.path().join(path))?;
            let bank: Weapons = serde_json::from_slice(&bytes)?;
            assert_eq!(bank.records.len(), 179);
            assert!(matches!(bank.records[&366], Attachment::Shield(_)));
            assert!(matches!(bank.records[&528], Attachment::Weapon(_)));
            assert!(matches!(bank.records[&367], Attachment::Nonvisual));
            assert_eq!(
                attachment_model(&bank.records[&366])?.source_sha256,
                attachment_model(&bank.records[&528])?.source_sha256
            );
            assert!(!bank.records.contains_key(&530));
            let sword = attachment_model(bank.records.get(&135).context("missing Wooden Blade")?)?;
            assert_eq!(sword.parts.keys().copied().collect::<Vec<_>>(), [0, 1]);
            for part in sword.parts.values() {
                assert!(part.layers[0].scene.clips.is_empty());
                part.rig.skeleton.bind_pose()?;
            }
            for (index, color, uv) in [
                (135, [64, 32, 32], [448, 0, 64, 64]),
                (159, [32, 32, 96], [448, 128, 64, 64]),
                (175, [96, 32, 32], [448, 128, 64, 64]),
            ] {
                for trail in attachment_model(&bank.records[&index])?.trails.values() {
                    assert_eq!(trail.texture, Some(TrailTexture::Common { slot: 0 }));
                    assert_eq!(trail.palette, 0);
                    assert!(trail.additive);
                    assert_eq!(trail.color, color);
                    assert_eq!(trail.uv, uv);
                }
            }
            let kendama = attachment_model(bank.records.get(&175).context("missing Kendama")?)?;
            let part = &kendama.parts[&0];
            assert_eq!(part.rig.skeleton.bones.len(), 15);
            assert_eq!(
                part.layers[0]
                    .scene
                    .clips
                    .iter()
                    .map(|clip| clip.resource_slot)
                    .collect::<Vec<_>>(),
                [60, 90, 91, 92, 103, 105]
            );
            for clip in &part.layers[0].scene.clips {
                let motion = resonance_content::animation::Motion::decode(&fs::read(
                    output.path().join(&clip.motion),
                )?)?;
                assert!(motion.tracks.iter().all(|track| track.bind_channels
                    == part.rig.skeleton.bones[usize::from(track.bone)].bind_channels));
            }
            for attachment in bank.records.values() {
                let (Attachment::Weapon(weapon) | Attachment::Shield(weapon)) = attachment else {
                    continue;
                };
                for part in weapon.parts.values() {
                    for layer in &part.layers {
                        let skeleton = part
                            .layer_skeletons
                            .get(&layer.scene.resource)
                            .unwrap_or(&part.rig.skeleton);
                        assert!(
                            skeleton
                                .bones
                                .iter()
                                .map(|bone| &bone.name)
                                .eq(layer.scene.bone_names.iter())
                        );
                        for clip in &layer.scene.clips {
                            resonance_content::animation::Motion::decode(&fs::read(
                                output.path().join(&clip.motion),
                            )?)?
                            .validate(skeleton)?;
                        }
                    }
                }
            }
            let rings = attachment_model(&bank.records[&171])?;
            for part in rings.parts.values() {
                assert_eq!(part.rig.skeleton.bones.len(), 8);
                assert_eq!(part.layer_skeletons[&1].bones.len(), 4);
                assert_ne!(
                    part.layers[0].scene.clips[0].motion,
                    part.layers[1].scene.clips[0].motion
                );
            }
            if let Some(first) = &first {
                assert_eq!(&bytes, first);
            }
            first = Some(bytes);
        }
        Ok(())
    }
}
