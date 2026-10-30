#[path = "common/action.rs"]
mod action;
mod common;
use action::Action::{self, *};
use anyhow::Result;
use resonance_content::{
    menu_data::{CustomizeData, CustomizeSettings},
    options::{self, Document},
};
use resonance_game::menu::{Menu, customize::Focus};
use std::{fs, path::Path};

fn press(menu: &mut Menu, action: Action) -> Option<i16> {
    let cue = menu.step(action.input());
    for _ in 0..40 {
        menu.step(Default::default());
    }
    cue
}
fn pair(data: &CustomizeData, previous: &CustomizeData) -> Result<[Menu; 2]> {
    let mut result = [
        Menu::title_customize(data.defaults.clone(), data.clone())?,
        Menu::title_customize(previous.defaults.clone(), previous.clone())?,
    ];
    for menu in &mut result {
        press(menu, Idle);
    }
    Ok(result)
}
fn compare(menus: &mut [Menu; 2], action: Action) -> Result<()> {
    let [actual, expected] = menus;
    assert_eq!(press(actual, action), press(expected, action));
    assert_eq!(
        serde_json::to_value(&actual.customize)?,
        serde_json::to_value(&expected.customize)?
    );
    Ok(())
}

#[test]
#[ignore = "requires current options and pre-migration metadata; no devices"]
fn named_options_preserve_all_editors_reset_and_preference_serialization() -> Result<()> {
    let document: Document = common::cooked(options::PATH);
    let data = document.prepare()?;
    let mut before: serde_json::Value = serde_json::from_slice(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/structure-review-20261027/options-structure/before/game/menu/customize.json"))?)?;
    before["options"].as_array_mut().unwrap().extend(
        data.options[14..]
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()?,
    );
    before["confirmations"] = serde_json::to_value(&data.confirmations)?;
    let old: CustomizeData = serde_json::from_value(before)?;
    assert_eq!(data.defaults, old.defaults);
    let mut commands = 0;
    for row in 0..18 {
        let mut menus = pair(&data, &old)?;
        for menu in &mut menus {
            menu.customize.row = row;
            menu.customize.first = row.saturating_sub(8);
        }
        for action in [Left, Right, Right, Left] {
            compare(&mut menus, action)?;
            commands += 1;
        }
        if [4, 5, 6, 13].contains(&row) {
            compare(&mut menus, Accept)?;
            commands += 1;
        }
    }
    for group in 0..7 {
        for component in 1..=4 {
            let mut menus = pair(&data, &old)?;
            for menu in &mut menus {
                menu.customize.focus = Focus::Colors;
                menu.customize.color_group = group;
                menu.customize.component = component;
            }
            for action in [Left, Right] {
                compare(&mut menus, action)?;
                commands += 1;
            }
        }
    }
    for channel in 0..6 {
        let mut menus = pair(&data, &old)?;
        for menu in &mut menus {
            menu.customize.focus = Focus::Volume;
            menu.customize.channel = channel;
        }
        for action in [Left, Right] {
            compare(&mut menus, action)?;
            commands += 1;
        }
    }
    for button in 0..7 {
        let mut menus = pair(&data, &old)?;
        for menu in &mut menus {
            menu.customize.focus = Focus::Controls;
            menu.customize.button = button;
        }
        for action in [Left, Right, Right, Left] {
            compare(&mut menus, action)?;
            commands += 1;
        }
    }
    let mut menus = pair(&data, &old)?;
    for menu in &mut menus {
        menu.customize.focus = Focus::Position;
    }
    for action in [Left, Right, Up, Down, OpenMenu] {
        compare(&mut menus, action)?;
        commands += 1;
    }
    for menu in &mut menus {
        menu.customize.focus = Focus::Options;
        menu.customize.row = 18;
        menu.customize.defaults = true;
        menu.customize.draft.volumes.music = 10;
    }
    compare(&mut menus, Accept)?;
    commands += 1;
    assert_eq!(menus[0].customize.draft, data.defaults);
    let bytes = serde_json::to_vec(&menus[0].customize.draft)?;
    let restored: CustomizeSettings = serde_json::from_slice(&bytes)?;
    assert_eq!(restored, old.defaults);
    println!("{commands} menu edits across every option/editor and Reset match pre-migration data");
    Ok(())
}

#[test]
#[ignore = "requires current options; metadata-only mods"]
fn named_option_defaults_and_confirmations_reach_the_menu() -> Result<()> {
    let mut doc: Document = common::cooked(options::PATH);
    doc.defaults.window = "gold".into();
    doc.defaults.background = "speckled".into();
    doc.defaults.difficulty = "hard".into();
    doc.defaults
        .controls
        .insert("a".into(), "techniques".into());
    doc.defaults.controls.insert("b".into(), "attack".into());
    doc.themes.get_mut("gold").unwrap().menu = [2, 4, 6, 8];
    doc.options.get_mut("combined_battles").unwrap().description = "Nearby enemies join.".into();
    doc.confirmations
        .insert("forget_artes".into(), "Forget both kinds?".into());
    let data = doc.prepare()?;
    let settings = CustomizeSettings {
        learn_elemental_artes: true,
        learn_both_arte_paths: true,
        ..Default::default()
    };
    let mut menu = Menu::title_customize(settings.clone(), data.clone())?;
    press(&mut menu, Idle);
    assert_eq!(
        menu.customize.draft, settings,
        "opening must not replace user preferences"
    );
    menu.customize.row = 18;
    menu.customize.defaults = true;
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.window, 2);
    assert_eq!(menu.customize.draft.background, 3);
    assert_eq!(menu.customize.draft.battle_rank, 1);
    assert_eq!(menu.customize.draft.button_map, [1, 0, 2, 3, 4, 5, 6]);
    assert_eq!(menu.customize.draft.colors.menu, [2, 4, 6, 8]);
    assert_eq!(
        menu.customize_data()?.option_description(15),
        "Nearby enemies join."
    );
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.focus, Focus::ForgetArtes { yes: false });
    assert_eq!(
        menu.customize_data()?
            .confirmation(menu.customize.forget_confirmation_key().unwrap())?,
        "Forget both kinds?"
    );
    press(&mut menu, Cancel);
    assert!(
        menu.customize.draft.learn_elemental_artes && menu.customize.draft.learn_both_arte_paths
    );
    Ok(())
}
