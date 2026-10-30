//! Named catalogue models. Menu records contain a direct path to model.json;
//! geometry, textures and motions are prepared only when the viewer needs them.
use super::ModelPreview;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::sync::Arc;

pub const TOON_RAMP: &str = "effects/field/textures/toon-ramp.ktx2";

pub fn read(path: &str, mut read: impl FnMut(&str) -> Result<Arc<[u8]>>) -> Result<ModelPreview> {
    crate::library::validate_path(path)?;
    let mut model: ModelPreview = serde_json::from_slice(&read(path)?)
        .with_context(|| format!("decode model document {path}"))?;
    model.document = Some(path.into());
    Ok(model)
}

/// Inline model documents before the catalogue's single typed decode.
/// Return origins separately because they are preparation metadata, not JSON.
pub fn expand_catalogue(
    mut value: Value,
    mut read_file: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<(Value, Vec<String>)> {
    let records = value
        .get_mut("records")
        .and_then(Value::as_array_mut)
        .context("model catalogue needs records")?;
    let mut paths = Vec::with_capacity(records.len());
    for record in records {
        let path = record
            .get("preview")
            .and_then(Value::as_str)
            .context("named catalogue preview must be a model.json path")?
            .to_owned();
        crate::library::validate_path(&path)?;
        record["preview"] = serde_json::from_slice(&read_file(&path)?)
            .with_context(|| format!("decode model document {path}"))?;
        paths.push(path);
    }
    Ok((value, paths))
}

/// Capture referenced descriptors without loading every catalogue mesh/texture.
pub fn capture_catalogue(
    value: &Value,
    snapshot: &mut crate::library::Snapshot,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let records = value
        .get("records")
        .and_then(Value::as_array)
        .context("model catalogue needs records")?;
    for record in records {
        ensure!(!cancelled(), "catalogue preparation cancelled");
        let path = record
            .get("preview")
            .and_then(Value::as_str)
            .context("named catalogue preview must be a model.json path")?;
        snapshot
            .read(path, crate::preparation::JSON_LIMIT)
            .with_context(|| format!("catalogue preview refers to {path}"))?;
    }
    Ok(())
}

/// Current motion data supplies duration; bone/cloth relationships remain explicit.
pub fn prepare(
    model: &mut ModelPreview,
    mut read: impl FnMut(&str) -> Result<Arc<[u8]>>,
) -> Result<()> {
    for part in &mut model.parts {
        part.scene
            .prepare_bones(&read(&part.scene.mesh)?)
            .with_context(|| part.scene.mesh.clone())?;
        for texture in &part.scene.textures {
            read(texture)?;
        }
        for clip in &mut part.scene.clips {
            let motion = clip
                .decode_motion(&read(&clip.motion)?, &part.scene.bone_names)
                .with_context(|| format!("decode preview motion {}", clip.motion))?;
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
    }
    model.validate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joint_names_follow_current_glb_nodes_and_catalogues_stay_lazy() -> Result<()> {
        let definition = serde_json::to_vec(&serde_json::json!({
             "scale": 1, "elevation": 0,
            "hidden_geometry": ["root"], "behavior": null,
            "parts": [{"animation": null, "attached_to": null, "additive": false,
                "resource": 0, "mesh": "body.glb", "textures": [], "materials": []}]
        }))?;
        let catalogue = serde_json::json!({"records": [{"preview": "model.json"}]});
        let (expanded, _) = expand_catalogue(catalogue, |path| {
            assert_eq!(path, "model.json", "catalogue must not open media");
            Ok(definition.clone().into())
        })?;
        let declaration: ModelPreview =
            serde_json::from_value(expanded["records"][0]["preview"].clone())?;
        declaration.validate_definition()?;
        assert!(declaration.parts[0].scene.bone_names.is_empty());

        // Rigid and skinned models have the same game-node interface. Node order
        // may change in an editor without changing gameplay indices.
        for skinned in [false, true] {
            let mut gltf = serde_json::json!({
                "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [1]}],
                "nodes": [
                    {"name": "hand", "extras": {"resonance_bone": 1}},
                    {"name": "root", "children": [0], "extras": {"resonance_bone": 0}}
                ]
            });
            if skinned {
                gltf["skins"] = serde_json::json!([{"joints": [1, 0]}]);
            }
            let mut model = declaration.clone();
            let bytes = resonance_asset_writer::gltf::pack_glb(&gltf, &[])?;
            prepare(&mut model, |path| {
                assert_eq!(path, "body.glb");
                Ok(bytes.clone().into())
            })?;
            assert_eq!(model.parts[0].scene.bone_names, ["root", "hand"]);
            assert!(
                serde_json::to_value(&model)?["parts"][0]
                    .get("bone_names")
                    .is_none()
            );

            gltf["nodes"][0]["name"] = "finger".into();
            let edited = resonance_asset_writer::gltf::pack_glb(&gltf, &[])?;
            prepare(&mut model, |_| Ok(edited.clone().into()))?;
            assert_eq!(model.parts[0].scene.bone_names, ["root", "finger"]);

            gltf["nodes"][0]["extras"]["resonance_bone"] = 2.into();
            let invalid = resonance_asset_writer::gltf::pack_glb(&gltf, &[])?;
            assert!(prepare(&mut model, |_| Ok(invalid.clone().into())).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires installed library; reads current model declarations and GLB metadata"]
    fn installed_model_documents_derive_joints_and_visibility_from_glb() -> Result<()> {
        use std::{collections::BTreeSet, fs, path::Path};
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let root = Path::new(&root);
        fn no_prepared_metadata(value: &Value) -> Result<()> {
            ensure!(
                value.get("bone_names").is_none() && value.get("material_nodes").is_none(),
                "duplicated GLB joint or geometry metadata"
            );
            Ok(())
        }
        fn prepare(mut part: crate::ScenePart, root: &Path) -> Result<()> {
            part.prepare_bones(&fs::read(root.join(&part.mesh))?)?;
            part.validate(|_| true).with_context(|| part.mesh)
        }
        fn visit(
            value: &Value,
            root: &Path,
            meshes: &mut BTreeSet<String>,
            count: &mut usize,
        ) -> Result<()> {
            match value {
                Value::Object(object) => {
                    no_prepared_metadata(value)?;
                    let grouped = ["parts", "layers"].into_iter().find(|key| {
                        value[*key].as_array().is_some_and(|parts| {
                            parts.iter().any(|part| part.get("mesh").is_some())
                        })
                    });
                    if let Some(key) = grouped {
                        let parts: Vec<crate::ScenePart> = if key == "parts" {
                            crate::model_parts::deserialize(value.clone())?
                        } else {
                            crate::model_parts::layers::deserialize(value.clone())?
                        };
                        for (part, authored) in
                            parts.into_iter().zip(value[key].as_array().unwrap())
                        {
                            no_prepared_metadata(authored)?;
                            meshes.insert(part.mesh.clone());
                            prepare(part, root)?;
                            *count += 1;
                        }
                    } else if let Some(mesh) = object.get("mesh").and_then(Value::as_str) {
                        if object.contains_key("draws") {
                            let mut scene: crate::physical_scene::Scene =
                                serde_json::from_value(value.clone())?;
                            scene.prepare_bones(&fs::read(root.join(mesh))?)?;
                            scene.validate().with_context(|| mesh.to_owned())?;
                        } else {
                            prepare(serde_json::from_value(value.clone())?, root)?;
                        }
                        meshes.insert(mesh.into());
                        *count += 1;
                    }
                    for (key, value) in object {
                        if grouped != Some(key.as_str()) {
                            visit(value, root, meshes, count)?;
                        }
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        visit(value, root, meshes, count)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut pending = vec![root.to_path_buf()];
        let mut meshes = BTreeSet::new();
        let mut count = 0;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    let value = serde_json::from_slice(&fs::read(&path)?)?;
                    visit(&value, root, &mut meshes, &mut count)
                        .with_context(|| path.display().to_string())?;
                }
            }
        }
        ensure!(!meshes.is_empty(), "library contains no model declarations");
        println!(
            "All {count} current model/physical-scene bindings derive joints and visibility from {} GLB files.",
            meshes.len()
        );
        Ok(())
    }
}
