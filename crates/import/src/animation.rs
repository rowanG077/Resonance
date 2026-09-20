//! Decode skeletal curves once for sparse CPU poses and ordinary glTF channels.
use anyhow::{Result, ensure};
use resonance_content::battle::pose::{
    FRAME_HZ, Motion, QuaternionCurve, QuaternionInterpolation, Track as PoseTrack, Transform,
    VectorCurve, VectorInterpolation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod authored;
pub(crate) use authored::Animation as AuthoredAnimation;
pub(crate) use authored::is_animation;

/// Original lookup identities, independent of mesh-node order and generated labels.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ModelBindings {
    pub node_ids: Vec<u16>,
    pub names: Option<Vec<String>>,
}

impl ModelBindings {
    pub(crate) fn read(model: &[u8]) -> Result<Self> {
        Ok(Self::from_model(&crate::model::Model::parse(model)?))
    }

    pub(crate) fn from_model(model: &crate::model::Model) -> Self {
        Self {
            node_ids: model.nodes.iter().map(|node| node.node_id).collect(),
            names: model.names.clone(),
        }
    }

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

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
enum Channel {
    Translation = 1,
    Scale = 2,
    Rotation = 8,
}

/// Export every source descriptor before a model selects names or node IDs.
pub(crate) fn unbound(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(authored::decode(bytes)?)?)
}

/// Packed motion banks may omit name tables; tracks retain numeric node IDs.
pub(crate) fn unbound_indexed(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(authored::decode_indexed(bytes)?)?)
}

/// Bind a package clip, including banks that strip their name tables.
#[cfg(test)]
pub(crate) fn motion(bytes: &[u8], model: &[u8]) -> Result<Motion> {
    ModelBindings::read(model)?.motion(bytes)
}

fn accessor(gltf: &mut Value, binary: &mut Vec<u8>, data: &[f32], components: usize) -> usize {
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let offset = binary.len();
    for v in data {
        binary.extend(v.to_le_bytes());
    }
    let view = gltf["bufferViews"].as_array().unwrap().len();
    gltf["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":offset,"byteLength":data.len()*4}));
    let mut value = json!({"bufferView":view,"componentType":5126,"count":data.len()/components,"type":match components { 1 => "SCALAR",3 => "VEC3",4 => "VEC4",_ => unreachable!() }});
    if components == 1 {
        value["min"] = json!([data[0]]);
        value["max"] = json!([data[data.len() - 1]]);
    }
    let index = gltf["accessors"].as_array().unwrap().len();
    gltf["accessors"].as_array_mut().unwrap().push(value);
    index
}

/// Bake quarter-frame curve samples to ordinary animation channels. Slow clips
/// can advance by a quarter authored frame in one game update; interpolating
/// only half-frame keys visibly straightens their curved motion.
pub(crate) fn bake(
    bytes: &[u8],
    bindings: &ModelBindings,
    gltf: &mut Value,
    binary: &mut Vec<u8>,
    name: &str,
) -> Result<f32> {
    let motion = bindings.motion(bytes)?;
    bake_motion(&motion, gltf, binary, name)
}

pub(crate) fn bake_motion(
    motion: &Motion,
    gltf: &mut Value,
    binary: &mut Vec<u8>,
    name: &str,
) -> Result<f32> {
    let duration = motion.duration_frames;
    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    let mut secondary_pose_nodes = Vec::new();
    for track in &motion.tracks {
        let node = usize::from(track.bone);
        if track.times.len() > 2 {
            secondary_pose_nodes.push(node);
        }
        let steps = (duration * 4.).round() as usize;
        ensure!(
            steps as f32 == duration * 4.,
            "animation duration is not aligned to quarter frames"
        );
        let times = (0..=steps)
            .map(|i| i as f32 / (4. * FRAME_HZ))
            .collect::<Vec<_>>();
        let input = accessor(gltf, binary, &times, 1);
        let samples = (0..=steps)
            .map(|i| track.sample(i as f32 * 0.25, Transform::default()))
            .collect::<Result<Vec<_>>>()?;
        for component in [Channel::Translation, Channel::Scale, Channel::Rotation] {
            let present = match component {
                Channel::Translation => track.translation.is_some(),
                Channel::Scale => track.scale.is_some(),
                Channel::Rotation => track.rotation.is_some(),
            };
            if !present {
                continue;
            }
            let values = samples
                .iter()
                .flat_map(|sample| {
                    match component {
                        Channel::Translation => sample.translation.as_slice(),
                        Channel::Scale => sample.scale.as_slice(),
                        Channel::Rotation => sample.rotation.as_slice(),
                    }
                    .iter()
                    .copied()
                })
                .collect::<Vec<_>>();
            let width = if matches!(component, Channel::Rotation) {
                4
            } else {
                3
            };
            let output = accessor(gltf, binary, &values, width);
            channels
                .push(json!({"sampler":samplers.len(),"target":{"node":node,"path":component}}));
            samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
        }
    }
    if channels.is_empty() {
        // Props also carry placeholder clips for nodes absent from their
        // model. The source leaves those models at their bind pose while the
        // clip clock runs. glTF requires a channel, so retain that duration
        // with a constant bind translation.
        let translation = gltf["nodes"][0]
            .get("translation")
            .cloned()
            .unwrap_or_else(|| json!([0., 0., 0.]));
        let values: Vec<f32> = serde_json::from_value(translation)?;
        ensure!(
            values.len() == 3 && values.iter().all(|v| v.is_finite()),
            "invalid bind translation"
        );
        let input = accessor(gltf, binary, &[0., duration / FRAME_HZ], 1);
        let output = accessor(gltf, binary, &[values.clone(), values].concat(), 3);
        channels.push(json!({"sampler":0,"target":{"node":0,"path":"translation"}}));
        samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
    }
    if gltf.get("animations").is_none() {
        gltf["animations"] = json!([]);
    }
    gltf["animations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":name,"channels":channels,"samplers":samplers,"extras":{"secondary_pose_nodes":secondary_pose_nodes}}));
    gltf["buffers"][0]["byteLength"] = json!(binary.len());
    Ok(duration / FRAME_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(
            motion(&bytes, &model).is_err(),
            "unverified easing must fail cooking"
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
