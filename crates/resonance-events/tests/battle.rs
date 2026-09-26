use resonance_events::{Actor, ActorMotion, EventRuntime, GameWorld, ResourceLibrary, battle};
use std::sync::Arc;
use symphonia_script::{NativeCall, Program, Width};

fn native(words: &mut Vec<u16>, call: NativeCall, args: &[i32]) {
    for &arg in args {
        words.extend([
            0x0200,
            arg as u16,
            (arg as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    words.push(0x2000 | call as u16);
}

fn world() -> GameWorld {
    let mut world = GameWorld::default();
    // This bridge never reads or changes the roster; encounter preparation
    // validates playable members before activation.
    world.party = Some(
        serde_json::from_value(serde_json::json!({
            "members": [], "formation": [], "items": {}, "found_items": [],
            "recent_items": [], "gald": 500, "spent_gald": 0,
            "settings": {"battle_controls": [1,2,2,2]}
        }))
        .unwrap(),
    );
    world.input_enabled = true;
    world.tick = 12;
    let mut actor = Actor::new(1, [0.; 3]);
    actor.motion = Some(ActorMotion {
        target: [10., 0., 0.],
        speed: 1.,
    });
    world.actors.insert(1, actor);
    world
}

fn start(args: &[i32], world: GameWorld) -> anyhow::Result<EventRuntime> {
    let mut words = vec![4, 0, 0, 0];
    for bit in [40, 41] {
        native(&mut words, NativeCall::StartBattle, args);
        words.push(0x3000);
        native(&mut words, NativeCall::SetEventBit, &[bit]);
    }
    words.push(0x20ff);
    let program = Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )?;
    EventRuntime::with_state(
        Arc::new(program),
        Arc::new(ResourceLibrary::default()),
        world,
        Default::default(),
    )
}

fn arguments(defeat_allowed: i32) -> [i32; 12] {
    // The original consumes the last three arguments without reading them.
    [1, 13, defeat_allowed, 0, 0, 0, 0, 0, 0, -5, 1234, 99]
}

#[test]
fn battle_holds_the_field_after_request_transfer_and_resumes_each_caller_once() {
    let mut events = start(&arguments(0), world()).unwrap();
    let first = events.world.battle_request.take().unwrap();
    assert_eq!(
        first.setup,
        battle::Setup {
            encounter: 1,
            arena: 13,
            defeat: battle::DefeatPolicy::GameOver,
            music: None,
        }
    );
    for _ in 0..30 {
        assert!(events.battle_pending());
        assert!(!events.player_has_control());
        events.step().unwrap();
        assert_eq!(events.tick(), 12);
        assert_eq!(events.world.actors[&1].position, [0.; 3]);
        assert!(events.world.event_flags.is_empty());
        assert_eq!(events.world.party.as_ref().unwrap().gald, 500);
    }
    first.complete(battle::Outcome::Victory).unwrap();
    assert!(!events.battle_pending());
    assert!(
        !events.player_has_control(),
        "caller has not consumed its result"
    );
    assert!(first.complete(battle::Outcome::Victory).is_err());
    events.step().unwrap();
    assert_eq!(events.tick(), 13);
    assert_eq!(events.world.actors[&1].position, [1., 0., 0.]);
    assert_eq!(events.world.event_flags, [40].into());
    assert_eq!(events.memory().read(0x20, Width::S32).unwrap(), 2);
    assert_eq!(events.memory().read(0x24, Width::S32).unwrap(), 2);
    assert_eq!(events.memory().read(0x28, Width::S32).unwrap(), 0);
    let second = events.world.battle_request.take().unwrap();
    assert_ne!(first.id(), second.id());
    assert!(first.complete(battle::Outcome::Escaped).is_err());
    events.step().unwrap();
    assert_eq!(events.tick(), 13);
    second.complete(battle::Outcome::Escaped).unwrap();
    events.step().unwrap();
    assert!(events.player_has_control());
    assert_eq!(events.world.event_flags, [40, 41].into());
    assert_eq!(events.memory().read(0x24, Width::S32).unwrap(), 1);
}

#[test]
fn fatal_defeat_does_not_resume_and_retiring_field_invalidates_late_results() {
    let mut events = start(&arguments(2), world()).unwrap();
    let request = events.world.battle_request.take().unwrap();
    assert!(request.complete(battle::Outcome::Defeat).is_err());
    assert!(request.is_pending());
    events.step().unwrap();
    assert!(events.world.event_flags.is_empty());
    events.cancel();
    assert!(!request.is_pending());
    assert!(request.complete(battle::Outcome::Victory).is_err());
    assert!(!events.battle_pending());
    assert!(events.world.battle_request.is_none());
}

#[test]
fn allowed_defeat_and_dropped_field_have_distinct_completion_paths() {
    let mut events = start(&arguments(7), world()).unwrap();
    let request = events.world.battle_request.take().unwrap();
    assert_eq!(request.setup.defeat, battle::DefeatPolicy::ResumeEvent);
    request.complete(battle::Outcome::Defeat).unwrap();
    events.step().unwrap();
    assert_eq!(events.memory().read(0x24, Width::S32).unwrap(), 3);
    let next = events.world.battle_request.take().unwrap();
    drop(events);
    assert!(!next.is_pending());
    assert!(next.complete(battle::Outcome::Victory).is_err());
}

#[test]
fn invalid_setup_fails_before_publishing_a_request() {
    let mut no_party = world();
    no_party.party = None;
    assert!(start(&arguments(0), no_party).is_err());
    for (index, value) in [
        (0, -1),
        (1, -1),
        (3, -2),
        (4, 1),
        (5, 1),
        (6, 1),
        (7, 1),
        (8, 1),
    ] {
        let mut args = arguments(0);
        args[index] = value;
        assert!(start(&args, world()).is_err(), "argument {index}");
    }
}

#[test]
fn request_pass_keeps_foreground_dispatch_but_holds_later_background_events() {
    let mut main = Vec::new();
    native(&mut main, NativeCall::SpawnEvent, &[42]);
    main.push(0x3000);
    native(&mut main, NativeCall::YieldCommand, &[0, 1]);
    native(&mut main, NativeCall::StartBattle, &arguments(0));
    main.extend([0x3000, 0x20ff]);
    let mut background = Vec::new();
    native(&mut background, NativeCall::YieldCommand, &[0, 1]);
    native(&mut background, NativeCall::SetEventBit, &[70]);
    background.push(0x20ff);
    let mut foreground = Vec::new();
    native(&mut foreground, NativeCall::SetEventBit, &[71]);
    foreground.push(0x20ff);
    let mut words = vec![
        16,
        0,
        0,
        2,
        0,
        2,
        0,
        42,
        0,
        main.len() as u16,
        0,
        1,
        0,
        43,
        0,
        (main.len() + background.len()) as u16,
    ];
    words.extend(main);
    words.extend(background);
    words.extend(foreground);
    let program = Arc::new(
        Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    let mut events = EventRuntime::with_state(
        program,
        Arc::new(ResourceLibrary::default()),
        world(),
        Default::default(),
    )
    .unwrap();
    assert!(events.trigger(43, false).unwrap());
    events.step().unwrap();
    assert_eq!(events.tick(), 13);
    assert_eq!(events.world.event_flags, [71].into());
    let request = events.world.battle_request.take().unwrap();
    assert!(
        !events.trigger(43, false).unwrap(),
        "pending battle owns field input"
    );
    events.step().unwrap();
    assert_eq!(events.tick(), 13);
    request.complete(battle::Outcome::Victory).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.event_flags, [70, 71].into());
}
