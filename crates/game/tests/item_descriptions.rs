mod common;
use anyhow::Result;
use common::gameplay;
use resonance_content::{
    items::{Description, Document},
    library::{Library, Mod},
    menu_data::MenuData,
    session::SessionData,
};
use resonance_events::party::Party;

#[test]
#[ignore = "requires current item library; metadata-only mod preparation"]
fn named_status_description_edits_share_text_without_changing_gameplay() -> Result<()> {
    let root = common::asset_root();
    let retained = gameplay(&Library::open(&root, &[])?, true)?;
    let original = MenuData::load(&retained)?;
    let mut document: Document = retained.json(resonance_content::items::PATH)?;
    let id = |name: &str| {
        document
            .mappings
            .iter()
            .find(|(_, value)| *value == name)
            .unwrap()
            .0 as u16
    };
    let (half, third) = (id("faerie-ring"), id("emerald-ring"));
    let edits = tempfile::tempdir()?;
    // Documents resolve at startup, so every edit opens the library again.
    let save = |value: &Document| -> Result<Library> {
        common::write_mod(edits.path(), resonance_content::items::PATH, value)?;
        Ok(Library::open(
            &root,
            &[Mod {
                name: "description edits".into(),
                root: edits.path().into(),
            }],
        )?)
    };
    document.equipment_descriptions.remove("half-tp-cost");
    document.equipment_descriptions.insert(
        "custom-discount".into(),
        Description::new("Edited discount".into(), vec![]),
    );
    document
        .items
        .get_mut("faerie-ring")
        .unwrap()
        .properties
        .descriptions = vec!["custom-discount".into()];
    let library = save(&document)?;
    let files = gameplay(&library, true)?;
    let menus = MenuData::load(&files)?;
    let data = SessionData::load(&files)?;
    let mut member = Party::new(&data, Default::default())?.members[0].clone();
    member.equipment = [0, 0, 0, third, half, 0];
    assert_eq!(
        member.equipment_captions(&menus)?,
        ["third-tp-cost", "custom-discount"]
    );
    assert_eq!(member.tp_discount(&menus), member.tp_discount(&original));
    assert_eq!(
        menus.status_text()?.equipment_effects["custom-discount"],
        "Edited discount"
    );
    document.equipment_descriptions.insert(
        "custom-discount".into(),
        Description::new("Shared discount".into(), vec!["third-tp-cost".into()]),
    );
    document
        .items
        .get_mut("emerald-ring")
        .unwrap()
        .properties
        .descriptions
        .push("custom-discount".into());
    let library = save(&document)?;
    let menus = MenuData::load(&gameplay(&library, true)?)?;
    assert_eq!(
        member.equipment_captions(&menus)?,
        ["custom-discount", "custom-discount"]
    );
    assert_eq!(
        menus.status_text()?.equipment_effects["custom-discount"],
        "Shared discount"
    );
    assert_eq!(
        MenuData::load(&retained)?.status_text()?.equipment_effects["half-tp-cost"],
        original.status_text()?.equipment_effects["half-tp-cost"]
    );
    document
        .equipment_descriptions
        .insert("custom-discount".into(), Description::Text(false.into()));
    let library = save(&document)?;
    let tolerant = gameplay(&library, false)?;
    assert!(SessionData::load(&tolerant).is_ok());
    assert!(
        !MenuData::load(&tolerant)?
            .status_text()?
            .equipment_effects
            .contains_key("custom-discount")
    );
    assert!(MenuData::load(&gameplay(&library, true)?).is_err());
    Ok(())
}
