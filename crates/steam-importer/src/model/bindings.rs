//! The descriptor's pose-buffer channels select which motion targets it consumes.
use super::*;
use resonance_content::animation::Transform;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(super) struct Pose {
    pub channels: Vec<(u32, u32)>,
    pub rest: BTreeMap<u32, Transform>,
    pub visible: BTreeMap<u32, bool>,
}

pub(super) fn read(source: &Reader<'_>, section: usize) -> Result<Pose> {
    let channels = source.table(section, 8)?;
    let values = source.table(section + 8, 4)?;
    let mut dimensions = 0usize;
    let bindings = (0..channels.count)
        .map(|i| {
            let kind = source.word(channels.at(i))?;
            let binding = source.word(channels.at(i) + 4)?;
            ensure!(
                (1..=4).contains(&(kind & 15)),
                "invalid model animation channel width"
            );
            dimensions += (kind & 15) as usize;
            Ok((kind, binding))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        dimensions == values.count,
        "model animation value count mismatch"
    );
    let mut components = BTreeMap::new();
    let mut visible = BTreeMap::new();
    let mut offset = 0;
    for &(kind, binding) in &bindings {
        let width = (kind & 15) as usize;
        if matches!(kind, 3 | 0x10014 | 0x20003) {
            let mut value = [0.; 4];
            for (i, component) in value.iter_mut().take(width).enumerate() {
                *component = source.floats::<1>(values.at(offset + i))?[0];
            }
            ensure!(
                components.insert((kind, binding), value).is_none(),
                "duplicate model transform channel"
            );
        }
        if kind == 0x80001 {
            let value = source.floats::<1>(values.at(offset))?[0];
            ensure!(matches!(value, 0. | 1.), "non-binary model rest visibility");
            ensure!(
                visible.insert(binding, value != 0.).is_none(),
                "duplicate model visibility channel"
            );
        }
        offset += width;
    }
    let node_ids = bindings
        .iter()
        .filter(|(k, _)| *k == 3)
        .map(|(_, id)| *id)
        .collect::<BTreeSet<_>>();
    for kind in [3, 0x10014, 0x20003] {
        let declared = bindings
            .iter()
            .filter(|(k, _)| *k == kind)
            .collect::<Vec<_>>();
        ensure!(
            declared.len() == node_ids.len()
                && declared
                    .into_iter()
                    .map(|(_, id)| *id)
                    .collect::<BTreeSet<_>>()
                    == node_ids,
            "incomplete model transform channels"
        );
    }
    let rest = node_ids
        .into_iter()
        .map(|id| {
            let rotation = glam::Quat::from_array(components[&(0x10014, id)]);
            ensure!(
                rotation.is_finite() && (rotation.length_squared() - 1.).abs() < 0.00001,
                "invalid model rest quaternion"
            );
            let transform = Transform {
                scale: components[&(3, id)][..3].try_into().unwrap(),
                rotation: rotation.normalize().to_array(),
                translation: components[&(0x20003, id)][..3].try_into().unwrap(),
            };
            transform.validate()?;
            Ok((id, transform))
        })
        .collect::<Result<_>>()?;
    // Material aliases can repeat a channel identity. Native sampling writes
    // each occurrence; do not deduplicate or confuse those with bone indices.
    Ok(Pose {
        channels: bindings,
        rest,
        visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_directory_keeps_material_aliases_and_requires_complete_node_channels() -> Result<()>
    {
        let pairs = [
            (3u32, 17u32),
            (0x10014, 17),
            (0x20003, 17),
            (4, 0x8000001),
            (4, 0x8000001),
        ];
        let mut bytes = [16u32, pairs.len() as u32, 8 + pairs.len() as u32 * 8, 18]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend(
            pairs
                .into_iter()
                .flat_map(|(kind, binding)| [kind, binding])
                .flat_map(u32::to_le_bytes),
        );
        bytes.resize(bytes.len() + 18 * 4, 0);
        let start = 16 + pairs.len() * 8;
        let rotation = glam::Quat::from_rotation_y(0.7).to_array();
        let values = [-2., 0., -4.]
            .into_iter()
            .chain(rotation)
            .chain([4., 5., 6.]);
        for (i, value) in values.enumerate() {
            bytes[start + i * 4..start + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let pose = read(&Reader(&bytes), 0)?;
        assert_eq!(pose.channels, pairs);
        assert_eq!(pose.rest[&17].scale, [-2., 0., -4.]);
        assert_eq!(pose.rest[&17].translation, [4., 5., 6.]);
        assert!(
            glam::Quat::from_array(pose.rest[&17].rotation).dot(glam::Quat::from_array(rotation))
                > 0.999999
        );
        for (at, value) in [
            (12, 17u32),
            (16, 5),
            (20, 18),
            (32, 3),
            (start, f32::NAN.to_bits()),
            (start + 24, 0),
        ] {
            let mut invalid = bytes.clone();
            invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(read(&Reader(&invalid), 0).is_err());
        }
        assert!(read(&Reader(&bytes[..bytes.len() - 1]), 0).is_err());
        Ok(())
    }
}
