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
    common::write_mod(edits.path(), resonance_content::characters::PATH, &document)?;
    common::write_mod(edits.path(), resonance_content::titles::PATH, &titles)?;
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
