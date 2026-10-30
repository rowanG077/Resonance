use super::*;
use crate::animation::{Skeleton, transform_point};
use serde_json::json;
use std::sync::Arc;

#[test]
fn costume_animation_groups_share_edits_and_keep_independent_parts_explicit() -> Result<()> {
    let mut definition: PartyDefinition = serde_json::from_value(json!({
        "parts": [
            {"resource":0,"mesh":"body.glb","textures":[],"materials":[]},
            {"resource":1,"mesh":"outline.glb","textures":[],"materials":[]},
            {"resource":2,"mesh":"accessory.glb","textures":[],"materials":[]}
        ],
        "animations":[
            {"parts":[0,1],"clips":[{"slots":[0],"motion":"idle.glb","pose_chains":["cape-left","cape-right"]}]},
            {"parts":[2],"clips":[{"slots":[0],"motion":"accessory.glb"}]}
        ],
        "victory":{"file":"victory.json","costume":"formal"}
    }))?;
    let before = definition.clone().take_model_parts()?;
    definition.animations[0].clips[0].motion = "edited.glb".into();
    definition.animations[0].clips[0].pose_chains = vec!["hair".into()];
    let after = definition.clone().take_model_parts()?;
    for index in [0, 1] {
        assert_eq!(before[index].clips[0].motion, "idle.glb");
        assert_eq!(after[index].clips[0].motion, "edited.glb");
        assert_eq!(after[index].clips[0].pose_chains, ["hair"]);
    }
    assert_eq!(after[2].clips, before[2].clips);
    let mut republished = after;
    let groups = take_part_animations(&mut republished);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].parts, [0, 1]);
    assert!(republished.iter().all(|part| part.clips.is_empty()));
    definition.animations[1].parts.push(0);
    assert!(
        definition
            .clone()
            .take_model_parts()
            .unwrap_err()
            .to_string()
            .contains("multiple animation groups")
    );
    definition.animations[1].parts = vec![3];
    assert!(
        definition
            .clone()
            .take_model_parts()
            .unwrap_err()
            .to_string()
            .contains("missing party animation part")
    );
    definition.animations[1].parts.clear();
    assert!(definition.clone().take_model_parts().is_err());
    definition.animations[1].parts = vec![2];
    definition.parts[0].clips = before[0].clips.clone();
    assert!(
        definition
            .clone()
            .take_model_parts()
            .unwrap_err()
            .to_string()
            .contains("animations section")
    );
    Ok(())
}

#[test]
fn trail_edits_follow_named_joints_and_shared_or_inline_materials() -> Result<()> {
    let geometry = |names: [&str; 2]| -> Result<Arc<[u8]>> {
        Ok(resonance_asset_writer::gltf::pack_glb(
            &json!({
                "asset":{"version":"2.0"},
                "nodes":[
                    {"name":names[0], "extras":{"resonance_bone":0}},
                    {"name":names[1], "extras":{"resonance_bone":1}}
                ]
            }),
            &[],
        )?
        .into())
    };
    let mut definition = json!({
        "parts":{"0":{"layers":[{
            "resource":0,"mesh":"blade.glb","textures":[],"materials":[],
            "animation":null,"attached_to":null,"additive":false
        }]}},
        "trails":{"0":{"joints":["blade-base","blade-tip"],"material":"blue"}}
    });
    let mut files = BTreeMap::from([
        ("blade.glb", geometry(["blade-tip", "blade-base"])?),
        (
            trails::MATERIALS,
            Arc::<[u8]>::from(serde_json::to_vec(&json!({
                "blue":{"color":[0.25,0.25,0.75]}
            }))?),
        ),
    ]);
    let load = |definition: &serde_json::Value,
                files: &BTreeMap<&str, Arc<[u8]>>,
                diagnostics: &Diagnostics| {
        read_weapon(
            serde_json::from_value(definition.clone())?,
            diagnostics,
            &mut |path| {
                files
                    .get(path)
                    .cloned()
                    .with_context(|| format!("missing file {path}"))
            },
        )
    };
    let strict = Diagnostics::new(true);
    let before = load(&definition, &files, &strict)?;
    assert_eq!(before.trails[&0].bones, [1, 0]);
    assert_eq!(before.trails[&0].material.color, [0.25, 0.25, 0.75]);
    files.insert("blade.glb", geometry(["blade-base", "blade-tip"])?);
    files.insert(
        trails::MATERIALS,
        serde_json::to_vec(&json!({"blue":{"color":[1,0,0]}}))?.into(),
    );
    let edited = load(&definition, &files, &strict)?;
    assert_eq!(edited.trails[&0].bones, [0, 1]);
    assert_eq!(edited.trails[&0].material.color, [1., 0., 0.]);

    // An inline custom material needs no palette file. Ordered repeated points
    // remain useful for a tapered ribbon and must not be sorted or deduplicated.
    files.remove(trails::MATERIALS);
    definition["trails"]["0"]["material"] = json!({"color":[0,1,0],"additive":false});
    definition["trails"]["0"]["joints"] = json!(["blade-tip", "blade-base", "blade-tip"]);
    let custom = load(&definition, &files, &strict)?;
    assert_eq!(custom.trails[&0].bones, [1, 0, 1]);
    assert!(!custom.trails[&0].material.additive);
    definition["trails"]["0"]["joints"][0] = "missing".into();
    let tolerant = Diagnostics::default();
    let broken = load(&definition, &files, &tolerant)?;
    assert!(broken.trails.is_empty() && !broken.parts.is_empty());
    assert!(tolerant.has_errors());
    assert!(
        format!("{:#}", load(&definition, &files, &strict).unwrap_err())
            .contains("absent joint missing")
    );
    definition["trails"]["0"]["joints"] = json!(["blade-tip", "blade-tip"]);
    files.insert("blade.glb", geometry(["blade-tip", "blade-tip"])?);
    assert!(
        format!("{:#}", load(&definition, &files, &strict).unwrap_err())
            .contains("ambiguous joint blade-tip")
    );
    files.insert("blade.glb", geometry(["blade-base", "blade-tip"])?);
    definition["trails"]["0"]["material"] = "unknown".into();
    files.insert(trails::MATERIALS, Arc::from(&b"{}"[..]));
    assert!(
        format!("{:#}", load(&definition, &files, &strict).unwrap_err())
            .contains("unknown trail material unknown")
    );
    assert!(
        load(&definition, &files, &Diagnostics::default())?
            .trails
            .is_empty()
    );
    Ok(())
}

#[test]
fn model_edits_supply_combat_rigs_and_named_attachments_without_sidecars() -> Result<()> {
    let mut geometry = json!({
        "asset":{"version":"2.0"}, "scenes":[{"nodes":[1]}], "scene":0,
        "nodes":[
            {"name":"blade-tip", "extras":{"resonance_bone":1},
                "matrix":[1,0,0,0, 0,1,0,0, 0,0,1,0, 2,0,0,1]},
            {"name":"hilt", "extras":{"resonance_bone":0}, "children":[0], "translation":[5,0,0]}
        ]
    });
    let glow = resonance_asset_writer::gltf::pack_glb(
        &json!({
            "asset":{"version":"2.0"}, "nodes":[{"name":"glow", "extras":{"resonance_bone":0}}]
        }),
        &[],
    )?;
    let mut definition = json!({
        "attachment_nodes":{"0":"blade-tip"}, "independent_layers":[3],
        "layers":[
            {"resource":2,"mesh":"blade.glb","textures":[],"materials":[],
             "animation":null,"attached_to":null,"additive":false},
            {"resource":3,"mesh":"glow.glb","textures":[],"materials":[],
             "animation":null,"attached_to":null,"additive":true}
        ]
    });
    let load =
        |definition: &serde_json::Value, geometry: &serde_json::Value| -> Result<ModelPart> {
            let files: BTreeMap<&str, Arc<[u8]>> = [
                ("model.json", serde_json::to_vec(definition)?.into()),
                (
                    "blade.glb",
                    resonance_asset_writer::gltf::pack_glb(geometry, &[])?.into(),
                ),
                ("glow.glb", glow.clone().into()),
            ]
            .into();
            serde_json::from_value::<Model>(definition.clone())?.prepare(&mut |path| {
                files
                    .get(path)
                    .cloned()
                    .with_context(|| format!("unexpected file {path}"))
            })
        };
    let original = load(&definition, &geometry)?;
    assert_eq!(original.rig.attachments[&0], 1);
    assert_eq!(
        original.layer_skeletons.keys().copied().collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(original.layer_skeletons[&3].bones[0].name, "glow");
    let position = |model: &ModelPart| -> Result<[f32; 3]> {
        Ok(transform_point(
            model.rig.skeleton.bind_pose()?.global[usize::from(model.rig.attachments[&0])],
            [0.; 3],
        ))
    };
    assert_eq!(position(&original)?, [7., 0., 0.]);
    geometry["nodes"][1]["translation"] = json!([25, 0, 0]);
    let edited = load(&definition, &geometry)?;
    assert_eq!(position(&edited)?, [27., 0., 0.]);
    let published = serde_json::to_value(Model::try_from(edited)?)?;
    assert!(published.get("model").is_none() && published.get("rig").is_none());
    assert_eq!(
        published["attachment_nodes"],
        definition["attachment_nodes"]
    );
    assert_eq!(published["independent_layers"], json!([3]));
    assert!(!serde_json::to_string(&published)?.contains("bones"));

    // Ordinary editor group nodes stay in the hierarchy even though gameplay
    // indexes only the actual joints. Their transforms also survive animation.
    geometry["nodes"].as_array_mut().unwrap().extend([
        json!({"name":"Armature", "translation":[100,0,0], "children":[1]}),
        json!({"name":"Grip adjustment", "translation":[10,0,0], "children":[0]}),
    ]);
    geometry["nodes"][1]["children"] = json!([3]);
    geometry["scenes"][0]["nodes"] = json!([2]);
    let grouped = load(&definition, &geometry)?;
    assert_eq!(grouped.rig.skeleton.bones[1].parent, Some(0));
    assert_eq!(position(&grouped)?, [137., 0., 0.]);
    use crate::animation::{Channel, Interpolation, Motion, NodeTrack, Track, Transform};
    let motion = Motion {
        duration_frames: 30.,
        tracks: vec![Track {
            bone: 0,
            nodes: vec![NodeTrack {
                translation: Some(Channel {
                    interpolation: Interpolation::Linear,
                    times: vec![0., 1.],
                    values: vec![[25., 0., 0.], [35., 0., 0.]],
                }),
                ..NodeTrack::new(Transform::default())
            }],
        }],
    };
    assert_eq!(
        grouped
            .rig
            .skeleton
            .sample_point(&motion, 30., 1, [0.; 3])?,
        [147., 0., 0.]
    );
    assert_eq!(
        grouped
            .rig
            .skeleton
            .sample(&motion, 30.)?
            .point(1, [0.; 3])?,
        [147., 0., 0.]
    );

    let mut cyclic = geometry.clone();
    cyclic["nodes"][2]["children"] = json!([1, 2]);
    assert!(load(&definition, &cyclic).is_err());
    let mut multiple_parents = geometry.clone();
    multiple_parents["nodes"][2]["children"] = json!([1, 0]);
    assert!(load(&definition, &multiple_parents).is_err());

    definition["attachment_nodes"]["0"] = "missing".into();
    assert!(
        format!("{:#}", load(&definition, &geometry).unwrap_err()).contains("absent joint missing")
    );
    definition["attachment_nodes"]["0"] = "blade-tip".into();
    definition["independent_layers"] = json!([99]);
    assert!(
        format!("{:#}", load(&definition, &geometry).unwrap_err())
            .contains("independent layer is absent")
    );
    definition["independent_layers"] = json!([3]);
    geometry["nodes"][0]["matrix"][4] = 0.5.into();
    assert!(
        load(&definition, &geometry).is_err(),
        "unsupported shear must not silently change combat geometry"
    );
    Ok(())
}

#[test]
#[ignore = "requires installed Lloyd and saved Blender model exports; no conversion"]
fn blender_model_groups_preserve_rest_and_animated_attachments() -> Result<()> {
    use crate::animation::Motion;
    use std::{fs, path::Path};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let root = Path::new(&root);
    let work = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261026/model-rigs");
    let labels: BTreeMap<String, String> = serde_json::from_slice(&fs::read(
        root.join(crate::field::document::JOINT_MAPPINGS),
    )?)?;
    let definition: PartyDefinition = serde_json::from_slice(&fs::read(
        root.join("battle/party/lloyd/default/party.json"),
    )?)?;
    let bytes = fs::read(root.join(&definition.parts[0].mesh))?;
    let original = Rig::from_glb(&bytes, &definition.attachment_nodes)?;
    let rest = original.skeleton.bind_pose()?;
    let names = original
        .skeleton
        .bones
        .iter()
        .map(|b| b.name.clone())
        .collect::<Vec<_>>();
    let motion = Motion::decode_for_bones(
        &fs::read(root.join(&definition.animations[0].clips[0].motion))?,
        &names,
    )?;

    fn visit(node: gltf::Node<'_>, parent: glam::Mat4, globals: &mut [glam::Mat4]) {
        let matrix = parent * glam::Mat4::from_cols_array_2d(&node.transform().matrix());
        globals[node.index()] = matrix;
        for child in node.children() {
            visit(child, matrix, globals);
        }
    }
    let mut worst_rest = 0f32;
    let mut worst_motion = 0f32;
    for (file, offset) in [("lloyd-roundtrip.glb", 0.), ("lloyd-edited.glb", 5.)] {
        let bytes = fs::read(work.join(file))?;
        let rig = Rig::from_glb(&bytes, &definition.attachment_nodes)?;
        assert_eq!(rig.attachments, original.attachments);
        assert_eq!(rig.skeleton.bones.len(), original.skeleton.bones.len());
        let pose = rig.skeleton.bind_pose()?;
        let gltf = gltf::Gltf::from_slice(&bytes)?;
        let mut globals = vec![glam::Mat4::IDENTITY; gltf.nodes().len()];
        for node in gltf.default_scene().context("missing scene")?.nodes() {
            visit(node, glam::Mat4::IDENTITY, &mut globals);
        }
        let length = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
        let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length])?;
        for (node, value) in json["nodes"].as_array().unwrap().iter().enumerate() {
            let Some(bone) = value["extras"]["resonance_bone"].as_u64() else {
                continue;
            };
            let bone = bone as usize;
            let saved_name = &rig.skeleton.bones[bone].name;
            assert_eq!(
                labels.get(saved_name).unwrap_or(saved_name),
                &original.skeleton.bones[bone].name
            );
            assert_eq!(
                rig.skeleton.bones[bone].parent,
                original.skeleton.bones[bone].parent
            );
            for point in [[0.; 3], [1., 2., 3.]] {
                let actual = transform_point(pose.global[bone], point);
                let independent = globals[node].transform_point3(point.into()).to_array();
                for axis in 0..3 {
                    ensure!(
                        (actual[axis] - independent[axis]).abs() < 0.0001,
                        "gameplay and ordinary glTF rest pose differ: {file}, joint {bone}"
                    );
                }
                let mut expected = transform_point(rest.global[bone], point);
                expected[0] += offset;
                for axis in 0..3 {
                    worst_rest = worst_rest.max((actual[axis] - expected[axis]).abs());
                }
                for frame in [0., motion.duration_frames / 2., motion.duration_frames] {
                    let mut expected =
                        original
                            .skeleton
                            .sample_point(&motion, frame, bone as u16, point)?;
                    expected[0] += offset;
                    let actual = rig
                        .skeleton
                        .sample_point(&motion, frame, bone as u16, point)?;
                    for axis in 0..3 {
                        worst_motion = worst_motion.max((actual[axis] - expected[axis]).abs());
                    }
                }
            }
        }
    }
    eprintln!(
        "Blender model exports: max rest error {worst_rest}, animated error {worst_motion}; all 83 joints, native clip, +5 X group edit and named attachments checked."
    );
    ensure!(
        worst_rest < 0.001 && worst_motion < 0.001,
        "Blender roundtrip changed the model pose"
    );
    Ok(())
}

#[test]
#[ignore = "requires installed library and model-rig migration evidence; no media conversion"]
fn installed_rigs_match_every_original_rest_pose_and_attachment() -> Result<()> {
    use std::{fs, path::Path};
    #[derive(Deserialize)]
    struct Expected {
        document: String,
        kind: String,
        original: String,
        pointer: String,
        attachments: BTreeMap<u8, u16>,
    }
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let root = Path::new(&root);
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261026/model-rigs");
    let labels: BTreeMap<String, String> = serde_json::from_slice(&fs::read(
        root.join(crate::field::document::JOINT_MAPPINGS),
    )?)?;
    let entries: Vec<Expected> =
        serde_json::from_slice(&fs::read(evidence.join("expected.json"))?)?;
    let inlined: BTreeMap<String, serde_json::Value> =
        serde_json::from_slice(&fs::read(evidence.join("../equipment/inlined.json"))?)?;
    let enemy_components: BTreeMap<String, [String; 2]> = serde_json::from_slice(&fs::read(
        evidence.join("../enemy-components/components.json"),
    )?)?;
    let relocated: BTreeMap<String, String> = serde_json::from_slice(&fs::read(
        evidence.join("../enemy-components/document-paths.json"),
    )?)?;
    let enemy_owners: BTreeMap<String, [String; 2]> =
        serde_json::from_slice(&fs::read(evidence.join("../enemy-owners/owners.json"))?)?;
    let costume_migration: serde_json::Value = serde_json::from_slice(&fs::read(
        evidence.join("../party-bindings/migration.json"),
    )?)?;
    let costume_paths: BTreeMap<String, String> =
        serde_json::from_value(costume_migration["paths"].clone())?;
    let effects_evidence = evidence.join("../visual-effects");
    let effect_banks: Vec<String> =
        serde_json::from_slice(&fs::read(effects_evidence.join("banks.json"))?)?;
    let mut effect_models = BTreeMap::new();
    for bank in effect_banks {
        let previous = effects_evidence.join("previous");
        let definition: serde_json::Value =
            serde_json::from_slice(&fs::read(previous.join(&bank))?)?;
        if let Some(art) = definition["art"].as_str() {
            let art: serde_json::Value = serde_json::from_slice(&fs::read(previous.join(art))?)?;
            for (slot, model) in art["models"].as_object().context("missing old models")? {
                effect_models.insert(
                    model.as_str().context("missing old model path")?.to_owned(),
                    (bank.clone(), format!("/artwork/models/{slot}")),
                );
            }
        }
    }
    let current = |path: &str| -> Result<serde_json::Value> {
        // Historical rest-pose evidence keeps its original filenames. Resolve
        // current outfit names and direct aliases before reading today's data.
        let path = costume_paths.get(path).map(String::as_str).unwrap_or(path);
        if let Some((owner, pointer)) = effect_models.get(path) {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(root.join(owner))?)?;
            Ok(value
                .pointer(pointer)
                .context("missing inline effect model")?
                .clone())
        } else if let Some([owner, key]) = enemy_owners.get(path) {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(root.join(owner))?)?;
            Ok(value[key].clone())
        } else {
            Ok(serde_json::from_slice(
                &fs::read(root.join(path)).with_context(|| format!("current rig owner {path}"))?,
            )?)
        }
    };
    let mut count = 0;
    for row in entries {
        let before: serde_json::Value = serde_json::from_slice(&fs::read(
            evidence.join("previous-json").join(&row.original),
        )?)?;
        let mut expected: Skeleton = serde_json::from_value(
            before
                .pointer(&row.pointer)
                .context("missing original skeleton")?
                .clone(),
        )?;
        // The saved poses predate readable joint labels. The same mapping
        // resolves script names; numeric joints, hierarchy and transforms stay exact.
        for bone in &mut expected.bones {
            if let Some(label) = labels.get(&bone.name) {
                bone.name.clone_from(label);
            }
        }
        let bytes = if let Some([path, pointer]) = enemy_components.get(&row.document) {
            let document = current(path)?;
            serde_json::to_vec(
                document
                    .pointer(pointer)
                    .context("missing inline enemy component")?,
            )?
        } else if let Some(inline) = inlined.get(&row.document) {
            let owner = inline["owner"].as_str().context("missing inline owner")?;
            let slot = inline["slot"].as_str().context("missing inline slot")?;
            let (path, pointer) = if owner.starts_with("meshes/equipment/") {
                (format!("{owner}/equipment.json"), format!("/parts/{slot}"))
            } else {
                (
                    format!("{}/scene.json", owner.strip_suffix("/weapon").unwrap()),
                    format!("/weapon/parts/{slot}"),
                )
            };
            let definition: serde_json::Value = serde_json::from_slice(&fs::read(
                root.join(relocated.get(&path).unwrap_or(&path)),
            )?)?;
            serde_json::to_vec(
                definition
                    .pointer(&pointer)
                    .context("missing inline model")?,
            )?
        } else {
            serde_json::to_vec(&current(&row.document)?)?
        };
        let (skeleton, attachments) = match row.kind.as_str() {
            "component" => {
                let definition: Model = serde_json::from_slice(&bytes)?;
                let mut model =
                    definition.prepare(&mut |path| Ok(fs::read(root.join(path))?.into()))?;
                if let Some(slot) = row.pointer.strip_prefix("/model/layer_skeletons/") {
                    (
                        model
                            .layer_skeletons
                            .remove(&slot.parse()?)
                            .context("missing independent rig")?,
                        BTreeMap::new(),
                    )
                } else {
                    (model.rig.skeleton, model.rig.attachments)
                }
            }
            "party" | "enemy" => {
                let (mesh, targets) = if row.kind == "party" {
                    let definition: PartyDefinition = serde_json::from_slice(&bytes)?;
                    (
                        definition.parts[0].mesh.clone(),
                        definition.attachment_nodes,
                    )
                } else {
                    let definition: enemy::Definition = serde_json::from_slice(&bytes)?;
                    let model = crate::model_preview::document::read(&definition.model, |path| {
                        Ok(fs::read(root.join(path))?.into())
                    })?;
                    (
                        model.parts[0].scene.mesh.clone(),
                        definition.attachment_nodes,
                    )
                };
                let rig = Rig::from_glb(&fs::read(root.join(mesh))?, &targets)?;
                (rig.skeleton, rig.attachments)
            }
            "event" => {
                let actor: serde_json::Value = serde_json::from_slice(&bytes)?;
                let mesh = actor["parts"][0]["mesh"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("reviewed event rig has no body mesh"))?;
                (
                    Skeleton::from_glb(&fs::read(root.join(mesh))?)?,
                    BTreeMap::new(),
                )
            }
            _ => anyhow::bail!("unknown rig kind"),
        };
        assert_eq!(
            skeleton.bones.len(),
            expected.bones.len(),
            "{}{}",
            row.original,
            row.pointer
        );
        for (joint, (actual, expected)) in skeleton.bones.iter().zip(&expected.bones).enumerate() {
            // Shadow inclusion is a separately published drawing property;
            // these saved poses predate that property and cannot specify it.
            assert_eq!(
                (
                    &actual.name,
                    actual.parent,
                    actual.parent_transform,
                    serde_json::to_value(actual.bind)?
                ),
                (
                    &expected.name,
                    expected.parent,
                    expected.parent_transform,
                    serde_json::to_value(expected.bind)?
                ),
                "rest rig changed: {}{} joint {joint}",
                row.original,
                row.pointer
            );
        }
        ensure!(
            attachments == row.attachments,
            "attachment bindings changed: {}",
            row.document
        );
        count += 1;
    }
    ensure!(count == 1259, "incomplete rig coverage: {count}");
    println!(
        "All {count} current GLB-derived rigs preserve every original joint, parent, rest transform and attachment slot."
    );
    Ok(())
}
