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
                await field::wait_ticks(ticks(0));
                story::set_flag(43, true);
                await field::wait_ticks(ticks(2));
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
            let slow = spawn work(ticks(2), 10);
            let fast = spawn work(ticks(1), 20);
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
