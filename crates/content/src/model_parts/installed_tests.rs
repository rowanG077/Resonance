use super::*;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn rewrite(v: &mut Value, names: &BTreeMap<String, String>, moves: &BTreeMap<String, String>) {
    match v {
        Value::String(s) => {
            if let Some(new) = moves.get(s).or_else(|| names.get(s)) {
                *s = new.clone();
            } else if let Some((joint, step)) = s.rsplit_once(" transform ") {
                if let Some(new) = names.get(joint) {
                    *s = format!("{new} transform {step}");
                }
            }
        }
        Value::Array(v) => v.iter_mut().for_each(|v| rewrite(v, names, moves)),
        Value::Object(v) => v.values_mut().for_each(|v| rewrite(v, names, moves)),
        _ => (),
    }
}

fn parts(v: Value, layers: bool) -> Result<(Value, Vec<crate::ScenePart>)> {
    if layers {
        let model: crate::battle_model::document::Model = serde_json::from_value(v)?;
        Ok((
            serde_json::to_value(&model)?,
            model.layers.into_iter().map(|p| p.scene).collect(),
        ))
    } else if v.get("victory").is_some() {
        let mut model: crate::battle_model::document::PartyDefinition = serde_json::from_value(v)?;
        Ok((serde_json::to_value(&model)?, model.take_model_parts()?))
    } else if v.get("collision").is_some() {
        let model: crate::field::document::Model = serde_json::from_value(v)?;
        Ok((serde_json::to_value(&model)?, model.parts))
    } else {
        let model: crate::model_preview::ModelPreview = serde_json::from_value(v)?;
        Ok((
            serde_json::to_value(&model)?,
            model.parts.into_iter().map(|p| p.scene).collect(),
        ))
    }
}

// Compare expanded settings, keeping layer controls but moving media equivalence
// to the actual GLBs below. Clip request IDs, order and pose-chain choices remain.
fn settings(mut value: Value, scenes: &[crate::ScenePart]) -> Result<Value> {
    let object = value.as_object_mut().unwrap();
    let shared = object.remove("shared").unwrap_or_default();
    object.remove("animations");
    let key = if object.contains_key("layers") {
        "layers"
    } else {
        "parts"
    };
    let parts = object[key].as_array_mut().unwrap();
    ensure!(parts.len() == scenes.len(), "model part count changed");
    for (part, scene) in parts.iter_mut().zip(scenes) {
        if let Some(shared) = shared.as_object() {
            for (key, value) in shared {
                part.as_object_mut()
                    .unwrap()
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        part["mesh"] = Value::Null;
        let mut clips = scene.clips.clone();
        for clip in &mut clips {
            clip.motion.clear();
            clip.joints.clear();
        }
        part["clips"] = serde_json::to_value(clips)?;
    }
    Ok(value)
}

fn renamed_glb(bytes: &[u8], names: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let glb = gltf::binary::Glb::from_slice(bytes)?;
    let mut value: Value = serde_json::from_slice(&glb.json)?;
    rewrite(&mut value, names, &BTreeMap::new());
    resonance_asset_writer::gltf::pack_glb(&value, glb.bin.as_deref().unwrap_or_default())
}

#[test]
#[ignore = "requires installed assets and retained model/animation evidence; no media conversion"]
fn installed_model_authoring_preserves_parts_rigs_and_animation_targets() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let root = Path::new(&root);
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/model-authoring-ledger");
    let read = |p: &Path| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(p)?)?) };
    let migration = read(&evidence.join("migration.json"))?;
    let names: BTreeMap<String, String> =
        serde_json::from_value(read(&evidence.join("joint-renames.json"))?)?;
    let moves: BTreeMap<String, String> = serde_json::from_value(migration["moves"].clone())?;
    let models = read(&evidence.join("models.json"))?;
    let mut documents = BTreeMap::<String, (Value, Value)>::new();
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    // These copies precede clip sharing, path retirement and joint renaming,
    // respectively. Unmoved files remain in the library. Missing media is an
    // error, never an ignored edit.
    let history = [
        local.join("animation-sharing-20261028/before"),
        evidence.parent().unwrap().join("fixed-ledger/retired"),
        evidence.join("before"),
        root.to_path_buf(),
    ];
    let previous_media = |path: &str| -> Result<Vec<u8>> {
        let file = history
            .iter()
            .map(|base| base.join(path))
            .find(|p| p.is_file())
            .with_context(|| format!("missing retained model media {path}"))?;
        renamed_glb(&fs::read(file)?, &names)
    };
    let mut prepared = BTreeSet::new();
    let mut meshes = BTreeSet::new();
    let mut curves = BTreeSet::new();
    let mut count = 0;
    for row in models.as_array().unwrap() {
        let path = row["path"].as_str().unwrap();
        if !documents.contains_key(path) {
            let after = read(&root.join(path))?;
            let backup = evidence.join("before").join(path);
            let mut before = if backup.exists() {
                read(&backup)?
            } else {
                after.clone()
            };
            rewrite(&mut before, &names, &moves);
            documents.insert(path.into(), (before, after));
        }
        let (before, after) = &documents[path];
        let pointer = row["location"].as_str().unwrap();
        let layers = row["kind"] == "layers";
        let (expected, previous_scenes) = parts(before.pointer(pointer).unwrap().clone(), layers)
            .with_context(|| format!("previous {path}{pointer}"))?;
        let (actual, scenes) = parts(after.pointer(pointer).unwrap().clone(), layers)
            .with_context(|| format!("current {path}{pointer}"))?;
        ensure!(
            settings(expected, &previous_scenes)? == settings(actual, &scenes)?,
            "prepared settings differ at {path}{pointer}"
        );
        for (before, mut scene) in previous_scenes.into_iter().zip(scenes) {
            let bytes = fs::read(root.join(&scene.mesh))?;
            if before.mesh != scene.mesh && meshes.insert((before.mesh.clone(), scene.mesh.clone()))
            {
                ensure!(
                    previous_media(&before.mesh)? == renamed_glb(&bytes, &names)?,
                    "renamed geometry differs: {} -> {}",
                    before.mesh,
                    scene.mesh
                );
            }
            scene
                .prepare_bones(&bytes)
                .with_context(|| scene.mesh.clone())?;
            scene
                .validate(|_| true)
                .with_context(|| scene.mesh.clone())?;
            ensure!(
                before.clips.len() == scene.clips.len(),
                "clip request count changed"
            );
            for (old, current) in before.clips.iter().zip(&scene.clips) {
                if (old.motion != current.motion || old.joints != current.joints)
                    && curves.insert(serde_json::to_string(&(&scene.mesh, old, current))?)
                {
                    let mut a =
                        old.decode_motion(&previous_media(&old.motion)?, &scene.bone_names)?;
                    let mut b = current
                        .decode_motion(&fs::read(root.join(&current.motion))?, &scene.bone_names)?;
                    // A single TRS track uses the target model's rest pose; its
                    // authoring-reference bind is deliberately not sampled.
                    for track in a.tracks.iter_mut().chain(&mut b.tracks) {
                        if !track.is_affine() {
                            track.nodes[0].bind = Default::default();
                        }
                    }
                    ensure!(
                        a == b,
                        "bound curves differ at {path}{pointer}: {} -> {}",
                        old.motion,
                        current.motion
                    );
                }
            }
            if prepared.insert((
                scene.mesh.clone(),
                scene.clips.first().map(|c| c.motion.clone()),
            )) {
                if let Some(clip) = scene.clips.first() {
                    let motion = clip
                        .decode_motion(&fs::read(root.join(&clip.motion))?, &scene.bone_names)
                        .with_context(|| clip.motion.clone())?;
                    motion.validate_bones(scene.bone_names.len())?;
                }
            }
        }
        count += 1;
    }
    ensure!(count == 2365, "fixed model inventory changed");
    println!(
        "{count} typed models preserve all settings; {} distinct mesh/clip bindings prepare; {} renamed meshes and {} changed curve bindings compare exactly",
        prepared.len(),
        meshes.len(),
        curves.len()
    );
    Ok(())
}
