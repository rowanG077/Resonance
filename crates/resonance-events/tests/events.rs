use resonance_events::*;
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;

fn arg(code: &mut Vec<u16>, n: i32) {
    code.extend([0x0200, n as u16, (n as u32 >> 16) as u16, 0x3000, 0x4000]);
}
fn native(code: &mut Vec<u16>, op: u8, a: &[i32]) {
    for &a in a {
        arg(code, a);
    }
    code.push(0x2000 | u16::from(op));
}
fn program(main: &[u16], child: &[u16]) -> Arc<Program> {
    program_kind(main, child, 2)
}
fn program_kind(main: &[u16], child: &[u16], kind: u16) -> Arc<Program> {
    let mut words = vec![10, 0, 0, 1, 0, kind, 0, 42, 0, main.len() as u16];
    words.extend(main);
    words.extend(child);
    Arc::new(
        Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    )
}
fn runtime(program: Arc<Program>, resources: ResourceLibrary, world: GameWorld) -> EventRuntime {
    EventRuntime::with_state(program, Arc::new(resources), world, Default::default()).unwrap()
}
#[test]
fn line_events_use_their_registry_and_finish_once() {
    for confirmed in [false, true] {
        let mut child = Vec::new();
        native(&mut child, 0x60, &[]);
        native(&mut child, 0x64, &[0, 3]);
        child.push(0x20ff);
        let mut world = GameWorld::default();
        world.input_enabled = true;
        let mut events = runtime(
            program_kind(&[0x20ff], &child, if confirmed { 2 } else { 1 }),
            Default::default(),
            world,
        );
        assert!(!events.trigger(42, !confirmed).unwrap());
        assert!(events.trigger(42, confirmed).unwrap());
        assert!(!events.trigger(42, confirmed).unwrap());
        assert!(events.world.input_enabled);
        events.step().unwrap();
        assert!(!events.world.input_enabled);
        for _ in 0..4 {
            events.step().unwrap();
        }
        assert!(events.world.input_enabled);
        assert_eq!(events.active_instances(), 0);
    }
}
#[test]
fn a_guarded_line_event_does_not_take_control_or_restart_walking() {
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    let mut actor = Actor::new(1, [0.; 3]);
    actor.motion = Some(ActorMotion {
        target: [100., 0., 0.],
        speed: 4.,
    });
    world.actors.insert(1, actor);
    let mut events = runtime(
        program_kind(&[0x20ff], &[0x20ff], 1),
        Default::default(),
        world,
    );
    assert!(events.trigger(42, false).unwrap());
    assert!(events.world.input_enabled);
    assert!(events.world.actors[&1].motion.is_some());
    events.step().unwrap();
    assert_eq!(events.world.actors[&1].position, [4., 0., 0.]);
    assert!(events.world.input_enabled);
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn locomotion_matches_native_speed_and_keeps_its_phase_across_rate_changes() {
    let mut resources = ResourceLibrary::default();
    resources.models.insert(
        1,
        ModelResource {
            clips: [12, 36, 40]
                .into_iter()
                .map(|slot| {
                    (
                        slot,
                        AnimationClip {
                            duration_ticks: 80,
                            attachments: Default::default(),
                        },
                    )
                })
                .collect(),
            ..Default::default()
        },
    );
    let mut world = GameWorld::default();
    world.controlled_actor = 1;
    world.input_enabled = true;
    let mut actor = Actor::new(1, [0.; 3]);
    actor.motion = Some(ActorMotion {
        target: [10000., 0., 0.],
        speed: 4.,
    });
    world.actors.insert(1, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
    for age in 0u32..125 {
        events.step().unwrap();
        let a = events.world.actors[&1].animation.as_ref().unwrap();
        // Dolphin walking checkpoint: 40 native frames, speed 1/frame,
        // blend duration 2. Includes three complete gait loops.
        assert_eq!(a.slot, 36);
        assert_eq!(a.start_tick, 1);
        assert_eq!(a.rate, 2.);
        assert_eq!(a.blend_ticks, 2);
        let elapsed = age.saturating_sub(1) as f32 * 2.;
        let expected = if elapsed > 80. {
            (elapsed - 1.).rem_euclid(80.) + 1.
        } else {
            elapsed
        };
        assert_eq!(a.sample(events.tick(), 0, 80.), expected);
    }
    let tick = events.tick();
    let previous = events.world.actors[&1]
        .animation
        .as_ref()
        .unwrap()
        .sample(tick + 1, 0, 80.);
    events
        .world
        .actors
        .get_mut(&1)
        .unwrap()
        .motion
        .as_mut()
        .unwrap()
        .speed = 2.;
    events.step().unwrap();
    let a = events.world.actors[&1].animation.as_ref().unwrap();
    assert_eq!(a.start_tick, 1);
    assert_eq!(a.rate, 1.);
    assert_eq!(a.sample(events.tick(), 0, 80.), previous);
    events
        .world
        .actors
        .get_mut(&1)
        .unwrap()
        .motion
        .as_mut()
        .unwrap()
        .speed = 8.;
    events.step().unwrap();
    let a = events.world.actors[&1].animation.as_ref().unwrap();
    assert_eq!((a.slot, a.rate, a.blend_ticks), (40, 0.8, 2));
}

#[test]
fn walking_settles_to_whole_degree_facing_without_quantizing_the_path() {
    // Raine's question checkpoint observes heading 181 even though the
    // displacement points at 181.956 degrees. Position remains continuous.
    let start = [222.20871, 12.970599, 0.];
    let target = [210., 370., 0.];
    let mut actor = Actor::new(4, start);
    actor.face(180.);
    actor.motion = Some(ActorMotion {
        target,
        speed: 1.875,
    });
    let mut world = GameWorld::default();
    world.actors.insert(4, actor);
    let mut events = runtime(program(&[0x20ff], &[0x20ff]), Default::default(), world);
    for _ in 0..31 {
        events.step().unwrap();
    }
    let actor = &events.world.actors[&4];
    assert_eq!(actor.heading, 181.);
    assert_eq!(actor.target_heading, 181.);
    let traveled = (actor.position[0] - start[0]).hypot(actor.position[1] - start[1]);
    assert!((traveled - 31. * 1.875).abs() < 0.0001);
    let cross = (actor.position[0] - start[0]) * (target[1] - start[1])
        - (actor.position[1] - start[1]) * (target[0] - start[0]);
    assert!(
        cross.abs() < 0.05,
        "facing must not change the movement vector"
    );
}

#[test]
fn colette_oracle_turn_waits_before_opening_and_returns_in_34_updates() {
    // Recorded Colette turns: 180 → 351 and 351 → 180 each take 34 updates.
    // The final six-degree step enters the target’s angular sector.
    let mut code = Vec::new();
    native(&mut code, 0x14, &[2, 351]);
    native(&mut code, 0x0c, &[0, 64, -1, 2, 0, 0, 0, 0]);
    native(&mut code, 0x64, &[2, 0]);
    native(&mut code, 0x14, &[2, 180]);
    code.push(0x20ff);
    let mut actor = Actor::new(2, [0.; 3]);
    actor.face(180.);
    let mut world = GameWorld::default();
    world.actors.insert(2, actor);
    let resources = ResourceLibrary {
        messages: vec![symphonia_script::message::Message { tokens: vec![] }],
        ..Default::default()
    };
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    for age in 1..=34 {
        events.step().unwrap();
        let expected = if age == 34 {
            351.
        } else {
            180. + age as f32 * 5.
        };
        assert_eq!(events.world.actors[&2].heading, expected);
        assert_eq!(events.world.dialogue[&0].opening_actor, Some(2));
    }
    events.step().unwrap();
    assert_eq!(events.world.dialogue[&0].opening_actor, None);
    // Dismissing the page changes only the target; it cannot snap the actor.
    events.world.dialogue[&0].operation.complete(None).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.actors[&2].target_heading, 351.);
    events.step().unwrap(); // Completed service resumes on the next dispatch.
    assert_eq!(events.world.actors[&2].heading, 351.);
    assert_eq!(events.world.actors[&2].target_heading, 180.);
    for age in 1..=34 {
        events.step().unwrap();
        let expected = if age == 34 {
            180.
        } else {
            351. - age as f32 * 5.
        };
        assert_eq!(events.world.actors[&2].heading, expected);
    }
}
#[test]
fn event_control_selects_the_leaders_idle_without_overriding_other_actors() {
    for has_event_idle in [false, true] {
        let mut clips = BTreeMap::new();
        for slot in [12, 60].into_iter().chain(has_event_idle.then_some(116)) {
            clips.insert(
                slot,
                AnimationClip {
                    duration_ticks: 60,
                    attachments: Default::default(),
                },
            );
        }
        let mut resources = ResourceLibrary::default();
        resources.models.insert(
            3,
            ModelResource {
                clips,
                ..Default::default()
            },
        );
        let mut world = GameWorld::default();
        world.controlled_actor = 3;
        world.input_enabled = true;
        world.actors.insert(3, Actor::new(3, [0.; 3]));
        let mut bystander = Actor::new(3, [20., 0., 0.]);
        bystander.idle_animation = 60;
        world.actors.insert(42, bystander);
        let mut events = runtime(program(&[0x20ff], &[0x20ff]), resources, world);
        events.step().unwrap();
        assert_eq!(events.world.actors[&3].animation.as_ref().unwrap().slot, 12);
        events.world.input_enabled = false;
        events.step().unwrap();
        assert_eq!(
            events.world.actors[&3].animation.as_ref().unwrap().slot,
            if has_event_idle { 116 } else { 12 }
        );
        assert_eq!(
            events.world.actors[&42].animation.as_ref().unwrap().slot,
            60
        );
        // Authored event animation always wins over automatic idle selection.
        let actor = events.world.actors.get_mut(&3).unwrap();
        actor.scripted_animation = true;
        actor.animation.as_mut().unwrap().slot = 60;
        events.step().unwrap();
        assert_eq!(events.world.actors[&3].animation.as_ref().unwrap().slot, 60);
        events.world.actors.get_mut(&3).unwrap().scripted_animation = false;
        events.world.input_enabled = true;
        events.step().unwrap();
        assert_eq!(events.world.actors[&3].animation.as_ref().unwrap().slot, 12);
    }
}

#[test]
fn eraser_rate_preserves_the_scripted_impact_cue() {
    // Play slot 80 at rate 50, wait 40 updates, then emit the impact cue.
    // The rate advances a quarter source frame per update; immediate binding
    // adds one sample, so impact occurs at frame 10.25.
    let mut code = Vec::new();
    native(&mut code, 0x9b, &[100, -1, 80, 1, 8]);
    native(&mut code, 0x9f, &[100, 0, 50]);
    code.push(0x3000);
    native(&mut code, 0x64, &[0, 40]);
    native(&mut code, 0xe0, &[236, 0, 255, 255]);
    native(
        &mut code,
        0xd3,
        &[0, 180, 80, -635, 140, 0, 0, 0, 0, 10, 75, -1, 0, 0],
    );
    code.extend([0x3000, 0x20ff]);
    let mut resources = ResourceLibrary::default();
    resources.models.insert(
        68196,
        ModelResource {
            clips: [(
                80,
                AnimationClip {
                    duration_ticks: 70,
                    attachments: Default::default(),
                },
            )]
            .into(),
            ..Default::default()
        },
    );
    let mut world = GameWorld::default();
    world.actors.insert(100, Actor::new(68196, [0.; 3]));
    let mut events = runtime(program(&code, &[0x20ff]), resources, world);
    for _ in 0..39 {
        events.step().unwrap();
    }
    assert!(events.world.billboards.is_empty());
    assert!(events.world.audio_commands.is_empty());
    events.step().unwrap();
    let animation = events.world.actors[&100].animation.as_ref().unwrap();
    assert_eq!(animation.sample(events.tick(), 0, 70.), 20.5);
    assert_eq!(
        events.world.billboards.values().next().unwrap().born,
        events.tick()
    );
    assert!(matches!(
        events.world.audio_commands.as_slice(),
        [AudioCommand::Sound { id: 236, .. }]
    ));
}
#[test]
fn script_light_updates_preserve_order_and_default_selector_alias() {
    let mut code = Vec::new();
    native(&mut code, 0x4f, &[-1, 7, 128, 128, 128]);
    native(&mut code, 0x4f, &[0, 0, 240, 240, 240]);
    native(&mut code, 0x4f, &[1, 0, 256, 256, 256]);
    code.push(0x20ff);
    let events = EventRuntime::new(
        program(&code, &[0x20ff]),
        Arc::new(ResourceLibrary::default()),
    )
    .unwrap();
    assert_eq!(events.world.character_lights[&0].bright, [60; 3]);
    assert_eq!(events.world.character_lights[&0].shade, [45; 3]);
    assert_eq!(events.world.character_lights[&1].bright, [64; 3]);
    let mut light = events.world.character_lights[&0].clone();
    let mut target = events.world.character_lights[&1].clone();
    target.color_step = 2;
    light.approach(&target);
    assert_eq!(light.bright, [62; 3]);
    light.approach(&target);
    assert_eq!(light.bright, [64; 3]);
}
#[test]
fn general_shims_run_non_title_events_with_shared_state_and_ordered_waits() {
    let mut main = Vec::new();
    native(&mut main, 0x39, &[42]);
    main.push(0x3000);
    native(&mut main, 0x64, &[0, 2]);
    main.push(0x20ff);
    let mut child = Vec::new();
    native(&mut child, 0xb9, &[77, 100, 200, 300, 0, 1234, 0, 0]);
    native(&mut child, 0x1d, &[77, 2, 450]);
    // Preserve the previous Y returned by set_actor_property in shared data.
    child.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
    native(&mut child, 0x9b, &[77, -1, 12, 0, 1]);
    native(&mut child, 0x64, &[0, 1]);
    native(&mut child, 0xad, &[555, 0, 0]);
    child.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(1234, (ResourceKind::Model, 99));
    resources.bindings.insert(555, (ResourceKind::Camera, 22));
    resources.models.insert(
        99,
        ModelResource {
            names: vec![],
            hidden_nodes: Default::default(),
            clips: BTreeMap::from([(
                12,
                AnimationClip {
                    duration_ticks: 30,
                    attachments: BTreeMap::new(),
                },
            )]),
        },
    );
    let mut events = EventRuntime::new(program(&main, &child), Arc::new(resources)).unwrap();
    assert_eq!(events.world.actors[&77].position, [100., 450., 300.]);
    assert_eq!(
        events
            .memory()
            .read(0x100, symphonia_script::Width::S32)
            .unwrap(),
        200
    );
    assert!(events.world.camera.is_none());
    assert_eq!(events.active_instances(), 2);
    events.step().unwrap();
    assert_eq!(events.world.camera.as_ref().unwrap().resource, 22);
    assert_eq!(events.world.camera.as_ref().unwrap().start_tick, 1);
    assert_eq!(events.active_instances(), 1);
    events.step().unwrap();
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn actor_attachments_start_hidden_and_script_toggles_are_instance_local() {
    let mut main = Vec::new();
    native(&mut main, 0x10, &[7, 0, 0, 0, 0, 99, 0, 0]);
    native(&mut main, 0x10, &[8, 0, 0, 0, 0, 99, 0, 0]);
    native(&mut main, 0x64, &[0, 1]);
    native(&mut main, 0xc5, &[7, 1, 1]);
    native(&mut main, 0x64, &[0, 1]);
    native(&mut main, 0xc5, &[7, 0, 0]);
    main.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(99, (ResourceKind::Model, 99));
    resources.models.insert(
        99,
        ModelResource {
            names: vec!["body".into(), "optional-accessory".into()],
            hidden_nodes: [1].into(),
            ..Default::default()
        },
    );
    let mut events = EventRuntime::new(program(&main, &[0x20ff]), Arc::new(resources)).unwrap();
    assert_eq!(events.world.actors[&7].appearance.hidden_nodes, [1].into());
    events.step().unwrap();
    assert!(events.world.actors[&7].appearance.hidden_nodes.is_empty());
    assert_eq!(events.world.actors[&8].appearance.hidden_nodes, [1].into());
    events.step().unwrap();
    assert_eq!(events.world.actors[&7].appearance.hidden_nodes, [0].into());
    assert_eq!(events.world.actors[&8].appearance.hidden_nodes, [1].into());
}

#[test]
fn unknown_native_stops_with_event_and_pc_context() {
    let error = EventRuntime::new(
        program(&[0x20f0, 0x20ff], &[0x20ff]),
        Arc::new(ResourceLibrary::default()),
    )
    .err()
    .unwrap();
    let text = format!("{error:#}");
    assert!(text.contains("handle 1"));
    assert!(text.contains("PC 0x0000"));
    assert!(text.contains("0xf0"));
}

#[test]
fn depth_property_returns_the_previous_bit_and_changes_presentation_state() {
    let mut code = Vec::new();
    native(&mut code, 0xb9, &[77, 0, 0, 0, 0, 1234, 0, 0]);
    native(&mut code, 0x1d, &[77, 46, 3]); // Low bit enables read-only depth.
    code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
    native(&mut code, 0x64, &[0, 1]);
    native(&mut code, 0x1d, &[77, 46, 2]); // Clear low bit restores writes.
    code.extend([0x3000, 0x1200, 0x104, 0x1200, 0x20, 0x3010, 0x3000, 0x20ff]);
    let mut resources = ResourceLibrary::default();
    resources.bindings.insert(1234, (ResourceKind::Model, 99));
    let mut events = EventRuntime::new(program(&code, &[0x20ff]), Arc::new(resources)).unwrap();
    assert!(!events.world.actors[&77].depth_write);
    assert_eq!(
        events
            .memory()
            .read(0x100, symphonia_script::Width::S32)
            .unwrap(),
        0
    );
    events.step().unwrap();
    assert!(events.world.actors[&77].depth_write);
    assert_eq!(
        events
            .memory()
            .read(0x104, symphonia_script::Width::S32)
            .unwrap(),
        1
    );
}

#[test]
fn runtime_cannot_continue_after_a_failed_update() {
    let mut code = Vec::new();
    native(&mut code, 0x64, &[0, 1]);
    code.extend([0x20f0, 0x20ff]);
    let mut events = EventRuntime::new(
        program(&code, &[0x20ff]),
        Arc::new(ResourceLibrary::default()),
    )
    .unwrap();
    let error = format!("{:#}", events.step().unwrap_err());
    for context in ["handle 1", "PC", "0xf0"] {
        assert!(error.contains(context), "{error}");
    }
    let tick = events.tick();
    assert!(events.step().unwrap_err().to_string().contains("stopped"));
    assert_eq!(events.tick(), tick);
}

#[test]
fn dialogue_completion_resumes_only_its_caller_and_cannot_fire_twice() {
    use symphonia_script::message::{Message, Token};
    let mut main = Vec::new();
    native(&mut main, 0x39, &[42]);
    main.push(0x3000);
    native(&mut main, 0x0c, &[0, 4, -2, 4, 0, 0, 0, 1]);
    native(&mut main, 0x64, &[2, 0]);
    native(&mut main, 0x46, &[0, 7]);
    main.push(0x20ff);
    let mut child = Vec::new();
    native(&mut child, 0x64, &[0, 1]);
    native(&mut child, 0x46, &[1, 9]);
    child.push(0x20ff);
    let resources = ResourceLibrary {
        messages: vec![
            Message { tokens: vec![] },
            Message {
                tokens: vec![Token::Text {
                    text: "Wake up!".into(),
                }],
            },
        ],
        ..Default::default()
    };
    let mut events = EventRuntime::new(program(&main, &child), Arc::new(resources)).unwrap();
    let dialogue = events.world.dialogue[&0].operation.clone();
    dialogue.advance(8).unwrap(); // Text is ready; dismissal is still pending.
    for _ in 0..10 {
        events.step().unwrap();
    }
    assert_eq!(events.world.render_settings.get(&1), Some(&9));
    assert!(!events.world.render_settings.contains_key(&0));
    dialogue.complete(None).unwrap();
    events.step().unwrap();
    assert!(!events.world.render_settings.contains_key(&0));
    events.step().unwrap();
    assert_eq!(events.world.render_settings.get(&0), Some(&7));
    assert_eq!(events.active_instances(), 0);
    assert!(dialogue.complete(None).is_err());
}

#[test]
fn choices_return_the_selected_line_and_completion_reason_once() {
    use resonance_events::dialogue::ChoiceExit;
    use symphonia_script::{
        Width,
        message::{Message, Token},
    };
    for (reason, flags, expected_reason) in [
        (ChoiceExit::Confirm, 0x104, 0),
        (ChoiceExit::Cancel, 4, 1),
        (ChoiceExit::Timeout, 0x104, -1),
    ] {
        let mut code = Vec::new();
        native(&mut code, 0x0c, &[1, 0, -2, 7, 0, 0, 0, 1]);
        // Select lines 3..5, with line 4 initially selected. First line is
        // deliberately not 1, and therefore cannot be mistaken for a default.
        native(&mut code, 0x66, &[1, 3, 5, 30, flags]);
        code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, 0x46, &[0, 42]);
        code.push(0x20ff);
        let resources = ResourceLibrary {
            messages: vec![
                Message { tokens: vec![] },
                Message {
                    tokens: vec![Token::Text {
                        text: "Heading\nDescription\nOne\nTwo\nThree".into(),
                    }],
                },
            ],
            ..Default::default()
        };
        let mut events = EventRuntime::new(program(&code, &[0x20ff]), Arc::new(resources)).unwrap();
        assert_eq!(events.world.choices[&1].selected_line, 3);
        for _ in 0..4 {
            events.step().unwrap();
        }
        assert!(events.world.render_settings.is_empty());
        let choice = events.world.choices.get_mut(&1).unwrap();
        choice.selected_line = 4;
        let callback = choice.clone();
        choice.finish(reason).unwrap();
        events.step().unwrap();
        assert_eq!(events.memory().read(0x100, Width::S32).unwrap(), 5);
        assert_eq!(
            events.memory().read(0x24, Width::S32).unwrap(),
            expected_reason
        );
        assert_eq!(events.world.render_settings[&0], 42);
        assert_eq!(events.active_instances(), 0);
        assert!(callback.finish(reason).is_err());
        events.cancel();
        assert!(events.world.choices.is_empty());
    }
}

#[test]
fn replacing_a_choice_window_invalidates_its_waiting_callback() {
    use symphonia_script::message::Message;
    let mut main = Vec::new();
    native(&mut main, 0x39, &[42]);
    main.push(0x3000);
    native(&mut main, 0x0c, &[1, 0, -2, 7, 0, 0, 0, 0]);
    native(&mut main, 0x66, &[1, 1, 1, 0, 0x100]);
    main.push(0x20ff);
    let mut child = Vec::new();
    native(&mut child, 0x64, &[0, 1]);
    native(&mut child, 0x0c, &[1, 0, -2, 7, 0, 0, 0, 0]);
    child.push(0x20ff);
    let resources = ResourceLibrary {
        messages: vec![Message { tokens: vec![] }],
        ..Default::default()
    };
    let mut events = EventRuntime::new(program(&main, &child), Arc::new(resources)).unwrap();
    let old = events.world.choices[&1].clone();
    events.step().unwrap();
    assert!(
        old.finish(resonance_events::dialogue::ChoiceExit::Confirm)
            .is_err()
    );
    assert!(format!("{:#}", events.step().unwrap_err()).contains("cancelled"));
}

#[test]
fn external_media_wait_15_also_waits_for_dialogue_voice() {
    for stop_early in [false, true] {
        let mut main = Vec::new();
        native(&mut main, 0x64, &[0, 1]);
        native(&mut main, 0x64, &[14, 0]);
        native(&mut main, 0x46, &[0, 1]);
        native(&mut main, 0x64, &[15, 0]);
        native(&mut main, 0x46, &[1, 2]);
        main.push(0x20ff);
        let mut events = EventRuntime::new(
            program(&main, &[0x20ff]),
            Arc::new(ResourceLibrary::default()),
        )
        .unwrap();
        events.world.voice = Some(resonance_events::VoicePlayback {
            resource: 655379,
            end_tick: 10,
        });
        for _ in 0..if stop_early { 4 } else { 9 } {
            events.step().unwrap();
        }
        assert_eq!(
            events.world.render_settings.len(),
            1,
            "decoded voice is ready, but its end wait must remain blocked"
        );
        if stop_early {
            events.world.voice = None;
        }
        events.step().unwrap();
        assert_eq!(events.world.render_settings.len(), 1);
        events.step().unwrap();
        assert_eq!(events.world.render_settings.len(), 2);
    }
}

#[test]
fn satisfied_service_waits_preserve_separate_resume_updates() {
    let mut main = Vec::new();
    native(&mut main, 0x64, &[15, 0]);
    native(&mut main, 0x46, &[0, 1]);
    native(&mut main, 0x64, &[15, 0]);
    native(&mut main, 0x46, &[1, 2]);
    main.push(0x20ff);
    let mut events = EventRuntime::new(
        program(&main, &[0x20ff]),
        Arc::new(ResourceLibrary::default()),
    )
    .unwrap();
    assert!(events.world.render_settings.is_empty());
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 2);
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn movie_waits_follow_decoding_presentation_and_completion_not_elapsed_ticks() {
    let mut main = Vec::new();
    native(&mut main, 0x56, &[8]);
    native(&mut main, 0x64, &[14, 0]);
    native(&mut main, 0x46, &[0, 1]);
    native(&mut main, 0x64, &[19, 12]);
    native(&mut main, 0x46, &[1, 2]);
    native(&mut main, 0x64, &[15, 0]);
    native(&mut main, 0x46, &[2, 3]);
    main.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.movies.insert(8);
    let mut events = EventRuntime::new(program(&main, &[0x20ff]), Arc::new(resources)).unwrap();
    let movie = events.world.movie.as_ref().unwrap().operation.clone();
    for _ in 0..20 {
        events.step().unwrap();
    }
    assert!(events.world.render_settings.is_empty());
    movie.advance(0).unwrap();
    events.step().unwrap();
    assert!(events.world.render_settings.is_empty());
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    movie.advance(11).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    movie.advance(12).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 1);
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 2);
    movie.complete(None).unwrap();
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 2);
    events.step().unwrap();
    assert_eq!(events.world.render_settings.len(), 3);
    assert_eq!(events.active_instances(), 0);
}

#[test]
fn cancelling_a_scene_stops_its_scripts_and_invalidates_movie_callbacks() {
    let mut main = Vec::new();
    native(&mut main, 0x54, &[8]);
    native(&mut main, 0x64, &[15, 0]);
    native(&mut main, 0x46, &[0, 1]);
    main.push(0x20ff);
    let mut resources = ResourceLibrary::default();
    resources.movies.insert(8);
    let mut events = EventRuntime::new(program(&main, &[0x20ff]), Arc::new(resources)).unwrap();
    let callback = events.world.movie.as_ref().unwrap().operation.clone();
    events.cancel();
    assert!(callback.complete(None).is_err());
    events.step().unwrap();
    assert_eq!(events.active_instances(), 0);
    assert!(events.world.render_settings.is_empty());
}

#[test]
fn changing_fields_moves_globals_but_retires_locals_actors_and_callbacks() {
    use symphonia_script::Width;
    use symphonia_script_vm::Memory;
    let mut main = Vec::new();
    native(&mut main, 0x40, &[340, -719, -371, 0, 0]);
    native(&mut main, 0x46, &[0, 99]);
    main.push(0x20ff);
    let resources = ResourceLibrary {
        fields: [340].into(),
        ..Default::default()
    };
    let mut memory = Memory::default();
    for offset in [0x40, 0x3fc, 0x400, 0x800] {
        memory.write(offset, Width::S32, 123).unwrap();
    }
    let mut world = GameWorld::default();
    world.tick = 47;
    world.random_state = 0x12345678;
    world.event_flags.insert(27);
    world.actors.insert(1, Actor::new(1, [1., 2., 3.]));
    let mut events = EventRuntime::with_state(
        program(&main, &[0x20ff]),
        Arc::new(resources),
        world,
        memory,
    )
    .unwrap();
    let callback = events
        .world
        .field_transition
        .as_ref()
        .unwrap()
        .operation
        .clone();
    let persistent = events.take_persistent().unwrap();
    assert!(callback.complete(None).is_err());
    assert_eq!(events.active_instances(), 0);
    assert!(events.world.field_transition.is_none());
    let (world, memory) = persistent.into_world();
    assert_eq!(world.tick, 47);
    assert_eq!(world.random_state, 0x12345678);
    assert!(world.event_flags.contains(&27));
    assert!(world.actors.is_empty());
    assert_eq!(memory.read(0x40, Width::S32).unwrap(), 123);
    assert_eq!(memory.read(0x3fc, Width::S32).unwrap(), 123);
    assert_eq!(memory.read(0x400, Width::S32).unwrap(), 0);
    assert_eq!(memory.read(0x800, Width::S32).unwrap(), 0);
}
