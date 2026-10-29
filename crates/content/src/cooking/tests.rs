use super::*;

#[test]
fn ingredients_resolve_names_and_admit_only_whole_mastery_grades() -> Result<()> {
    let items = [("beef", 86), ("empty", 0)].into();
    let groups = [("greens", 5)].into();
    let selection: Ingredient = serde_json::from_value(
        serde_json::json!({"any":"greens","from_mastery":3,"through_mastery":5}),
    )?;
    assert_eq!(
        selection.prepare(&items, &groups)?,
        (runtime::Ingredient::Any(5), 3, 5)
    );
    assert_eq!(
        Ingredient::Item("beef".into()).prepare(&items, &groups)?,
        (runtime::Ingredient::Item(86), 0, 8)
    );
    for value in [
        serde_json::json!({"any":"greens","item":"beef"}),
        serde_json::json!({"any":"missing"}),
        serde_json::json!({"item":"empty"}),
        serde_json::json!({"any":"greens","from_mastery":1}),
        serde_json::json!({"any":"greens","through_mastery":3}),
        serde_json::json!({"any":"greens","from_mastery":6,"through_mastery":5}),
        serde_json::json!({}),
    ] {
        assert!(
            serde_json::from_value::<Ingredient>(value)?
                .prepare(&items, &groups)
                .is_err()
        );
    }
    assert!(
        serde_json::from_value::<Ingredient>(serde_json::json!({"any":"greens","from_mastry":3}))
            .is_err()
    );
    Ok(())
}
