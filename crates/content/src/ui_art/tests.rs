use super::*;
use crate::{battle_ui, diagnostics::Diagnostics};
use serde_json::{Value, json};
use std::path::PathBuf;

fn root() -> Result<PathBuf> {
    Ok(std::env::var_os("RESONANCE_TEST_ASSETS")
        .context("set RESONANCE_TEST_ASSETS")?
        .into())
}

#[test]
#[ignore = "requires current library; modded named UI and missing-portrait policy, no media output"]
fn named_battle_roles_mod_independently_and_bad_portraits_stay_optional() -> Result<()> {
    let root = root()?;
    let view = std::cell::RefCell::new(crate::library::Library::open_paths(&root, &[])?.snapshot());
    let original = view
        .borrow_mut()
        .json::<Value>(battle_ui::document::PATH, crate::preparation::JSON_LIMIT)?;
    let mut edited = original.clone();
    edited["commands"]["names"]["items"] = "Inventory".into();
    edited["commands"]["faces"]["colette"] = json!([0, 0, 16, 16]);
    edited["meters"]["numbers"]["tp"] = json!([[1, 2, 3, 255], [4, 5, 6, 255]]);
    edited["portraits"]["lloyd"] =
        json!({"image": "ui/dialogue.json", "size": original["portraits"]["lloyd"]["size"]});
    let bytes = serde_json::to_vec(&edited)?;
    let mut read = |p: &str| {
        Ok(if p == battle_ui::document::PATH {
            bytes.clone().into()
        } else {
            view.borrow_mut()
                .read(p, crate::preparation::ASSET_LIMIT)?
                .into()
        })
    };
    assert!(battle_ui::document::read(&mut read, &Diagnostics::new(true)).is_err());
    let diagnostics = Diagnostics::new(false);
    let art = battle_ui::document::read(&mut read, &diagnostics)?;
    assert!(art.portraits[0].is_none());
    assert!(art.portraits[1..].iter().all(Option::is_some));
    assert_eq!(art.commands.names[4], "Inventory");
    assert_eq!(art.commands.faces[1], [0, 0, 16, 16]);
    assert_eq!(art.meters.numbers[1], [[1, 2, 3, 255], [4, 5, 6, 255]]);
    assert_eq!(
        art.commands.names[0],
        original["commands"]["names"]["techniques"]
    );
    assert!(diagnostics.has_errors());
    let mut invalid = original.clone();
    invalid["version"] = 44.into();
    assert!(serde_json::from_value::<battle_ui::Art<String>>(invalid).is_err());
    let mut invalid = original;
    invalid["commands"]["names"]["invented"] = "bad".into();
    assert!(serde_json::from_value::<battle_ui::Art<String>>(invalid).is_err());
    Ok(())
}
