mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    library::{Library, Mappings},
    menu_data::MenuData,
    overworld::PartyCharacter,
    session::SessionData,
    strategy::{Document, Group, Lane},
};
use resonance_events::party::Party;
use resonance_game::menu::strategy::{Focus, Strategy};
use std::{fs, path::Path};

#[test]
#[ignore = "requires current strategy library and pre-migration metadata; no media cooking"]
fn saved_strategy_selections_and_all_character_choices_survive_named_rules() -> Result<()> {
    let files = gameplay(&Library::open(&common::asset_root(), &[])?, true)?;
    let session = SessionData::load(&files)?;
    let current = session.rules.as_ref().unwrap();
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../local/structure-review-20261027/strategy-options/before/game/menu-data.json",
        ))?)?;
    let mut old = (**current).clone();
    old.strategy = serde_json::from_value(before["strategy"].clone())?;
    old.presentation.strategy = Some(serde_json::from_value(
        before["presentation"]["strategy"].clone(),
    )?);
    let mut expected = Party::new(&session, Default::default())?;
    expected.strategy_presets = Some(old.strategy_presets()?);
    expected.strategy_presets.as_mut().unwrap()[0].name = "My team".into();
    let saved = serde_json::to_vec(&expected)?;
    let mut actual: Party = serde_json::from_slice(&saved)?;
    actual.bind_rules(&session);
    actual.validate(&session)?;
    for character in 0..9 {
        actual.formation = vec![character as u8 + 1];
        expected.formation = actual.formation.clone();
        for group in 0..3 {
            let state = Strategy {
                group,
                focus: Focus::Setting,
                ..Default::default()
            };
            assert_eq!(
                state.page(&actual, current).options(),
                state.page(&expected, &old).options()
            );
            for choice in 0..current.strategy.groups[group].len() {
                for preset in [None, Some(0), Some(1), Some(2)] {
                    assert_eq!(
                        actual.set_strategy(
                            &current.strategy,
                            character,
                            group,
                            choice as u8,
                            preset
                        ),
                        expected.set_strategy(
                            &old.strategy,
                            character,
                            group,
                            choice as u8,
                            preset
                        )
                    );
                    assert_eq!(
                        serde_json::to_value(&actual)?,
                        serde_json::to_value(&expected)?
                    );
                }
            }
        }
        for choice in 0..7 {
            assert_eq!(
                current.strategy.lane(character, choice),
                old.strategy.lane(character, choice)
            );
        }
    }
    println!(
        "900 character/choice/preset transactions, 63 formation lanes and saved custom presets match"
    );
    Ok(())
}
#[test]
#[ignore = "requires current strategy library; metadata-only mod"]
fn named_mods_change_eligibility_lanes_defaults_and_keep_saved_preset_edits() -> Result<()> {
    let root = common::asset_root();
    let files = gameplay(&Library::open(&root, &[])?, true)?;
    let session = SessionData::load(&files)?;
    let menu = session.rules.as_ref().unwrap();
    let mut party = Party::new(&session, Default::default())?;
    party.strategy_presets = Some(menu.strategy_presets()?);
    party.strategy_presets.as_mut().unwrap()[0].name = "Custom".into();
    let saved = serde_json::to_vec(&party)?;
    let mut doc: Document = files.json(resonance_content::strategy::PATH)?;
    let action = doc.groups.get_mut(&Group::Action).unwrap();
    let entry = action.options.remove("attack-close").unwrap();
    action.options.insert("nearby-enemy".into(), entry);
    action.mappings = Mappings::new(
        action
            .mappings
            .iter()
            .map(|(id, name)| {
                (
                    id,
                    if name == "attack-close" {
                        "nearby-enemy"
                    } else {
                        name
                    }
                    .to_owned(),
                )
            })
            .collect(),
    )?;
    doc.presets.get_mut("reserve").unwrap().default.action = "nearby-enemy".into();
    doc.presets.get_mut("reserve").unwrap().default.skill_magic = "at-once".into();
    doc.presets.get_mut("reserve").unwrap().name = "Modded".into();
    doc.default_positions
        .insert(PartyCharacter::Lloyd, Lane::Rear);
    doc.groups
        .get_mut(&Group::SkillMagic)
        .unwrap()
        .options
        .get_mut("heal")
        .unwrap()
        .characters = Some(vec![PartyCharacter::Colette, PartyCharacter::Raine]);
    let output = tempfile::tempdir()?;
    fs::create_dir(output.path().join("game"))?;
    fs::write(
        output.path().join(resonance_content::strategy::PATH),
        serde_json::to_vec(&doc)?,
    )?;
    let modified = gameplay(
        &Library::open_paths(&root, &[output.path().to_owned()])?,
        true,
    )?;
    let modded = MenuData::load(&modified)?;
    assert_eq!(modded.strategy.lane(0, 0), 2);
    assert_eq!(modded.strategy_presets()?[0].members[0], [1, 1, 0]);
    assert_eq!(
        modded.strategy_presets()?[0].members[3],
        [1, 2, 0],
        "Raine's override survives a changed default"
    );
    assert_eq!(modded.strategy.groups[1][7].characters, (1 << 1) | (1 << 3));
    assert_eq!(modded.strategy_presets()?[0].name, "Modded");
    assert_eq!(serde_json::to_vec(&party)?, saved);
    let restored: Party = serde_json::from_slice(&saved)?;
    let state = Strategy {
        focus: Focus::Presets,
        ..Default::default()
    };
    assert_eq!(
        state.page(&restored, &modded).presets()?,
        party.strategy_presets.unwrap()
    );
    Ok(())
}
