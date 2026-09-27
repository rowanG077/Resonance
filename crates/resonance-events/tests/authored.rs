use resonance_events::{
    EventRuntime, GameWorld, Outcome, ResourceLibrary, authored::native_declarations,
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::{NativeCall, Program};

fn compile(source: &str) -> Arc<Program> {
    let sources = BTreeMap::from([("test".into(), format!("script field;\n{source}"))]);
    Arc::new(
        symphonia_script_compiler::compile("test", &sources, &native_declarations())
            .unwrap()
            .program,
    )
}

fn runtime() -> EventRuntime {
    runtime_with(ResourceLibrary::default())
}

fn runtime_with(resources: ResourceLibrary) -> EventRuntime {
    // A legacy event remains suspended while authored foreground work uses another slot.
    let words = [
        4,
        0,
        0,
        0,
        0,
        0x3000,
        0x4000,
        5,
        0x3000,
        0x4000,
        0x2000 | NativeCall::YieldCommand as u16,
        0x20ff,
    ];
    let program = Arc::new(
        Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    let mut world = GameWorld::default();
    world.input_enabled = true;
    EventRuntime::with_state(program, Arc::new(resources), world, Default::default()).unwrap()
}

#[test]
fn typed_templates_expand_in_rust_and_wait_for_their_notice() {
    let program = compile(
        r#"
        use game::field;
        use game::text;
        use game::story;
        message found(who: text::Character, item: text::Item, count: i32) = "{who} found {count} {item}.";
        pub task main() {
            let who = text::character(1);
            let item = text::item(7);
            let body = found(who, item, -2);
            await field::notice(body);
            story::set_flag(42, true);
        }
    "#,
    );
    let mut runtime = runtime_with(ResourceLibrary {
        actor_names: [(1, "Default Lloyd".into())].into(),
        text: Arc::new(resonance_content::session::GameText {
            characters: [(1, "Lloyd".into())].into(),
            items: [(7, "Apple Gels".into())].into(),
            ..Default::default()
        }),
        ..Default::default()
    });
    runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    let notice = runtime.world.dialogue.values().next().unwrap();
    let text = notice
        .body
        .tokens
        .iter()
        .map(|token| match token {
            resonance_events::dialogue::TextToken::Text { text } => text.as_str(),
            _ => panic!("template should resolve to ordinary literal dialogue"),
        })
        .collect::<String>();
    assert_eq!(text, "Lloyd found -2 Apple Gels.");
    assert!(!runtime.player_has_control());
    assert!(!runtime.world.event_flags.contains(&42));
    notice.operation.complete(None).unwrap();
    for _ in 0..2 {
        runtime.step().unwrap();
    }
    assert!(runtime.world.event_flags.contains(&42));
    assert!(runtime.player_has_control());

    let invalid = compile("use game::text; pub task main() { let item = text::item(-1); }");
    runtime.start_authored(invalid, "test::main", &[]).unwrap();
    let error = format!("{:#}", runtime.step().unwrap_err());
    assert!(error.contains("invalid item text ID"), "{error}");
}

#[test]
fn compiled_field_branch_and_wait_share_the_existing_event_dispatcher() {
    let program = compile(
        r#"
        use game::field;
        use game::story;
        const CustomFlag: i32 = 65535;
        pub task main() {
            if (!story::flag(CustomFlag)) {
                story::set_flag(41, true);
                await field::wait_ticks(0ticks);
                story::set_flag(43, true);
                await field::wait_ticks(2ticks);
                story::set_flag(CustomFlag, true);
            }
        }
    "#,
    );
    let mut runtime = runtime();
    runtime
        .start_authored(program.clone(), "test::main", &[])
        .unwrap();
    runtime.step().unwrap();
    assert_eq!(
        runtime
            .world
            .event_flags
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [41, 43]
    );
    assert_eq!(runtime.active_instances(), 2);
    runtime.step().unwrap();
    assert!(!runtime.world.event_flags.contains(&u16::MAX));
    runtime.step().unwrap();
    assert!(runtime.world.event_flags.contains(&u16::MAX));
    assert!(runtime.player_has_control());
    assert!(!runtime.main_finished());
    runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    assert!(runtime.player_has_control());
    runtime.step().unwrap();
    assert!(runtime.main_finished());
    assert_eq!(runtime.active_instances(), 0);
}

#[test]
fn authored_story_flags_reject_ids_outside_save_storage() {
    for id in [-1, 65536] {
        let program = compile(&format!(
            "use game::story; pub task main() {{ story::set_flag({id}, true); }}"
        ));
        let mut runtime = runtime();
        runtime.start_authored(program, "test::main", &[]).unwrap();
        assert!(format!("{:#}", runtime.step().unwrap_err()).contains("invalid story flag ID"));
    }
}

#[test]
fn authored_utf8_notice_uses_real_dialogue_and_cancels_late_completion() {
    let mut runtime = runtime();
    let program = compile(
        r#"
        use game::field;
        use game::story;
        pub task main() {
            let child = spawn nested();
            await child;
            story::set_flag(42, true);
        }
        task nested() {
            let child = spawn prompt();
            await child;
        }
        task prompt() {
            await field::notice("Café — コレット");
            story::set_flag(43, true);
        }
    "#,
    );
    let handle = runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    let notice = runtime.world.dialogue.values().next().unwrap();
    let operation = notice.operation.clone();
    let resonance_events::dialogue::TextToken::Text { text } = &notice.body.tokens[0] else {
        panic!("missing text")
    };
    assert_eq!(text, "Café — コレット");
    runtime.cancel_authored(handle).unwrap();
    assert_eq!(operation.progress().outcome, Some(Outcome::Cancelled));
    assert!(operation.complete(None).is_err());
    assert!(runtime.world.dialogue.is_empty());
    assert!(runtime.player_has_control());
    runtime.step().unwrap();
    assert!(!runtime.world.event_flags.contains(&42));
    assert!(!runtime.world.event_flags.contains(&43));
    assert!(!runtime.main_finished());
}

#[test]
fn dialogue_completion_resumes_the_authored_caller_without_an_extra_service_wait() {
    let mut runtime = runtime();
    let program = compile(
        r#"
        use game::field;
        use game::story;
        pub task main() {
            await field::notice("Ready.");
            story::set_flag(42, true);
        }
    "#,
    );
    runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    runtime
        .world
        .dialogue
        .values()
        .next()
        .unwrap()
        .operation
        .complete(None)
        .unwrap();
    runtime.step().unwrap();
    assert!(runtime.world.event_flags.contains(&42));
    assert!(runtime.player_has_control());
}

#[test]
fn concurrent_children_join_fixed_results_in_the_existing_stable_slot_order() {
    let program = compile(
        r#"
        use game::field;
        use game::story;
        struct Pair { x: i32, y: i32 }
        pub task main() {
            let slow = spawn work(2ticks, 10);
            let fast = spawn work(1ticks, 20);
            let a = await slow;
            let b = await fast;
            if (a.x + b.x == 30 && a.y + b.y == 32) { story::set_flag(42, true); }
        }
        task work(duration: ticks, value: i32) -> Pair {
            await field::wait_ticks(duration);
            return Pair { x: value, y: value + 1 };
        }
    "#,
    );
    let mut runtime = runtime();
    runtime.start_authored(program, "test::main", &[]).unwrap();
    for active in [4, 3, 2] {
        runtime.step().unwrap();
        assert_eq!(runtime.active_instances(), active);
        assert!(!runtime.world.event_flags.contains(&42));
    }
    runtime.step().unwrap();
    assert!(runtime.world.event_flags.contains(&42));
    assert!(runtime.player_has_control());
    assert_eq!(runtime.active_instances(), 1);
}

#[test]
fn parent_completion_cancels_queued_and_running_unjoined_children() {
    let program = compile(
        r#"
        use game::field;
        use game::story;
        pub task main() { spawn prompt(); }
        pub task delayed() { spawn prompt(); await field::next_update(); }
        task prompt() {
            story::set_flag(41, true);
            await field::notice("Waiting.");
            story::set_flag(42, true);
        }
    "#,
    );
    let mut runtime = runtime();
    runtime
        .start_authored(program.clone(), "test::main", &[])
        .unwrap();
    runtime.step().unwrap();
    assert!(!runtime.world.event_flags.contains(&41));
    assert!(runtime.player_has_control());
    runtime
        .start_authored(program, "test::delayed", &[])
        .unwrap();
    runtime.step().unwrap();
    let operation = runtime
        .world
        .dialogue
        .values()
        .next()
        .unwrap()
        .operation
        .clone();
    assert!(runtime.world.event_flags.contains(&41));
    runtime.step().unwrap();
    assert_eq!(operation.progress().outcome, Some(Outcome::Cancelled));
    assert!(!runtime.world.event_flags.contains(&42));
    assert!(runtime.world.dialogue.is_empty());
    assert!(runtime.player_has_control());
    assert_eq!(runtime.active_instances(), 1);
}

#[test]
fn a_child_fault_retires_its_parent_and_sibling_operations() {
    let program = compile(
        r#"
        use game::field;
        use game::story;
        pub task main() {
            let sibling = spawn prompt();
            let broken = spawn fail();
            await broken;
            await sibling;
            story::set_flag(42, true);
        }
        task prompt() { await field::notice("Waiting."); }
        task fail() {
            await field::next_update();
            let zero = 0;
            let result = 1 / zero;
        }
    "#,
    );
    let mut runtime = runtime();
    runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    let operation = runtime
        .world
        .dialogue
        .values()
        .next()
        .unwrap()
        .operation
        .clone();
    let error = runtime.step().unwrap_err();
    assert!(format!("{error:#}").contains("division by zero"));
    assert_eq!(operation.progress().outcome, Some(Outcome::Cancelled));
    assert!(operation.complete(None).is_err());
    assert!(runtime.world.dialogue.is_empty());
    assert!(!runtime.world.event_flags.contains(&42));
    assert_eq!(runtime.active_instances(), 1);
}

#[test]
fn joining_consumes_the_child_handle() {
    let program = compile(
        r#"
        pub task main() {
            let child = spawn done();
            await child;
            await child;
        }
        task done() {}
    "#,
    );
    let mut runtime = runtime();
    runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    let error = runtime.step().unwrap_err();
    assert!(format!("{error:#}").contains("already joined"));
    assert_eq!(runtime.active_instances(), 1);
}

#[test]
fn authored_state_survives_field_retirement_without_retaining_a_vm() {
    let script = compile("state visits: i32 = 0; pub task main() { visits += 1; }");
    let mut first = runtime();
    first
        .start_authored(script.clone(), "test::main", &[])
        .unwrap();
    first.step().unwrap();
    assert_eq!(first.world.script_state["test::visits"], 1);

    let (mut world, memory) = first.persistent_state().unwrap().into_world();
    world.input_enabled = true;
    let legacy = Arc::new(Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap());
    let mut second =
        EventRuntime::with_state(legacy, Arc::new(ResourceLibrary::default()), world, memory)
            .unwrap();
    second.start_authored(script, "test::main", &[]).unwrap();
    second.step().unwrap();
    assert_eq!(second.world.script_state["test::visits"], 2);
}

#[test]
fn suspended_actor_handles_cannot_modify_a_replacement() {
    let program = compile(
        r#"
        use game::actors;
        use game::field;
        pub task main() {
            let actor = actors::controlled();
            await field::wait_ticks(1ticks);
            actors::show(actor, false);
        }
    "#,
    );
    let mut runtime = runtime();
    runtime.world.controlled_actor = 1;
    runtime
        .world
        .insert_actor(1, resonance_events::Actor::new(1, [0.; 3]));
    runtime.start_authored(program, "test::main", &[]).unwrap();
    runtime.step().unwrap();
    runtime
        .world
        .insert_actor(1, resonance_events::Actor::new(1, [10.; 3]));
    let error = format!("{:#}", runtime.step().unwrap_err());
    assert!(error.contains("actor handle is stale"), "{error}");
    assert!(runtime.world.actors[&1].visible);
}

#[test]
fn memory_circle_unlock_requires_confirmation_and_cancellation_releases_both_windows() {
    let source = include_str!("../../../scripts/field/memory.sym")
        .strip_prefix("script field;")
        .unwrap();
    let program = compile(source);
    let text = |text: &str| {
        vec![resonance_content::font::TextSpan {
            text: text.into(),
            color: 0,
        }]
    };
    let mut runtime = runtime_with(ResourceLibrary {
        memory_circle_text: resonance_events::MemoryCircleText {
            unlock: text("Unlock?\nYes\nNo"),
            no_gem: text("No memory gems."),
            ..Default::default()
        },
        ..Default::default()
    });
    runtime.world.party = Some(
        serde_json::from_value(serde_json::json!({
            "members": [], "formation": [], "items": {}, "found_items": [],
            "recent_items": [], "gald": 0, "spent_gald": 0,
            "settings": resonance_events::party::Settings::default()
        }))
        .unwrap(),
    );
    runtime.world.save_points.push(resonance_events::SavePoint {
        actor: 0,
        position: [0.; 3],
        resource: 0,
        born: 0,
        active: false,
        unlock_flag: Some(851),
        glow_scale: 0.08,
    });
    let task = runtime
        .start_authored(program.clone(), "test::unlock", &[0])
        .unwrap();
    runtime.step().unwrap();
    runtime.cancel_authored(task).unwrap();
    assert!(runtime.world.dialogue.is_empty());
    assert!(runtime.world.choices.is_empty());
    assert!(runtime.player_has_control());
    assert!(!runtime.world.event_flags.contains(&851));

    use resonance_events::dialogue::ChoiceExit::{Cancel, Confirm, Timeout};
    // Choice stores zero-based message lines: Yes is line 1, No is line 2.
    for (reason, line, gems, unlocked) in [
        (Cancel, 1, 1, false),
        (Confirm, 2, 1, false),
        (Timeout, 1, 1, false),
        (Confirm, 1, 0, false),
        (Confirm, 1, 1, true),
    ] {
        runtime
            .world
            .party
            .as_mut()
            .unwrap()
            .items
            .insert(491, gems);
        runtime
            .start_authored(program.clone(), "test::unlock", &[0])
            .unwrap();
        runtime.step().unwrap();
        let choice = runtime.world.choices.get_mut(&0).unwrap();
        choice.selected_line = line;
        choice.finish(reason).unwrap();
        runtime.world.dialogue[&0].operation.complete(None).unwrap();
        runtime.step().unwrap();
        if gems == 0 {
            assert!(!runtime.player_has_control());
            runtime.world.dialogue[&0].operation.complete(None).unwrap();
            runtime.step().unwrap();
        }
        assert!(runtime.player_has_control());
        assert_eq!(runtime.world.event_flags.contains(&851), unlocked);
        assert_eq!(
            runtime
                .world
                .party
                .as_ref()
                .unwrap()
                .items
                .get(&491)
                .copied()
                .unwrap_or(0),
            gems - u8::from(unlocked)
        );
    }
}
