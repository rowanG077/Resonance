//! Enemy weapons are model packages, independent of the party item catalog.
use crate::{
    field::sections,
    model_preview::{Layer, layers},
    read::u32 as word,
    scene::SourceClip,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::visual::{AttachmentVisual, Rig},
    model_preview::ModelPreview,
};
use std::{collections::BTreeMap, path::Path};

#[allow(clippy::too_many_arguments)]
pub(super) fn cook(
    bytes: &[u8],
    metadata: &[u8],
    body: &Rig,
    model: &mut ModelPreview,
    name: &str,
    output: &Path,
    trails: &mut super::trails::Cooker,
    atlas: Option<super::trails::Atlas<'_>>,
) -> Result<BTreeMap<u8, AttachmentVisual>> {
    let count = *metadata
        .get(0x1e4)
        .context("missing enemy attachment count")?;
    ensure!(count <= 8, "too many enemy attachments");
    // Replace the catalogue's static KK layers while preserving body/PA parts.
    model.parts.retain(|part| {
        !part.attached_to.as_ref().is_some_and(|bone| {
            bone.get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("kk"))
        })
    });
    let mut result = BTreeMap::new();
    for slot in 0..count {
        let offset = word(bytes, 0x160 + usize::from(slot) * 4)? as usize;
        ensure!(offset != 0, "missing declared enemy attachment {slot}");
        let package = bytes
            .get(offset..)
            .context("enemy attachment exceeds package")?;
        let ranges = sections(package)?;
        ensure!(
            (5..=7).contains(&ranges.len()),
            "unsupported enemy attachment package"
        );
        let at = |index: usize| {
            ranges
                .get(index)
                .and_then(Option::as_ref)
                .map(|r| &package[r.clone()])
        };
        let bone = *body
            .weapon_bones
            .get(&slot)
            .context("missing enemy attachment anchor")?;
        let anchor = body
            .skeleton
            .bones
            .get(usize::from(bone))
            .context("invalid enemy attachment anchor")?;
        let flags = *metadata
            .get(0x124 + usize::from(slot) * 24)
            .context("missing enemy attachment flags")?;
        // The battle constructor always initializes member 1. The menu's
        // optional-primary branch does not make an empty combat rig valid.
        let primary = at(1).context("missing enemy attachment model")?;
        let clips = at(3)
            .map(|bytes| SourceClip {
                slot: 0,
                bytes,
                resource: None,
            })
            .into_iter()
            .collect::<Vec<_>>();
        let rig = super::rig(primary, &clips, super::RigKind::Weapon)?;
        let trail_bones = trail_bones(&rig)?;
        let trail_style = (trail_bones.len() >= 2)
            .then(|| {
                trails.cook(
                    super::trails::Recipe::EnemyWeapon(
                        at(0).context("missing enemy trail recipe")?,
                    ),
                    atlas.as_ref().map(|atlas| super::trails::Atlas {
                        kind: atlas.kind,
                        key: atlas.key,
                        bytes: atlas.bytes,
                    }),
                )
            })
            .transpose()?;
        let start = model.parts.len();
        let name = format!("{name}/attachments/{slot}");
        layers(
            Layer {
                model: primary,
                outline: at(2),
                animation: at(3),
                attached_to: Some(anchor.name.clone()),
                additive: flags & 0x10 != 0,
            },
            &mut model.parts,
            &name,
            output,
        )?;
        if let Some(extra) = at(5) {
            // The enemy constructor binds the extra model without an ANM controller.
            ensure!(
                at(6).is_none(),
                "enemy attachment extra motion needs a playback binding"
            );
            layers(
                Layer {
                    model: extra,
                    outline: None,
                    animation: None,
                    attached_to: Some(anchor.name.clone()),
                    additive: true,
                },
                &mut model.parts,
                &name,
                output,
            )?;
        }
        result.insert(
            slot,
            AttachmentVisual {
                bone,
                parts: (start..model.parts.len())
                    .map(u16::try_from)
                    .collect::<Result<_, _>>()?,
                trail_bones,
                trail_style,
                rig,
                toon: flags & 2 != 0,
                follow_bone: flags & 0x80 == 0,
            },
        );
    }
    Ok(result)
}

pub(super) fn trail_bones(rig: &Rig) -> Result<Vec<u16>> {
    let mut bones = [0; 5];
    let mut count = 0;
    for (index, bone) in rig.skeleton.bones.iter().enumerate() {
        let name = bone.name.as_bytes();
        if !name
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"ki"))
        {
            continue;
        }
        let slot = name.get(3).copied().context("truncated trail bone name")?;
        ensure!(
            matches!(slot, b'0'..=b'4'),
            "invalid trail endpoint {}",
            bone.name
        );
        // Every KI node increments the ribbon length; repeated labels replace
        // their cell in the zero-initialized endpoint array.
        bones[usize::from(slot - b'0')] = u16::try_from(index)?;
        count += 1;
        ensure!(count <= bones.len(), "too many trail endpoints");
    }
    Ok(bones[..count].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compression, digest, read::u16 as half};
    use std::fs;

    #[test]
    #[ignore = "requires privately extracted US enemy archives and main.dol; no cooking or rendering"]
    fn original_commander_keeps_autonomous_motion_and_authored_at_ki_branches() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let directory = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
        let dol = fs::read(extracted.join("sys/main.dol")).unwrap();
        // fn_8006EB68 initializes every package controller from this native constant.
        assert_eq!(
            crate::dol::slice(&dol, 0x8035b8ac, 4).unwrap(),
            &0.5f32.to_be_bytes()
        );
        let table = word(&directory, 0x2c).unwrap() as usize;
        let start = word(&directory, table + 101 * 4).unwrap() as usize;
        let end = word(&directory, table + 102 * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        assert_eq!(
            digest(&bytes),
            "4b89e421078a1475a7b4f6b4090c387b46f290d9ccb8a03e6d077166f8328498"
        );
        let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
        assert_eq!(metadata[0x1e4], 2);
        for (slot, expected_model, attacks, trails) in [
            (
                0,
                "7ec1597d33dd8d0c231b0e475a2fd1384b56a7755d3c18bb5a695034211b4e08",
                [6, 7],
                [8, 9],
            ),
            (
                1,
                "9f4fbf0bd64238d8f4f11ea5419eba2e57fe3dd0f6329daf39517399ffaab4a9",
                [1, 2],
                [3, 4],
            ),
        ] {
            assert_eq!(
                metadata[0x124 + slot * 24],
                0,
                "ordinary body-following attachment"
            );
            let package = &bytes[word(&bytes, 0x160 + slot * 4).unwrap() as usize..];
            let ranges = sections(package).unwrap();
            let at = |i: usize| {
                ranges
                    .get(i)
                    .and_then(Option::as_ref)
                    .map(|range| &package[range.clone()])
            };
            let primary = at(1).unwrap();
            assert_eq!(digest(primary), expected_model);
            let clips = at(3)
                .map(|bytes| SourceClip {
                    slot: 0,
                    bytes,
                    resource: None,
                })
                .into_iter()
                .collect::<Vec<_>>();
            let rig = super::super::rig(primary, &clips, super::super::RigKind::Weapon).unwrap();
            assert_eq!(rig.attack_groups[&0], attacks);
            assert_eq!(trail_bones(&rig).unwrap(), trails);
            if slot == 0 {
                assert_eq!(
                    digest(at(3).unwrap()),
                    "2bdd4ffa43ac7f0d4aa66f4ab0fe9fab7fc11f0820e78790c6c57703d90a825c"
                );
                assert_eq!(rig.motions.keys().copied().collect::<Vec<_>>(), [0]);
                let motion = &rig.motions[&0];
                assert_eq!(motion.duration_frames, 60.);
                assert_eq!(
                    motion
                        .tracks
                        .iter()
                        .map(|track| track.bone)
                        .collect::<Vec<_>>(),
                    [1, 5]
                );
                let first = rig.skeleton.sample(motion, 0.).unwrap();
                let later = rig.skeleton.sample(motion, 30.).unwrap();
                assert_ne!(first.local[1], later.local[1]);
                assert_ne!(first.local[5], later.local[5]);
                // The spinning rings do not replace the independent AT/KI skeleton branches.
                for bone in attacks.into_iter().chain(trails) {
                    assert_eq!(
                        first.point(bone, [0.; 3]).unwrap(),
                        later.point(bone, [0.; 3]).unwrap()
                    );
                }
            } else {
                assert!(rig.motions.is_empty());
            }
        }
    }
}
