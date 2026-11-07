use super::super::{coordinates, model::Rig};
use super::*;
use resonance_content::animation::{Channel, FRAME_HZ, Motion, NodeTrack, Skeleton, Track};
use std::collections::{BTreeMap, BTreeSet};

/// TOANMB clocks in this port count sixty units per second. The shared game's
/// thirty-unit clock and glTF seconds are independent of the stream encoding.
const SOURCE_HZ: f32 = 60.;

impl Animation {
    /// Bind node transforms to the common skeleton. Material and visibility
    /// channels are decoded, but their playback is not part of this pass.
    pub fn bind(&self, model: &Rig, skeleton: &Skeleton) -> Result<Motion> {
        ensure!(
            skeleton.bones.len() == model.nodes.len(),
            "Steam animation rig count differs"
        );
        let names = model.joint_names();
        ensure!(
            skeleton
                .bones
                .iter()
                .zip(&names)
                .all(|(bone, name)| bone.name == *name),
            "Steam animation rig names differ"
        );
        let bindings = model
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.binding, i))
            .collect::<BTreeMap<_, _>>();
        // Native 0x604440 resets every declared channel before selecting curves.
        // Shared sparse playback can retain an outgoing pose, so make those
        // resets explicit standard channels in the cook, including absent bones.
        let constant = |value| Channel {
            interpolation: Interpolation::Step,
            times: vec![0.],
            values: vec![value],
        };
        let mut tracks = model
            .nodes
            .iter()
            .enumerate()
            .map(|(bone, node)| {
                let rest = coordinates::transform(node.rest);
                (
                    bone,
                    NodeTrack {
                        bind: rest,
                        translation: Some(constant(rest.translation)),
                        rotation: Some(Channel {
                            interpolation: Interpolation::Step,
                            times: vec![0.],
                            values: vec![rest.rotation],
                        }),
                        scale: Some(constant(rest.scale)),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let declared = model
            .animation_bindings
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for target in &self.targets {
            // A clip may contain channels for other costumes and model parts.
            // Only declared node transforms belong to this rig.
            if !declared.contains(&(target.kind, target.binding))
                || !matches!(target.kind, 3 | 0x10014 | 0x20003)
            {
                continue;
            }
            let &bone = bindings
                .get(&target.binding)
                .context("declared animation channel has no skeleton node")?;
            let rest = coordinates::transform(model.nodes[bone].rest);
            let node = tracks.get_mut(&bone).context("missing bound Steam node")?;
            let curve = &self.curves[target.curve];
            let channel = |values| Channel {
                interpolation: curve.interpolation,
                times: curve.times.iter().map(|t| t / SOURCE_HZ).collect(),
                values,
            };
            match target.kind {
                3 => {
                    node.scale = Some(channel(
                        curve
                            .values
                            .iter()
                            // Scale keys are offsets from the native rest
                            // scale, including non-unit and mirrored axes.
                            .map(|v| std::array::from_fn(|i| rest.scale[i] + v[i]))
                            .collect(),
                    ))
                }
                0x20003 => {
                    node.translation = Some(channel(
                        curve
                            .values
                            .iter()
                            .map(|v| {
                                let delta = coordinates::vector([v[0], v[1], v[2]]);
                                std::array::from_fn(|i| rest.translation[i] + delta[i])
                            })
                            .collect(),
                    ))
                }
                0x10014 => {
                    let values = curve
                        .values
                        .iter()
                        .map(|&value| {
                            let delta = glam::Quat::from_array(coordinates::rotation(value));
                            ensure!(
                                delta.length_squared() > 1e-12,
                                "zero Steam animation quaternion"
                            );
                            Ok((glam::Quat::from_array(rest.rotation) * delta.normalize())
                                .normalize()
                                .to_array())
                        })
                        .collect::<Result<Vec<_>>>()?;
                    node.rotation = Some(Channel {
                        interpolation: curve.interpolation,
                        times: curve.times.iter().map(|t| t / SOURCE_HZ).collect(),
                        values,
                    });
                }
                _ => unreachable!(),
            }
        }
        let motion = Motion {
            duration_frames: self.duration_frames * FRAME_HZ / SOURCE_HZ,
            tracks: tracks
                .into_iter()
                .map(|(bone, node)| Track {
                    bone: bone as u16,
                    nodes: vec![node.into()],
                })
                .collect(),
        };
        motion.validate(skeleton)?;
        Ok(motion)
    }
}
