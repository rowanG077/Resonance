//! A GLB's curves are loaded once. Each model selects joints independently;
//! neither model node order nor a second instance can change the shared asset.
use super::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

type Binding = (Vec<String>, crate::JointMap);

#[derive(Debug)]
pub struct NamedMotion {
    source: Source,
    pub joints: Vec<String>,
    bound: Mutex<VecDeque<(Binding, Weak<Motion>)>>,
}

#[derive(Debug)]
enum Source {
    Skin(Motion),
    // Scene-only exports have no explicit joint boundary. Select their model
    // joints before folding editor-added parent transforms into each track.
    Scene(super::codec::Nodes),
}

impl NamedMotion {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let glb = super::codec::parse(bytes)?;
        let decoded = super::codec::Nodes::decode(&glb)?;
        let skin = glb.skins().next();
        let nodes: Vec<_> = match skin.as_ref() {
            Some(skin) => skin.joints().collect(),
            None => glb.nodes().collect(),
        };
        let joints: Vec<String> = nodes
            .into_iter()
            .map(|node| {
                node.name()
                    .map(str::to_owned)
                    .context("animation has an unnamed joint")
            })
            .collect::<Result<_>>()?;
        ensure!(
            joints.len() <= usize::from(u16::MAX),
            "too many animation joints"
        );
        let source = if skin.is_none() {
            Source::Scene(decoded)
        } else {
            Source::Skin(decoded.select(None, false)?)
        };
        Ok(Self {
            source,
            joints,
            bound: Default::default(),
        })
    }

    /// The captured clip owns this lookup. Expired targets are removed on use;
    /// only scene/asset owners retain curves, so visiting fields adds no history.
    pub fn bind_shared(
        &self,
        bones: &[String],
        selection: &crate::JointMap,
    ) -> Result<Arc<Motion>> {
        let mut bound = self.bound.lock().unwrap();
        bound.retain(|(_, motion)| motion.strong_count() > 0);
        if let Some(index) = bound
            .iter()
            .position(|((names, joints), _)| names == bones && joints == selection)
        {
            let entry = bound.remove(index).unwrap();
            if let Some(motion) = entry.1.upgrade() {
                bound.push_back(entry);
                return Ok(motion);
            }
        }
        let motion = Arc::new(self.bind(bones, selection)?);
        if bound.len() == 128 {
            bound.pop_front();
        }
        bound.push_back(((bones.to_vec(), selection.clone()), Arc::downgrade(&motion)));
        Ok(motion)
    }

    /// Bind named channels and validate the result against the target joints.
    pub fn bind(
        &self,
        bones: &[String],
        selection: &BTreeMap<String, Option<String>>,
    ) -> Result<Motion> {
        ensure!(
            bones.len() <= usize::from(u16::MAX),
            "too many model joints"
        );
        ensure!(
            bones
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == bones.len(),
            "duplicate model joint name"
        );
        let indices: BTreeMap<_, _> = self
            .joints
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i as u16))
            .collect();
        ensure!(
            indices.len() == self.joints.len(),
            "duplicate animation joint name"
        );
        for (target, source) in selection {
            ensure!(
                bones.contains(target),
                "animation targets missing model joint {target}"
            );
            if let Some(source) = source {
                ensure!(
                    indices.contains_key(source.as_str()),
                    "animation lacks joint {source}"
                );
            }
        }
        let mut tracks = Vec::new();
        let scene_motion;
        let (motion, scene_names) = match &self.source {
            Source::Skin(motion) => (motion, None),
            Source::Scene(nodes) => {
                let names = bones
                    .iter()
                    .filter_map(|name| match selection.get(name) {
                        Some(Some(source)) => Some(source.clone()),
                        Some(None) => None,
                        None => indices.contains_key(name.as_str()).then(|| name.clone()),
                    })
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                scene_motion = nodes.select(Some(&names), false)?;
                (&scene_motion, Some(names))
            }
        };
        for (bone, name) in bones.iter().enumerate() {
            let source = match selection.get(name) {
                Some(Some(source)) => source,
                Some(None) => continue,
                None => name,
            };
            let Some(&index) = indices.get(source.as_str()) else {
                continue;
            };
            let index = match &scene_names {
                Some(names) => names.iter().position(|n| n == source).unwrap() as u16,
                None => index,
            };
            if let Some(track) = motion.tracks.iter().find(|track| track.bone == index) {
                let mut track = track.clone();
                track.bone = u16::try_from(bone)?;
                tracks.push(track);
            }
        }
        let motion = Motion {
            duration_frames: motion.duration_frames,
            tracks,
        };
        // Each track keeps its validated immutable nodes. Binding changes only
        // its unique target index, checked above, not the underlying curves.
        Ok(motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Result<(Skeleton, Motion)> {
        let skeleton = Skeleton::single("hips");
        let mut node = NodeTrack::new(Transform::default());
        node.rotation = Some(Channel {
            interpolation: Interpolation::Linear,
            times: vec![0., 1.],
            values: vec![
                [0., 0., 0., 1.],
                glam::Quat::from_rotation_y(0.5).to_array(),
            ],
        });
        Ok((
            skeleton,
            Motion {
                duration_frames: 30.,
                tracks: vec![Track {
                    bone: 0,
                    nodes: vec![node.into()],
                }],
            },
        ))
    }
    #[test]
    fn shared_curves_bind_to_each_joint_order_and_keep_model_rest_poses() -> Result<()> {
        let (small, motion) = fixture()?;
        let shared = NamedMotion::decode(&motion.encode(&small)?)?;
        let mut large = Skeleton::single("hand");
        large.bones.push(Bone {
            name: "waist".into(),
            parent: None,
            parent_transform: None,
            shadow_bounds: true,
            bind: Transform {
                translation: [0., 7., 0.],
                ..Default::default()
            },
        });
        let overrides = [("waist".into(), Some("hips".into()))].into();
        let a = shared.bind(&["hips".into()], &BTreeMap::new())?;
        let b = shared.bind(&["hand".into(), "waist".into()], &overrides)?;
        assert_eq!((a.tracks[0].bone, b.tracks[0].bone), (0, 1));
        for frame in [0., 7.5, 30.] {
            assert_eq!(
                small.sample(&a, frame)?.local[0].rotation,
                large.sample(&b, frame)?.local[1].rotation
            );
            assert_eq!(large.sample(&b, frame)?.local[1].translation, [0., 7., 0.]);
        }
        // One edit reaches both targets, without mutating either prepared clip.
        let mut edited = motion;
        Arc::make_mut(&mut edited.tracks[0].nodes[0])
            .rotation
            .as_mut()
            .unwrap()
            .values[1] = glam::Quat::from_rotation_y(1.).to_array();
        let edited = NamedMotion::decode(&edited.encode(&small)?)?;
        assert_ne!(
            a.tracks,
            edited.bind(&["hips".into()], &BTreeMap::new())?.tracks
        );
        assert_ne!(
            b.tracks,
            edited
                .bind(&["hand".into(), "waist".into()], &overrides)?
                .tracks
        );
        Ok(())
    }
    #[test]
    fn sparse_names_explicit_exclusions_aliases_and_invalid_overrides_are_distinct() -> Result<()> {
        let (skeleton, motion) = fixture()?;
        let shared = NamedMotion::decode(&motion.encode(&skeleton)?)?;
        let bones = vec!["hips".into(), "accessory".into()];
        assert_eq!(shared.bind(&bones, &BTreeMap::new())?.tracks.len(), 1);
        assert!(
            shared
                .bind(&bones, &[("hips".into(), None)].into())?
                .tracks
                .is_empty()
        );
        let aliases = [("accessory".into(), Some("hips".into()))].into();
        assert_eq!(shared.bind(&bones, &aliases)?.tracks.len(), 2);
        assert!(
            shared
                .bind(&bones, &[("hips".into(), Some("missing".into()))].into())
                .is_err()
        );
        assert!(
            shared
                .bind(&bones, &[("missing".into(), Some("hips".into()))].into())
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn scene_export_group_transforms_are_kept_when_no_skin_declares_joint_boundaries() -> Result<()>
    {
        let (skeleton, motion) = fixture()?;
        let bytes = motion.encode_nodes(&skeleton)?;
        let glb = gltf::binary::Glb::from_slice(&bytes)?;
        let mut json: serde_json::Value = serde_json::from_slice(&glb.json)?;
        json["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name":"editor-group","translation":[3,4,5],"children":[0]}));
        json["scenes"][0]["nodes"] = serde_json::json!([1]);
        let bytes = resonance_asset_writer::gltf::pack_glb(&json, glb.bin.as_deref().unwrap())?;
        let named = NamedMotion::decode(&bytes)?;
        let bound = named.bind(&["hips".into()], &BTreeMap::new())?;
        let expected = Motion::decode_for_bones(&bytes, &["hips".into()])?;
        assert_eq!(bound, expected);
        assert_eq!(
            bound.tracks[0].sample_matrix(15., Transform::default())?[3],
            [3., 4., 5., 1.]
        );
        let alias = named.bind(
            &["ankle".into()],
            &[("ankle".into(), Some("hips".into()))].into(),
        )?;
        assert_eq!(bound, alias);
        assert!(Arc::ptr_eq(
            &bound.tracks[0].nodes[0],
            &alias.tracks[0].nodes[0]
        ));
        Ok(())
    }
}

#[cfg(test)]
mod shared_nodes_tests {
    use super::*;
    #[test]
    fn overflowing_duration_is_rejected_before_binding() -> Result<()> {
        let skeleton = Skeleton::single("root");
        let mut node = NodeTrack::new(Transform::default());
        node.scale = Some(Channel {
            interpolation: Interpolation::Step,
            times: vec![f32::MAX],
            values: vec![[1.; 3]],
        });
        let source = Motion {
            duration_frames: 1.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![node.into()],
            }],
        };
        let bytes = source.encode(&skeleton)?;
        assert!(NamedMotion::decode(&bytes).is_err());
        assert!(Motion::decode(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn distinct_rigs_share_validated_curves_and_node_edits_are_independent() -> Result<()> {
        let skeleton = Skeleton::single("root");
        let mut node = NodeTrack::new(Transform::default());
        node.translation = Some(Channel {
            interpolation: Interpolation::Linear,
            times: vec![0., 1.],
            values: vec![[0.; 3], [1.; 3]],
        });
        let source = Motion {
            duration_frames: FRAME_HZ,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![node.into()],
            }],
        };
        let named = NamedMotion::decode(&source.encode(&skeleton)?)?;
        let first = named.bind(&["root".into()], &BTreeMap::new())?;
        let mut second = named.bind(&["other".into(), "root".into()], &BTreeMap::new())?;
        assert_eq!(first.tracks[0].bone, 0);
        assert_eq!(second.tracks[0].bone, 1);
        assert!(Arc::ptr_eq(
            &first.tracks[0].nodes[0],
            &second.tracks[0].nodes[0]
        ));
        Arc::make_mut(&mut second.tracks[0].nodes[0])
            .translation
            .as_mut()
            .unwrap()
            .values[1] = [2.; 3];
        assert_eq!(
            first.tracks[0].nodes[0]
                .translation
                .as_ref()
                .unwrap()
                .values[1],
            [1.; 3]
        );
        assert_eq!(named.bind(&["root".into()], &BTreeMap::new())?, first);
        first.validate_bones(1)?;
        second.validate_bones(2)?;
        let round_trip: Motion = serde_json::from_value(serde_json::to_value(&first)?)?;
        assert_eq!(round_trip, first);
        Ok(())
    }
}
