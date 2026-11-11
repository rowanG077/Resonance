use super::*;
use serde_json::{Value, json};

fn keys() -> Vec<CameraKey> {
    vec![
        CameraKey::looking_at(0., [1., 2., 3.], [4., 5., 6.]),
        CameraKey::looking_at(150., [21., 42., 63.], [44., 55., 66.]),
    ]
}
fn patch(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let glb = gltf::binary::Glb::from_slice(bytes).unwrap();
    let mut document: Value = serde_json::from_slice(&glb.json).unwrap();
    edit(&mut document);
    resonance_asset_writer::gltf::pack_glb(&document, glb.bin.as_deref().unwrap()).unwrap()
}

#[test]
fn glb_camera_nodes_preserve_points_and_honor_editor_parent_transforms() -> Result<()> {
    let mut source = keys();
    for key in &mut source {
        let rotation = glam::Quat::from_array(key.orientation)
            * glam::Quat::from_rotation_z(150_f32.to_radians());
        key.orientation = rotation.to_array();
    }
    let motion = CameraMotion::from_keys(&source)?;
    let bytes = motion.encode()?;
    let glb = gltf::Gltf::from_slice(&bytes)?;
    assert_eq!(glb.skins().len(), 0);
    assert_eq!(
        glb.nodes()
            .map(|node| node.name().unwrap())
            .collect::<Vec<_>>(),
        NAMES
    );
    let prepared = CameraMotion::decode(&bytes)?;
    assert_eq!(prepared.duration_frames(), 150.);
    let point = prepared.sample(75.)?;
    assert_eq!(point.position, [11., 22., 33.]);
    assert_eq!(point.target, [24., 30., 36.]);
    let expected = glam::Quat::from_array(source[0].orientation)
        .slerp(glam::Quat::from_array(source[1].orientation), 0.5);
    assert!(
        expected
            .dot(glam::Quat::from_array(point.orientation))
            .abs()
            > 0.999999
    );
    let up = glam::Vec3::from_array(point.up());
    assert!(up.distance(expected * glam::Vec3::Y) < 0.00001);
    let edited = patch(&bytes, |doc| {
        doc["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"Move both paths", "children":[0,1],"translation":[10.,20.,30.]}));
        doc["scenes"][0]["nodes"] = json!([2]);
    });
    let point = CameraMotion::decode(&edited)?.sample(75.)?;
    assert_eq!(point.position, [21., -8., 53.]);
    assert_eq!(point.target, [34., 0., 56.]);
    let missing = patch(&bytes, |doc| doc["nodes"][0]["name"] = "missing".into());
    assert!(CameraMotion::decode(&missing).is_err());
    assert!(CameraMotion::decode(&serde_json::to_vec(&keys())?).is_err());
    Ok(())
}

#[test]
fn camera_edits_keep_step_cubic_and_static_target_behavior() -> Result<()> {
    let mut source = CameraMotion::from_keys(&keys())?;
    std::sync::Arc::make_mut(&mut source.tracks[0].nodes[0]).translation = Some(Channel {
        interpolation: Interpolation::CubicSpline,
        times: vec![0., 150.],
        values: vec![
            [0.; 3],
            [0.; 3],
            [4. / 30., 0., 0.],
            [0.; 3],
            [10., 0., 0.],
            [0.; 3],
        ],
    });
    std::sync::Arc::make_mut(&mut source.tracks[1].nodes[0])
        .translation
        .as_mut()
        .unwrap()
        .interpolation = Interpolation::Step;
    let decoded = CameraMotion::decode(&source.encode()?)?;
    assert_eq!(decoded.sample(75.)?.position, [7.5, 0., 0.]);
    assert_eq!(decoded.sample(75.)?.target, keys()[0].target);
    assert_eq!(decoded.sample(150.)?.target, keys()[1].target);
    let static_target = patch(&source.encode()?, |doc| {
        doc["animations"][0]["channels"]
            .as_array_mut()
            .unwrap()
            .retain(|channel| channel["target"]["node"] != 1);
        doc["nodes"][1]["translation"] = json!([7., 8., 9.]);
    });
    assert_eq!(
        CameraMotion::decode(&static_target)?.sample(75.)?.target,
        [7., -9., 8.]
    );
    Ok(())
}

#[test]
fn camera_mods_keep_captured_motion_and_follow_reordered_named_nodes() -> Result<()> {
    use crate::{
        library::{Descriptor, GAME, Library, VERSION},
        preparation::JSON_LIMIT,
    };
    let root = tempfile::tempdir()?;
    let base = root.path().join("base");
    let modified = root.path().join("mod");
    for p in [&base, &modified] {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(
        base.join("content.json"),
        serde_json::to_vec(&Descriptor {
            version: VERSION,
            game: GAME.into(),
        })?,
    )?;
    let original = CameraMotion::from_keys(&keys())?.encode()?;
    std::fs::write(base.join("camera.glb"), &original)?;
    std::fs::copy(base.join("camera.glb"), modified.join("camera.glb"))?;
    let library = Library::open_paths(&base, &[modified.clone()])?;
    let mut retained = library.snapshot();
    let read = |snapshot: &mut crate::library::Snapshot| {
        CameraMotion::decode(&snapshot.read("camera.glb", JSON_LIMIT)?)
    };
    let before = read(&mut retained)?.sample(75.)?;
    let mut changed = keys();
    changed[1].position[2] += 20.;
    let edit = patch(&CameraMotion::from_keys(&changed)?.encode()?, |doc| {
        doc["nodes"].as_array_mut().unwrap().swap(0, 1);
        for channel in doc["animations"][0]["channels"].as_array_mut().unwrap() {
            channel["target"]["node"] = json!(1 - channel["target"]["node"].as_u64().unwrap());
        }
    });
    std::fs::write(modified.join("camera.glb"), &edit)?;
    let after = read(&mut library.snapshot())?.sample(75.)?;
    assert_eq!(after.position, [11., 22., 43.]);
    assert_eq!(after.target, before.target);
    assert_eq!(read(&mut retained)?.sample(75.)?.position, before.position);
    Ok(())
}
