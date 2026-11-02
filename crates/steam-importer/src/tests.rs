use super::*;
use resonance_content::{arte, battle_rules, items, library::Mappings};
use std::{collections::BTreeMap, fs, path::PathBuf};
use symphonia_constants::{CharacterId, KnownId};

fn json<T: serde::de::DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(root.join(path))?)?)
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM pointing to an installed Steam copy"]
fn installed_categories_load_in_current_content_format() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    cook_all(&installation, output.path(), Language::English)?;
    let enemies: Mappings = json(
        output.path(),
        resonance_content::battle_enemy::document::MAPPINGS,
    )?;
    let formations: battle_rules::Formations = json(output.path(), "battle/formations.json")?;
    let formations = formations.prepare(&enemies)?;
    assert_eq!(formations.records.len(), 1000);
    assert_eq!(formations.pools.len(), 736);
    for (id, expected) in [(801, [251, 252, 253]), (802, [254, 255, 256])] {
        assert_eq!(
            formations.records[id]
                .resources
                .iter()
                .map(|r| r.enemy)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let items: items::Document = json(output.path(), items::PATH)?;
    assert_eq!(items.prepare()?.inventory.len(), 531);
    for id in 528..531 {
        let name = items.mappings.target(id)?;
        let expected = ["Free Pass", "Theater Ticket", "Forget-Me-Not"][(id - 528) as usize];
        assert_eq!(items.items[name].text["name"], expected);
    }
    let techniques: arte::document::Document = json(output.path(), arte::PATH)?;
    let prepared = techniques.prepare()?;
    assert_eq!(techniques.mappings.target(253)?, "lightning-thrust");
    assert_eq!(techniques.mappings.target(260)?, "infernal-ruin");
    assert_eq!(techniques.mappings.target(261)?, "crimson-devastation");
    assert_eq!(prepared.catalogue.definitions.len(), 262);
    let sword_rain = techniques.mappings.id("sword-rain-alpha")? as usize;
    assert_eq!(
        prepared.menu[sword_rain]
            .alternatives
            .iter()
            .filter(|&&id| id != 0)
            .count(),
        5
    );
    let fields: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
    assert_eq!(fields.iter().count(), 509);
    let forage: resonance_content::foraging::Document =
        json(output.path(), resonance_content::foraging::PATH)?;
    assert_eq!(forage.prepare(&items.mappings, &fields)?.pools.len(), 6);
    let ex: resonance_content::ex_skills::Document =
        json(output.path(), resonance_content::ex_skills::PATH)?;
    let ex = ex.prepare(&items.mappings)?;
    ex.validate_rules(531)?;
    let names = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|id| id.resource_name().map(|n| (id.value(), n.to_owned())))
            .collect::<BTreeMap<_, _>>(),
    )?;
    let recoil: battle_rules::Recoil = json(output.path(), resonance_content::battle_recoil::PATH)?;
    assert_eq!(recoil.prepare(&names)?.impulses.len(), 19);
    // Reruns overwrite interrupted output directly, without freshness state.
    let before = fs::read(output.path().join(arte::PATH))?;
    fs::write(output.path().join(arte::PATH), b"interrupted cook")?;
    cook_all(&installation, output.path(), Language::English)?;
    assert_eq!(fs::read(output.path().join(arte::PATH))?, before);
    for language in [
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        cook_all(&installation, output.path(), language)?;
        let translated: items::Document = json(output.path(), items::PATH)?;
        translated.prepare()?;
        assert_ne!(
            translated.items[translated.mappings.target(1)?].text,
            items.items[items.mappings.target(1)?].text
        );
        let techniques: arte::document::Document = json(output.path(), arte::PATH)?;
        techniques.prepare()?;
        let ex: resonance_content::ex_skills::Document =
            json(output.path(), resonance_content::ex_skills::PATH)?;
        ex.prepare(&translated.mappings)?.validate_rules(531)?;
    }
    Ok(())
}
