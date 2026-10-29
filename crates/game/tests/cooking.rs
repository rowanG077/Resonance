mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    cooking::Document,
    library::{Library, Mod},
    menu_data::CookingData,
    session::SessionData,
};
use resonance_events::party::Party;
use std::{fs, path::Path};

#[test]
#[ignore = "requires current cooking library and saved metadata; no media cooking"]
fn all_recipes_preserve_consumption_random_draws_outcomes_and_saved_state() -> Result<()> {
    let files = gameplay(&Library::open(&common::asset_root(), &[])?, true)?;
    let data = SessionData::load(&files)?;
    let menu = data.rules.as_ref().unwrap();
    let before = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/cooking-structure/before/game/menu-data.json");
    let old: serde_json::Value = serde_json::from_slice(&fs::read(before)?)?;
    let mut old_menu = (**menu).clone();
    old_menu.cooking = serde_json::from_value::<CookingData>(old["cooking"].clone())?;
    let mut total = 0;
    for recipe in 0..24 {
        for chef in 0..9 {
            for mastery in [0, 3, 6] {
                for stock in [0, 1, 10] {
                    for roll in [0u32, 99, 12345] {
                        let mut original = Party::new(&data, Default::default())?;
                        original.cooking.known = (1 << 24) - 1;
                        original.cooking.recipe = recipe as u8;
                        original.cooking.chef = chef as u8;
                        original.formation = vec![chef as u8 + 1];
                        original.field_leader = chef as u8 + 1;
                        original.members[chef].cooking[recipe] = mastery;
                        for member in &mut original.members {
                            member.hp = 1;
                            member.tp = 0;
                        }
                        if stock > 0 {
                            for (id, item) in menu.items.iter().enumerate() {
                                if (7..=12).contains(&item.category) {
                                    original.items.insert(id as u16, stock);
                                }
                            }
                        }
                        let mut actual = original.clone();
                        let (mut old_draws, mut new_draws) = (0, 0);
                        let (mut old_rng, mut new_rng) = (roll, roll);
                        let expected = original.cook(&old_menu, || {
                            old_draws += 1;
                            if roll == 12345 {
                                old_rng = old_rng.wrapping_mul(1664525u32).wrapping_add(1013904223);
                            }
                            old_rng
                        });
                        let found = actual.cook(menu, || {
                            new_draws += 1;
                            if roll == 12345 {
                                new_rng = new_rng.wrapping_mul(1664525u32).wrapping_add(1013904223);
                            }
                            new_rng
                        });
                        match (expected, found) {
                            (Ok(a), Ok(b)) => {
                                assert_eq!(serde_json::to_value(a)?, serde_json::to_value(b)?)
                            }
                            (Err(a), Err(b)) => assert_eq!(a, b),
                            other => panic!("meal admission differs: {other:?}"),
                        }
                        assert_eq!((old_draws, old_rng), (new_draws, new_rng));
                        assert_eq!(
                            serde_json::to_value(&actual)?,
                            serde_json::to_value(&original)?,
                            "recipe {recipe}, chef {chef}, mastery {mastery}, stock {stock}, roll {roll}"
                        );
                        let mut saved: Party =
                            serde_json::from_slice(&serde_json::to_vec(&actual)?)?;
                        saved.bind_rules(&data);
                        saved.validate(&data)?;
                        assert_eq!(
                            serde_json::to_value(&saved)?,
                            serde_json::to_value(&actual)?
                        );
                        total += 1;
                    }
                }
            }
        }
    }
    println!(
        "{total} meal attempts preserve admission, ingredient order, random draws, outcomes and saved party state."
    );
    Ok(())
}

#[test]
#[ignore = "requires current cooking library; metadata-only mod"]
fn named_recipe_group_and_starting_mastery_mods_are_used_without_recooking() -> Result<()> {
    use resonance_content::{
        cooking::{Ingredient, Selection},
        menu_data::MealEffect,
        overworld::PartyCharacter,
    };
    let root = common::asset_root();
    let retained = gameplay(&Library::open(&root, &[])?, true)?;
    let original = SessionData::load(&retained)?;
    let mut document: Document = retained.json(resonance_content::cooking::PATH)?;
    let mut characters: resonance_content::characters::Document =
        retained.json(resonance_content::characters::PATH)?;
    characters
        .characters
        .get_mut("lloyd")
        .unwrap()
        .initial
        .as_mut()
        .unwrap()
        .cooking
        .insert("sandwich".into(), 6);
    document.groups.get_mut("bread").unwrap().items = vec!["beef".into()];
    let recipe = document.recipes.get_mut("sandwich").unwrap();
    recipe.text["name"] = "Modded Sandwich".into();
    recipe.effects = vec![MealEffect::TpRecovery];
    let cook = recipe.cooks.get_mut(&PartyCharacter::Lloyd).unwrap();
    cook.recovery = [10, 20, 30];
    cook.extras = vec![Ingredient::Selection(Selection {
        item: Some("lemon".into()),
        any: None,
        from_mastery: 6,
        through_mastery: 8,
    })];
    let output = tempfile::tempdir()?;
    fs::create_dir(output.path().join("game"))?;
    for (path, value) in [
        (
            resonance_content::cooking::PATH,
            serde_json::to_vec(&document)?,
        ),
        (
            resonance_content::characters::PATH,
            serde_json::to_vec(&characters)?,
        ),
    ] {
        fs::write(output.path().join(path), value)?;
    }
    let files = gameplay(
        &Library::open(
            &root,
            &[Mod {
                name: "cooking edits".into(),
                root: output.path().into(),
            }],
        )?,
        true,
    )?;
    let data = SessionData::load(&files)?;
    let menu = data.rules.as_ref().unwrap();
    assert_eq!(menu.cooking_text()?.recipe(0)?.name, "Modded Sandwich");
    let mut party = Party::new(&data, Default::default())?;
    assert_eq!(party.members[0].cooking[0], 6);
    let items: resonance_content::items::Document = files.json(resonance_content::items::PATH)?;
    let id = |name| items.mappings.iter().find(|(_, n)| *n == name).unwrap().0 as u16;
    party.items.insert(id("beef"), 1);
    party.items.insert(id("lemon"), 1);
    let meal = party.cook(menu, || 0).unwrap();
    assert_eq!(meal.ingredients, [id("beef"), id("lemon")]);
    assert_eq!(meal.effects[&MealEffect::TpRecovery], 33);
    assert_eq!(meal.effects[&MealEffect::HpRecovery], 8);
    let mut saved: Party = serde_json::from_slice(&serde_json::to_vec(&Party::new(
        &original,
        Default::default(),
    )?)?)?;
    saved.bind_rules(&data);
    saved.validate(&data)?;
    assert_eq!(saved.cooking.recipe, 0);
    assert_eq!(saved.members[0].cooking[0], 0);
    assert_eq!(
        SessionData::load(&retained)?
            .rules
            .unwrap()
            .cooking_text()?
            .recipe(0)?
            .name,
        "Sandwich"
    );
    Ok(())
}
