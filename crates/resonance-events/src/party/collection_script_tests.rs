//! Collection/snapshot native ABI tests with no cooked assets or devices.
use super::{Party, tests::data};
use crate::{EventRuntime, GameWorld, ResourceLibrary};
use std::sync::Arc;
use symphonia_script::{NativeCall, Program, Width};

fn world() -> GameWorld {
    GameWorld {
        party: Some(Party::new(&data(), Default::default()).unwrap()),
        ..Default::default()
    }
}
fn run(world: GameWorld, calls: &[(NativeCall, &[i32])]) -> EventRuntime {
    let mut code = vec![0x1200, 0x20, 77, 0x3010, 0x3000];
    for (index, (call, arguments)) in calls.iter().enumerate() {
        for &argument in *arguments {
            code.extend([
                0x0200,
                argument as u16,
                (argument as u32 >> 16) as u16,
                0x3000,
                0x4000,
            ]);
        }
        code.push(0x2000 | *call as u16);
        if matches!(
            call,
            NativeCall::ConfigureFigurine | NativeCall::FindPartyMember
        ) {
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
    let bytes: Vec<_> = [4, 0, 0, 0]
        .into_iter()
        .chain(code)
        .flat_map(u16::to_be_bytes)
        .collect();
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

#[test]
fn figurine_operations_return_previous_ownership_and_unknown_operations_only_query() {
    use NativeCall::ConfigureFigurine as Figurine;
    let events = run(
        world(),
        &[
            (Figurine, &[0, 50]),
            (Figurine, &[0, 50]),
            (Figurine, &[2, 50]),
            (Figurine, &[-1, 50]),
            (Figurine, &[i32::MAX, 50]),
            (Figurine, &[1, 50]),
            (Figurine, &[2, 50]),
            (Figurine, &[0, 328]),
            (Figurine, &[0, i32::MAX]),
        ],
    );
    for (index, expected) in [0, 1, 1, 1, 1, 1, 0, 0, 0].into_iter().enumerate() {
        assert_eq!(
            events
                .memory()
                .read(0x100 + index as u16 * 4, Width::S32)
                .unwrap(),
            expected
        );
    }
    let party = events.world.party.as_ref().unwrap();
    assert!(party.figurines.is_empty() && party.figurine_reserved_flags.is_empty());
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 0);
}

#[test]
fn reserved_figurine_bank_bits_survive_saves_without_becoming_catalogue_entries() {
    use NativeCall::ConfigureFigurine as Figurine;
    let events = run(
        world(),
        &[
            (Figurine, &[0, 287]),
            (Figurine, &[0, 288]),
            (Figurine, &[0, 327]),
            (Figurine, &[2, 288]),
            (Figurine, &[2, 327]),
        ],
    );
    assert_eq!(events.memory().read(0x10c, Width::S32).unwrap(), 1);
    assert_eq!(events.memory().read(0x110, Width::S32).unwrap(), 1);
    let party = events.world.party.as_ref().unwrap();
    assert_eq!(party.figurines, [287].into());
    assert_eq!(party.figurine_reserved_flags, [288, 327].into());
    let saved = serde_json::to_value(party).unwrap();
    let restored: Party = serde_json::from_value(saved.clone()).unwrap();
    restored.validate(&data()).unwrap();
    assert_eq!(restored.figurine_reserved_flags, [288, 327].into());
    let mut old = saved;
    old.as_object_mut()
        .unwrap()
        .remove("figurine_reserved_flags");
    assert!(
        serde_json::from_value::<Party>(old)
            .unwrap()
            .figurine_reserved_flags
            .is_empty()
    );
    for invalid in [287, 328] {
        let mut invalid_party = restored.clone();
        invalid_party.figurine_reserved_flags.insert(invalid);
        assert!(
            invalid_party
                .validate(&data())
                .unwrap_err()
                .to_string()
                .contains("reserved figurine")
        );
    }
    for carry in [true, false] {
        let mut next = restored.clone();
        next.new_game_plus.purchases = Some(if carry {
            [resonance_content::grade::Benefit::Figurines].into()
        } else {
            Default::default()
        });
        next.start_new_game_plus(&data()).unwrap();
        assert_eq!(
            next.figurine_reserved_flags,
            if carry {
                [288, 327].into()
            } else {
                Default::default()
            }
        );
    }
    let cleared = run(
        events.world,
        &[(Figurine, &[1, 288]), (Figurine, &[1, 327])],
    );
    assert!(
        cleared
            .world
            .party
            .as_ref()
            .unwrap()
            .figurine_reserved_flags
            .is_empty()
    );
    assert_eq!(
        cleared.world.party.as_ref().unwrap().figurines,
        [287].into()
    );
}

#[test]
fn party_snapshot_capture_and_clear_never_restore_the_live_formation() {
    use NativeCall::{RemovePartyMember as Remove, SnapshotParty as Snapshot};
    let mut world = world();
    world.party.as_mut().unwrap().formation = (1..=8).collect();
    let captured = run(
        world,
        &[
            (Snapshot, &[0]),
            (Remove, &[2]),
            (Snapshot, &[2]),
            (Snapshot, &[-1]),
        ],
    );
    let party = captured.world.party.as_ref().unwrap();
    assert_eq!(party.travel.saved_formation, (1..=8).collect::<Vec<_>>());
    assert_eq!(party.formation, [1, 3, 4, 5, 6, 7, 8]);
    let restored: Party = serde_json::from_slice(&serde_json::to_vec(party).unwrap()).unwrap();
    assert_eq!(
        restored.travel.saved_formation,
        party.travel.saved_formation
    );
    let cleared = run(captured.world, &[(Snapshot, &[1])]);
    let party = cleared.world.party.as_ref().unwrap();
    assert!(party.travel.saved_formation.is_empty());
    assert_eq!(party.formation, [1, 3, 4, 5, 6, 7, 8]);
    assert_eq!(cleared.memory().read(0x20, Width::S32).unwrap(), 77);
}

// Captured through original DOL Native27 (80054100) and its unmodified
// formation search helper (80080B48). Original-binary evidence pins both bodies.
#[test]
fn party_queries_match_original_byte_search_and_encoded_slot_results() {
    let queries = [
        0,
        1,
        2,
        9,
        10,
        64,
        255,
        256,
        257,
        258,
        265,
        320,
        512,
        65535,
        65536,
        65537,
        65538,
        65539,
        65543,
        65544,
        131072,
        131079,
        2147483647,
        -65536,
        -65535,
        -65529,
        -65528,
        -1,
        -2147483648,
    ];
    let cases = [
        (
            &[1, 2, 3, 4][..],
            [
                0, 1, 2, 0, 0, 0, 0, 5, 1, 2, 0, 0, 5, 0, 1, 2, 3, 4, 0, 0, 1, 0, 0, 1, 2, 0, 0, 0,
                1,
            ],
        ),
        (
            &[2, 1, 9][..],
            [
                0, 2, 1, 3, 0, 0, 0, 4, 2, 1, 3, 0, 4, 0, 2, 1, 9, 0, 0, 0, 2, 0, 0, 2, 1, 0, 0, 0,
                2,
            ],
        ),
        (
            &[9][..],
            [
                0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 1, 0, 2, 0, 9, 0, 0, 0, 0, 0, 9, 0, 0, 9, 0, 0, 0, 0,
                9,
            ],
        ),
        (
            &[9, 8, 7, 6, 5, 4, 3, 2][..],
            [
                0, 0, 8, 1, 0, 0, 0, 0, 0, 8, 1, 0, 0, 0, 9, 8, 7, 6, 2, 0, 9, 2, 0, 9, 8, 2, 0, 0,
                9,
            ],
        ),
    ];
    for (formation, expected) in cases {
        let mut world = world();
        let party = world.party.as_mut().unwrap();
        party.formation = formation.to_vec();
        party.field_leader = formation[0];
        let before = serde_json::to_value(party).unwrap();
        let mut calls = queries
            .iter()
            .map(|query| (NativeCall::FindPartyMember, std::slice::from_ref(query)))
            .collect::<Vec<_>>();
        calls.push((NativeCall::SetEventBit, &[123]));
        let events = run(world, &calls);
        for (index, (&query, expected)) in queries.iter().zip(expected).enumerate() {
            assert_eq!(
                events
                    .memory()
                    .read(0x100 + index as u16 * 4, Width::S32)
                    .unwrap(),
                expected,
                "formation {formation:?}, query {query}",
            );
        }
        assert!(
            events.world.event_flags.contains(&123),
            "queries must not abort the continuation"
        );
        assert_eq!(
            serde_json::to_value(events.world.party.as_ref().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            events.memory().read(0x20, Width::S32).unwrap(),
            i32::from(formation[0])
        );
    }
}
