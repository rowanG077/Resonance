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
    common::write_mod(output.path(), resonance_content::strategy::PATH, &doc)?;
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
