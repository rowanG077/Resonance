use super::*;
use serde_json::json;

fn document() -> Result<Document> {
    Ok(serde_json::from_value(json!({
        "mappings": {"0":"no-item", "1":"apple-gel", "2":"blade", "3":"key"},
        "items": {
            "no-item": {"category":"unclassified"},
            "apple-gel": {"category":"gel", "field_usable":true,
                "field_use":{"kind":"recover", "hp":30}, "transforms_to":"key",
                "text":{"name":"Apple Gel", "details":"Restores 30% HP."}},
            "blade": {"category":"dual_blades", "characters":["lloyd","kratos"],
                "stats":{"slash":12, "accuracy":-3}, "properties":{"attack_element":"fire"}},
            "key": {"category":"key_item", "battle_usable":true, "characters":"all"}
        }
    }))?)
}

#[test]
fn named_items_bind_independent_rules_and_shared_captions() -> Result<()> {
    let mut document = document()?;
    let prepared = document.prepare()?;
    assert_eq!(prepared.inventory[1].stack_limit, 20);
    assert_eq!(prepared.inventory[3].stack_limit, 1);
    assert_eq!(prepared.inventory[2].equipment_kind, Some(0));
    assert_eq!(prepared.inventory[2].allowed_characters, 257);
    assert_eq!(prepared.inventory[3].allowed_characters, 511);
    assert_eq!(prepared.menu[2].equipment_stats, [12, 0, 0, 0, -3, 0, 0]);
    assert!(prepared.menu[1].field_usable && !prepared.menu[1].battle_usable);
    assert!(prepared.menu[3].battle_usable && !prepared.menu[3].field_usable);
    assert_eq!(prepared.menu[1].transforms_to, 3);
    let strict = Diagnostics::new(true);
    assert_eq!(prepared.names(&strict)?[&1], "Apple Gel");
    assert_eq!(
        prepared.captions(&strict)?[1].as_ref().unwrap().description,
        ""
    );

    // A mod can change a slot by changing its category, preserve eligibility on
    // an unclassified entry, or override the category's usual stack limit.
    let blade = document.items.get_mut("blade").unwrap();
    blade.category = Category::try_from("unclassified".to_owned())?;
    blade.stack_limit = Some(7);
    let modified = document.prepare()?;
    assert_eq!(modified.inventory[2].equipment_kind, None);
    assert_eq!(modified.inventory[2].allowed_characters, 257);
    assert_eq!(modified.inventory[2].stack_limit, 7);
    Ok(())
}

#[test]
fn named_item_actions_use_stat_points_and_named_encounter_auras() -> Result<()> {
    for (authored, expected) in [
        (
            json!({"kind":"herb","stat":"strength","amount":1.5}),
            ItemUse::Herb {
                stat: 2,
                amount: 15,
                percent: false,
            },
        ),
        (
            json!({"kind":"herb","stat":"max_tp","amount":5,"percent":true}),
            ItemUse::Herb {
                stat: 1,
                amount: 5,
                percent: true,
            },
        ),
        (
            json!({"kind":"encounter_rate","aura":"holy"}),
            ItemUse::EncounterRate { rate: 1 },
        ),
        (
            json!({"kind":"encounter_rate","aura":"dark"}),
            ItemUse::EncounterRate { rate: 2 },
        ),
    ] {
        let action: Action = serde_json::from_value(authored)?;
        assert_eq!(
            serde_json::to_value(ItemUse::try_from(action)?)?,
            serde_json::to_value(expected)?
        );
    }
    for amount in [0.01, -1., 65536.] {
        let action = Action::Herb {
            stat: GrowthStat::Strength,
            amount,
            percent: false,
        };
        assert!(ItemUse::try_from(action).is_err());
    }
    Ok(())
}

#[test]
fn invalid_item_rules_fail_without_discarding_other_optional_captions() -> Result<()> {
    let mut document = document()?;
    document.items.get_mut("apple-gel").unwrap().text["description"] = false.into();
    let prepared = document.prepare()?;
    assert!(prepared.captions(&Diagnostics::new(true)).is_err());
    let diagnostics = Diagnostics::new(false);
    assert!(prepared.captions(&diagnostics)?[1].is_none());
    assert_eq!(prepared.names(&diagnostics)?[&1], "Apple Gel");
    assert!(prepared.menu[1].field_usable);
    document.items.get_mut("apple-gel").unwrap().transforms_to = Some("missing".into());
    assert!(document.prepare().is_err());
    document.items.get_mut("apple-gel").unwrap().transforms_to = None;
    document.items.get_mut("blade").unwrap().characters =
        Characters::Named(vec![PartyCharacter::Lloyd, PartyCharacter::Lloyd]);
    assert!(document.prepare().is_err());
    assert!(serde_json::from_value::<Entry>(json!({"category":"not-a-category"})).is_err());
    assert!(
        serde_json::from_value::<Entry>(json!({"category":"gel", "stats":{"not-a-stat":4}}))
            .is_err()
    );
    let mut document = self::document()?;
    document.mappings = Mappings::new([(0, "no-item".into()), (2, "blade".into())].into())?;
    assert!(document.prepare().is_err());
    Ok(())
}

#[test]
fn equipment_descriptions_have_named_rules_and_independent_optional_text() -> Result<()> {
    let mut document = document()?;
    document.equipment_descriptions = serde_json::from_value(json!({
        "small-recovery": "Small recovery",
        "large-recovery": {"text":"Large recovery", "suppresses":["small-recovery"]}
    }))?;
    document
        .items
        .get_mut("blade")
        .unwrap()
        .properties
        .descriptions = vec!["large-recovery".into()];
    let prepared = document.prepare()?;
    prepared.status.validate(&prepared.menu)?;
    assert_eq!(
        prepared.status.equipment_effects["large-recovery"].suppresses,
        ["small-recovery"]
    );
    assert_eq!(
        prepared.description_text(&Diagnostics::new(true))?["small-recovery"],
        "Small recovery"
    );
    document.equipment_descriptions.insert(
        "large-recovery".into(),
        serde_json::from_value(json!({
            "text":false, "suppresses":["small-recovery"]
        }))?,
    );
    let prepared = document.prepare()?;
    prepared.status.validate(&prepared.menu)?;
    let labels = prepared.description_text(&Diagnostics::new(false))?;
    assert!(labels.contains_key("small-recovery") && !labels.contains_key("large-recovery"));
    assert!(prepared.description_text(&Diagnostics::new(true)).is_err());
    document
        .items
        .get_mut("blade")
        .unwrap()
        .properties
        .descriptions
        .push("missing".into());
    let prepared = document.prepare()?;
    assert!(prepared.status.validate(&prepared.menu).is_err());
    for value in [
        json!({"text":"label","supresses":[]}),
        json!({"text":"label","suppresses":12}),
    ] {
        document
            .equipment_descriptions
            .insert("invalid".into(), serde_json::from_value(value)?);
        assert!(document.prepare().is_err());
    }
    Ok(())
}
