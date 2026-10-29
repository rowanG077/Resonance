use super::*;

fn fixture() -> Result<(Document, Mappings)> {
    let mut mappings = BTreeMap::new();
    for id in 1..=9 {
        mappings.insert(
            PartyCharacter::try_from(id)?,
            Mappings::new([(1, "shared-title".into())].into())?,
        );
    }
    let document = Document {
        mappings,
        titles: [(
            "shared-title".into(),
            Entry {
                growth: [(Statistic::Hp, 2.), (Statistic::Strength, 0.2)].into(),
                text: serde_json::json!({"name":"Shared", "description":"A title"}),
                ..Default::default()
            },
        )]
        .into(),
        awards: Awards::default(),
    };
    Ok((
        document,
        Mappings::new([(0, "empty".into()), (1, "wooden-blade".into())].into())?,
    ))
}

#[test]
fn shared_titles_use_stat_points_without_changing_character_ids() -> Result<()> {
    let (document, items) = fixture()?;
    let prepared = document.prepare(&items, |_, _| anyhow::bail!("unexpected costume"))?;
    assert!(
        prepared
            .rules
            .iter()
            .all(|r| r[0].growth == [2, 0, 2, 0, 0, 0, 0])
    );
    let names = document.names(&Diagnostics::new(true))?;
    assert_eq!(names.len(), 9);
    assert_eq!(names[&2049], "Shared");
    for stat in Statistic::ALL {
        for value in 0..=255u8 {
            let mut changed = document.clone();
            changed
                .titles
                .get_mut("shared-title")
                .unwrap()
                .growth
                .insert(stat, f64::from(value) / stat.scale());
            assert_eq!(
                changed.prepare(&items, |_, _| unreachable!())?.rules[0][0].growth[stat as usize],
                value
            );
        }
    }
    Ok(())
}

#[test]
fn named_awards_preserve_order_recipients_slots_and_item_alternatives() -> Result<()> {
    let (mut document, items) = fixture()?;
    let rules: menu_data::TitleAwardRules = serde_json::from_value(serde_json::json!({
        "levels":[{"character":9,"title":1,"minimum":40},{"character":1,"title":1,"minimum":20}],
        "combos":[{"character":2,"title":1,"minimum":30}],
        "equipment":[{"character":6,"title":1,"requirements":[{"slots":[3,4],"items":[1]}]}]
    }))?;
    document.awards = Awards::from_parts(&rules, &document.mappings, &items)?;
    let prepared = document.prepare(&items, |_, _| unreachable!())?;
    assert_eq!(
        serde_json::to_value(prepared.awards)?,
        serde_json::to_value(rules)?
    );
    let encoded = serde_json::to_value(&document)?;
    assert_eq!(encoded["awards"]["levels"][0]["character"], "kratos");
    assert_eq!(
        encoded["awards"]["equipment"][0]["requirements"][0]["slots"],
        serde_json::json!(["accessory1", "accessory2"])
    );
    document.awards.equipment[0].requirements[0].items[0] = "missing".into();
    assert!(document.prepare(&items, |_, _| unreachable!()).is_err());
    Ok(())
}

#[test]
fn bad_captions_are_optional_but_invalid_rules_and_references_are_errors() -> Result<()> {
    let (mut document, items) = fixture()?;
    document.titles.get_mut("shared-title").unwrap().text["description"] = false.into();
    let prepared = document.prepare(&items, |_, _| unreachable!())?;
    let diagnostics = Diagnostics::new(false);
    assert!(
        prepared
            .captions(&diagnostics)?
            .iter()
            .all(|r| r[0].is_none())
    );
    assert!(diagnostics.has_errors());
    assert!(prepared.captions(&Diagnostics::new(true)).is_err());
    assert_eq!(document.names(&Diagnostics::new(true))?[&1], "Shared");
    for value in [-1., 0.05, 25.6, f64::NAN] {
        document
            .titles
            .get_mut("shared-title")
            .unwrap()
            .growth
            .insert(Statistic::Strength, value);
        assert!(document.prepare(&items, |_, _| unreachable!()).is_err());
    }
    document
        .titles
        .get_mut("shared-title")
        .unwrap()
        .growth
        .clear();
    document.titles.get_mut("shared-title").unwrap().costume = Some("unknown".into());
    assert!(
        document
            .prepare(&items, |_, _| anyhow::bail!("unknown costume"))
            .is_err()
    );
    document.titles.clear();
    assert!(document.prepare(&items, |_, _| unreachable!()).is_err());
    Ok(())
}
