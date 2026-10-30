use super::*;

fn motion() -> (Skeleton, Motion) {
    let skeleton = Skeleton::single("root");
    let mut node = NodeTrack::new(Transform::default());
    node.translation = Some(Channel {
        interpolation: Interpolation::CubicSpline,
        times: vec![0., 1.],
        values: vec![
            [0.; 3],
            [0.; 3],
            [0., 12., 0.],
            [0.; 3],
            [8., 0., 0.],
            [0.; 3],
        ],
    });
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![Track {
            bone: 0,
            nodes: vec![node],
        }],
    };
    (skeleton, motion)
}

#[test]
fn standard_channels_and_editor_node_reordering() -> Result<()> {
    let (skeleton, motion) = motion();
    let bytes = motion.encode(&skeleton)?;
    let glb = gltf::binary::Glb::from_slice(&bytes)?;
    let mut json: Value = serde_json::from_slice(&glb.json)?;
    assert!(json["animations"][0]["extras"].is_null());
    assert!(json["nodes"][0]["extras"].is_null());
    // A normal exporter can insert an object and reorder its node array.
    json["nodes"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"name":"Editor object"}));
    json["scenes"][0]["nodes"] = json!([0, 1]);
    json["skins"][0]["joints"] = json!([1]);
    json["animations"][0]["channels"][0]["target"]["node"] = 1.into();
    let bytes = resonance_asset_writer::gltf::pack_glb(&json, glb.bin.as_deref().unwrap())?;
    let decoded = Motion::decode_for_skeleton(&bytes, &skeleton)?;
    let p = skeleton.sample_point(&decoded, 15., 0, [0.; 3])?;
    assert_eq!(p, [4., 1.5, 0.]);
    assert_eq!(decoded.duration_frames, 30.);
    Ok(())
}

#[test]
fn independent_timelines_clamp_and_quaternion_cubics_normalize() -> Result<()> {
    let mut channel = Channel {
        interpolation: Interpolation::Linear,
        times: vec![0.5, 1.],
        values: vec![[2., 0., 0.], [4., 0., 0.]],
    };
    assert_eq!(channel.sample(0., false)?, [2., 0., 0.]);
    assert_eq!(channel.sample(2., false)?, [4., 0., 0.]);
    channel.interpolation = Interpolation::Step;
    assert_eq!(channel.sample(0.99, false)?, [2., 0., 0.]);
    assert_eq!(channel.sample(1., false)?, [4., 0., 0.]);
    let channel = Channel {
        interpolation: Interpolation::CubicSpline,
        times: vec![0., 1.],
        values: vec![
            [0.; 4],
            [0., 0., 0., 1.],
            [0.; 4],
            [0.; 4],
            [0., 0., 1., 0.],
            [0.; 4],
        ],
    };
    channel.validate(true)?;
    let q = channel.sample(0.5, true)?;
    assert!((q.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-6);
    assert!((q[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    Ok(())
}

#[test]
fn standard_transform_chain_preserves_affine_attachment() -> Result<()> {
    let skeleton = Skeleton::single("hand");
    let mut parent = NodeTrack::new(Transform {
        scale: [2., 1., 1.],
        ..Default::default()
    });
    parent.translation = Some(Channel {
        interpolation: Interpolation::Step,
        times: vec![0., 1.],
        values: vec![[3., 4., 5.]; 2],
    });
    let child = NodeTrack::new(Transform {
        rotation: glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_4).to_array(),
        ..Default::default()
    });
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![Track {
            bone: 0,
            nodes: vec![parent, child],
        }],
    };
    let bytes = motion.encode(&skeleton)?;
    let decoded = Motion::decode_for_skeleton(&bytes, &skeleton)?;
    assert!(decoded.tracks[0].is_affine());
    let a = skeleton.sample_point(&motion, 15., 0, [1., 2., 3.])?;
    let b = skeleton.sample_point(&decoded, 15., 0, [1., 2., 3.])?;
    assert!(a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5));
    Ok(())
}

#[test]
fn empty_and_hold_clips_have_standard_timing_without_bone_channels() -> Result<()> {
    for frames in [0., 30.] {
        let skeleton = Skeleton::single("root");
        let motion = Motion {
            duration_frames: frames,
            tracks: vec![],
        };
        let bytes = motion.encode(&skeleton)?;
        let decoded = Motion::decode_for_skeleton(&bytes, &skeleton)?;
        assert_eq!(decoded.duration_frames, frames);
        assert!(decoded.tracks.is_empty());
    }
    assert!(Motion::decode(br#"{"version":1,"duration_frames":30,"tracks":[]}"#).is_err());
    let (skeleton, motion) = motion();
    let bytes = motion.encode(&skeleton)?;
    assert!(Motion::decode_for_bones(&bytes, &["missing".into()]).is_err());
    for length in [0, 4, bytes.len() / 2, bytes.len() - 1] {
        assert!(Motion::decode(&bytes[..length]).is_err());
    }
    Ok(())
}
