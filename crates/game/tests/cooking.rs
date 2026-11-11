mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    cooking::Document,
    library::{Library, Mod},
    session::SessionData,
};
use resonance_events::party::Party;

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
    common::write_mod(output.path(), resonance_content::cooking::PATH, &document)?;
    common::write_mod(
        output.path(),
        resonance_content::characters::PATH,
        &characters,
    )?;
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
