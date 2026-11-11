mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    ex_skills::Document,
    library::{Library, Mod},
    session::SessionData,
};
use resonance_events::party::Party;

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
    common::write_mod(edits.path(), resonance_content::ex_skills::PATH, &document)?;
    common::write_mod(
        edits.path(),
        resonance_content::characters::PATH,
        &characters,
    )?;
    common::write_mod(edits.path(), resonance_content::foraging::PATH, &forage)?;
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
    let mut party = Party::new(&data, Default::default())?;
    for _ in 0..511 {
        party
            .step_field_ex(&data, 0, &mut || panic!("early foraging draw"))
            .map_err(anyhow::Error::msg)?;
    }
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
