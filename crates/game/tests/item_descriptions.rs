mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    items::{Description, Document},
    library::{Library, Mod},
    menu_data::MenuData,
    session::SessionData,
};
use resonance_events::party::Party;
use serde_json::Value;
use std::{fs, path::Path};

#[test]
#[ignore = "requires current item library and retained layout 71 metadata; no media conversion"]
fn every_item_pair_preserves_visible_status_order_suppression_and_gameplay() -> Result<()> {
    let root = common::asset_root();
    let before = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/item-descriptions/before/game");
    let old_items: Value = serde_json::from_slice(&fs::read(before.join("items.json"))?)?;
    let old_menu: Value = serde_json::from_slice(&fs::read(before.join("menu-data.json"))?)?;
    let files = gameplay(&Library::open(&root, &[])?, true)?;
    let current: Document = files.json(resonance_content::items::PATH)?;
    let menu = MenuData::load(&files)?;
    menu.validate()?;
    let session = SessionData::load(&files)?;
    let mut member = Party::new(&session, Default::default())?.members[0].clone();
    let captions: Vec<Vec<u8>> = current
        .mappings
        .iter()
        .map(|(id, name)| -> Result<_> {
            assert_eq!(old_items["mappings"][id.to_string()], name);
            let mut old = old_items["items"][name].clone();
            let mut new = serde_json::to_value(&current.items[name])?;
            let ids = old
                .get_mut("properties")
                .and_then(Value::as_object_mut)
                .and_then(|p| p.remove("caption_ids"));
            if let Some(properties) = new.get_mut("properties").and_then(Value::as_object_mut) {
                properties.remove("descriptions");
            }
            for value in [&mut old, &mut new] {
                if value["properties"]
                    .as_object()
                    .is_some_and(|p| p.is_empty())
                {
                    value.as_object_mut().unwrap().remove("properties");
                }
            }
            assert_eq!(old, new, "gameplay and item text: {name}");
            Ok(ids
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default())
        })
        .collect::<Result<_>>()?;
    let old_rules = &old_menu["status"]["equipment_effects"];
    let old_text = &old_menu["presentation"]["status"]["equipment_effects"];
    let text = &menu.status_text()?.equipment_effects;
    let expected = |ids: &[u8]| -> Vec<&str> {
        ids.iter()
            .filter(|id| {
                !ids.iter().any(|source| {
                    old_rules[source.to_string()]["suppresses"]
                        .as_array()
                        .unwrap()
                        .contains(&Value::from(**id))
                })
            })
            .map(|id| old_text[id.to_string()].as_str().unwrap())
            .filter(|text| !text.is_empty())
            .collect()
    };
    for a in 0..captions.len() {
        for b in 0..captions.len() {
            member.equipment = [a as u16, b as u16, 0, 0, 0, 0];
            let ids: Vec<_> = captions[a].iter().chain(&captions[b]).copied().collect();
            let actual: Vec<_> = member
                .equipment_captions(&menu)?
                .into_iter()
                .map(|name| text[name].as_str())
                .collect();
            assert_eq!(actual, expected(&ids), "item pair {a}/{b}");
        }
    }
    let id = |name: &str| {
        current
            .mappings
            .iter()
            .find(|(_, value)| *value == name)
            .unwrap()
            .0 as u16
    };
    member.equipment = [
        id("poison-charm"),
        id("stun-charm"),
        id("paralysis-charm"),
        id("emerald-ring"),
        id("faerie-ring"),
        id("stun-bracelet"),
    ];
    let ids: Vec<_> = [0, 1, 2, 5, 3, 4]
        .into_iter()
        .flat_map(|slot| captions[member.equipment[slot] as usize].iter().copied())
        .collect();
    assert_eq!(
        member
            .equipment_captions(&menu)?
            .into_iter()
            .map(|name| text[name].as_str())
            .collect::<Vec<_>>(),
        expected(&ids)
    );
    println!(
        "All 528 items preserve gameplay and text; all 278,784 ordered pairs and six-slot ordering preserve visible status captions and suppression."
    );
    Ok(())
}

#[test]
#[ignore = "requires current item library; metadata-only mod preparation"]
fn named_status_description_edits_share_text_without_changing_gameplay() -> Result<()> {
    let root = common::asset_root();
    let retained = gameplay(&Library::open(&root, &[])?, true)?;
    let original = MenuData::load(&retained)?;
    let mut document: Document = retained.json(resonance_content::items::PATH)?;
    let id = |name: &str| {
        document
            .mappings
            .iter()
            .find(|(_, value)| *value == name)
            .unwrap()
            .0 as u16
    };
    let (half, third) = (id("faerie-ring"), id("emerald-ring"));
    let edits = tempfile::tempdir()?;
    fs::create_dir(edits.path().join("game"))?;
    let library = Library::open(
        &root,
        &[Mod {
            name: "description edits".into(),
            root: edits.path().into(),
        }],
    )?;
    let save = |value: &Document| -> Result<()> {
        fs::write(
            edits.path().join(resonance_content::items::PATH),
            serde_json::to_vec(value)?,
        )?;
        Ok(())
    };
    document.equipment_descriptions.remove("half-tp-cost");
    document.equipment_descriptions.insert(
        "custom-discount".into(),
        Description::new("Edited discount".into(), vec![]),
    );
    document
        .items
        .get_mut("faerie-ring")
        .unwrap()
        .properties
        .descriptions = vec!["custom-discount".into()];
    save(&document)?;
    let files = gameplay(&library, true)?;
    let menus = MenuData::load(&files)?;
    let data = SessionData::load(&files)?;
    let mut member = Party::new(&data, Default::default())?.members[0].clone();
    member.equipment = [0, 0, 0, third, half, 0];
    assert_eq!(
        member.equipment_captions(&menus)?,
        ["third-tp-cost", "custom-discount"]
    );
    assert_eq!(member.tp_discount(&menus), member.tp_discount(&original));
    assert_eq!(
        menus.status_text()?.equipment_effects["custom-discount"],
        "Edited discount"
    );
    document.equipment_descriptions.insert(
        "custom-discount".into(),
        Description::new("Shared discount".into(), vec!["third-tp-cost".into()]),
    );
    document
        .items
        .get_mut("emerald-ring")
        .unwrap()
        .properties
        .descriptions
        .push("custom-discount".into());
    save(&document)?;
    let menus = MenuData::load(&gameplay(&library, true)?)?;
    assert_eq!(
        member.equipment_captions(&menus)?,
        ["custom-discount", "custom-discount"]
    );
    assert_eq!(
        menus.status_text()?.equipment_effects["custom-discount"],
        "Shared discount"
    );
    assert_eq!(
        MenuData::load(&retained)?.status_text()?.equipment_effects["half-tp-cost"],
        original.status_text()?.equipment_effects["half-tp-cost"]
    );
    document
        .equipment_descriptions
        .insert("custom-discount".into(), Description::Text(false.into()));
    save(&document)?;
    let tolerant = gameplay(&library, false)?;
    assert!(SessionData::load(&tolerant).is_ok());
    assert!(
        !MenuData::load(&tolerant)?
            .status_text()?
            .equipment_effects
            .contains_key("custom-discount")
    );
    assert!(MenuData::load(&gameplay(&library, true)?).is_err());
    Ok(())
}
