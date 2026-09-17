//! Decode authored skeletal curves without resampling or geometry export.
use anyhow::{Result, ensure};
use resonance_content::animation::{
    Motion, QuaternionCurve, QuaternionInterpolation, Track as PoseTrack, VectorCurve,
    VectorInterpolation,
};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;

mod authored;
pub(crate) use authored::Animation as AuthoredAnimation;
pub(crate) use authored::decode;
pub(crate) use authored::is_animation;

/// Resource directories may bound a compressed clip before its stripped name table.
pub(crate) fn read_member(bytes: &[u8]) -> Result<AuthoredAnimation> {
    authored::decode_indexed(&crate::compression::payload(bytes.to_vec())?)
}

/// Original lookup identities, independent of mesh-node order and generated labels.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ModelBindings {
    pub node_ids: Vec<u16>,
    pub channels: Vec<resonance_content::animation::TransformChannels>,
    pub names: Option<Vec<String>>,
}

impl ModelBindings {
    #[cfg(test)]
    pub(crate) fn read(model: &[u8]) -> Result<Self> {
        Ok(Self::from_model(&crate::model::Model::parse(model)?))
    }

    pub(crate) fn from_model(model: &crate::model::Model) -> Self {
        Self {
            node_ids: model.nodes.iter().map(|node| node.node_id).collect(),
            channels: model
                .nodes
                .iter()
                .map(|node| {
                    resonance_content::animation::TransformChannels(
                        node.data_words.first().map_or(0, |word| (word >> 24) as u8),
                    )
                })
                .collect(),
            names: model.names.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn motion(&self, bytes: &[u8]) -> Result<Motion> {
        authored::decode_indexed(bytes)?.motion(self)
    }

    fn bind(
        &self,
        ids: &[u16],
        names: Option<&[String]>,
        force_index: bool,
    ) -> Result<Vec<(u16, usize)>> {
        ensure!(
            self.node_ids.len() <= u16::MAX as usize
                && self.channels.len() == self.node_ids.len()
                && self
                    .names
                    .as_ref()
                    .is_none_or(|names| names.len() == self.node_ids.len())
                && names.is_none_or(|names| names.len() == ids.len()),
            "invalid animation binding names or IDs"
        );
        let use_names = !force_index
            && match (names, self.names.as_ref()) {
                (Some(a), Some(m)) => a.iter().any(|name| m.contains(name)),
                _ => false,
            };
        let bindings: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, &id)| {
                if use_names {
                    self.names
                        .as_ref()
                        .unwrap()
                        .iter()
                        .position(|name| *name == names.unwrap()[index])
                } else {
                    Some(usize::from(id))
                }
            })
            .collect();
        // Repeated names/IDs select the first descriptor; aliased model IDs share it.
        Ok(self
            .node_ids
            .iter()
            .enumerate()
            .filter_map(|(bone, id)| {
                bindings
                    .iter()
                    .position(|bound| *bound == Some(usize::from(*id)))
                    .map(|track| (bone as u16, track))
            })
            .collect())
    }
}

/// Export every source descriptor before a model selects names or node IDs.
#[cfg(test)]
pub(crate) fn unbound(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(decode(bytes)?)?)
}

/// Packed motion banks may omit name tables; tracks retain numeric node IDs.
#[cfg(test)]
pub(crate) fn unbound_indexed(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(authored::decode_indexed(bytes)?)?)
}

/// Bind a package clip, including banks that strip their name tables.
#[cfg(test)]
pub(crate) fn motion(bytes: &[u8], model: &[u8]) -> Result<Motion> {
    ModelBindings::read(model)?.motion(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::Transform;
    #[test]
    fn packed_curve_components_keep_their_offsets_and_fractional_frame_values() {
        let mut bytes = vec![0; 272];
        let word = |data: &mut [u8], at: usize, value: u32| {
            data[at..at + 4].copy_from_slice(&value.to_be_bytes())
        };
        let half = |data: &mut [u8], at: usize, value: u16| {
            data[at..at + 2].copy_from_slice(&value.to_be_bytes())
        };
        word(&mut bytes, 0, 0x007b7960);
        word(&mut bytes, 4, 24);
        half(&mut bytes, 10, 1);
        half(&mut bytes, 12, 1);
        half(&mut bytes, 14, 2);
        word(&mut bytes, 28, 36);
        half(&mut bytes, 32, 1);
        word(&mut bytes, 36, 2f32.to_bits());
        word(&mut bytes, 40, 52);
        half(&mut bytes, 44, 2);
        half(&mut bytes, 46, 42);
        bytes[48..52].copy_from_slice(&[0x34, 11, 3 | (3 << 2) | (5 << 4), 1]);
        for (index, value) in [128, 148].into_iter().enumerate() {
            let tangent = 168 + index * 52;
            word(&mut bytes, 52 + index * 12, (index as f32 * 2.).to_bits());
            word(&mut bytes, 56 + index * 12, value as u32);
            word(&mut bytes, 60 + index * 12, tangent as u32);
            for axis in 0..3 {
                half(&mut bytes, value + axis * 2, 16);
            }
            half(&mut bytes, value + 12, 16384);
            half(&mut bytes, value + 14, index as u16 * 32);
            half(&mut bytes, tangent + 22, 16384);
            half(&mut bytes, tangent + 30, 16384);
        }
        let mut model = vec![0; 60];
        word(&mut model, 0, 0x007b7960);
        half(&mut model, 6, 1);
        word(&mut model, 12, 32);
        half(&mut model, 54, 42);
        let decoded = motion(&bytes, &model).unwrap();
        let sample = decoded.tracks[0].sample(0.5, Transform::default()).unwrap();
        assert_eq!(sample.scale, [1.; 3]);
        assert_eq!(sample.translation, [0.3125, 0., 0.]);
        assert_eq!(sample.rotation, [0., 0., 0., 1.]);
        assert_eq!(decoded.tracks[0].bone, 0);
        bytes[202] = 1;
        let eased = motion(&bytes, &model).unwrap();
        assert_eq!(
            eased.tracks[0].rotation.as_ref().unwrap().ease[0][1],
            1. / 64.
        );
    }

    #[test]
    fn malformed_animation_tables_are_rejected() {
        assert!(authored::decode(&[0; 24]).is_err());
        let mut data = vec![0; 36];
        data[0..4].copy_from_slice(&0x007b7960u32.to_be_bytes());
        data[4..8].copy_from_slice(&24u32.to_be_bytes());
        data[10..12].copy_from_slice(&1u16.to_be_bytes());
        data[12..14].copy_from_slice(&1u16.to_be_bytes());
        assert!(authored::decode(&data).is_err());
    }
}

#[cfg(test)]
#[path = "animation/bindings_tests.rs"]
mod bindings_tests;
