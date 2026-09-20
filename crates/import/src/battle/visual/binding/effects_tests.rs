use super::*;
use resonance_content::battle::pose::{Bone, Transform};
use serde_json::json;

#[test]
fn controlled_outline_copies_trs_and_inverse_binds_without_changing_other_payloads() -> Result<()> {
    let primary = Skeleton {
        bones: vec![
            Bone {
                bind_channels: Default::default(),
                name: "root".into(),
                parent: None,
                bind: Transform::default(),
            },
            Bone {
                bind_channels: Default::default(),
                name: "joint".into(),
                parent: Some(0),
                bind: Transform::default(),
            },
        ],
    };
    let mut outline = primary.clone();
    outline.bones[1].name = "JOINT".into();
    outline.bones[1].bind.translation[0] = 3.;
    let scene = |rig: &Skeleton, inverse: u8| -> Result<Glb> {
        let nodes: Vec<_> = rig
            .bones
            .iter()
            .enumerate()
            .map(|(index, bone)| {
                json!({
                    "name": bone.name, "translation": bone.bind.translation,
                    "rotation": bone.bind.rotation, "scale": bone.bind.scale,
                    "children": if index == 0 { vec![1] } else { vec![] },
                })
            })
            .collect();
        let json = json!({
            "asset": {"version":"2.0"}, "nodes": nodes,
            "buffers": [{"byteLength": 160}],
            "bufferViews": [{"buffer":0,"byteOffset":16,"byteLength":128}],
            "accessors": [{"bufferView":0,"componentType":5126,"count":2,"type":"MAT4"}],
            "skins": [{"joints":[0,1],"inverseBindMatrices":0}],
            "meshes": [{"name":"preserve-outline-geometry"}],
            "animations": [{"name":"preserve-authored-clip"}],
        });
        let mut binary = vec![inverse; 160];
        binary[..16].fill(0x12);
        binary[144..].fill(0x34);
        Glb::parse(&crate::scene::pack_glb(&json, &mut binary)?)
    };
    let source = scene(&primary, 0x56)?;
    let destination = scene(&outline, 0x78)?;
    let original_json = destination.json.clone();
    let original_binary = destination.binary.clone();
    let bytes = controlled_outline(source, destination, &primary, &outline)?.unwrap();
    let mut derived = Glb::parse(&bytes)?;
    assert_eq!(&derived.binary[..16], &original_binary[..16]);
    assert_eq!(&derived.binary[144..], &original_binary[144..]);
    assert_eq!(derived.binary[16..144], [0x56; 128]);
    assert_eq!(derived.json["nodes"][1]["translation"], json!([0., 0., 0.]));
    derived.json["nodes"][1]["translation"] = original_json["nodes"][1]["translation"].clone();
    assert_eq!(derived.json, original_json);
    assert!(
        controlled_outline(
            scene(&primary, 0x56)?,
            scene(&primary, 0x56)?,
            &primary,
            &primary
        )?
        .is_none()
    );
    // Different names are harmless; different ordinal hierarchies cannot use local poses.
    outline.bones[1].parent = None;
    assert!(
        controlled_outline(
            scene(&primary, 0x56)?,
            scene(&outline, 0x78)?,
            &primary,
            &outline
        )
        .is_err()
    );
    let mut invalid = scene(&primary, 0x56)?;
    invalid.json["skins"][0]["joints"] = json!([1, 0]);
    assert!(invalid.inverse_binds(2).is_err());
    Ok(())
}

#[test]
#[ignore = "requires both original discs and recooked all-assets; reads models without conversion"]
fn original_package_model_library_retains_266_rigs_and_17_authored_outline_pairs() -> Result<()> {
    use crate::battle::effect_program::{MagicArchive, SkillArchive, magic_member};
    use crate::battle::pose;
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = project.join("local/all-assets");
    for disc in 1..=2 {
        let extracted = project.join(format!("local/extracted/disc{disc}"));
        let sources = Sources::read(&extracted)?;
        let magic = MagicArchive::read(&extracted)?;
        let skill = SkillArchive::read(&extracted)?;
        let mut models = 0;
        let mut animated = 0;
        let mut pairs = BTreeSet::new();
        let mut mismatches = Vec::new();
        for (kind, source) in [
            ("magic", sources.archive(Archive::Magic)),
            ("skill", sources.archive(Archive::Skill)),
        ] {
            let directory = Directory::open(&root, disc, source)?;
            let mut records = BTreeSet::new();
            for namespace in directory.publications() {
                let entries = match fs::read_dir(root.join(namespace).join("battle/all/visuals")) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                for entry in entries {
                    records.insert(
                        entry?
                            .path()
                            .file_stem()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_owned(),
                    );
                }
            }
            for name in records {
                if !name.starts_with(&format!("{kind}-")) || !name.contains("-model-") {
                    continue;
                }
                let (_, bytes) = directory.resolve(&format!("battle/all/visuals/{name}.json"))?;
                let cooked: super::super::Cooked = serde_json::from_slice(&bytes)?;
                let Asset::EffectModel(binding) = cooked.asset else {
                    anyhow::bail!("wrong model asset");
                };
                let Visual::PackageModel(authored) = directory.read(cooked.asset, &name)? else {
                    anyhow::bail!("missing authored rig in {name}; recook all-assets");
                };
                let (package, index) = match binding {
                    ModelRef::Magic { package, index } | ModelRef::Skill { package, index } => {
                        (package, index)
                    }
                    _ => anyhow::bail!("wrong package binding"),
                };
                let skill_bytes;
                let bytes = if kind == "magic" {
                    magic.package(package)?
                } else {
                    skill_bytes = skill.package(package)?;
                    &skill_bytes
                };
                let model = magic_member(bytes, 12 + usize::from(index) * 4)?
                    .context("missing source model")?;
                let outline = magic_member(bytes, 52 + usize::from(index) * 4)?;
                assert_eq!(
                    serde_json::to_value(&authored.rig.skeleton)?,
                    serde_json::to_value(pose::skeleton(model)?)?,
                    "{name}"
                );
                assert_eq!(
                    serde_json::to_value(&authored.outline)?,
                    serde_json::to_value(outline.map(pose::skeleton).transpose()?)?,
                    "{name}"
                );
                let mut slots = BTreeSet::new();
                for slot in 0..4 {
                    if let Some(clip) =
                        magic_member(bytes, 92 + usize::from(index) * 16 + slot * 4)?
                    {
                        slots.insert(slot as u16);
                        assert_eq!(
                            serde_json::to_value(&authored.rig.motions[&(slot as u16)])?,
                            serde_json::to_value(pose::motion(clip, model)?)?,
                            "{name} clip {slot}"
                        );
                    }
                }
                assert_eq!(
                    authored
                        .rig
                        .motions
                        .keys()
                        .copied()
                        .collect::<BTreeSet<_>>(),
                    slots
                );
                models += 1;
                animated += usize::from(!slots.is_empty());
                for (part, skeleton) in
                    authored.model.parts.iter().zip(
                        std::iter::once(&authored.rig.skeleton).chain(authored.outline.as_ref()),
                    )
                {
                    Glb::read(&root.join(&part.scene.mesh))?.validate_skeleton(skeleton)?;
                }
                if let Some(outline) = &authored.outline {
                    pairs.insert((package, index));
                    let primary = &authored.rig.skeleton;
                    let differences: Vec<_> = primary
                        .bones
                        .iter()
                        .zip(&outline.bones)
                        .enumerate()
                        .filter_map(|(index, (a, b))| (a.bind != b.bind).then_some(index))
                        .collect();
                    if !differences.is_empty() {
                        mismatches.push((package, index, differences));
                    }
                    let source = Glb::read(&root.join(&authored.model.parts[0].scene.mesh))?;
                    let destination = Glb::read(&root.join(&authored.model.parts[1].scene.mesh))?;
                    let original_json = destination.json.clone();
                    let original_binary = destination.binary.clone();
                    let range = destination.inverse_binds(outline.bones.len())?.unwrap();
                    let primary_binds =
                        source.binary[source.inverse_binds(primary.bones.len())?.unwrap()].to_vec();
                    let derived = controlled_outline(source, destination, primary, outline)?;
                    assert_eq!(derived.is_some(), (package, index) == (86, 0), "{name}");
                    if let Some(bytes) = derived {
                        let mut derived = Glb::parse(&bytes)?;
                        assert_eq!(derived.binary[range.clone()], primary_binds);
                        assert_eq!(
                            derived.binary[..range.start],
                            original_binary[..range.start]
                        );
                        assert_eq!(derived.binary[range.end..], original_binary[range.end..]);
                        for (index, bone) in primary.bones.iter().enumerate() {
                            assert_eq!(
                                serde_json::from_value::<[f32; 3]>(
                                    derived.json["nodes"][index]["translation"].clone()
                                )?,
                                bone.bind.translation
                            );
                            assert_eq!(
                                serde_json::from_value::<[f32; 4]>(
                                    derived.json["nodes"][index]["rotation"].clone()
                                )?,
                                bone.bind.rotation
                            );
                            assert_eq!(
                                serde_json::from_value::<[f32; 3]>(
                                    derived.json["nodes"][index]["scale"].clone()
                                )?,
                                bone.bind.scale
                            );
                            for field in ["translation", "rotation", "scale"] {
                                derived.json["nodes"][index][field] =
                                    original_json["nodes"][index][field].clone();
                            }
                        }
                        assert_eq!(
                            derived.json, original_json,
                            "geometry, names, hierarchy and animations stay authored"
                        );
                    }
                }
            }
        }
        assert_eq!((models, animated), (266, 18), "disc {disc}");
        assert_eq!(
            pairs,
            BTreeSet::from([
                (37, 0),
                (37, 1),
                (37, 2),
                (37, 3),
                (82, 0),
                (84, 0),
                (85, 0),
                (86, 0),
                (86, 1),
                (86, 2),
                (87, 0),
                (88, 0),
                (89, 0),
                (90, 0),
                (91, 0),
                (92, 0),
                (93, 0)
            ])
        );
        assert_eq!(mismatches, [(86, 0, vec![17, 62])]);
    }
    Ok(())
}
