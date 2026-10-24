use super::*;

#[test]
fn catalogue_models_have_named_documents_and_independent_editable_owners() -> Result<()> {
    use resonance_content::{animation::Motion, model_preview::document};
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    fs::create_dir_all(source.path().join("original"))?;
    fs::create_dir_all(source.path().join("effects"))?;
    fs::write(
        source.path().join("original/mesh.glb"),
        b"opaque mesh bytes",
    )?;
    fs::write(source.path().join("original/skin.png"), b"original image")?;
    fs::write(
        source.path().join(crate::field_lighting::SCRATCH_RAMP),
        resonance_asset_writer::ktx2::encode_rgba8(1, 1, &[0, 255, 0, 255])?,
    )?;
    fs::write(
        source.path().join("original/idle.motion.json"),
        Motion {
            duration_frames: 30.,
            tracks: vec![],
        }
        .encode()?,
    )?;
    let preview = serde_json::json!({
        "scale": 2, "elevation": 3, "hidden_geometry": [], "behavior": null,
        "parts": [{"animation": 4, "attached_to": null, "additive": false,
            "scene": {"resource": 0, "mesh": "original/mesh.glb", "textures": ["original/skin.png"],
                "materials": [], "translation": [0,0,0], "autoplay": false,
                "texture_animations": [], "bone_names": ["root"],
                "clips": [{"motion": "original/idle.motion.json", "resource_slot": 4, "duration_seconds": 1,
                    "animation_resource": null, "secondary_pose_nodes": [0]}]}}
        ]
    });
    let mut book = serde_json::json!({"records": [
        {"id": 0, "name": "A changed caption", "preview": preview},
        {"id": 1, "name": "A changed caption", "preview": preview},
    ]});
    let mut publisher = Publisher::new(source.path(), output.path())?;
    publisher.preview_catalogue(&mut book, "monsters")?;
    let paths: Vec<_> = book["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["preview"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "meshes/monsters/torent-0000/model.json",
            "meshes/monsters/orcrot-0001/model.json"
        ]
    );
    let mut models = Vec::new();
    for path in paths {
        let mut model =
            document::read(path, |path| Ok(fs::read(output.path().join(path))?.into()))?;
        document::prepare(&mut model, |path| {
            Ok(fs::read(output.path().join(path))?.into())
        })?;
        assert_eq!((model.scale, model.elevation), (2., 3.));
        assert_eq!(model.parts[0].scene.clips[0].duration_seconds, 1.);
        assert_eq!(model.parts[0].scene.clips[0].secondary_pose_nodes, [0]);
        assert!(!serde_json::to_string(&model)?.contains("original/"));
        models.push(model);
    }
    let first = &models[0].parts[0].scene.textures[0];
    let second = &models[1].parts[0].scene.textures[0];
    assert_ne!(first, second);
    fs::write(output.path().join(first), b"edited image")?;
    assert_eq!(fs::read(output.path().join(second))?, b"original image");
    Ok(())
}
