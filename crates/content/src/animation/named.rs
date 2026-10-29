//! A GLB's curves are loaded once. Each model selects joints independently;
//! neither model node order nor a second instance can change the shared asset.
use super::*;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct NamedMotion {
    pub motion: Motion,
    pub joints: Vec<String>,
    // Scene-only exports have no explicit joint boundary. Select their model
    // joints before folding editor-added parent transforms into each track.
    scene: Option<gltf::Gltf>,
}

impl NamedMotion {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let glb = super::codec::parse(bytes)?;
        let motion = super::codec::decode(&glb, None, false)?;
        let skin = glb.skins().next();
        let nodes: Vec<_> = match skin.as_ref() {
            Some(skin) => skin.joints().collect(),
            None => glb.nodes().collect(),
        };
        let joints = nodes
            .into_iter()
            .map(|node| {
                node.name()
                    .map(str::to_owned)
                    .context("animation has an unnamed joint")
            })
            .collect::<Result<_>>()?;
        let scene_only = skin.is_none();
        Ok(Self {
            motion,
            joints,
            scene: scene_only.then_some(glb),
        })
    }

    /// Bind named channels and validate the result against the target joints.
    pub fn bind(
        &self,
        bones: &[String],
        selection: &BTreeMap<String, Option<String>>,
    ) -> Result<Motion> {
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
        let scene_names = self.scene.as_ref().map(|_| {
            bones
                .iter()
                .filter_map(|name| match selection.get(name) {
                    Some(Some(source)) => Some(source.clone()),
                    Some(None) => None,
                    None => indices.contains_key(name.as_str()).then(|| name.clone()),
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        });
        let scene = self
            .scene
            .as_ref()
            .zip(scene_names.as_ref())
            .map(|(glb, names)| super::codec::decode(glb, Some(names), false))
            .transpose()?;
        for (bone, name) in bones.iter().enumerate() {
            let source = match selection.get(name) {
                Some(Some(source)) => source,
                Some(None) => continue,
                None => name,
            };
            let Some(&index) = indices.get(source.as_str()) else {
                continue;
            };
            let (motion, index) = match (&scene, &scene_names) {
                (Some(motion), Some(names)) => (
                    motion,
                    names.iter().position(|n| n == source).unwrap() as u16,
                ),
                _ => (&self.motion, index),
            };
            if let Some(track) = motion.tracks.iter().find(|track| track.bone == index) {
                let mut track = track.clone();
                track.bone = u16::try_from(bone)?;
                tracks.push(track);
            }
        }
        let motion = Motion {
            duration_frames: self.motion.duration_frames,
            tracks,
        };
        motion.validate_bones(bones.len())?;
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
                    nodes: vec![node],
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
        edited.tracks[0].nodes[0].rotation.as_mut().unwrap().values[1] =
            glam::Quat::from_rotation_y(1.).to_array();
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
        Ok(())
    }
}
