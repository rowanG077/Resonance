use super::*;
use crate::battle_effect::EffectOperation;
use serde_json::json;

#[test]
fn names_survive_reordering_edits_and_unused_prototypes() -> Result<()> {
    let mut authored = json!({"templates":{
        "spark":{"kind":"unsupported","reason":"sprite prototype"},
        "unused":{"kind":"unsupported","reason":"next mod"}},
        "programs":[[{"kind":"spawn","particle":"spark"}]]});
    let first: Definition = serde_json::from_value(authored.clone())?;
    authored["templates"]["aardvark"] = json!({"kind":"unsupported","reason":"earlier key"});
    let second: Definition = serde_json::from_value(authored.clone())?;
    let EffectOperation::Spawn { particle: a, .. } = first.programs[0][0].operation else {
        panic!()
    };
    let EffectOperation::Spawn { particle: b, .. } = second.programs[0][0].operation else {
        panic!()
    };
    assert_ne!(a, b);
    assert_eq!(
        first.templates[usize::from(a)],
        second.templates[usize::from(b)]
    );
    assert_eq!(serde_json::to_value(&second)?, authored);
    let template = authored["templates"]
        .as_object_mut()
        .unwrap()
        .remove("spark")
        .unwrap();
    authored["templates"]["renamed"] = template;
    assert!(serde_json::from_value::<Definition>(authored.clone()).is_err());
    authored["programs"][0][0]["particle"] = "renamed".into();
    let renamed: Definition = serde_json::from_value(authored.clone())?;
    assert_eq!(serde_json::to_value(renamed)?, authored);
    Ok(())
}

#[test]
fn array_templates_and_numeric_references_are_rejected() {
    for value in [
        json!({"templates":[]}),
        json!({"templates":{"":{"kind":"unsupported","reason":"empty"}}}),
        json!({"templates":{"spark":{"kind":"unsupported","reason":"test"}},"programs":[[{"kind":"spawn","particle":0}]]}),
    ] {
        assert!(serde_json::from_value::<Definition>(value).is_err());
    }
}
