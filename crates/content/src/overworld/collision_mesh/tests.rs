use super::*;
use anyhow::ensure;
use resonance_asset_writer::gltf::pack_glb;
use serde_json::Value;

#[test]
fn editor_object_order_names_and_transforms_keep_explicit_collision_properties() -> Result<()> {
    let groups: Vec<_> = [TerrainSurface::Grassland, TerrainSurface::Water]
        .into_iter()
        .map(|surface| CollisionGroup {
            surface: surface.index() as u32,
            vertices: vec![[1., 2., 3.], [5., 2., 3.], [1., 6., 3.]],
            triangles: vec![[0, 1, 2]],
        })
        .collect();
    let bytes = encode(&groups)?;
    let restored = decode(&bytes)?;
    assert_eq!(
        serde_json::to_value(&restored)?,
        serde_json::to_value(&groups)?
    );
    let glb = gltf::binary::Glb::from_slice(&bytes)?;
    let mut document: Value = serde_json::from_slice(&glb.json)?;
    document["scenes"][0]["nodes"] = json!([1, 0]);
    document["nodes"][0]["name"] = "Edited plateau".into();
    document["nodes"][0]["translation"] = json!([5, 7, 11]);
    document["nodes"][0]["extras"]["surface"] = "wasteland".into();
    let binary = glb.bin.as_deref().unwrap();
    let edited = decode(&pack_glb(&document, binary)?)?;
    assert_eq!(edited[0].vertices[0], [6., -9., 10.]);
    assert_eq!(edited[0].surface, TerrainSurface::Wasteland.index() as u32);
    assert_eq!(edited[1].surface, TerrainSurface::Water.index() as u32);
    document["nodes"][0]["extras"]
        .as_object_mut()
        .unwrap()
        .remove("surface");
    assert!(decode(&pack_glb(&document, binary)?).is_err());
    document["nodes"][0]["extras"]["surface"] = 16.into();
    assert!(decode(&pack_glb(&document, binary)?).is_err());
    assert!(decode(&serde_json::to_vec(&groups)?).is_err());
    Ok(())
}

#[test]
#[ignore = "requires the saved Blender collision exports; no editor or conversion is launched"]
fn blender_roundtrip_and_geometry_edit_preserve_surface_queries() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/world-collision-semantics");
    let original = decode(&std::fs::read(root.join("editor-original.glb"))?)?;
    let roundtrip = decode(&std::fs::read(root.join("editor-roundtrip.glb"))?)?;
    let edited = decode(&std::fs::read(root.join("editor-edited.glb"))?)?;
    for (actual, lift) in [(&roundtrip, 0.), (&edited, 10.)] {
        assert_eq!(actual.len(), original.len());
        for (old, new) in original.iter().zip(actual) {
            assert_eq!(old.surface, new.surface);
            assert_eq!(old.triangles.len(), new.triangles.len());
            for (a, b) in old.triangles.iter().zip(&new.triangles) {
                for (&a, &b) in a.iter().zip(b) {
                    let a = old.vertices[usize::from(a)];
                    let b = new.vertices[usize::from(b)];
                    for axis in 0..3 {
                        let expected = a[axis] + if axis == 2 { lift } else { 0. };
                        ensure!(
                            (expected - b[axis]).abs() < 0.003,
                            "editor changed collision face ordering or geometry"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
