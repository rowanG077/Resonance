//! Original equipment-native behavior, independent of media and menu transactions.
use super::{Party, tests::data};
use crate::{EventRuntime, GameWorld, ResourceLibrary};
use resonance_content::session::SessionData;
use std::sync::Arc;
use symphonia_script::{NativeCall, Program, Width};

fn run(data: SessionData, world: GameWorld, calls: &[(NativeCall, i32, i32)]) -> EventRuntime {
    let mut code = Vec::new();
    for (index, &(call, character, value)) in calls.iter().enumerate() {
        for argument in [character, value] {
            code.extend([
                0x0200,
                argument as u16,
                (argument as u32 >> 16) as u16,
                0x3000,
                0x4000,
            ]);
        }
        code.push(0x2000 | call as u16);
        if call == NativeCall::GetEquippedItem {
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
    }
    code.push(0x20ff);
    let words = [4, 0, 0, 0].into_iter().chain(code);
    let bytes: Vec<_> = words.flat_map(u16::to_be_bytes).collect();
    EventRuntime::with_state(
        Arc::new(Program::decode(&bytes).unwrap()),
        Arc::new(ResourceLibrary {
            session_data: Some(Arc::new(data)),
            ..Default::default()
        }),
        world,
        Default::default(),
    )
    .unwrap()
}
fn world(data: &SessionData) -> GameWorld {
    GameWorld {
        party: Some(Party::new(data, Default::default()).unwrap()),
        controlled_actor: 1,
        ..Default::default()
    }
}

#[test]
fn equipment_queries_follow_script_slot_order_and_return_zero_for_invalid_selectors() {
    let data = data();
    let mut world = world(&data);
    world.party.as_mut().unwrap().members[0].equipment = [1, 2, 3, 2, 1, 3];
    let mut calls: Vec<_> = (0..6)
        .map(|slot| (NativeCall::GetEquippedItem, 1, slot))
        .collect();
    calls.extend([
        (NativeCall::GetEquippedItem, 999999, 5),
        (NativeCall::GetEquippedItem, 1, -1),
        (NativeCall::GetEquippedItem, 1, 6),
        (NativeCall::GetEquippedItem, 0, 0),
        (NativeCall::GetEquippedItem, 10, 0),
    ]);
    let events = run(data, world, &calls);
    for (index, expected) in [1, 2, 3, 2, 1, 3, 3, 0, 0, 0, 0].into_iter().enumerate() {
        assert_eq!(
            events
                .memory()
                .read(0x100 + index as u16 * 4, Width::S32)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn scripted_removal_clears_each_slot_when_inventory_is_full_and_ignores_invalid_slots() {
    let data = data();
    let mut world = world(&data);
    let party = world.party.as_mut().unwrap();
    party.members[0].equipment = [1; 6];
    party.items.insert(1, 20);
    let mut calls = vec![
        (NativeCall::UnequipItem, 1, -1),
        (NativeCall::UnequipItem, 1, 6),
        (NativeCall::UnequipItem, 0, 0),
        (NativeCall::UnequipItem, 10, 0),
    ];
    calls.push((NativeCall::GetEquippedItem, 1, 0));
    calls.extend((0..6).map(|slot| (NativeCall::UnequipItem, 999999, slot)));
    let events = run(data, world, &calls);
    let party = events.world.party.as_ref().unwrap();
    assert_eq!(events.memory().read(0x110, Width::S32).unwrap(), 1);
    assert_eq!(party.members[0].equipment, [0; 6]);
    assert_eq!(party.items.get(&1), Some(&20));
    assert!(!party.found_items.contains(&1));
    assert!(party.recent_items.is_empty());
}

#[test]
fn scripted_non_accessory_replacement_adds_the_requested_item_then_consumes_it() {
    for (kind, slot) in [(0, 0), (1, 1), (2, 2), (3, 5)] {
        for (old, count) in [(0, 1), (1, 1), (1, 20), (2, 20)] {
            let occupied = old != 0;
            let mut data = data();
            data.items[1].equipment_kind = Some(kind);
            data.items[2].equipment_kind = Some(kind);
            let mut world = world(&data);
            let party = world.party.as_mut().unwrap();
            party.members[0].equipment[slot] = old;
            party.items = [(1, 4), (2, count)].into();
            let events = run(data, world, &[(NativeCall::EquipItem, 999999, 2)]);
            let party = events.world.party.as_ref().unwrap();
            assert_eq!(party.members[0].equipment[slot], 2);
            assert_eq!(party.items.get(&1), Some(&4)); // The old item is never returned.
            let remaining = if occupied && count < 20 {
                count
            } else {
                count - 1
            };
            assert_eq!(party.items.get(&2).copied().unwrap_or(0), remaining);
            assert_eq!(party.found_items.contains(&2), occupied && count < 20);
            assert_eq!(
                party.recent_items.first().copied(),
                (occupied && count < 20).then_some(2)
            );
        }
    }
}

#[test]
fn scripted_accessories_fill_empty_slots_then_replace_the_second_even_at_capacity() {
    for equipment in [[0, 0], [1, 0], [1, 2]] {
        let mut data = data();
        for item in &mut data.items[1..] {
            item.equipment_kind = Some(4);
            item.stack_limit = 20;
        }
        let mut world = world(&data);
        let party = world.party.as_mut().unwrap();
        party.members[0].equipment[3..5].copy_from_slice(&equipment);
        party.items = [(2, 20), (3, 1)].into();
        let events = run(data, world, &[(NativeCall::EquipItem, 1, 3)]);
        let party = events.world.party.as_ref().unwrap();
        assert_eq!(
            &party.members[0].equipment[3..5],
            if equipment[0] == 0 { &[3, 0] } else { &[1, 3] }
        );
        assert!(!party.items.contains_key(&3));
        assert_eq!(party.items.get(&2), Some(&20));
    }
}
