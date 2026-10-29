//! Ordinary glTF animation, with name binding independent of exported node order.
use super::*;
use anyhow::bail;
use gltf::animation::util::ReadOutputs;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

impl Motion {
    /// Decode in the file's joint order. Model consumers use `decode_for_bones`.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        decode(&parse(bytes)?, None, false)
    }

    pub fn decode_for_skeleton(bytes: &[u8], skeleton: &Skeleton) -> Result<Self> {
        let names = skeleton
            .bones
            .iter()
            .map(|bone| bone.name.clone())
            .collect::<Vec<_>>();
        Self::decode_for_bones(bytes, &names)
    }

    pub fn decode_for_bones(bytes: &[u8], names: &[String]) -> Result<Self> {
        decode(&parse(bytes)?, Some(names), false)
    }

    /// Evaluate named nodes in scene space, including static nodes and every
    /// animated/static ancestor. Useful for cameras and other point trajectories.
    pub fn decode_world_nodes(bytes: &[u8], names: &[String]) -> Result<Self> {
        decode(&parse(bytes)?, Some(names), true)
    }

    /// Write a standard rig and animation. No application extras are required.
    pub fn encode(&self, skeleton: &Skeleton) -> Result<Vec<u8>> {
        self.encode_rig(skeleton, true)
    }

    /// Standard animated scene nodes without an unused skin declaration.
    pub fn encode_nodes(&self, skeleton: &Skeleton) -> Result<Vec<u8>> {
        self.encode_rig(skeleton, false)
    }

    fn encode_rig(&self, skeleton: &Skeleton, skin: bool) -> Result<Vec<u8>> {
        self.validate(skeleton)?;
        skeleton.validate()?;
        let names: BTreeSet<_> = skeleton.bones.iter().map(|bone| &bone.name).collect();
        ensure!(
            names.len() == skeleton.bones.len(),
            "animation rig has duplicate joint names"
        );
        let mut writer = Writer::default();
        let mut nodes: Vec<_> = skeleton
            .bones
            .iter()
            .map(|bone| node(&bone.name, bone.bind))
            .collect();
        let mut roots = Vec::new();
        let mut edges = Vec::new();
        for (bone, spec) in skeleton.bones.iter().enumerate() {
            let track = self
                .tracks
                .iter()
                .find(|track| usize::from(track.bone) == bone);
            let mut chain = Vec::new();
            if let Some(track) = track {
                for (step, transform) in track.nodes.iter().enumerate() {
                    let index = if step + 1 == track.nodes.len() {
                        nodes[bone] = node(&spec.name, transform.bind);
                        bone
                    } else {
                        let index = nodes.len();
                        nodes.push(node(
                            &format!("{} transform {}", spec.name, step + 1),
                            transform.bind,
                        ));
                        index
                    };
                    chain.push(index);
                    writer.channels(index, transform)?;
                }
            } else {
                chain.push(bone);
            }
            for pair in chain.windows(2) {
                edges.push((pair[0], pair[1]));
            }
            if let Some(parent) = spec.parent {
                edges.push((usize::from(parent), chain[0]));
            } else {
                roots.push(chain[0]);
            }
        }
        for (parent, child_index) in edges {
            child(&mut nodes, parent, child_index);
        }
        // A standard animated node carries a hold-only clip's duration, without
        // authoring a channel on a gameplay joint that was originally untouched.
        if writer.samplers.is_empty()
            || writer
                .timelines
                .iter()
                .map(|(t, _)| *t.last().unwrap())
                .fold(0., f32::max)
                < self.duration_frames / FRAME_HZ
        {
            let index = nodes.len();
            nodes.push(node("Clip timeline", Transform::default()));
            roots.push(index);
            writer.channel(
                index,
                "scale",
                &Channel {
                    interpolation: Interpolation::Step,
                    times: if self.duration_frames == 0. {
                        vec![0.]
                    } else {
                        vec![0., self.duration_frames / FRAME_HZ]
                    },
                    values: vec![[1.; 3]; if self.duration_frames == 0. { 1 } else { 2 }],
                },
            )?;
        }
        let mut document = json!({
            "asset": {"version":"2.0", "generator":"resonance-import"},
            "scene":0, "scenes":[{"nodes":roots}], "nodes":nodes,
            "animations":[{"name":"Performance", "channels":writer.channels, "samplers":writer.samplers}],
            "buffers":[{"byteLength":writer.binary.len()}],
            "bufferViews":writer.views, "accessors":writer.accessors
        });
        if skin {
            document["skins"] = json!([{"joints":(0..skeleton.bones.len()).collect::<Vec<_>>()}]);
        }
        resonance_asset_writer::gltf::pack_glb(&document, &writer.binary)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<gltf::Gltf> {
    ensure!(
        bytes.starts_with(b"glTF"),
        "animation must be a glTF 2.0 GLB"
    );
    let glb = gltf::Gltf::from_slice(bytes).context("decode glTF animation")?;
    ensure!(
        glb.animations().len() == 1,
        "a clip GLB must contain exactly one animation"
    );
    ensure!(
        glb.buffers().len() == 1
            && glb
                .buffers()
                .all(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin)),
        "animation GLB needs one embedded buffer"
    );
    ensure!(
        glb.extensions_required().next().is_none(),
        "unsupported required glTF animation extension"
    );
    Ok(glb)
}

pub(super) fn decode(
    glb: &gltf::Gltf,
    names: Option<&[String]>,
    world_space: bool,
) -> Result<Motion> {
    let binary = glb
        .blob
        .as_deref()
        .context("animation GLB has no keyframe buffer")?;
    let mut parents = vec![None; glb.nodes().len()];
    let mut by_name = BTreeMap::new();
    let mut transforms = Vec::new();
    for node in glb.nodes() {
        if let Some(name) = node.name() {
            ensure!(
                by_name.insert(name, node.index()).is_none(),
                "duplicate animation node name: {name}"
            );
        }
        for child in node.children() {
            ensure!(
                parents[child.index()].replace(node.index()).is_none(),
                "animation node has multiple parents"
            );
        }
        let (translation, rotation, scale) = node.transform().decomposed();
        let bind = Transform {
            translation,
            rotation,
            scale,
        };
        bind.validate()?;
        // glTF matrices are required to be decomposable; reject malformed shear
        // instead of letting the reader's decomposition silently discard it.
        let expected = node.transform().matrix();
        let rebuilt = bind.matrix();
        ensure!(
            expected
                .iter()
                .flatten()
                .zip(rebuilt.iter().flatten())
                .all(|(a, b)| (a - b).abs() <= 0.0001 * (1. + a.abs())),
            "animation node matrix cannot be represented by glTF TRS"
        );
        transforms.push(NodeTrack::new(bind));
    }
    let selected: Vec<usize> = if let Some(names) = names {
        ensure!(
            names.iter().collect::<BTreeSet<_>>().len() == names.len(),
            "model has duplicate joint names"
        );
        names
            .iter()
            .map(|name| {
                by_name
                    .get(name.as_str())
                    .copied()
                    .with_context(|| format!("animation lacks model joint {name}"))
            })
            .collect::<Result<_>>()?
    } else if let Some(skin) = glb.skins().next() {
        skin.joints().map(|node| node.index()).collect()
    } else {
        glb.nodes().map(|node| node.index()).collect()
    };
    ensure!(
        selected.len() <= usize::from(u16::MAX),
        "too many animation joints"
    );
    let selected_set: BTreeSet<_> = selected.iter().copied().collect();
    let mut end = 0_f32;
    for channel in glb.animations().next().unwrap().channels() {
        let target = channel.target();
        ensure!(
            !matches!(
                target.node().transform(),
                gltf::scene::Transform::Matrix { .. }
            ),
            "animated node must use TRS"
        );
        let reader = channel.reader(|buffer| (buffer.index() == 0).then_some(binary));
        let times: Vec<_> = reader
            .read_inputs()
            .context("missing animation input")?
            .collect();
        if let Some(&last) = times.last() {
            end = end.max(last);
        }
        let interpolation = match channel.sampler().interpolation() {
            gltf::animation::Interpolation::Step => Interpolation::Step,
            gltf::animation::Interpolation::Linear => Interpolation::Linear,
            gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
        };
        let node = &mut transforms[target.node().index()];
        match reader.read_outputs().context("missing animation output")? {
            ReadOutputs::Translations(values) => {
                ensure!(node.translation.is_none(), "duplicate translation channel");
                let value = Channel {
                    interpolation,
                    times,
                    values: values.collect(),
                };
                value.validate(false)?;
                node.translation = Some(value);
            }
            ReadOutputs::Scales(values) => {
                ensure!(node.scale.is_none(), "duplicate scale channel");
                let value = Channel {
                    interpolation,
                    times,
                    values: values.collect(),
                };
                value.validate(false)?;
                node.scale = Some(value);
            }
            ReadOutputs::Rotations(values) => {
                ensure!(node.rotation.is_none(), "duplicate rotation channel");
                let value = Channel {
                    interpolation,
                    times,
                    values: values.into_f32().collect(),
                };
                value.validate(true)?;
                node.rotation = Some(value);
            }
            ReadOutputs::MorphTargetWeights(_) => {
                bail!("skeletal clip cannot contain morph-weight channels")
            }
        }
    }
    let mut tracks = Vec::new();
    for (bone, &index) in selected.iter().enumerate() {
        let mut path = vec![index];
        let mut cursor = parents[index];
        while let Some(parent) = cursor {
            ensure!(
                path.len() <= parents.len(),
                "cyclic animation node hierarchy"
            );
            if !world_space && selected_set.contains(&parent) {
                break;
            }
            path.push(parent);
            cursor = parents[parent];
        }
        path.reverse();
        if world_space || path.iter().any(|&index| transforms[index].channels().any()) {
            let nodes = path
                .into_iter()
                .filter(|&index| {
                    index == selected[bone]
                        || transforms[index].channels().any()
                        || transforms[index].bind != Transform::default()
                })
                .map(|index| transforms[index].clone())
                .collect();
            tracks.push(Track {
                bone: bone as u16,
                nodes,
            });
        }
    }
    let motion = Motion {
        duration_frames: end * FRAME_HZ,
        tracks,
    };
    motion.validate_bones(selected.len())?;
    Ok(motion)
}

fn node(name: &str, mut transform: Transform) -> Value {
    transform.rotation = glam::Quat::from_array(transform.rotation)
        .normalize()
        .to_array();
    json!({"name":name, "translation":transform.translation, "rotation":transform.rotation, "scale":transform.scale})
}
fn child(nodes: &mut [Value], parent: usize, child: usize) {
    if nodes[parent]["children"].is_null() {
        nodes[parent]["children"] = json!([]);
    }
    nodes[parent]["children"]
        .as_array_mut()
        .unwrap()
        .push(json!(child));
}

#[derive(Default)]
struct Writer {
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    channels: Vec<Value>,
    samplers: Vec<Value>,
    // Reuse ordinary arrays within this file; no digest or external registry.
    timelines: Vec<(Vec<f32>, usize)>,
}
impl Writer {
    fn accessor<const N: usize>(&mut self, values: &[[f32; N]], kind: &str) -> usize {
        let offset = self.binary.len();
        self.binary
            .extend(values.iter().flatten().flat_map(|v| v.to_le_bytes()));
        let view = self.views.len();
        self.views
            .push(json!({"buffer":0, "byteOffset":offset, "byteLength":self.binary.len()-offset}));
        let accessor = self.accessors.len();
        self.accessors.push(
            json!({"bufferView":view, "componentType":5126, "count":values.len(), "type":kind}),
        );
        accessor
    }
    fn channel<const N: usize>(
        &mut self,
        node: usize,
        path: &str,
        curve: &Channel<N>,
    ) -> Result<()> {
        curve.validate(path == "rotation")?;
        let input = if let Some((_, accessor)) = self
            .timelines
            .iter()
            .find(|(times, _)| *times == curve.times)
        {
            *accessor
        } else {
            let input = self.accessor(
                &curve.times.iter().map(|&t| [t]).collect::<Vec<_>>(),
                "SCALAR",
            );
            self.accessors[input]["min"] = json!([curve.times[0]]);
            self.accessors[input]["max"] = json!([curve.times.last().unwrap()]);
            self.timelines.push((curve.times.clone(), input));
            input
        };
        let output = self.accessor(&curve.values, if N == 3 { "VEC3" } else { "VEC4" });
        let sampler = self.samplers.len();
        self.samplers.push(json!({"input":input, "output":output, "interpolation": match curve.interpolation {
            Interpolation::Step => "STEP", Interpolation::Linear => "LINEAR", Interpolation::CubicSpline => "CUBICSPLINE"
        }}));
        self.channels
            .push(json!({"sampler":sampler, "target":{"node":node,"path":path}}));
        Ok(())
    }
    fn channels(&mut self, node: usize, track: &NodeTrack) -> Result<()> {
        if let Some(curve) = &track.translation {
            self.channel(node, "translation", curve)?;
        }
        if let Some(curve) = &track.rotation {
            self.channel(node, "rotation", curve)?;
        }
        if let Some(curve) = &track.scale {
            self.channel(node, "scale", curve)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
