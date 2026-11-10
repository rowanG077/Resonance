//! Battle-flag instruction dispatch and persistence, without assets or devices.
use super::{Party, tests::data};
use crate::{EventRuntime, GameWorld, ResourceLibrary, SavedProgress};
use std::sync::Arc;
use symphonia_script::{NativeCall, Program, Width};

fn native(code: &mut Vec<u16>, call: NativeCall, args: &[i32]) {
    for &value in args {
        code.extend([
            0x0200,
            value as u16,
            (value as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    code.push(0x2000 | u16::from(call as u8));
}

fn runtime(world: GameWorld, calls: &[(i32, i32)], wait: bool) -> EventRuntime {
    let mut code = vec![];
    if wait {
        native(&mut code, NativeCall::YieldCommand, &[0, 1]);
    }
    for (index, &(selector, value)) in calls.iter().enumerate() {
        native(
            &mut code,
            NativeCall::ConfigureBattleRules,
            &[selector, value],
        );
        // Copy the actual immediate result register to a distinct saved global.
        code.extend([
            0x3000,
            0x1200,
            0x100 + index as u16 * 4,
            0x1200,
            0x20,
            0x3010,
            0x3000,
        ]);
    }
    code.push(0x20ff);
    let mut words = vec![4, 0, 0, 0];
    words.extend(code);
    let bytes: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
    EventRuntime::with_state(
        Arc::new(Program::decode(&bytes).unwrap()),
        Arc::new(ResourceLibrary {
            session_data: Some(Arc::new(data())),
            ..Default::default()
        }),
        world,
        Default::default(),
    )
    .unwrap()
}

fn world() -> GameWorld {
    GameWorld {
        party: Some(Party::new(&data(), Default::default()).unwrap()),
        random_state: 0x12345678,
        ..Default::default()
    }
}

#[test]
fn battle_flags_exchange_supported_values_and_return_previous_value() {
    let mut world = world();
    world.party.as_mut().unwrap().battle_rules.modifiers = 8;
    let random = world.random_state;
    let gameplay = world.gameplay_random;
    let events = runtime(world, &[(0, 0), (0, 8), (0, 0)], false);
    for (index, old) in [8, 0, 8].into_iter().enumerate() {
        assert_eq!(
            events
                .memory()
                .read(0x100 + index as u16 * 4, Width::S32)
                .unwrap(),
            old
        );
    }
    assert_eq!(
        events.world.party.as_ref().unwrap().battle_rules.modifiers,
        0
    );
    assert_eq!(events.world.random_state, random);
    assert_eq!(events.world.gameplay_random, gameplay);
}

#[test]
fn legacy_battle_flags_truncate_to_the_native_halfword() {
    for (value, expected) in [
        (-1, 65535),
        (65536, 0),
        (65544, 8),
        (-65528, 8),
        (i32::MIN, 0),
        (i32::MAX, 65535),
    ] {
        let mut world = world();
        world.party.as_mut().unwrap().battle_rules.modifiers = 8;
        let random = world.random_state;
        let gameplay = world.gameplay_random;
        let events = runtime(world, &[(0, value)], false);
        assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 8);
        assert_eq!(
            events.world.party.as_ref().unwrap().battle_rules.modifiers,
            expected
        );
        assert_eq!(events.world.random_state, random);
        assert_eq!(events.world.gameplay_random, gameplay);
    }
}

#[test]
fn unknown_battle_rule_selectors_return_zero_without_mutation() {
    for selector in [-1, 6, i32::MIN, i32::MAX] {
        let mut world = world();
        world.party.as_mut().unwrap().battle_rules.modifiers = 8;
        let before = world.party.as_ref().unwrap().battle_rules;
        let random = world.random_state;
        let gameplay = world.gameplay_random;
        let events = runtime(world, &[(selector, 65535)], false);
        assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 0);
        assert_eq!(events.world.party.as_ref().unwrap().battle_rules, before);
        assert_eq!(events.world.random_state, random);
        assert_eq!(events.world.gameplay_random, gameplay);
    }
}

#[test]
fn battle_rule_bytes_return_old_signed_values_and_booleanize_coliseum() {
    let events = runtime(
        world(),
        &[
            (1, -1),
            (1, 256),
            (3, 255),
            (3, 128),
            (4, 255),
            (4, 129),
            (5, -128),
            (5, 511),
            (2, -1),
            (2, 0),
        ],
        false,
    );
    for (index, old) in [0, 255, 0, -1, 0, -1, 0, -128, 0, 1]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            events
                .memory()
                .read(0x100 + index as u16 * 4, Width::S32)
                .unwrap(),
            old
        );
    }
    let rules = events.world.party.as_ref().unwrap().battle_rules;
    assert_eq!(rules.disabled_commands, 0);
    assert_eq!(
        [
            rules.attack_adjustment,
            rules.defense_adjustment,
            rules.intelligence_adjustment
        ],
        [-128, -127, -1]
    );
    assert!(!rules.coliseum);
}

#[test]
fn battle_flags_survive_field_transfer_and_saved_progress() {
    let data = data();
    let mut world = world();
    world.party.as_mut().unwrap().unison_gauge = 37;
    world.event_flags.insert(77);
    let events = runtime(world, &[(0, 8)], false);
    let (transferred, memory) = events.persistent_state().unwrap().into_world();
    assert_eq!(
        transferred.party.as_ref().unwrap().battle_rules.modifiers,
        8
    );
    assert_eq!(memory.read(0x100, Width::S32).unwrap(), 0);
    assert!(transferred.event_flags.contains(&77));
    let saved = events.save_progress().unwrap();
    let value = serde_json::to_value(&saved).unwrap();
    let restored: SavedProgress = serde_json::from_value(value.clone()).unwrap();
    let (restored, _) = restored.into_state(&data).unwrap().into_world();
    let party = restored.party.as_ref().unwrap();
    assert_eq!((party.battle_rules.modifiers, party.unison_gauge), (8, 37));
    assert_eq!(restored.random_state, events.world.random_state);
    assert_eq!(restored.gameplay_random, events.world.gameplay_random);
    assert_eq!(restored.event_flags, events.world.event_flags);

    for supported in [1, 9, 0x0150, 0x03ff] {
        let mut modified = value.clone();
        modified["party"]["battle_rules"]["modifiers"] = supported.into();
        let modified: SavedProgress = serde_json::from_value(modified).unwrap();
        let (world, _) = modified.into_state(&data).unwrap().into_world();
        assert_eq!(
            world.party.unwrap().initial_unison_full().unwrap(),
            supported & 8 != 0
        );
    }
    for unsupported in [0x0400, 0x8000, 65535] {
        let mut invalid = value.clone();
        invalid["party"]["battle_rules"]["modifiers"] = unsupported.into();
        let invalid: SavedProgress = serde_json::from_value(invalid).unwrap();
        let (world, _) = invalid.into_state(&data).unwrap().into_world();
        let party = world.party.unwrap();
        assert_eq!(
            party.battle_rules.modifiers,
            unsupported & crate::battle::Rules::SUPPORTED_MODIFIERS
        );
        assert_eq!(party.initial_unison_full().unwrap(), unsupported & 8 != 0);
    }
    for outside_u16 in [-1, 65536] {
        let mut invalid = value.clone();
        invalid["party"]["battle_rules"]["modifiers"] = outside_u16.into();
        assert!(serde_json::from_value::<SavedProgress>(invalid).is_err());
    }
}
