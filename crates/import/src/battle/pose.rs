//! Collision poses and mesh animation use the same parsed model and bindings.
use crate::{animation::ModelBindings, geometry, model::Model};
use anyhow::{Result, ensure};
use resonance_content::battle::pose::{Bone, Skeleton, Transform, TransformChannels};
use std::ops::Range;

pub(super) fn model_range(resource: &[u8]) -> Result<Range<usize>> {
    let (base, model) = geometry::model_resource(resource)?;
    let range = geometry::skeleton_range(model)?;
    Ok(base + range.start..base + range.end)
}

#[cfg(test)]
pub(super) fn model(resource: &[u8]) -> Result<&[u8]> {
    Ok(&resource[model_range(resource)?])
}

pub(super) struct RigSource {
    pub skeleton: Skeleton,
    pub bindings: ModelBindings,
}

impl RigSource {
    pub fn read(resource: &[u8]) -> Result<Self> {
        let model = Model::parse(&resource[model_range(resource)?])?;
        let skeleton = Skeleton {
            bones: geometry::model_node_info(&model)
                .into_iter()
                .zip(&model.nodes)
                .map(|(node, source)| Bone {
                    name: node.name,
                    parent: source.parent.map(|p| p as u16),
                    bind_channels: TransformChannels(
                        source
                            .data_words
                            .first()
                            .map_or(0, |word| (word >> 24) as u8),
                    ),
                    bind: Transform {
                        translation: node.translation,
                        rotation: node.rotation,
                        scale: node.scale,
                    },
                })
                .collect(),
        };
        skeleton.validate()?;
        Ok(Self {
            skeleton,
            bindings: ModelBindings::from_model(&model),
        })
    }

    /// Contact and trail classification requires authored labels, not display fallbacks.
    pub fn require_names(&self) -> Result<()> {
        ensure!(
            self.bindings
                .names
                .as_ref()
                .is_some_and(|names| names.iter().all(|name| !name.is_empty())),
            "battle contact rig requires authored bone names"
        );
        Ok(())
    }
}

pub(crate) fn skeleton(resource: &[u8]) -> Result<Skeleton> {
    Ok(RigSource::read(resource)?.skeleton)
}

pub(super) fn rig_skeleton(resource: &[u8]) -> Result<Skeleton> {
    let source = RigSource::read(resource)?;
    source.require_names()?;
    Ok(source.skeleton)
}

#[cfg(test)]
pub(crate) fn motion(
    anm: &[u8],
    resource: &[u8],
) -> Result<resonance_content::battle::pose::Motion> {
    RigSource::read(resource)?.bindings.motion(anm)
}

#[test]
fn model_names_cannot_borrow_a_terminator_from_the_next_resource() -> Result<()> {
    let mut bytes = vec![0; 32 + 32 + 28];
    for (at, value) in [
        (4, 32u32),
        (8, 64),
        (32, 0x007b7960),
        (44, 32),
        (56, 1),
        (60, 60),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    bytes[38..40].copy_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(b"root\0");
    // The declared model ends before the terminator, which belongs to a sibling.
    assert!(RigSource::read(&bytes).is_err());
    bytes[8..12].copy_from_slice(&65u32.to_be_bytes());
    let source = RigSource::read(&bytes)?;
    source.require_names()?;
    assert_eq!(source.skeleton.bones[0].name, "root");
    assert_eq!(source.bindings.names.unwrap(), ["root"]);
    assert_eq!(source.skeleton.bones[0].bind_channels, TransformChannels(0));

    // Explicit identity scale is numerically identical to an absent scale, but
    // retains a different native transform-presence flag.
    bytes.resize(152, 0);
    bytes[8..12].copy_from_slice(&120u32.to_be_bytes());
    bytes[64..68].copy_from_slice(&68u32.to_be_bytes());
    bytes[100] = 1;
    for at in [104, 108, 112] {
        bytes[at..at + 4].copy_from_slice(&1f32.to_be_bytes());
    }
    let present = RigSource::read(&bytes)?;
    assert_eq!(
        present.skeleton.bones[0].bind,
        source.skeleton.bones[0].bind
    );
    assert_eq!(
        present.skeleton.bones[0].bind_channels,
        TransformChannels(1)
    );
    Ok(())
}
