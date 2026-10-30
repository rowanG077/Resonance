#[path = "common/action.rs"]
mod action;
mod common;
use action::Action::{self, *};
use anyhow::Result;
use resonance_content::{
    menu_data::{CUSTOMIZE_OPTIONS, CustomizeSettings},
    options::{self, Document},
};
use resonance_game::menu::{Menu, customize::Focus};

fn press(menu: &mut Menu, action: Action) -> Option<i16> {
    let cue = menu.step(action.input());
    for _ in 0..40 {
        menu.step(Default::default());
    }
    cue
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
    menu.customize.row = CUSTOMIZE_OPTIONS;
    menu.customize.defaults = true;
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.window, 2);
    assert_eq!(menu.customize.draft.background, 3);
    assert_eq!(menu.customize.draft.battle_rank, 1);
    assert_eq!(menu.customize.draft.button_map, [0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        menu.customize.draft.keymap.bindings[0],
        data.defaults.keymap.bindings[1]
    );
    assert_eq!(menu.customize.draft.colors.menu, [2, 4, 6, 8]);
    assert_eq!(
        menu.customize_data()?
            .option_description(resonance_content::menu_data::COMBINED_BATTLES_OPTION),
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
