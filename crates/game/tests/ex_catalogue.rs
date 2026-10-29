mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    ex_skills::Document,
    library::{Library, Mod},
    session::SessionData,
};
use resonance_events::party::Party;
use std::{fs, path::Path, sync::Arc};

fn previous(data: &SessionData) -> Result<SessionData> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../local/structure-review-20261027/ex-skill-structure/before/game/menu-data.json",
    );
    let before: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let mut old = data.clone();
    let mut rules = (**data.rules.as_ref().unwrap()).clone();
    rules.ex_skills = serde_json::from_value(before["ex_skills"].clone())?;
    old.rules = Some(Arc::new(rules));
    Ok(old)
}

fn foraging_party(data: &SessionData) -> Result<Party> {
    let mut party = Party::new(data, Default::default())?;
    for _ in 0..511 {
        party
            .step_field_ex(data, 0, &mut || panic!("early foraging draw"))
            .map_err(anyhow::Error::msg)?;
    }
    Ok(party)
}

#[test]
#[ignore = "requires current EX library and saved metadata; no media cooking"]
fn all_named_skill_and_compound_loadouts_preserve_stats_discovery_and_saves() -> Result<()> {
    let files = gameplay(&Library::open(&common::asset_root(), &[])?, true)?;
    let data = SessionData::load(&files)?;
    let old = previous(&data)?;
    let menu = data.rules.as_ref().unwrap();
    let old_menu = old.rules.as_ref().unwrap();
    let mut checked = 0;
    for character in 0..9 {
        let choices = &menu.ex_skills.characters[character];
        let loadouts = choices
            .levels
            .iter()
            .flatten()
            .map(|&skill| vec![skill])
            .chain(choices.compounds.iter().map(|c| c.required.clone()));
        for loadout in loadouts {
            let mut actual = Party::new(&data, Default::default())?;
            let mut expected = Party::new(&old, Default::default())?;
            for party in [&mut actual, &mut expected] {
                let member = &mut party.members[character];
                member.ex_gems = [5; 4];
                for (slot, &skill) in loadout.iter().enumerate() {
                    member.ex_skills[slot] = skill;
                }
                member.compound_ex_skills = (0..choices.compounds.len() as u8).collect();
                member.recent_compound_ex_skills = member.compound_ex_skills.clone();
            }
            actual.validate(&data)?;
            expected.validate(&old)?;
            assert_eq!(
                actual.members[character].stats(menu),
                expected.members[character].stats(old_menu)
            );
            assert_eq!(
                actual.members[character].maximum_vitals(),
                expected.members[character].maximum_vitals()
            );
            assert_eq!(
                actual.members[character].active_compound_ex(&menu.ex_skills, character),
                expected.members[character].active_compound_ex(&old_menu.ex_skills, character)
            );
            let saved = serde_json::to_vec(&expected)?;
            let mut restored: Party = serde_json::from_slice(&saved)?;
            restored.bind_rules(&data);
            restored.validate(&data)?;
            assert_eq!(
                serde_json::to_value(&restored)?,
                serde_json::to_value(&actual)?
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 360);
    println!(
        "All 360 single-skill and compound loadouts preserve stats, active discoveries and saved state."
    );
    Ok(())
}

#[test]
#[ignore = "requires current EX library and saved metadata; no media cooking"]
fn every_foraging_selection_preserves_random_draws_item_results_and_inventory_caps() -> Result<()> {
    let files = gameplay(&Library::open(&common::asset_root(), &[])?, true)?;
    let data = SessionData::load(&files)?;
    let old = previous(&data)?;
    let before = &old.rules.as_ref().unwrap().ex_skills.foraging;
    let current = &data.rules.as_ref().unwrap().ex_skills.foraging;
    assert_eq!(current.fields.len(), 169);
    let actual_base = foraging_party(&data)?;
    let expected_base = foraging_party(&old)?;
    let mut checked = 0;
    for &field in current.fields.keys() {
        let choices = &before.fields[&field];
        for (choice, &pool) in choices.iter().enumerate() {
            for item in 0..before.pools[pool].len() {
                for full in [false, true] {
                    let mut actual = actual_base.clone();
                    let mut expected = expected_base.clone();
                    for party in [&mut actual, &mut expected] {
                        party.formation = vec![7];
                        party.field_leader = 7;
                        party.members[6].ex_gems[0] = 2;
                        party.members[6].ex_skills[0] = 44;
                        if full {
                            for pool in &before.pools {
                                for &id in pool {
                                    party.items.insert(id, 20);
                                }
                            }
                        }
                    }
                    let draws: Vec<_> = if choices.len() == 1 {
                        vec![item as u32]
                    } else {
                        vec![choice as u32, item as u32]
                    };
                    let (mut a, mut b) = (draws.iter().copied(), draws.iter().copied());
                    let old_item = expected
                        .step_field_ex(&old, field, &mut || a.next().expect("unexpected old draw"))
                        .map_err(anyhow::Error::msg)?;
                    let new_item = actual
                        .step_field_ex(&data, field, &mut || b.next().expect("unexpected new draw"))
                        .map_err(anyhow::Error::msg)?;
                    assert!(a.next().is_none() && b.next().is_none());
                    assert_eq!(new_item, old_item);
                    assert_eq!(new_item, (!full).then_some(before.pools[pool][item]));
                    assert_eq!(
                        serde_json::to_value(&actual)?,
                        serde_json::to_value(&expected)?
                    );
                    checked += 1;
                }
            }
        }
    }
    println!(
        "{checked} foraging selections preserve both random stages, results and full-inventory behavior across all 169 fields."
    );
    Ok(())
}

#[test]
#[ignore = "requires current EX library; metadata-only mods"]
fn named_ex_combinations_starting_state_captions_and_foraging_are_editable() -> Result<()> {
    use resonance_content::{characters::Gem, foraging::Area, overworld::PartyCharacter};
    let root = common::asset_root();
    let retained = gameplay(&Library::open(&root, &[])?, true)?;
    let original = SessionData::load(&retained)?;
    let mut document: Document = retained.json(resonance_content::ex_skills::PATH)?;
    document.skills.get_mut("strong").unwrap().stat_bonuses[0].percent = 10;
    document.skills.get_mut("strong").unwrap().text["name"] = "Mighty".into();
    let shared = document
        .skills
        .get_mut("ex-attack")
        .unwrap()
        .combinations
        .iter_mut()
        .find(|c| c.characters.contains(&PartyCharacter::Lloyd))
        .unwrap();
    assert_eq!(
        shared.characters,
        [
            PartyCharacter::Lloyd,
            PartyCharacter::Sheena,
            PartyCharacter::Regal
        ]
    );
    shared.requires = vec!["strong".into(), "dodge".into()];
    let mut characters: resonance_content::characters::Document =
        retained.json(resonance_content::characters::PATH)?;
    let initial = characters
        .characters
        .get_mut("lloyd")
        .unwrap()
        .initial
        .as_mut()
        .unwrap();
    initial.ex_gems = [
        (
            1,
            Gem {
                level: 5,
                skill: Some("strong".into()),
            },
        ),
        (
            2,
            Gem {
                level: 5,
                skill: Some("dodge".into()),
            },
        ),
    ]
    .into();
    initial.compound_ex_skills = vec!["ex-attack".into()];
    initial.recent_compound_ex_skills = vec!["ex-attack".into()];
    let mut forage: resonance_content::foraging::Document =
        retained.json(resonance_content::foraging::PATH)?;
    forage
        .pools
        .insert("lemon-grove".into(), vec!["lemon".into()]);
    let field = forage.areas[0].fields[0].clone();
    for area in &mut forage.areas {
        area.fields.retain(|f| *f != field);
    }
    forage.areas.retain(|a| !a.fields.is_empty());
    forage.areas.push(Area {
        fields: vec![field.clone()],
        pools: vec!["lemon-grove".into()],
    });
    let edits = tempfile::tempdir()?;
    fs::create_dir(edits.path().join("game"))?;
    fs::create_dir(edits.path().join("fields"))?;
    for (path, value) in [
        (
            resonance_content::ex_skills::PATH,
            serde_json::to_vec(&document)?,
        ),
        (
            resonance_content::characters::PATH,
            serde_json::to_vec(&characters)?,
        ),
        (
            resonance_content::foraging::PATH,
            serde_json::to_vec(&forage)?,
        ),
    ] {
        fs::write(edits.path().join(path), value)?;
    }
    let files = gameplay(
        &Library::open(
            &root,
            &[Mod {
                name: "EX edits".into(),
                root: edits.path().into(),
            }],
        )?,
        true,
    )?;
    let data = SessionData::load(&files)?;
    let menu = data.rules.as_ref().unwrap();
    assert_eq!(menu.ex_skill_text()?.skills[&1].name, "Mighty");
    for (character, index) in [(0, 0), (4, 4), (7, 0)] {
        assert_eq!(
            menu.ex_skills.characters[character].compounds[index].required,
            [1, 5]
        );
    }
    let mut party = foraging_party(&data)?;
    party.validate(&data)?;
    assert_eq!(party.members[0].active_compound_ex(&menu.ex_skills, 0), [0]);
    assert_eq!(party.members[0].recent_compound_ex_skills, [0].into());
    assert_eq!(party.members[0].stats(menu).strength, 48);
    let fields: resonance_content::library::Mappings =
        files.json(resonance_content::library::FIELDS)?;
    let id = fields.iter().find(|(_, name)| *name == field).unwrap().0;
    party.formation = vec![7];
    party.field_leader = 7;
    party.members[6].ex_gems[0] = 2;
    party.members[6].ex_skills[0] = 44;
    let mut draws = 0;
    let found = party
        .step_field_ex(&data, id, &mut || {
            draws += 1;
            0
        })
        .map_err(anyhow::Error::msg)?;
    let items: resonance_content::items::Document = files.json(resonance_content::items::PATH)?;
    let lemon = items
        .mappings
        .iter()
        .find(|(_, name)| *name == "lemon")
        .unwrap()
        .0 as u16;
    assert_eq!((draws, found), (1, Some(lemon)));
    let mut saved: Party = serde_json::from_slice(&serde_json::to_vec(&Party::new(
        &original,
        Default::default(),
    )?)?)?;
    saved.bind_rules(&data);
    saved.validate(&data)?;
    assert_eq!(saved.members[0].ex_skills, [0; 4]);
    assert_eq!(
        SessionData::load(&retained)?
            .rules
            .unwrap()
            .ex_skill_text()?
            .skills[&1]
            .name,
        "Strong"
    );
    Ok(())
}
