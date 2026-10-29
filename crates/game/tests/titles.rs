mod common;
use anyhow::Result;
use resonance_content::{library::Library, menu_data::Title, session::SessionData};
use resonance_events::party::Party;
use std::{fs, path::Path};

#[test]
#[ignore = "requires current title library and retained title metadata; no media conversion"]
fn every_title_preserves_level_growth_random_draws_and_saved_ids() -> Result<()> {
    let files = common::gameplay(&Library::open(&common::asset_root(), &[])?, true)?;
    let data = SessionData::load(&files)?;
    let menu = data.rules.as_ref().unwrap();
    let before = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/title-structure/before/game/menu-data.json");
    let old: serde_json::Value = serde_json::from_slice(&fs::read(before)?)?;
    let old: Vec<Vec<Title>> = serde_json::from_value(old["titles"].clone())?;
    let mut count = 0;
    for (character, titles) in old.iter().enumerate() {
        for (id, title) in titles.iter().enumerate() {
            let mut original = Party::new(&data, Default::default())?;
            original.members[character].title = id as u8 + 1;
            original.members[character].titles.insert(id as u8 + 1);
            original.members[character].costume = title.costume.map_or(0, |c| c as u8);
            let saved = serde_json::to_vec(&original)?;
            let mut restored: Party = serde_json::from_slice(&saved)?;
            restored.bind_rules(&data);
            restored.validate(&data)?;
            assert_eq!(
                serde_json::to_value(&restored)?,
                serde_json::to_value(&original)?
            );
            let mut old_random = 17u32;
            let mut new_random = old_random;
            original
                .raise_level(&data, character, 20, Some(title.growth), || {
                    old_random = old_random.wrapping_mul(1664525).wrapping_add(1013904223);
                    old_random
                })
                .map_err(anyhow::Error::msg)?;
            restored
                .raise_level(
                    &data,
                    character,
                    20,
                    Some(menu.titles[character][id].growth),
                    || {
                        new_random = new_random.wrapping_mul(1664525).wrapping_add(1013904223);
                        new_random
                    },
                )
                .map_err(anyhow::Error::msg)?;
            assert_eq!(old_random, new_random);
            assert_eq!(
                serde_json::to_value(restored)?,
                serde_json::to_value(original)?,
                "character {character}, title {id}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 159);
    println!(
        "All 159 titles preserve saved IDs/costumes, every party value and random draw through level 20."
    );
    Ok(())
}
