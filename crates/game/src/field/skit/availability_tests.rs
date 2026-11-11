use super::*;
use resonance_content::{
    library::Mappings,
    skit::{self, SkitDefinition},
};

#[test]
fn named_fields_and_saved_progress_control_actual_prompts() -> Result<()> {
    let fields: Mappings =
        serde_json::from_str(r#"{"81":"house-of-salvation","95":"palma-costa"}"#)?;
    let make = |name: &str| -> Result<SkitCatalog> {
        let mut skits: Vec<SkitDefinition> = serde_json::from_value(serde_json::json!([{
            "id":7,"title":"A conversation","trigger":"entry", "location":{"field":name},
            "condition":{"all":[
                {"range":{"value":"story_progress","min":100,"max":200}},
                {"range":{"value":"world_side","min":1,"max":1}}
            ]}
        }]))?;
        skit::fields::prepare_fields(&mut skits, &fields)?;
        Ok(SkitCatalog {
            skits,
            preview_order: vec![],
            resources: BTreeMap::new(),
            portraits: BTreeMap::new(),
            portrait_recipes: BTreeMap::new(),
            media: BTreeMap::new(),
        })
    };
    let mut events = tests::events();
    events.set_global(16, 100)?;
    events.set_global(20, 1)?;
    for (name, map, expected) in [
        ("house-of-salvation", 81, Some(7)),
        ("palma-costa", 81, None),
        ("palma-costa", 95, Some(7)),
    ] {
        let mut prompts = Skits::new(Some(Arc::new(make(name)?)));
        prompts.step(&events, map, true, None)?;
        assert_eq!(prompts.open(), expected);
    }
    for (story, side, expected) in [
        (99, 1, false),
        (100, 1, true),
        (200, 1, true),
        (201, 1, false),
        (100, 0, false),
    ] {
        events.set_global(16, story)?;
        events.set_global(20, side)?;
        let mut prompts = Skits::new(Some(Arc::new(make("house-of-salvation")?)));
        prompts.step(&events, 81, true, None)?;
        assert_eq!(prompts.open().is_some(), expected);
    }
    assert!(
        format!("{:#}", make("missing-place").unwrap_err())
            .contains("unknown skit field missing-place")
    );
    Ok(())
}
