use super::*;
use resonance_content::{
    model_behavior::Node,
    model_preview::{ModelPreview, PreviewPart},
};
use resonance_model_behavior::PreparedBehavior;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
use symphonia_script_tools::PreparationCache;

fn part(names: &[String], attached: bool) -> PreviewPart {
    serde_json::from_value(json!({
        "scene":{"resource":0,"mesh":"meshes/test.glb","textures":[],"materials":[],
            "translation":[0,0,0],"clips":[],"autoplay":false,"texture_animations":[],
            "bone_names":names},
        "animation":null,"attached_to":attached.then(|| &names[0]),"additive":false
    }))
    .unwrap()
}

fn make_preview(parts: Vec<PreviewPart>) -> ModelPreview {
    ModelPreview {
        scale: 1.,
        elevation: 0.,
        parts,
        hidden_geometry: vec![],
        behavior: None,
    }
}

struct Sources;
impl symphonia_script_compiler::SourceResolver for Sources {
    fn source(&self, module: &str) -> Option<&str> {
        resonance_script_content::MODULES
            .iter()
            .find_map(|&(name, source)| (name == module).then_some(source))
    }
}

#[test]
fn named_bindings_resolve_body_and_outline_and_preserve_all_conditions() -> Result<()> {
    let mut bones: Vec<_> = (0..71).map(|i| format!("bone{i}")).collect();
    bones[..3].clone_from_slice(&["kk00".into(), "kk00_second".into(), "kk06_Hane".into()]);
    bones[70] = "Bone_sippo01".into();
    bones[32] = "Bone_hane01_L".into();
    bones[63] = "Bone_hane01_R".into();
    let preview = make_preview(vec![
        part(&bones, false),
        part(&bones, false),
        part(&bones, true),
    ]);
    let mut bindings = Bindings::new()?;
    assert!(bindings.finish(Catalogue::Monsters).is_err());
    let mut cache = PreparationCache::default();
    for (ids, bone, scale) in [
        (&[236, 237, 238][..], 0, [0.; 3]),
        (&[208, 209, 210][..], 2, [0.5; 3]),
    ] {
        for &id in ids {
            let entry = bindings.bind(Subject::Monster(id)).unwrap();
            let pose = PreparedBehavior::prepare(&mut cache, &Sources, &entry, &preview)?
                .evaluate(|_| false)?;
            assert_eq!(
                pose.scales,
                BTreeMap::from([
                    (Node { part: 0, bone }, scale),
                    (Node { part: 1, bone }, scale)
                ])
            );
        }
    }
    let sword = bindings.bind(Subject::Monster(191)).unwrap();
    let sword = PreparedBehavior::prepare(&mut cache, &Sources, &sword, &preview)?;
    for flags in 0..4 {
        let pose = sword.evaluate(|flag| match flag {
            147 => flags & 1 != 0,
            148 => flags & 2 != 0,
            _ => false,
        })?;
        let mut expected = BTreeMap::new();
        for (bone, enabled) in [
            (70, flags & 1 == 0),
            (32, flags & 2 == 0),
            (63, flags & 2 == 0),
        ] {
            if enabled {
                for part in 0..2 {
                    expected.insert(Node { part, bone }, [0.; 3]);
                }
            }
        }
        assert_eq!(pose.scales, expected);
    }
    bindings.finish(Catalogue::Monsters)?;
    assert!(bindings.bind(Subject::Monster(190)).is_none());
    assert!(
        bindings
            .bind(Subject::Figurine(Resource::DirectNpc(73)))
            .is_none()
    );
    let entry = bindings
        .bind(Subject::Figurine(Resource::TaggedNpc(73)))
        .unwrap();
    assert_eq!(
        PreparedBehavior::prepare(&mut cache, &Sources, &entry, &preview)?
            .evaluate(|_| false)?
            .translation,
        Some([0., 0., -80.])
    );
    bindings.finish(Catalogue::Figurines)?;
    let entry = bindings.bind(Subject::Monster(236)).unwrap();
    assert!(
        PreparedBehavior::prepare(
            &mut cache,
            &Sources,
            &entry,
            &make_preview(vec![part(&["kk00_suffix".into()], false)]),
        )?
        .evaluate(|_| false)
        .is_err()
    );
    let mut manifest: Value = serde_json::from_str(resonance_script_content::PREVIEW_BINDINGS)?;
    let duplicate = manifest[0].clone();
    manifest.as_array_mut().unwrap().push(duplicate);
    assert!(Bindings::parse(&manifest.to_string()).is_err());
    assert!(
        Bindings::parse(
            r#"[{"select":{"monsters":[191]},"module":"preview::sword_dancer","function":"apply"}]"#
        )
        .is_err()
    );
    assert!(Bindings::parse(r#"[{"select":{"monsters":["Unknown"]},"module":"preview::sword_dancer","function":"apply"}]"#).is_err());
    assert!(Bindings::parse(r#"[{"select":{"monsters":["SwordDancer"]},"module":"preview::sword_dancer","function":"apply","arguments":[]}]"#).is_err());
    Ok(())
}

#[test]
fn recooking_restores_only_shipped_sources_and_receipts_track_their_bytes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let custom = root.path().join("scripts/custom.sym");
    fs::create_dir_all(custom.parent().unwrap())?;
    fs::write(&custom, "user content")?;
    let paths: Vec<_> = resonance_script_content::FILES
        .iter()
        .map(|(path, _)| format!("scripts/{path}"))
        .collect();
    let paths: Vec<_> = paths.iter().map(String::as_str).collect();
    let cook = || {
        crate::all_assets::reuse::cook(root.path(), &"preview-script-defaults", &paths, || {
            publish(root.path())
        })
    };
    assert!(!cook()?);
    assert!(cook()?);
    fs::write(root.path().join(paths[0]), "edited shipped content")?;
    assert!(!cook()?);
    for ((_, source), path) in resonance_script_content::FILES.iter().zip(paths) {
        assert_eq!(fs::read_to_string(root.path().join(path))?, *source);
    }
    assert_eq!(fs::read_to_string(custom)?, "user content");
    Ok(())
}
