use super::*;
use crate::{diagnostics::Diagnostics, library::Library};
use serde_json::json;
use std::path::PathBuf;

fn fixture() -> Result<(PathBuf, Files)> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
    );
    let mods = std::env::var_os("RESONANCE_TEST_MOD_ASSETS")
        .map(PathBuf::from)
        .into_iter()
        .collect::<Vec<_>>();
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = Library::open_paths(&root, &mods)?.snapshot();
    crate::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    Ok((root, Files::from_snapshot(snapshot, || false, diagnostics)?))
}

#[test]
#[ignore = "requires named menu library; checks catalogue bounds without media conversion"]
fn current_menu_references_follow_supplied_catalogue_sizes() -> Result<()> {
    let (root, files) = fixture()?;
    let mut menu = MenuData::load(&files)?;
    let outside = u16::try_from(menu.techniques.len())?;
    menu.validate_gameplay()?;
    menu.techniques[1].prerequisite = outside;
    assert!(menu.validate_gameplay().is_err());
    menu.techniques[1].prerequisite = 0;
    menu.techniques[1].alternatives[0] = outside;
    assert!(menu.validate_gameplay().is_err());
    menu.techniques.push(menu.techniques[0].clone());
    menu.techniques[1].prerequisite = outside;
    menu.validate_gameplay()?;

    let item_count = menu.items.len();
    menu.items[1].transforms_to = u16::try_from(item_count - 1)?;
    menu.presentation.monsters.as_mut().unwrap().records[0].drops[0] =
        Some(u16::try_from(item_count - 1)?);
    menu.validate_gameplay()?;
    menu.monsters()?.validate(item_count)?;
    menu.items[1].transforms_to = u16::try_from(item_count)?;
    assert!(menu.validate_gameplay().is_err());
    menu.items[1].transforms_to = 0;
    menu.presentation.monsters.as_mut().unwrap().records[0].drops[0] =
        Some(u16::try_from(item_count)?);
    assert!(menu.monsters()?.validate(item_count).is_err());
    menu.items.push(menu.items[0].clone());
    menu.items[1].transforms_to = u16::try_from(item_count)?;
    menu.validate_gameplay()?;
    menu.monsters()?.validate(menu.items.len())?;

    let mut art = crate::menu::MenuArt::decode(
        &std::fs::read(root.join("ui/menu.json"))?,
        &Diagnostics::new(true),
    )?;
    art.prepare_resources(|path| Ok(std::fs::read(root.join(path))?));
    art.validate(item_count)?;
    assert!(art.validate(menu.items.len()).is_err());
    let sprites = art
        .sprites
        .rects
        .get_mut(&crate::menu::Sprite::ItemImages)
        .unwrap();
    sprites.push(sprites[0]);
    art.validate(menu.items.len())?;
    let pattern = art.windows[&0].patterns[0];
    art.windows.get_mut(&0).unwrap().patterns[0] = usize::MAX;
    assert!(art.validate(menu.items.len()).is_err());
    art.windows.get_mut(&0).unwrap().patterns[0] = pattern;
    art.textures
        .remove(&(crate::menu::MenuArt::PORTRAIT_TEXTURES.end - 1));
    assert!(art.validate(menu.items.len()).is_err());
    Ok(())
}

#[test]
#[ignore = "requires named menu library; checks authored edits and isolated captions"]
fn named_menu_edits_retain_ids_order_and_independent_presets() -> Result<()> {
    let (root, files) = fixture()?;
    let items: crate::items::Document = files.json(crate::items::PATH)?;
    let original = serde_json::to_value(Document::load(&files)?)?;
    for section in [
        "strategy",
        "cooking",
        "ex_skills",
        "names",
        "titles",
        "techniques",
        "shops",
        "world_map",
    ] {
        let mut misplaced = original.clone();
        misplaced["presentation"][section] = json!({});
        let error = serde_json::from_value::<Document>(misplaced)?
            .prepare(&files, &items.mappings)
            .unwrap_err();
        assert!(
            error.to_string().contains("captions do not belong"),
            "{error:#}"
        );
    }
    let mut edited = original.clone();
    let shop = edited["world_map"]["mappings"]["1"]
        .as_str()
        .unwrap()
        .to_owned();
    edited["world_map"]["shops"][&shop]["items"] =
        json!([items.mappings.target(3)?, items.mappings.target(1)?]);
    edited["world_map"]["shops"][&shop]["name"] = "A merchant".into();
    edited["world_map"]["names"]["sylvarant"] = "Home world".into();
    let vendor = edited["crafting"]["mappings"]["1"]
        .as_str()
        .unwrap()
        .to_owned();
    let recipe = edited["crafting"]["vendors"][&vendor]["recipes"][0]
        .as_str()
        .unwrap()
        .to_owned();
    edited["crafting"]["recipes"][&recipe]["ingredients"] = json!({(items.mappings.target(1)?):2});
    let changed: Document = serde_json::from_value(edited.clone())?;
    let changed = changed.prepare(&files, &items.mappings)?;
    assert_eq!(changed.world_map.shops[1].items, [3, 1]);
    assert_eq!(
        changed.presentation.shops.as_ref().unwrap()[1],
        "A merchant"
    );
    assert_eq!(
        changed.presentation.world_map.as_ref().unwrap().names[0],
        "Home world"
    );
    let selected = changed.crafting.vendors[1].recipes[0] as usize;
    assert_eq!(
        changed.crafting.recipes[selected].ingredients,
        [(1, 2)].into()
    );
    assert_eq!(changed.crafting.vendors[1].recipes.len(), 3);
    let retained = Document::load(&files)?.prepare(&files, &items.mappings)?;
    assert_ne!(
        retained.world_map.shops[1].items,
        changed.world_map.shops[1].items
    );
    edited["world_map"]["shops"][&shop]["name"] = 42.into();
    assert!(
        serde_json::from_value::<Document>(edited.clone())?
            .prepare(&files, &items.mappings)
            .is_err()
    );
    let diagnostics = Diagnostics::new(false);
    // Keep normal content available; only the shop caption is malformed.
    let mods = std::env::var_os("RESONANCE_TEST_MOD_ASSETS")
        .map(PathBuf::from)
        .into_iter()
        .collect::<Vec<_>>();
    let mut snapshot = Library::open_paths(&root, &mods)?.snapshot();
    crate::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let tolerant = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let data = serde_json::from_value::<Document>(edited)?.prepare(&tolerant, &items.mappings)?;
    assert!(data.presentation.shops.is_none());
    assert_eq!(data.world_map.shops[1].items, [3, 1]);
    let mut journal: Value = files.json(SYNOPSIS_PATH)?;
    let key = journal["mappings"]["0"].as_str().unwrap().to_owned();
    journal["entries"][&key]["text"] = json!(["A modded journal."]);
    let renamed = journal["entries"]
        .as_object_mut()
        .unwrap()
        .remove(&key)
        .unwrap();
    journal["entries"]["opening"] = renamed;
    for target in journal["mappings"].as_object_mut().unwrap().values_mut() {
        if *target == key {
            *target = "opening".into();
        }
    }
    let journal: SynopsisData = serde_json::from_value(journal)?;
    assert_eq!(
        journal.entries[0].lines(1)[0].0[0].text,
        "A modded journal."
    );
    let mut rename: Value = files.json(RENAME_PATH)?;
    rename["defaults"]["lloyd"] = "Reset preset".into();
    rename["commands"]["confirm"] = "Apply".into();
    rename["keyboard"].as_array_mut().unwrap().swap(0, 1);
    let renamed: RenameData = serde_json::from_value(rename)?;
    renamed.validate()?;
    assert_eq!(renamed.defaults[0], "Reset preset");
    assert_eq!(renamed.commands[0], "Apply");
    assert_eq!(&renamed.keyboard[..13], "NOPQRSTUVWXYZ");
    let mut legacy = original;
    legacy["version"] = 37.into();
    assert!(serde_json::from_value::<Document>(legacy).is_err());
    Ok(())
}
