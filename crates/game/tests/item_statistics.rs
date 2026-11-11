//! Start changes item information without editing the selected equipment or party.
#[path = "common/menu_fixture.rs"]
mod menu_fixture;
use resonance_events::{SavedProgress, input::Button};
use resonance_game::{
    field::{FieldCheckpoint, FieldInput},
    menu::{Menu, Page, Resources, equipment::Equipment},
};
use std::sync::Arc;

fn menu() -> Menu {
    let (session, data, party) = menu_fixture::fixture();
    let mut menu = Menu::new(
        Page::Main,
        Some(resonance_game::Checkpoint::Field(FieldCheckpoint {
            allow_incomplete_scripts: false,
            map_id: 330,
            position: [0.; 3],
            heading: 0.,
            camera: None,
            played_ticks: 0,
            progress: SavedProgress::new(party),
        })),
        false,
    );
    menu.resources = Some(Arc::new(Resources {
        session,
        data: Arc::new(data),
        files: Default::default(),
    }));
    menu
}

fn details() -> FieldInput {
    FieldInput {
        pressed_buttons: [Button::Start].into(),
        ..Default::default()
    }
}

#[test]
fn item_information_mode_survives_menu_page_changes_and_does_not_toggle_party_stats() {
    let mut menu = menu();
    let before = serde_json::to_value(menu.party()).unwrap();
    for (page, expected) in [
        (Page::Equip, true),
        (Page::Items, false),
        (Page::Collection, true),
        (Page::WorldMap, false),
    ] {
        menu.page = page;
        assert_eq!(menu.step(details()), Some(1));
        assert_eq!(menu.item_statistics, expected);
        assert!(!menu.party_statistics);
        assert_eq!(serde_json::to_value(menu.party()).unwrap(), before);
    }
    menu.page = Page::Main;
    assert_eq!(menu.step(details()), Some(1));
    assert!(menu.party_statistics);
    assert!(!menu.item_statistics);
}

#[test]
fn item_statistics_waits_for_scrolls_and_target_panels_to_finish() {
    let mut opening = menu();
    opening.page = Page::Equip;
    opening.equipment = Equipment::opening();
    assert_eq!(opening.step(details()), None);
    assert!(!opening.item_statistics);
    for page in [Page::Items, Page::Collection] {
        let mut menu = menu();
        menu.page = page;
        match page {
            Page::Items => menu.inventory.scroll = 3,
            Page::Collection => menu.collection.scroll = 3,
            _ => unreachable!(),
        }
        assert_eq!(menu.step(details()), None);
        assert!(!menu.item_statistics);
        // The final scroll update is ready for input on this same tick.
        assert_eq!(menu.step(details()), Some(1));
        assert!(menu.item_statistics);
    }
    let mut menu = menu();
    menu.page = Page::WorldMap;
    menu.world_map.focus = resonance_game::menu::world_map::Focus::Items;
    menu.world_map.shops_opacity = 255;
    assert_eq!(menu.step(details()), None);
    for _ in 0..6 {
        menu.step(Default::default());
    }
    assert_eq!(menu.step(details()), Some(1));
    assert!(menu.item_statistics);
}
