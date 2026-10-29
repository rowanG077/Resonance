use super::*;
use anyhow::Result;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};

fn geometry() -> Value {
    json!({
        "asset":{"version":"2.0"},
        "nodes":[
            {"name":"root","extras":{"resonance_bone":0},"children":[1,4]},
            {"name":"coat","extras":{"resonance_bone":1},"children":[2]},
            {"name":"editor group","children":[3]},
            {"name":"cloth","mesh":0},
            {"name":"body","mesh":1},
            {"name":"skinned surface","mesh":2,"skin":0}
        ],
        "meshes":[{}, {}, {}]
    })
}
fn packed(value: &Value) -> Result<Vec<u8>> {
    resonance_asset_writer::gltf::pack_glb(value, &[])
}
fn definition() -> Value {
    json!({"parts":[{"mesh":"body.glb","materials":[{"meshes":[0,1,2]}]}]})
}

#[test]
fn model_edits_use_flat_layers_and_glb_geometry_ownership_with_retained_snapshots() -> Result<()> {
    let base = tempfile::tempdir()?;
    let modification = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(&json!({
            "version":crate::library::VERSION,"game":crate::library::GAME
        }))?,
    )?;
    fs::write(
        base.path().join("model.json"),
        serde_json::to_vec(&definition())?,
    )?;
    let mut gltf = geometry();
    fs::write(base.path().join("body.glb"), packed(&gltf)?)?;
    let library = crate::library::Library::open(base.path(), &[])?;
    let mut retained = library.snapshot();
    let mut model = document::read("model.json", |p| retained.read(p, 1024 * 1024))?;
    document::prepare(&mut model, |p| retained.read(p, 1024 * 1024))?;
    assert_eq!((model.scale, model.elevation), (1., 0.));
    assert!(model.behavior.is_none() && model.hidden_geometry.is_empty());
    assert_eq!(
        model.parts[0].scene.material_nodes,
        [vec![1], vec![0], vec![]]
    );
    assert_eq!(serde_json::to_value(&model)?, definition());

    // Reparent geometry using an ordinary editor group, rename the joint and
    // reorder every GLB node. Only the hierarchy determines visibility ownership.
    gltf["nodes"][1]["name"] = "cape".into();
    gltf["nodes"][1]["children"] = json!([]);
    gltf["nodes"][0]["children"] = json!([1, 2, 4]);
    let order = [5, 3, 2, 1, 4, 0];
    let indices: BTreeMap<usize, usize> = order
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    let old = gltf["nodes"].as_array().unwrap().clone();
    gltf["nodes"] = Value::Array(
        order
            .into_iter()
            .map(|i| {
                let mut node = old[i].clone();
                if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
                    for child in children {
                        *child = json!(indices[&(child.as_u64().unwrap() as usize)]);
                    }
                }
                node
            })
            .collect(),
    );
    fs::write(modification.path().join("body.glb"), packed(&gltf)?)?;
    let edited = crate::library::Library::open(
        base.path(),
        &[crate::library::Mod {
            name: "geometry edit".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut fresh = edited.snapshot();
    let mut edited = document::read("model.json", |p| fresh.read(p, 1024 * 1024))?;
    document::prepare(&mut edited, |p| fresh.read(p, 1024 * 1024))?;
    assert_eq!(edited.parts[0].scene.bone_names, ["root", "cape"]);
    assert_eq!(
        edited.parts[0].scene.material_nodes,
        [vec![0], vec![0], vec![]]
    );
    document::prepare(&mut model, |p| retained.read(p, 1024 * 1024))?;
    assert_eq!(
        model.parts[0].scene.material_nodes,
        [vec![1], vec![0], vec![]]
    );
    assert_eq!(model.parts[0].scene.bone_names, ["root", "coat"]);
    Ok(())
}

#[test]
fn geometry_visibility_handles_shared_meshes_and_rejects_invalid_hierarchies() -> Result<()> {
    let mut part: ScenePart = serde_json::from_value(definition()["parts"][0].clone())?;
    let mut gltf = geometry();
    gltf["nodes"][4]["mesh"] = 0.into();
    part.prepare_bones(&packed(&gltf)?)?;
    assert_eq!(part.material_nodes, [vec![1, 0], vec![], vec![]]);
    for (node, field, value) in [
        (0, "children", json!([99])),
        (0, "children", json!([1, 3, 4])),
        (3, "mesh", json!(99)),
    ] {
        let mut broken = geometry();
        broken["nodes"][node][field] = value;
        assert!(part.prepare_bones(&packed(&broken)?).is_err());
    }
    let mut cycle = geometry();
    cycle["nodes"][1]["children"] = json!([]);
    cycle["nodes"][3]["children"] = json!([2]);
    assert!(part.prepare_bones(&packed(&cycle)?).is_err());
    Ok(())
}
