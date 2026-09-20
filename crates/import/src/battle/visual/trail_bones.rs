//! Body trails read two endpoints from each four-column authored row.
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{pose::Skeleton, visual::TrailVisual};
use std::collections::BTreeMap;

/// Missing cells remain explicit; an incomplete row cannot supply a ribbon.
pub(super) fn body(skeleton: &Skeleton) -> Result<BTreeMap<u8, [Option<u16>; 2]>> {
    let mut cells = BTreeMap::new();
    let mut count = 0u8;
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let name = bone.name.as_bytes();
        if !name
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"ki"))
        {
            continue;
        }
        let digits = name.get(2..4).context("truncated body trail bone name")?;
        ensure!(
            digits.iter().all(u8::is_ascii_digit),
            "invalid body trail bone {}",
            bone.name
        );
        let index =
            u16::from(u8::try_from(index).context("body trail bone exceeds native skeleton")?);
        // Repeated names overwrite their cell while still increasing the declared count.
        cells.insert(4 * (digits[0] - b'0') + digits[1] - b'0', index);
        count += 1;
        ensure!(count <= 16, "body trails exceed eight command slots");
    }
    // Columns two and three do not become another ribbon. Allocation uses the
    // number of KI nodes, independently of populated rows or duplicate names.
    Ok((0..count / 2)
        .map(|slot| {
            (
                slot,
                [
                    cells.get(&(4 * slot)).copied(),
                    cells.get(&(4 * slot + 1)).copied(),
                ],
            )
        })
        .collect())
}

/// A complete KI row binds body joints, including when KK attachments have no KI nodes.
pub(super) fn cook(
    skeleton: &Skeleton,
    metadata: &[u8],
    cooker: &mut super::trails::Cooker,
    atlas: Option<super::trails::Atlas<'_>>,
) -> Result<BTreeMap<u8, TrailVisual>> {
    let rows = body(skeleton)?
        .into_iter()
        .filter_map(|(slot, endpoints)| {
            // A requested incomplete row remains a strict runtime authoring error.
            // KI02/03 are columns in row zero; do not invent a second pair.
            let [Some(first), Some(second)] = endpoints else {
                return None;
            };
            Some((slot, vec![first, second]))
        })
        .collect::<BTreeMap<_, _>>();
    if rows.is_empty() {
        return Ok(BTreeMap::new());
    }
    let style = cooker.cook(super::trails::Recipe::EnemyBody(metadata), atlas)?;
    Ok(rows
        .into_iter()
        .map(|(slot, bones)| {
            (
                slot,
                TrailVisual {
                    bones,
                    style: style.clone(),
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::pose::Bone;

    fn groups(names: &[&str]) -> BTreeMap<u8, [Option<u16>; 2]> {
        body(&Skeleton {
            bones: names
                .iter()
                .map(|name| Bone {
                    bind_channels: Default::default(),
                    name: (*name).to_owned(),
                    parent: None,
                    bind: Default::default(),
                })
                .collect(),
        })
        .unwrap()
    }

    #[test]
    fn native_rows_preserve_zombie_gaps_and_overwrite_order() {
        assert_eq!(
            groups(&["KI00", "KI01", "KI02", "KI03"]),
            BTreeMap::from([(0, [Some(0), Some(1)]), (1, [None, None])])
        );
        assert_eq!(
            groups(&["ki10", "ki01", "ki00", "ki11"]),
            BTreeMap::from([(0, [Some(2), Some(1)]), (1, [Some(0), Some(3)])])
        );
        assert_eq!(
            groups(&["KI00", "KI01", "KI00", "KI01"]),
            BTreeMap::from([(0, [Some(2), Some(3)]), (1, [None, None])])
        );
    }

    #[test]
    #[ignore = "requires privately extracted US enemy archives; no cooking or rendering"]
    fn original_enemy72_body_trail_uses_ki_sword_pair_and_enemy_palette_seven() {
        use crate::{
            compression, digest,
            read::{u16 as half, u32 as word},
        };
        use std::{fs, path::Path};
        let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
        let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
        let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        let start = word(&usual, table + 72 * 4).unwrap() as usize;
        let end = word(&usual, table + 73 * 4).unwrap() as usize;
        assert_eq!(
            digest(&archive[start..end]),
            "2b14109aac30e3da6a40fe30bf7dc4a915fa152ffbbf3881339f0c08a5018d75"
        );
        let bytes = compression::decode(&archive[start..end]).unwrap();
        assert_eq!(
            digest(&bytes),
            "42a918fe45e0d0edb2fa388c32f1a7af68eb901384d720d7eb3d94030dadbf46"
        );
        let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
        let rig = super::super::rig(
            &bytes[word(&bytes, 0x18).unwrap() as usize..],
            &[],
            super::super::RigKind::Actor,
        )
        .unwrap();
        assert_eq!(
            body(&rig.skeleton).unwrap(),
            BTreeMap::from([(0, [Some(7), Some(8)])])
        );
        assert_eq!(rig.skeleton.bones[7].name, "ki00_Bone_ken06");
        assert_eq!(rig.skeleton.bones[8].name, "ki01_Bone_ken07");
        assert_eq!(&metadata[0x10c..0x10f], &[64, 64, 64]);
        assert_eq!(
            [0, 2, 4, 6].map(|offset| half(metadata, 0x110 + offset).unwrap()),
            [32, 0, 64, 64]
        );
        assert_eq!(&metadata[0x118..0x11b], &[2, 7, 1]);
        let atlas = super::super::enemy_atlas(&bytes, "enemy-72")
            .unwrap()
            .unwrap();
        assert_eq!(atlas.kind, 2);
        assert_eq!(
            digest(atlas.bytes),
            "8d8fbfb24bcec77aea19687724f0fbb7bb6a0b334972d3d63c4c859b4fcd319f"
        );
        let images = crate::tpl::parse_tpl(atlas.bytes).unwrap();
        assert_eq!(images.len(), 1);
        let image = &images[0];
        assert_eq!((image.width, image.height, image.format), (256, 256, 8));
        assert_eq!(image.palette_entries, 256);
        assert!(usize::from(metadata[0x119]) * 16 + 16 <= image.palette_entries);
    }
}
