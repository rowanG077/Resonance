//! Gem transactions, stat changes and save restoration with the cooked rules.
mod common;
use common::cooked;
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::{SavedProgress, party::Party};
use std::sync::Arc;

#[test]
#[ignore = "requires locally cooked EX skill definitions; no devices"]
fn gems_stats_compounds_and_save_restore() {
    let menu: MenuData = cooked("game/menu-data.json");
    let mut data: SessionData = cooked("game/session-data.json");
    data.ex_skills = Some(Arc::new(menu.ex_skills.clone()));
    let mut party = Party::new(&data, Default::default()).unwrap();
    let baseline = serde_json::to_value(&party).unwrap();
    assert!(!party.set_ex_gem(&data, 0, 0, 1).unwrap());
    assert_eq!(serde_json::to_value(&party).unwrap(), baseline);
    for (id, count) in [(40, 3), (41, 2), (42, 1), (43, 1), (496, 1)] {
        party.change_item(&data, id, count).unwrap();
    }
    assert!(party.set_ex_gem(&data, 0, 0, 1).unwrap());
    assert!(party.set_ex_skill(&data, 0, 0, 1).unwrap());
    assert_eq!(party.members[0].stats(&menu).strength, 46);
    assert!(party.set_ex_gem(&data, 0, 1, 1).unwrap());
    let before = serde_json::to_value(&party).unwrap();
    assert!(!party.set_ex_gem(&data, 0, 1, 1).unwrap());
    assert!(!party.set_ex_skill(&data, 0, 1, 1).unwrap());
    assert!(!party.set_ex_skill(&data, 0, 1, 7).unwrap());
    assert!(party.set_ex_gem(&data, 0, 4, 1).is_err());
    assert_eq!(serde_json::to_value(&party).unwrap(), before);
    assert!(party.set_ex_skill(&data, 0, 1, 2).unwrap());
    assert!(
        party.members[0]
            .active_compound_ex(&menu.ex_skills, 0)
            .is_empty()
    );
    party.members[0].compound_ex_skills.insert(0);
    party.members[0].recent_compound_ex_skills.insert(0);
    assert_eq!(party.members[0].active_compound_ex(&menu.ex_skills, 0), [0]);

    assert!(party.set_ex_gem(&data, 0, 0, 2).unwrap());
    assert_eq!(party.items[&40], 1, "replacement destroys the old gem");
    assert_eq!(party.items[&41], 1);
    assert_eq!(party.members[0].ex_skills[0], 0);
    assert_eq!(party.members[0].stats(&menu).strength, 44);
    assert!(
        party.members[0]
            .active_compound_ex(&menu.ex_skills, 0)
            .is_empty()
    );
    assert!(party.set_ex_gem(&data, 0, 0, 5).unwrap());
    assert!(!party.items.contains_key(&496));
    assert!(party.set_ex_skill(&data, 0, 0, 7).unwrap());
    assert!(party.set_ex_gem(&data, 0, 2, 3).unwrap());
    assert!(party.set_ex_skill(&data, 0, 2, 10).unwrap());
    assert_eq!(party.members[0].maximum_vitals(), [210, 27]);
    party.members[0].hp = 210;
    party.members[0].tp = 27;
    party.validate(&data).unwrap();

    let progress = SavedProgress {
        script_globals: vec![0; 256],
        party,
        event_flags: Default::default(),
        event_records: Default::default(),
        random_state: 42,
        gameplay_random: Default::default(),
        tick: 0,
    };
    let json = serde_json::to_value(&progress).unwrap();
    assert!(json["party"]["members"][0].get("ex_rules").is_none());
    let restored: SavedProgress = serde_json::from_value(json).unwrap();
    let mut party = restored.into_state(&data).unwrap().party.unwrap();
    assert_eq!(party.members[0].recent_compound_ex_skills, [0].into());
    assert_eq!(party.members[0].maximum_vitals(), [210, 27]);
    assert!(party.set_ex_skill(&data, 0, 0, 1).unwrap());
    assert!(party.set_ex_gem(&data, 0, 2, 4).unwrap());
    assert_eq!([party.members[0].hp, party.members[0].tp], [200, 26]);
    assert_eq!(party.members[0].active_compound_ex(&menu.ex_skills, 0), [0]);
    party.validate(&data).unwrap();
    party.members[0].ex_skills[2] = 1;
    assert!(
        party.validate(&data).is_err(),
        "invalid saved selection must fail"
    );
}
