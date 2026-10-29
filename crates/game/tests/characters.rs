mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    characters::{Document, Gain},
    library::{Library, Mod},
    session::SessionData,
    titles::Statistic,
};
use resonance_events::party::Party;
use std::{fs, path::Path};

#[test]
#[ignore = "requires current character library and retained starting defaults; no media cooking"]
fn all_character_growth_randomness_and_saved_party_values_match_before_consolidation() -> Result<()>
{
    let files = gameplay(&Library::open(&common::asset_root(), &[])?, true)?;
    let data = SessionData::load(&files)?;
    let before = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../local/structure-review-20261027/character-structure/before/game/session-data.json",
    );
    let mut old: SessionData = serde_json::from_slice(&fs::read(before)?)?;
    old.items = data.items.clone();
    old.rules = data.rules.clone();
    for (old, new) in old.characters.iter_mut().zip(&data.characters) {
        old.allowed_techniques = new.allowed_techniques.clone();
        old.level_techniques = new.level_techniques.clone();
    }
    for active_title in [false, true] {
        let mut expected = Party::new(&old, Default::default())?;
        let mut actual = Party::new(&data, Default::default())?;
        assert_eq!(
            serde_json::to_value(&actual)?,
            serde_json::to_value(&expected)?
        );
        for character in 0..9 {
            let title =
                active_title.then(|| data.rules.as_ref().unwrap().titles[character][1].growth);
            let (mut old_random, mut new_random) = (17u32, 17u32);
            let (mut old_draws, mut new_draws) = (0, 0);
            expected
                .raise_level(&old, character, 250, title, || {
                    old_draws += 1;
                    old_random = old_random.wrapping_mul(1664525).wrapping_add(1013904223);
                    old_random
                })
                .map_err(anyhow::Error::msg)?;
            actual
                .raise_level(&data, character, 250, title, || {
                    new_draws += 1;
                    new_random = new_random.wrapping_mul(1664525).wrapping_add(1013904223);
                    new_random
                })
                .map_err(anyhow::Error::msg)?;
            assert_eq!((old_random, old_draws), (new_random, new_draws));
            assert_eq!(new_draws, 249 * 7);
            assert_eq!(
                serde_json::to_value(&actual)?,
                serde_json::to_value(&expected)?,
                "character {character}, title {active_title}"
            );
        }
        let mut restored: Party = serde_json::from_slice(&serde_json::to_vec(&expected)?)?;
        restored.bind_rules(&data);
        restored.validate(&data)?;
        assert_eq!(
            serde_json::to_value(&restored)?,
            serde_json::to_value(&expected)?
        );
    }
    println!(
        "All nine characters preserve starting/saved values and growth through level 250, both fallback and active-title bonuses, including all 31,374 random draws."
    );
    Ok(())
}

#[test]
#[ignore = "requires current character library; only metadata mods"]
fn shared_growth_mods_preserve_fixed_range_random_draws_and_current_title_fallback() -> Result<()> {
    let root = common::asset_root();
    let files = gameplay(&Library::open(&root, &[])?, true)?;
    let original = SessionData::load(&files)?;
    let mut document: Document = files.json(resonance_content::characters::PATH)?;
    document
        .growth_profiles
        .get_mut("magic-swordsman")
        .unwrap()
        .insert(Statistic::Hp, Gain::Fixed(80.));
    let mut titles: resonance_content::titles::Document =
        files.json(resonance_content::titles::PATH)?;
    let name = titles.mappings[&resonance_content::overworld::PartyCharacter::Zelos]
        .target(1)?
        .to_owned();
    titles
        .titles
        .get_mut(&name)
        .unwrap()
        .growth
        .insert(Statistic::Hp, 4.);
    let edits = tempfile::tempdir()?;
    fs::create_dir(edits.path().join("game"))?;
    fs::write(
        edits.path().join(resonance_content::characters::PATH),
        serde_json::to_vec(&document)?,
    )?;
    fs::write(
        edits.path().join(resonance_content::titles::PATH),
        serde_json::to_vec(&titles)?,
    )?;
    let library = Library::open(
        &root,
        &[Mod {
            name: "growth edits".into(),
            root: edits.path().into(),
        }],
    )?;
    let data = SessionData::load(&gameplay(&library, true)?)?;
    let mut party = Party::new(&data, Default::default())?;
    for (character, gain) in [(5, 84), (8, 80)] {
        let mut draws = 0;
        party
            .raise_level(&data, character, 2, None, || {
                draws += 1;
                9999
            })
            .map_err(anyhow::Error::msg)?;
        assert_eq!(draws, 7);
        assert_eq!(
            party.members[character].base_stats[0],
            original.characters[character].base_stats[0] + gain
        );
    }
    let mut saved: Party = serde_json::from_slice(&serde_json::to_vec(&Party::new(
        &original,
        Default::default(),
    )?)?)?;
    saved.bind_rules(&data);
    saved.validate(&data)?;
    assert_eq!(
        saved.members[5].base_stats,
        original.characters[5].base_stats
    );
    Ok(())
}
