//! Bounded weapon model packages. Resource selection: 159BC/16060; AT labels: 153BC.
use crate::{
    field::{MapArchive, sections},
    model_preview::{self, Layer},
    rel::Rel,
    scene::decoded::Package,
    source_assets::{Sources, physical_ranges, read_range},
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_model::{ModelPart, TrailMaterial, WEAPONS_PATH, Weapon, Weapons};
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
    let records = offsets[..count]
        .iter()
        .enumerate()
        .map(|(index, offset)| {
            if index != 0 && *offset == 0 {
                Ok(None)
            } else {
                Ok(Some(
                    records
                        .get(offset)
                        .context("missing weapon package")?
                        .clone(),
                ))
            }
        })
        .collect::<Result<_>>()?;
    let bank = Weapons {
        source_sha256: crate::media::hash_file(&path)?,
        table_sha256: crate::digest(table),
        owner_motion_sha256: owner_archive.source_sha256.clone(),
        records,
    };
    crate::write_atomic(&output.join(WEAPONS_PATH), &serde_json::to_vec(&bank)?)?;
    Ok(WEAPONS_PATH.into())
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
        for (model, outline, motion) in [(Some(primary), at(2), at(3)), (at(5), None, at(6))] {
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
        }
        parts.insert(
            slot.try_into()?,
            ModelPart {
                rig: super::rig(primary, super::RigKind::Weapon)?,
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
    Ok(TrailMaterial {
        texture: bytes[0x0d] as i8,
        palette: bytes[0x0e],
        flags: bytes[0x0f],
        color: bytes[0x10..0x13].try_into()?,
        uv: std::array::from_fn(|i| i16::from_be_bytes([bytes[0x14 + i * 2], bytes[0x15 + i * 2]])),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted original discs; publishes shared weapon models"]
    fn original_weapon_packages_preserve_slots_and_rigid_sword_contact_points() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let path = publish(&local.join(format!("disc{disc}")), output.path())?;
            let bytes = fs::read(output.path().join(path))?;
            let bank: Weapons = serde_json::from_slice(&bytes)?;
            assert_eq!(
                bank.table_sha256,
                "14d6ad5e9869a478cd8aad371591760f9ac0c262c93fbf6ff461f9abb686c98d"
            );
            assert_eq!(bank.records.len(), 156);
            let sword = bank.records[0].as_ref().context("missing Wooden Blade")?;
            assert_eq!(sword.parts.keys().copied().collect::<Vec<_>>(), [0, 1]);
            for (&slot, part) in &sword.parts {
                assert!(part.layers[0].scene.clips.is_empty());
                let pose = part.rig.skeleton.bind_pose()?;
                let contacts = part.rig.attack_groups[&0]
                    .iter()
                    .map(|&bone| pose.point(bone, [0.; 3]))
                    .collect::<Result<Vec<_>>>()?;
                assert!(!contacts.is_empty());
                eprintln!("Wooden Blade {slot}: {contacts:?}");
            }
            for (index, color, uv) in [
                (0, [64, 32, 32], [448, 0, 64, 64]),
                (24, [32, 32, 96], [448, 128, 64, 64]),
                (40, [96, 32, 32], [448, 128, 64, 64]),
            ] {
                for trail in bank.records[index].as_ref().unwrap().trails.values() {
                    assert_eq!(trail.texture, 0);
                    assert_eq!(trail.palette, 0);
                    assert_eq!(trail.flags & 3, 1);
                    assert_eq!(trail.color, color);
                    assert_eq!(trail.uv, uv);
                }
            }
            let kendama = bank.records[40].as_ref().context("missing Kendama")?;
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
            for weapon in bank.records.iter().flatten() {
                for part in weapon.parts.values() {
                    assert!(part.rig.transform_kinds.iter().all(|&kind| kind == 1));
                    assert!(
                        part.rig
                            .skeleton
                            .bones
                            .iter()
                            .map(|bone| &bone.name)
                            .eq(part.layers[0].scene.bone_names.iter())
                    );
                    for clip in &part.layers[0].scene.clips {
                        resonance_content::animation::Motion::decode(&fs::read(
                            output.path().join(&clip.motion),
                        )?)?
                        .validate(&part.rig.skeleton)?;
                    }
                }
            }
            if let Some(first) = &first {
                assert_eq!(&bytes, first);
            }
            first = Some(bytes);
        }
        Ok(())
    }
}
