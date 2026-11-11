use super::*;
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
