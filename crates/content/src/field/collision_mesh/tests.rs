use super::*;
use resonance_asset_writer::gltf::pack_glb;
use serde_json::{Value, json};

fn triangle(surface: u32) -> CollisionGroup {
    CollisionGroup {
        surface,
        vertices: vec![[0., 0., 2.], [10., 0., 2.], [0., 10., 2.]],
        triangles: vec![[0, 1, 2]],
    }
}

#[test]
fn editor_properties_keep_lighting_separate_from_collision_flags() -> Result<()> {
    let collision = Collision {
        ground: vec![triangle(FLAGS | 0x8420)],
        regions: vec![triangle(0xa003)],
    };
    let bytes = encode(&collision)?;
    let glb = gltf::binary::Glb::from_slice(&bytes)?;
    let mut document: Value = serde_json::from_slice(&glb.json)?;
    assert_eq!(document["nodes"][0]["extras"]["attributes"], 0x8420);
    assert_eq!(document["nodes"][0]["extras"]["ignore_characters"], true);
    assert_eq!(document["nodes"][1]["extras"]["light"], 3);
    assert_eq!(
        serde_json::to_value(decode(&bytes)?)?,
        serde_json::to_value(&collision)?
    );
    document["scenes"][0]["nodes"] = json!([1, 0]);
    document["nodes"][0]["name"] = json!("my edited ramp");
    document["nodes"][0]["translation"] = json!([0, 10, 0]);
    document["nodes"][0]["extras"]["ignore_characters"] = false.into();
    document["nodes"][1]["extras"]["light"] = 7.into();
    let edited = decode(&pack_glb(&document, glb.bin.as_deref().unwrap())?)?;
    assert_eq!(edited.ground[0].vertices[0], [0., 0., 12.]);
    assert_eq!(
        edited.ground[0].surface,
        collision.ground[0].surface & !IGNORE_CHARACTERS
    );
    assert_eq!(edited.regions[0].surface, 0xa007);
    assert!(super::super::CollisionQuery::Player.accepts(edited.ground[0].surface));
    assert!(!super::super::CollisionQuery::Enemy.accepts(edited.ground[0].surface));
    document["nodes"][0]["extras"]["role"] = "solid".into();
    assert!(decode(&pack_glb(&document, glb.bin.as_deref().unwrap())?).is_err());
    assert!(decode(&serde_json::to_vec(&collision)?).is_err());
    Ok(())
}

#[test]
fn model_roles_preserve_solid_volumes_and_floor_priority() -> Result<()> {
    let collision = ModelCollision {
        floors: vec![triangle(0), triangle(super::super::SLIDING_SURFACE | 0x200)],
        solids: vec![triangle(IGNORE_BLOCKS)],
    };
    let bytes = encode_model(&collision)?;
    let glb = gltf::binary::Glb::from_slice(&bytes)?;
    let mut document: Value = serde_json::from_slice(&glb.json)?;
    document["scenes"][0]["nodes"] = json!([2, 1, 0]);
    for (index, node) in document["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        node["name"] = format!("custom {index}").into();
    }
    let result = decode_model(&pack_glb(&document, glb.bin.as_deref().unwrap())?)?;
    assert_eq!(
        serde_json::to_value(&result)?,
        serde_json::to_value(&collision)?
    );
    document["nodes"][2]["scale"] = json!([-1, 1, 1]);
    let reflected = decode_model(&pack_glb(&document, glb.bin.as_deref().unwrap())?)?;
    assert_eq!(reflected.solids[0].vertices[1], [-10., 0., 2.]);
    assert_eq!(reflected.solids[0].triangles[0], [0, 2, 1]);
    document["nodes"][0]["extras"]["attributes"] = IGNORE_ENEMIES.into();
    assert!(decode_model(&pack_glb(&document, glb.bin.as_deref().unwrap())?).is_err());
    assert!(decode(&bytes).is_err());
    Ok(())
}

#[test]
#[ignore = "requires saved Blender exports; no editor process launched"]
fn blender_field_collision_edit_preserves_roles_flags_and_order() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261026/field-collision");
    let original = decode(&std::fs::read(root.join("editor-original.glb"))?)?;
    for (filename, lift) in [("editor-roundtrip.glb", 0.), ("editor-edited.glb", 10.)] {
        let edited = decode(&std::fs::read(root.join(filename))?)?;
        assert_eq!(original.ground.len(), edited.ground.len());
        assert_eq!(original.regions.len(), edited.regions.len());
        for (a, b) in original
            .ground
            .iter()
            .chain(&original.regions)
            .zip(edited.ground.iter().chain(&edited.regions))
        {
            assert_eq!(a.surface, b.surface);
            assert_eq!(a.triangles.len(), b.triangles.len());
            for (a_face, b_face) in a.triangles.iter().zip(&b.triangles) {
                for (&a_index, &b_index) in a_face.iter().zip(b_face) {
                    let a = a.vertices[usize::from(a_index)];
                    let b = b.vertices[usize::from(b_index)];
                    for axis in 0..3 {
                        ensure!(
                            (a[axis] + if axis == 2 { lift } else { 0. } - b[axis]).abs() < 0.003,
                            "editor altered face order or geometry"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
