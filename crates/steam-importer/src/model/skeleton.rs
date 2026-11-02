use super::*;
use resonance_content::animation::Transform;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, serde::Serialize)]
pub struct Node {
    pub binding: u32,
    pub name: String,
    pub flags: u16,
    pub parent: Option<u16>,
    /// Authored pose-buffer values; motion deltas compose with these directly.
    pub rest: Transform,
    /// Draw eligibility, independent of the node's transform and skin weights.
    pub visible: bool,
    pub global: [f32; 16],
    pub inverse_bind: [f32; 16],
}

// CAnimationRootBase::Apply (0x5dc660 -> 0x5def30) combines motion
// deltas with section 11's pose buffer, not decomposed bind matrices. Reading
// the original TRS preserves zero scales, mirrored axes and their orientation.
pub(super) fn read(
    source: &Reader<'_>,
    section: usize,
    pose: &bindings::Pose,
) -> Result<Vec<Node>> {
    let ids = source.table(section + 24, 4)?;
    let parents = source.table(section + 32, 4)?;
    let hierarchy = source.table(section + 40, 8)?;
    let names = source.table(section + 48, 4)?;
    let global = source.table(section + 64, 64)?;
    let inverse = source.table(section + 72, 64)?;
    let visibility_parents = source.table(section + 80, 2)?;
    // Native 0x5d04f0 points directly into this range of the pose buffer.
    // Verify the channel identities really occupy each node's scalar slot.
    let visibility_start =
        source.word(section + 4)? as usize + usize::from(source.half(section + 12)?);
    let mut offset = 0;
    let mut visibility_offsets = BTreeMap::new();
    for &(kind, binding) in &pose.channels {
        if kind == 0x80001 {
            visibility_offsets.insert(binding, offset);
        }
        offset += (kind & 15) as usize;
    }
    ensure!(
        ids.count < u16::MAX as usize
            && ids.count == pose.rest.len()
            && ids.count == pose.visible.len()
            && [
                &parents,
                &hierarchy,
                &names,
                &global,
                &inverse,
                &visibility_parents
            ]
            .iter()
            .all(|table| table.count == ids.count),
        "inconsistent Steam skeleton tables"
    );
    let matrices = (0..ids.count)
        .map(|i| source.floats::<16>(global.at(i)))
        .collect::<Result<Vec<_>>>()?;
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    for i in 0..ids.count {
        let row = hierarchy.at(i);
        let binding = source.word(ids.at(i))?;
        ensure!(
            visibility_offsets.get(&binding) == Some(&(visibility_start + i)),
            "Steam visibility channel differs from its skeleton pose slot"
        );
        ensure!(
            seen.insert(binding),
            "duplicate Steam node binding {binding}"
        );
        ensure!(
            source.half(row)? & 0x100 != 0
                && source.half(row + 2)? as usize == i
                && source.half(row + 6)? as usize == i,
            "unsupported Steam skeleton pose remapping"
        );
        let parent = source.signed(row + 4)?;
        // Native 0x5d08e0 inherits draw visibility independently. The installed
        // models use the transform hierarchy, which glTF can preserve directly.
        ensure!(
            source.signed(visibility_parents.at(i))? == parent,
            "Steam visibility hierarchy differs from transform hierarchy"
        );
        ensure!(
            parent == -1 || (parent >= 0 && (parent as usize) < ids.count && parent as usize != i),
            "invalid Steam node parent"
        );
        let parent = (parent >= 0).then_some(parent as u16);
        let rest = *pose
            .rest
            .get(&binding)
            .context("model transform channels differ from skeleton")?;
        let inverse_bind = source.floats::<16>(inverse.at(i))?;
        for values in [&matrices[i], &inverse_bind] {
            ensure!(
                values.iter().all(|v| v.is_finite())
                    && [3, 7, 11].iter().all(|&j| values[j].abs() < 1e-6)
                    && (values[15] - 1.).abs() < 1e-6,
                "non-affine Steam bind matrix"
            );
        }
        nodes.push(Node {
            binding,
            name: source.name(names.at(i))?,
            flags: source.half(row)?,
            parent,
            rest,
            visible: *pose
                .visible
                .get(&binding)
                .context("missing model rest visibility")?,
            global: matrices[i],
            inverse_bind,
        });
    }
    for i in 0..nodes.len() {
        let mut parent = nodes[i].parent;
        let mut depth = 0;
        while let Some(index) = parent {
            ensure!(
                usize::from(index) != i && depth < nodes.len(),
                "cyclic Steam skeleton"
            );
            parent = nodes[usize::from(index)].parent;
            depth += 1;
        }
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(bytes: &mut Vec<u8>, pointer: usize, content: &[u8]) -> usize {
        let start = bytes.len();
        bytes[pointer..pointer + 4].copy_from_slice(&((start - pointer) as u32).to_le_bytes());
        bytes[pointer + 4..pointer + 8].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(content);
        start
    }

    #[test]
    fn skeleton_keeps_group_ids_and_rejects_cycles_aliases_and_truncation() -> Result<()> {
        let mut bytes = vec![0; 96];
        table(
            &mut bytes,
            24,
            &[70000u32, 90000]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        table(
            &mut bytes,
            32,
            &[u32::MAX, 70000]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let hierarchy = table(
            &mut bytes,
            40,
            &[0x110i16, 0, -1, 0, 0x110, 1, 0, 1]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let names = table(&mut bytes, 48, &[0; 8]);
        for (i, name) in [b"root\0".as_slice(), b"child\0"].into_iter().enumerate() {
            let pointer = names + i * 4;
            let start = bytes.len();
            bytes.extend(name);
            bytes[pointer..pointer + 4].copy_from_slice(&((start - pointer) as u32).to_le_bytes());
        }
        let global = [
            glam::Mat4::from_translation(glam::Vec3::new(10., 0., 0.)),
            glam::Mat4::from_translation(glam::Vec3::new(20., 0., 0.)),
        ];
        table(
            &mut bytes,
            64,
            &global
                .iter()
                .flat_map(|m| m.to_cols_array().into_iter().flat_map(f32::to_le_bytes))
                .collect::<Vec<_>>(),
        );
        table(
            &mut bytes,
            72,
            &global
                .iter()
                .flat_map(|m| {
                    m.inverse()
                        .to_cols_array()
                        .into_iter()
                        .flat_map(f32::to_le_bytes)
                })
                .collect::<Vec<_>>(),
        );
        let visibility_parents = table(
            &mut bytes,
            80,
            &[-1i16, 0]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let rest = [70000, 90000]
            .map(|id| {
                (
                    id,
                    Transform {
                        translation: [10., 0., 0.],
                        ..Default::default()
                    },
                )
            })
            .into_iter()
            .collect();
        let pose = bindings::Pose {
            channels: vec![(0x80001, 70000), (0x80001, 90000)],
            rest,
            visible: [(70000, false), (90000, true)].into_iter().collect(),
        };
        let nodes = read(&Reader(&bytes), 0, &pose)?;
        assert_eq!(
            nodes.iter().map(|n| n.binding).collect::<Vec<_>>(),
            [70000, 90000]
        );
        assert_eq!(nodes[1].rest.translation[0], 10.);
        assert_eq!(nodes[1].name, "child");
        assert_eq!(nodes[1].parent, Some(0));
        assert!(!nodes[0].visible && nodes[1].visible);
        let mut bad = bytes.clone();
        bad[hierarchy + 4..hierarchy + 6].copy_from_slice(&1u16.to_le_bytes());
        assert!(read(&Reader(&bad), 0, &pose).is_err());
        let mut bad = bytes.clone();
        bad[100..104].copy_from_slice(&70000u32.to_le_bytes());
        assert!(read(&Reader(&bad), 0, &pose).is_err());
        assert!(read(&Reader(&bytes[..bytes.len() - 1]), 0, &pose).is_err());
        let mut missing = pose.clone();
        let removed = missing.rest.remove(&90000).unwrap();
        missing.rest.insert(90001, removed);
        assert!(read(&Reader(&bytes), 0, &missing).is_err());
        let mut missing = pose.clone();
        missing.visible.remove(&90000);
        assert!(read(&Reader(&bytes), 0, &missing).is_err());
        let mut bad = bytes.clone();
        bad[visibility_parents + 2..visibility_parents + 4].copy_from_slice(&(-1i16).to_le_bytes());
        assert!(read(&Reader(&bad), 0, &pose).is_err());
        let mut bad = bytes.clone();
        bad[12..14].copy_from_slice(&1u16.to_le_bytes());
        assert!(read(&Reader(&bad), 0, &pose).is_err());
        let mut bad = bytes;
        bad[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&Reader(&bad), 0, &pose).is_err());
        Ok(())
    }
}
