use super::*;
use resonance_content::{ScenePart, model_preview::PreviewPart};

fn binding() -> ModelBehaviorBinding {
    ModelBehaviorBinding {
        module: "appearance".into(),
        function: "apply".into(),
    }
}
fn preview(elevation: f32) -> ModelPreview {
    let part = PreviewPart {
        scene: ScenePart {
            resource: 0,
            mesh: "shared/model.glb".into(),
            textures: vec![],
            materials: vec![],
            appearance: None,
            translation: [0.; 3],
            clips: vec![],
            autoplay: false,
            texture_animations: vec![],
            bone_names: vec!["root".into(), "tail".into()],
            material_nodes: vec![],
            outline_color: None,
            secondary_motion: Default::default(),
        },
        animation: None,
        attached_to: None,
        additive: false,
        uv_offsets: vec![],
    };
    let mut attachment = part.clone();
    attachment.attached_to = Some("root".into());
    ModelPreview {
        scale: 1.,
        elevation,
        parts: vec![part.clone(), part, attachment],
        hidden_geometry: vec![],
        behavior: None,
    }
}
fn sources(source: &str, model: &ModelPreview) -> BTreeMap<String, String> {
    let names = node_names(&model.parts[0].scene.bone_names);
    BTreeMap::from([
        ("appearance".into(), source.into()),
        (
            "std::nodes".into(),
            format!(
                "script library; pub const Root: string = {:?}; pub const Tail: string = {:?};",
                names[0], names[1]
            ),
        ),
    ])
}
fn prepare(source: &str, model: &ModelPreview) -> Result<PreparedBehavior> {
    PreparedBehavior::prepare(
        &mut PreparationCache::default(),
        &sources(source, model),
        &binding(),
        model,
    )
}

#[test]
fn names_resolve_body_and_outline_and_each_evaluation_starts_without_overrides() -> Result<()> {
    let source = "script model; use model; use std::nodes; use game::story;
        const unrelated: string = \"not a node\";
        pub fn apply() {
            model::set_translation(0.0, 0.0, model::elevation());
            if !story::flag(147) { model::set_scale(model::node(nodes::Tail), 0.0, 0.0, 0.0); }
        }";
    let first = prepare(source, &preview(-80.))?;
    let second = prepare(source, &preview(3.))?;
    let hidden = first.evaluate(|_| false)?;
    assert_eq!(hidden.translation, Some([0., 0., -80.]));
    assert_eq!(
        hidden.scales,
        BTreeMap::from([
            (Node { part: 0, bone: 1 }, [0.; 3]),
            (Node { part: 1, bone: 1 }, [0.; 3]),
        ])
    );
    assert!(first.evaluate(|flag| flag == 147)?.scales.is_empty());
    assert_eq!(first.evaluate(|_| false)?, hidden);
    let mut duplicate = preview(0.);
    duplicate.parts.truncate(2);
    for part in &mut duplicate.parts {
        part.scene.bone_names[0] = "tail".into();
    }
    assert_eq!(
        prepare(source, &duplicate)?.evaluate(|_| false)?.scales,
        hidden.scales
    );
    let mut extra_outline_node = preview(0.);
    extra_outline_node.parts[1].scene.bone_names[0] = "tail".into();
    assert_eq!(
        prepare(source, &extra_outline_node)?
            .evaluate(|_| false)?
            .scales,
        BTreeMap::from([
            (Node { part: 0, bone: 1 }, [0.; 3]),
            (Node { part: 1, bone: 0 }, [0.; 3]),
        ])
    );
    assert_eq!(
        second.evaluate(|_| true)?,
        PoseOverrides {
            translation: Some([0., 0., 3.]),
            scales: BTreeMap::new()
        }
    );
    let signed = prepare(
        &source.replace("0.0, 0.0, 0.0", "-1.0, -2.0, 1.0"),
        &preview(0.),
    )?;
    assert_eq!(
        signed.evaluate(|_| false)?.scales,
        hidden
            .scales
            .into_keys()
            .map(|node| (node, [-1., -2., 1.]))
            .collect()
    );
    Ok(())
}

#[test]
fn immutable_sources_reuse_programs_and_missing_names_fail_when_requested() -> Result<()> {
    let source = "script model; use model; use std::nodes; use game::story;
        pub fn apply() { if story::flag(147) { model::node(nodes::Tail); } }";
    let model = preview(0.);
    let mut sources = sources(source, &model);
    let mut cache = PreparationCache::default();
    let first = PreparedBehavior::prepare(&mut cache, &sources, &binding(), &model)?;
    let reused = PreparedBehavior::prepare(&mut cache, &sources, &binding(), &model)?;
    assert!(Arc::ptr_eq(&first.module, &reused.module));
    let mut different = model.clone();
    different.parts[0].scene.bone_names[1] = "wings".into();
    let changed = PreparedBehavior::prepare(&mut cache, &sources, &binding(), &different)?;
    assert_eq!(changed.evaluate(|_| false)?, PoseOverrides::default());
    let error = changed.evaluate(|_| true).unwrap_err().to_string();
    assert!(error.contains("unknown or ambiguous model node"), "{error}");
    assert!(
        error.contains("tail") && error.contains("appearance"),
        "{error}"
    );
    sources.remove("std::nodes");
    assert!(PreparedBehavior::prepare(&mut cache, &sources, &binding(), &model).is_err());
    assert_eq!(first.evaluate(|_| false)?, PoseOverrides::default());
    Ok(())
}

#[test]
fn invalid_entries_and_bad_operations_fail_explicitly() -> Result<()> {
    let model = preview(0.);
    for source in [
        "script field; pub fn apply() {}",
        "script library; pub fn apply() {}",
        "script model; pub fn apply(value: f32) {}",
        "script model; pub task apply() {}",
        "script model; pub fn apply() -> i32 { return 1; }",
        "script model; use missing; pub fn apply() {}",
        "script model; use model; use std::nodes; pub fn apply() { model::node(nodes::Absent); }",
    ] {
        assert!(prepare(source, &model).is_err(), "accepted {source}");
    }
    assert!(prepare("script model; pub fn apply() {}", &preview(f32::NAN)).is_err());
    for (source, diagnostic) in [
        (
            "script model; pub fn apply() { while true {} }",
            "budget exhausted",
        ),
        (
            "script model; use game::story; pub fn apply() { story::flag(65536); }",
            "invalid story flag",
        ),
    ] {
        let error = prepare(source, &model)?
            .evaluate(|_| false)
            .unwrap_err()
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
        assert!(error.contains("appearance"), "{error}");
    }
    prepare(
        "script model; use game::story; pub fn apply() { story::flag(65535); }",
        &model,
    )?
    .evaluate(|flag| {
        assert_eq!(flag, u16::MAX);
        true
    })?;
    Ok(())
}

#[test]
fn duplicate_selectors_are_unambiguous_and_escape_literal_hashes() -> Result<()> {
    let bones = ["tail", "tail", "tail#0", "root"].map(str::to_owned);
    assert_eq!(node_names(&bones), ["tail#0", "tail#1", "tail##0", "root"]);
    let mut model = preview(0.);
    model.parts.truncate(2);
    for part in &mut model.parts {
        part.scene.bone_names[0] = "tail".into();
    }
    let bare = prepare(
        "script model; use model; pub fn apply() { model::node(\"tail\"); }",
        &model,
    )?;
    assert!(
        bare.evaluate(|_| false)
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    Ok(())
}
