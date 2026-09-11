#[path = "common/action.rs"]
mod action;
mod common;
use action::{Action, Action::*};
use common::cooked;
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::{SavedProgress, party::Party};
use resonance_game::{
    field::FieldCheckpoint,
    menu::{Menu, Page, Resources, unison::Focus},
};
use std::sync::Arc;

#[test]
#[ignore = "requires locally cooked menu definitions; no devices"]
fn unison_unlock_navigation_and_shared_shortcuts_survive_save() {
    let session: Arc<SessionData> = Arc::new(cooked("game/session-data.json"));
    let data: Arc<MenuData> = Arc::new(cooked("game/menu-data.json"));
    data.validate().unwrap();
    let mut party = Party::new(&session, Default::default()).unwrap();
    party.formation = vec![1, 2, 3];
    party.members[0].techniques = session.characters[0].allowed_techniques[..12]
        .iter()
        .copied()
        .collect();
    let healing = session.characters[3].allowed_techniques[0];
    party.members[3].techniques.insert(healing);
    let mut globals = vec![0; 256];
    globals[16] = 1_402_999;
    let checkpoint = FieldCheckpoint {
        map_id: 332,
        position: [0.; 3],
        heading: 0.,
        camera: None,
        played_ticks: None,
        progress: SavedProgress {
            script_globals: globals,
            party,
            event_flags: Default::default(),
            event_records: Default::default(),
            random_state: 42,
            gameplay_random: Default::default(),
            tick: 0,
        },
    };
    let mut menu = Menu::new(Page::Main, Some(checkpoint), false);
    menu.resources = Some(Arc::new(Resources {
        session: session.clone(),
        data,
    }));
    menu.selected = 1;
    let press = |menu: &mut Menu, action: Action| {
        let result = menu.step(action.input());
        for _ in 0..40 {
            menu.step(Default::default());
            if !menu.main_animating() && (menu.page != Page::Unison || !menu.unison.animating()) {
                break;
            }
        }
        assert!(
            !menu.main_animating() && (menu.page != Page::Unison || !menu.unison.animating()),
            "menu transition did not finish"
        );
        result
    };
    assert_eq!(press(&mut menu, Accept), Some(4));
    assert_eq!(menu.page, Page::Main);
    menu.checkpoint.as_mut().unwrap().progress.script_globals[16] += 1;
    press(&mut menu, Accept);
    assert_eq!(menu.page, Page::Unison);
    assert_eq!(menu.unison_techniques().len(), 12);
    press(&mut menu, Accept);
    press(&mut menu, PageDown);
    assert_eq!((menu.unison.row, menu.unison.first), (8, 8));
    press(&mut menu, PageDown);
    assert_eq!((menu.unison.row, menu.unison.first), (11, 8));
    assert_eq!(press(&mut menu, Down), None);
    let chosen = menu.unison_selection().unwrap().technique;
    press(&mut menu, Accept);
    assert_eq!(menu.unison.focus, Focus::Slots);
    press(&mut menu, Accept);
    assert_eq!((menu.unison.row, menu.unison.first), (11, 4));
    press(&mut menu, Cancel);
    press(&mut menu, Alternate);
    assert!(menu.unison_selection().is_none());
    assert_eq!(press(&mut menu, Alternate), None);
    press(&mut menu, Accept);
    press(&mut menu, PageDown);
    press(&mut menu, PageDown);
    press(&mut menu, Accept);
    press(&mut menu, Right);
    assert_eq!(menu.unison.character, 2);
    assert_eq!(press(&mut menu, Right), None);
    press(&mut menu, Up);
    assert_eq!((menu.unison.character, menu.unison.slot), (1, 3));
    press(&mut menu, Cancel);
    press(&mut menu, Accept);
    assert_eq!((menu.unison.character, menu.unison.slot), (1, 0));
    press(&mut menu, Cancel);
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .formation
        .push(4);
    menu.unison.character = 3;
    press(&mut menu, Accept);
    assert_eq!(menu.unison.character, 3);
    press(&mut menu, Accept);
    let healing = menu.unison_selection().unwrap();
    assert!(
        !menu.resources.as_ref().unwrap().data.techniques[usize::from(healing.technique)]
            .unison_usable
    );
    assert_eq!(press(&mut menu, Accept), Some(2));
    assert_eq!(menu.unison_selection(), Some(healing));
    press(&mut menu, Cancel);
    menu.checkpoint
        .as_mut()
        .unwrap()
        .progress
        .party
        .formation
        .pop();
    press(&mut menu, Accept);
    assert_eq!(menu.unison.character, 0);
    press(&mut menu, Cancel);
    let saved = menu.checkpoint.as_ref().unwrap();
    let restored: FieldCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(saved).unwrap()).unwrap();
    let restored = restored.progress.into_state(&session).unwrap();
    assert_eq!(restored.party.unwrap().members[0].shortcuts[0], chosen);
    menu.character = 0;
    menu.selected = 0;
    press(&mut menu, Accept);
    press(&mut menu, Accept);
    assert_eq!(menu.page, Page::Tech);
    assert_eq!(menu.selected_technique().unwrap().technique, chosen);
}
