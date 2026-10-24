use crate::{menu_data::MenuData, session::SessionData};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

/// Canonical shared gameplay definitions, independent of resource filenames,
/// presentation choices and source JSON formatting.
pub fn gameplay(session: &SessionData, mut rules: MenuData) -> Result<Vec<u8>> {
    rules.validate_gameplay()?;
    for item in &mut rules.items {
        item.properties.caption_ids.clear();
    }
    // Prices, recipes, thresholds and visit aliases govern future state changes.
    // Display labels and source provenance do not govern those changes.
    Ok(serde_json::to_vec(&(
        &session.items,
        &session.characters,
        &session.experience,
        &rules.items,
        &rules.titles,
        &rules.techniques,
        &rules.strategy,
        &rules.cooking,
        world_map(&rules.world_map),
        &rules.ex_skills,
        (
            (
                &rules.title_awards.levels,
                &rules.title_awards.combos,
                &rules.title_awards.equipment,
            ),
            &rules.crafting.recipes,
            rules
                .crafting
                .vendors
                .iter()
                .map(|vendor| &vendor.recipes)
                .collect::<Vec<_>>(),
            rules
                .grade_shop
                .options
                .iter()
                .map(|option| (option.benefit, option.price, &option.excludes))
                .collect::<Vec<_>>(),
        ),
    ))?)
}

#[derive(serde::Serialize)]
struct WorldMap<'a> {
    locations: &'a BTreeMap<u16, crate::menu_data::MapLocation>,
    field_locations: &'a BTreeMap<u32, u16>,
    shops: &'a [crate::menu_data::Shop],
    shop_visit_groups: &'a [Vec<u8>],
}

fn world_map(data: &crate::menu_data::WorldMapData) -> WorldMap<'_> {
    WorldMap {
        locations: &data.locations,
        field_locations: &data.field_locations,
        shops: &data.shops,
        shop_visit_groups: &data.shop_visit_groups,
    }
}

#[test]
fn shop_visit_groups_and_stock_both_change_saved_gameplay() -> Result<()> {
    use crate::menu_data::{Shop, WorldMapData};
    let mut data = WorldMapData {
        locations: BTreeMap::new(),
        field_locations: BTreeMap::new(),
        shops: vec![Shop { items: vec![1] }; 2],
        shop_visit_groups: vec![],
    };
    let before = serde_json::to_vec(&world_map(&data))?;
    assert_eq!(
        before,
        br#"{"locations":{},"field_locations":{},"shops":[{"items":[1]},{"items":[1]}],"shop_visit_groups":[]}"#
    );
    data.shop_visit_groups = vec![vec![0, 1]];
    data.validate(2)?;
    let grouped = serde_json::to_vec(&world_map(&data))?;
    assert_ne!(grouped, before);
    data.shops[1].items = vec![2];
    assert_ne!(serde_json::to_vec(&world_map(&data))?, grouped);
    Ok(())
}

/// The current menu document also owns enemy statistics and rewards. Those
/// remain gameplay even though the catalogue sits beside Monster List artwork.
pub(super) fn monsters(value: &serde_json::Value) -> Result<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct Book {
        records: Vec<Monster>,
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Monster {
        version: u32,
        id: u8,
        unseen_count_group: u8,
        statistics: Vec<crate::monster::MonsterStats>,
        drops: [Option<u16>; 2],
        drop_chances: [u8; 2],
        grade: i16,
        steal: Option<u16>,
        steal_chance: u8,
        attack_element: Option<crate::menu_data::Element>,
        affinities: [i8; 9],
    }
    let book: Book = serde_json::from_value(value.clone())?;
    ensure!(
        book.records.len() == crate::monster::MONSTER_COUNT
            && book
                .records
                .iter()
                .enumerate()
                .all(|(id, monster)| usize::from(monster.id) == id
                    && monster.version == crate::monster::MONSTER_VERSION),
        "invalid gameplay monster catalogue"
    );
    Ok(serde_json::to_vec(&book.records)?)
}
