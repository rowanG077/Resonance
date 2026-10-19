use resonance_events::{EventRuntime, GameWorld, ResourceLibrary};
use std::sync::Arc;
use symphonia_script::{NativeCall, Program};

#[test]
fn owner_observation_borrows_call_and_service_state_without_polling_or_resuming() {
    // A real legacy Call enters a real YieldCommand(2, 0); no VM seek or stack seed.
    let mut words = vec![4, 0, 0, 0, 0x2002, 3, 0x20ff];
    for value in [2_u16, 0] {
        words.extend([0x0200, value, 0, 0x3000, 0x4000]);
    }
    words.extend([0x2000 | NativeCall::YieldCommand as u16, 0x2003]);
    let code: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
    let program = Arc::new(Program::decode(&code).unwrap());
    let mut world = GameWorld::default();
    let dialogue = world
        .show_notice(
            resonance_events::dialogue::ResolvedMessage { tokens: vec![] },
            0,
        )
        .unwrap();
    let mut events = EventRuntime::with_state(
        program,
        Arc::new(ResourceLibrary::default()),
        world,
        Default::default(),
    )
    .unwrap();
    let read = |events: &EventRuntime| serde_json::to_value(events.observed_instances()).unwrap();
    let initial = read(&events);
    assert_eq!(initial[0]["pc"], 14);
    assert_eq!(initial[0]["legacy_return_stack"], serde_json::json!([2]));
    assert_eq!(initial[0]["legacy_program"], true);
    assert_eq!(initial[0]["wait"]["kind"], "service");
    assert_eq!(initial[0]["wait"]["condition"]["kind"], "complete");
    assert_eq!(
        initial[0]["wait"]["condition"]["operation"]["dialogue_slots"],
        serde_json::json!([0])
    );
    assert_eq!(initial[0]["wait"]["ready_at"], serde_json::Value::Null);
    let before_tick = events.tick();
    for _ in 0..3 {
        assert_eq!(read(&events), initial);
    }
    assert_eq!(events.tick(), before_tick);
    // Reading a now-satisfied operation must not poll the service or set its latch.
    dialogue.complete(None).unwrap();
    let completed = read(&events);
    assert_eq!(completed[0]["wait"]["ready_at"], serde_json::Value::Null);
    assert_eq!(
        completed[0]["wait"]["condition"]["operation"]["outcome"]["kind"],
        "completed"
    );
    events.step().unwrap();
    let latched = read(&events);
    assert!(latched[0]["wait"]["ready_at"].is_u64());
    assert_eq!(latched[0]["legacy_return_stack"], serde_json::json!([2]));
    assert_eq!(read(&events), latched);
    events.step().unwrap();
    assert_eq!(events.active_instances(), 0);
    assert_eq!(read(&events), serde_json::json!([]));
}
