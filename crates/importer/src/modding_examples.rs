//! Add repository-authored examples while generating native game documents.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::{Channel, Interpolation, Motion, NodeTrack, Skeleton, Track, Transform},
    arte, characters,
    library::Mappings,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    id: u16,
    name: String,
    character: String,
    initially_known: bool,
    technique: arte::document::Entry,
    motion_reference: MotionReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionReference {
    skeleton: String,
    motion: String,
    frame: f32,
}

fn example() -> Result<Example> {
    serde_json::from_str(include_str!("../../../mods/ghost-wolf/example.json"))
        .context("Ghost Wolf modding example")
}

pub(crate) fn add_techniques(document: &mut arte::document::Document) -> Result<()> {
    let example = example()?;
    document.characters.id(&example.character)?;
    let mut mappings: BTreeMap<_, _> = document
        .mappings
        .iter()
        .map(|(id, name)| (id, name.to_owned()))
        .collect();
    ensure!(
        !mappings.contains_key(&u32::from(example.id)),
        "modding example catalogue ID {} is already used",
        example.id
    );
    ensure!(
        !mappings.values().any(|name| name == &example.name)
            && !document.techniques.contains_key(&example.name),
        "modding example {} already exists in the imported catalogue",
        example.name
    );
    let learning = document
        .learning
        .get_mut(&example.character)
        .context("modding example character has no learning list")?;
    mappings.insert(u32::from(example.id), example.name.clone());
    let mappings = Mappings::new(mappings)?;
    learning.push(example.name.clone());
    document.mappings = mappings;
    document.techniques.insert(example.name, example.technique);
    Ok(())
}

pub(crate) fn add_characters(document: &mut characters::Document) -> Result<()> {
    let example = example()?;
    if !example.initially_known {
        return Ok(());
    }
    let initial = document
        .characters
        .get_mut(&example.character)
        .context("modding example character is missing")?
        .initial
        .as_mut()
        .context("modding example character has no initial state")?;
    initial.techniques.push(example.name);
    Ok(())
}

pub(crate) fn cook_assets(library: &Path) -> Result<()> {
    let reference = example()?.motion_reference;
    let read = |path: &str| {
        std::fs::read(library.join(path))
            .with_context(|| format!("read modding example reference {path}"))
    };
    let skeleton = Skeleton::from_glb(&read(&reference.skeleton)?)?;
    let pose = Motion::decode_for_skeleton(&read(&reference.motion)?, &skeleton)?;
    let motion = resolve_motion(
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/ghost-wolf.glb"),
        &skeleton,
        &pose,
        reference.frame,
    )?;
    for (path, bytes) in ASSETS {
        crate::output::write(&library.join(path), bytes)?;
    }
    crate::output::write(
        &library.join("battle/artes/ghost-wolf/ghost-wolf.glb"),
        &motion,
    )?;
    Ok(())
}

/// The distributable clip contains component deltas on flat, identity nodes.
/// Restore the locally cooked reference pose before writing a normal runtime GLB.
fn resolve_motion(
    bytes: &[u8],
    skeleton: &Skeleton,
    reference: &Motion,
    frame: f32,
) -> Result<Vec<u8>> {
    let reference = skeleton.sample(reference, frame)?.local;
    let relative = Motion::decode_for_skeleton(bytes, skeleton)?;
    let mut tracks: BTreeMap<_, _> = relative
        .tracks
        .into_iter()
        .map(|track| (track.bone, track.nodes))
        .collect();
    let tracks = reference
        .into_iter()
        .enumerate()
        .map(|(bone, reference)| {
            let mut nodes = tracks
                .remove(&(bone as u16))
                .unwrap_or_else(|| vec![NodeTrack::new(Transform::default())]);
            ensure!(
                nodes.len() == 1 && nodes[0].bind == Transform::default(),
                "relative motion must use flat identity nodes"
            );
            let mut node = nodes.remove(0);
            node.bind = reference;
            node.translation = Some(offset_channel(node.translation, reference.translation));
            let rotation = glam::Quat::from_array(reference.rotation).normalize();
            node.rotation = Some(reference_channel(
                node.rotation,
                [0., 0., 0., 1.],
                |delta, _| (rotation * glam::Quat::from_array(delta)).to_array(),
            ));
            // Additive scale also represents restoring a part hidden by zero scale.
            node.scale = Some(offset_channel(node.scale, reference.scale));
            Ok(Track {
                bone: bone as u16,
                nodes: vec![node],
            })
        })
        .collect::<Result<_>>()?;
    Motion {
        duration_frames: relative.duration_frames,
        tracks,
    }
    .encode(skeleton)
}

fn offset_channel(channel: Option<Channel<3>>, reference: [f32; 3]) -> Channel<3> {
    reference_channel(channel, [0.; 3], |delta, tangent| {
        std::array::from_fn(|axis| delta[axis] + if tangent { 0. } else { reference[axis] })
    })
}

fn reference_channel<const N: usize>(
    channel: Option<Channel<N>>,
    identity: [f32; N],
    apply: impl Fn([f32; N], bool) -> [f32; N],
) -> Channel<N> {
    let mut channel = channel.unwrap_or(Channel {
        interpolation: Interpolation::Step,
        times: vec![0.],
        values: vec![identity],
    });
    for (index, value) in channel.values.iter_mut().enumerate() {
        let tangent = channel.interpolation == Interpolation::CubicSpline && index % 3 != 1;
        *value = apply(*value, tangent);
    }
    channel
}

const ASSETS: &[(&str, &[u8])] = &[
    (
        "battle/artes/ghost-wolf/action.json",
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/action.json"),
    ),
    (
        "battle/artes/ghost-wolf/effects.json",
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/effects.json"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn relative_motion_resolves_a_local_pose_without_changing_interpolation() -> Result<()> {
        let mut skeleton = Skeleton::single("root");
        skeleton.bones[0].bind.translation = [2., 3., 4.];
        skeleton.bones[0].bind.scale = [2., 0., 4.];
        let mut child = Skeleton::single("unkeyed").bones.remove(0);
        child.parent = Some(0);
        child.bind.translation = [0., 5., 0.];
        skeleton.bones.push(child);
        let reference = Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![NodeTrack {
                    rotation: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![
                            glam::Quat::IDENTITY.to_array(),
                            glam::Quat::from_rotation_y(1.2).to_array(),
                        ],
                    }),
                    ..NodeTrack::new(skeleton.bones[0].bind)
                }],
            }],
        };
        let relative = Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![NodeTrack {
                    bind: Transform::default(),
                    translation: Some(Channel {
                        interpolation: Interpolation::CubicSpline,
                        times: vec![0., 1.],
                        values: vec![[1.; 3], [0.; 3], [1.; 3], [1.; 3], [1.; 3], [1.; 3]],
                    }),
                    rotation: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![
                            glam::Quat::IDENTITY.to_array(),
                            glam::Quat::from_rotation_x(0.9).to_array(),
                        ],
                    }),
                    scale: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![[0.; 3], [1.; 3]],
                    }),
                }],
            }],
        };
        let mut flat = skeleton.clone();
        for bone in &mut flat.bones {
            bone.parent = None;
            bone.bind = Transform::default();
        }
        let output = resolve_motion(&relative.encode_nodes(&flat)?, &skeleton, &reference, 15.)?;
        let output = Motion::decode_for_skeleton(&output, &skeleton)?;
        assert_eq!(output.duration_frames, 30.);
        for frame in [0., 7.5, 15., 30.] {
            let fraction = frame / 30.;
            let actual = skeleton.sample(&output, frame)?.local;
            let expected = Transform {
                translation: [2. + fraction, 3. + fraction, 4. + fraction],
                rotation: (glam::Quat::from_rotation_y(0.6)
                    * glam::Quat::from_rotation_x(0.9 * fraction))
                .to_array(),
                scale: [2. + fraction, fraction, 4. + fraction],
            };
            assert!(
                actual[0]
                    .matrix()
                    .iter()
                    .flatten()
                    .zip(expected.matrix().iter().flatten())
                    .all(|(actual, expected)| (actual - expected).abs() < 0.00001)
            );
            assert_eq!(actual[1], skeleton.bones[1].bind);
        }
        assert!(
            resolve_motion(&relative.encode_nodes(&flat)?, &skeleton, &reference, 31.).is_err()
        );
        Ok(())
    }

    fn native_techniques() -> arte::document::Document {
        serde_json::from_value(json!({
            "mappings": {"1": "native-arte"},
            "characters": {"1": "lloyd", "2": "colette"},
            "techniques": {
                "native-arte": {"rules": {"tp_cost": 7}}
            },
            "learning": {"lloyd": ["native-arte"], "colette": ["native-arte"]}
        }))
        .unwrap()
    }

    fn native_characters() -> characters::Document {
        serde_json::from_value(json!({
            "mappings": {"1": "lloyd", "2": "colette"},
            "characters": {
                "lloyd": {"name": "Lloyd", "initial": {
                    "stats": {"hp": 200}, "techniques": ["native-arte"]}},
                "colette": {"name": "Colette", "initial": {
                    "stats": {"hp": 172}, "techniques": ["native-arte"]}}
            },
            "experience": {"1": 0, "2": 25}
        }))
        .unwrap()
    }

    #[test]
    fn adds_example_to_generated_native_documents() -> Result<()> {
        let mut techniques = native_techniques();
        let mut characters = native_characters();
        let before_techniques = serde_json::to_value(&techniques)?;
        let before_characters = serde_json::to_value(&characters)?;
        add_techniques(&mut techniques)?;
        add_characters(&mut characters)?;
        let mut techniques = serde_json::to_value(techniques)?;
        assert_eq!(techniques["mappings"]["260"], "ghost-wolf-lloyd");
        assert_eq!(
            techniques["techniques"]["ghost-wolf-lloyd"]["rules"]["action"],
            "battle/artes/ghost-wolf/action.json"
        );
        assert_eq!(
            techniques["techniques"]["ghost-wolf-lloyd"]["rules"]["required_level"],
            6
        );
        assert_eq!(
            techniques["techniques"]["ghost-wolf-lloyd"]["rules"]["learn_on_level_up"],
            true
        );
        techniques["mappings"]
            .as_object_mut()
            .unwrap()
            .remove("260");
        techniques["techniques"]
            .as_object_mut()
            .unwrap()
            .remove("ghost-wolf-lloyd");
        assert_eq!(
            techniques["learning"]["lloyd"]
                .as_array_mut()
                .unwrap()
                .pop(),
            Some(json!("ghost-wolf-lloyd"))
        );
        assert_eq!(techniques, before_techniques);
        assert_eq!(serde_json::to_value(characters)?, before_characters);
        Ok(())
    }

    #[test]
    fn rejects_collisions_with_imported_techniques() -> Result<()> {
        for mappings in [
            json!({"1": "native-arte", "260": "future-native-arte"}),
            json!({"1": "native-arte", "261": "ghost-wolf-lloyd"}),
            json!({"1": "native-arte", "260": "ghost-wolf-lloyd"}),
        ] {
            let mut techniques = serde_json::to_value(native_techniques())?;
            techniques["mappings"] = mappings;
            let mut techniques: arte::document::Document = serde_json::from_value(techniques)?;
            let before = serde_json::to_value(&techniques)?;
            assert!(add_techniques(&mut techniques).is_err());
            assert_eq!(serde_json::to_value(&techniques)?, before);
        }
        Ok(())
    }
}
