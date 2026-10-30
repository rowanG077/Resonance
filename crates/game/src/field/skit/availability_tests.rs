use super::*;
use resonance_content::{
    library::Mappings,
    skit::{self, SkitCondition, SkitDefinition},
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

#[test]
#[ignore = "requires installed catalogue and the saved F04 before-state; evaluates ranges without cooking"]
fn every_authored_field_range_keeps_its_inclusive_native_boundary() -> Result<()> {
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let root = local.join("layout-migration/final-integration-20261025-supervised/library");
    let before: serde_json::Value = serde_json::from_slice(&std::fs::read(
        local.join("structure-review-20261027/skit-ledger/before/skits/catalogue.json"),
    )?)?;
    let mut current: skit::document::Definition =
        serde_json::from_slice(&std::fs::read(root.join(skit::document::PATH))?)?;
    let fields = serde_json::from_slice(&std::fs::read(
        root.join(resonance_content::library::FIELDS),
    )?)?;
    skit::fields::prepare_fields(&mut current.skits, &fields)?;
    fn original_ranges(value: &serde_json::Value, out: &mut Vec<[u16; 2]>) {
        match value {
            serde_json::Value::Object(v) => {
                if let Some(v) = v.get("maps") {
                    out.push(serde_json::from_value(v.clone()).unwrap());
                } else {
                    for v in v.values() {
                        original_ranges(v, out);
                    }
                }
            }
            serde_json::Value::Array(v) => {
                for v in v {
                    original_ranges(v, out);
                }
            }
            _ => {}
        }
    }
    fn authored_ranges<'a>(condition: &'a SkitCondition, out: &mut Vec<&'a SkitCondition>) {
        match condition {
            SkitCondition::Maps(_) => out.push(condition),
            SkitCondition::All(v) | SkitCondition::Any(v) => {
                for v in v {
                    authored_ranges(v, out);
                }
            }
            SkitCondition::Not(v) => authored_ranges(v, out),
            _ => {}
        }
    }
    let events = tests::events();
    let mut checked = 0;
    let mut single = 0;
    for (old, skit) in before["skits"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&current.skits)
    {
        assert_eq!(old["id"].as_u64(), Some(u64::from(skit.id)));
        if let Some(id) = old["location"]["map"].as_u64() {
            let SkitLocation::Map(field) = &skit.location else {
                anyhow::bail!("missing single field selector")
            };
            assert_eq!(u64::from(field.id()?), id);
            single += 1;
        }
        let mut original = vec![];
        original_ranges(&old["condition"], &mut original);
        let mut ranges = vec![];
        authored_ranges(&skit.condition, &mut ranges);
        assert_eq!(original.len(), ranges.len(), "skit {}", skit.id);
        for ([from, through], range) in original.into_iter().zip(ranges) {
            for map in [from.saturating_sub(1), from, through, through + 1, 94] {
                let context = conditions::Context {
                    events: &events,
                    party: events.world.party.as_ref().unwrap(),
                    map: u32::from(map),
                    exploration_ticks: 0,
                    overworld: None,
                };
                assert_eq!(
                    context.matches(range)?,
                    (from..=through).contains(&map),
                    "skit {}, field {map}",
                    skit.id
                );
            }
            checked += 1;
        }
    }
    assert_eq!((current.skits.len(), checked, single), (319, 92, 11));
    Ok(())
}
