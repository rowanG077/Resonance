use super::*;
use serde_json::json;

fn document() -> Value {
    json!({"nodes":[
        {"name":"parent","translation":[10,20,3],"children":[1,2]},
        {"name":"same hinge","extras":{"resonance_bone":2,
            "door_action":"pull","door_approach":[70,-45.287,0],"door_angle_degrees":-45}},
        {"name":"same hinge","translation":[0,400,0],"extras":{"resonance_bone":1,
            "door_action":"push","door_approach":[-70,-55.287,0],"door_angle_degrees":30}}
    ]})
}

#[test]
fn moved_parents_scenery_and_renamed_hinges_move_their_interactions() -> Result<()> {
    let mut doc = document();
    let old = from_gltf(&doc, [0.; 3])?;
    assert_eq!(old.iter().map(|d| d.bone).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(old[1].position, [10., 20., 3.]);
    assert_eq!(old[1].heading, 180.);
    assert!(old[1].pull);
    assert!((old[1].approach[1] + 25.287).abs() < 0.00001);
    doc["nodes"][0]["translation"] = json!([110, 220, 33]);
    doc["nodes"][1]["name"] = json!("edited left door");
    let edited = from_gltf(&doc, [7., 11., 13.])?;
    for (a, b) in old.iter().zip(&edited) {
        assert_eq!(
            (a.bone, a.pull, a.angle, a.heading),
            (b.bone, b.pull, b.angle, b.heading)
        );
        for axis in 0..3 {
            let delta = [107., 211., 43.][axis];
            assert!((b.position[axis] - a.position[axis] - delta).abs() < 0.0001);
            assert!((b.approach[axis] - a.approach[axis] - delta).abs() < 0.0001);
        }
    }
    doc["nodes"].as_array_mut().unwrap().swap(1, 2);
    assert_eq!(
        serde_json::to_value(from_gltf(&doc, [7., 11., 13.])?)?,
        serde_json::to_value(edited)?
    );
    Ok(())
}

#[test]
fn incomplete_settings_and_cyclic_or_duplicate_hinges_are_rejected() {
    for key in [
        "door_action",
        "door_approach",
        "door_angle_degrees",
        "resonance_bone",
    ] {
        let mut doc = document();
        doc["nodes"][1]["extras"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(from_gltf(&doc, [0.; 3]).is_err(), "{key}");
    }
    let mut doc = document();
    doc["nodes"][1]["children"] = json!([0]);
    assert!(from_gltf(&doc, [0.; 3]).is_err());
    let mut doc = document();
    doc["nodes"][1]["extras"]["resonance_bone"] = json!(1);
    assert!(from_gltf(&doc, [0.; 3]).is_err());
    for value in [json!(0), json!(181), json!("thirty")] {
        let mut doc = document();
        doc["nodes"][1]["extras"]["door_angle_degrees"] = value;
        assert!(from_gltf(&doc, [0.; 3]).is_err());
    }
    let mut doc = document();
    doc["nodes"][1]["extras"]["door_action"] = json!("swing");
    assert!(from_gltf(&doc, [0.; 3]).is_err());
}

#[test]
#[ignore = "requires saved Blender door exports; does not run the editor"]
fn blender_keeps_door_properties_and_moves_interactions_with_hinges() -> Result<()> {
    use std::{fs, path::Path};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/field-doors");
    let original = decode(&fs::read(root.join("editor-original.glb"))?, [0.; 3])?;
    for (file, lift) in [("editor-roundtrip.glb", 0.), ("editor-edited.glb", 10.)] {
        let current = decode(&fs::read(root.join(file))?, [0.; 3])?;
        assert_eq!(current.len(), original.len());
        for (a, b) in original.iter().zip(&current) {
            assert_eq!((a.bone, a.pull, a.angle), (b.bone, b.pull, b.angle));
            assert_eq!(a.heading, b.heading);
            for axis in 0..3 {
                let delta = if axis == 2 { lift } else { 0. };
                assert!((b.position[axis] - a.position[axis] - delta).abs() < 0.003);
                assert!((b.approach[axis] - a.approach[axis] - delta).abs() < 0.003);
            }
        }
    }
    Ok(())
}
