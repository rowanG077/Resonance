use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::authored::{MessagePart, NativeDeclaration, TextReferenceKind, Type};
use symphonia_script_compiler::{Compilation, ScriptKind, compile, format, script_kind};
use symphonia_script_vm::{Fault, Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

const WAIT: NativeDeclaration = NativeDeclaration {
    name: "game::wait",
    opcode: 1,
    parameters: &[Type::Ticks],
    result: None,
    suspends: true,
};
const RECORD: NativeDeclaration = NativeDeclaration {
    name: "game::record",
    opcode: 2,
    parameters: &[Type::I32],
    result: None,
    suspends: false,
};
const TEXT: NativeDeclaration = NativeDeclaration {
    name: "game::text",
    opcode: 3,
    parameters: &[Type::Message],
    result: None,
    suspends: false,
};
const ASSET: NativeDeclaration = NativeDeclaration {
    name: "game::emit",
    opcode: 4,
    parameters: &[Type::Asset("game::Projectile")],
    result: None,
    suspends: false,
};

#[derive(Default)]
struct TestHost {
    records: Vec<i32>,
    waits: Vec<i32>,
}
impl Host for TestHost {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(WAIT, |host, args, _| {
            host.waits.push(args[0]);
            Ok(NativeResult::Suspend)
        })
        .register_typed(RECORD, |host, args, _| {
            host.records.push(args[0]);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(TEXT, |host, args, _| {
            host.records.push(args[0]);
            Ok(NativeResult::Continue(None))
        })
        .register_typed(ASSET, |host, args, _| {
            host.records.push(args[0]);
            Ok(NativeResult::Continue(None))
        });
}
fn build(source: &str) -> Compilation {
    compile(
        "main",
        &BTreeMap::from([("main".into(), source.into())]),
        &TestHost::AUTHORED_NATIVES
            .declarations()
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
fn execute(source: &str) -> (TestHost, Vec<i32>) {
    let program = Arc::new(build(source).program);
    let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
    let mut host = TestHost::default();
    let mut memory = Memory::default();
    loop {
        match vm.run(&mut host, &mut memory, 10_000).unwrap().event {
            RunEvent::Halted => break,
            RunEvent::Suspended { .. } => vm.complete(None, &mut memory).unwrap(),
            RunEvent::SuspendedTask { .. } => panic!("test did not install a child-task host"),
        }
    }
    (host, vm.result().unwrap())
}

#[test]
fn readable_task_keeps_its_call_frames_and_locals_across_native_waits() {
    let (host, result) = execute(
        r#"
        script field;
        use game;
        pub task main() -> i32 {
            let mut total = 0;
            for age in 1..4 {
                total += await step(age);
            }
            return total;
        }
        task step(age: i32) -> i32 {
            let before = age * 2;
            await game::wait(ticks(age));
            game::record(before);
            return before + 1;
        }
    "#,
    );
    assert_eq!(host.waits, [1, 2, 3]);
    assert_eq!(host.records, [2, 4, 6]);
    assert_eq!(result, [15]);
}

#[test]
fn external_arguments_are_checked_before_script_execution_including_aggregate_domains() {
    let program = Arc::new(
        build(
            r#"
        script field;
        pub task main(enabled: bool, delay: Ticks, factor: f32) -> i32 {
            if enabled { return i32(delay) + i32(factor); }
            return 0;
        }
    "#,
        )
        .program,
    );
    let finite = 3.5f32.to_bits() as i32;
    for (arguments, expected) in [
        ([2, 2, finite], Type::Bool),
        ([1, -1, finite], Type::Ticks),
        ([1, 2, f32::NAN.to_bits() as i32], Type::F32),
        ([1, 2, f32::INFINITY.to_bits() as i32], Type::F32),
        ([1, 2, f32::NEG_INFINITY.to_bits() as i32], Type::F32),
    ] {
        assert_eq!(
            Vm::validate_arguments(&program, program.entry(), &arguments)
                .unwrap_err()
                .fault,
            Fault::Type(expected)
        );
        assert_eq!(
            Vm::with_arguments(program.clone(), program.entry(), &arguments)
                .err()
                .unwrap()
                .fault,
            Fault::Type(expected)
        );
    }
    let mut vm = Vm::with_arguments(program.clone(), program.entry(), &[1, 2, finite]).unwrap();
    vm.run(&mut TestHost::default(), &mut Memory::default(), 100)
        .unwrap();
    assert_eq!(vm.result(), Some(vec![5]));

    let aggregate = Arc::new(
        build(
            r#"
        script field;
        enum Choice { Flag(bool), Float(f32), Nothing }
        struct Settings { flags: [bool; 2], delay: Ticks, choice: Option<Choice> }
        pub task main(settings: Settings) {}
    "#,
        )
        .program,
    );
    for arguments in [
        [1, 0, 2, 1, 1, (-2.0f32).to_bits() as i32],
        [1, 0, 2, 1, 0, 1],
        [1, 0, 2, 0, 0, 0],
    ] {
        Vm::with_arguments(aggregate.clone(), aggregate.entry(), &arguments).unwrap();
    }
    for arguments in [
        [1, 2, 2, 0, 0, 0],  // array bool
        [1, 0, -1, 0, 0, 0], // nested ticks
        [1, 0, 2, 1, 0, 2],  // selected bool payload
        [1, 0, 2, 1, 1, f32::NAN.to_bits() as i32],
        [1, 0, 2, 2, 0, 0], // invalid Option tag
        [1, 0, 2, 1, 3, 0], // invalid enum tag
        [1, 0, 2, 0, 0, 1], // inactive payload padding
    ] {
        assert!(
            Vm::with_arguments(aggregate.clone(), aggregate.entry(), &arguments).is_err(),
            "accepted {arguments:?}"
        );
    }
}

#[test]
fn duration_conversion_and_arithmetic_fault_before_invalid_values_escape() {
    for (body, fault) in [
        (
            "let delay = ticks(-1); return delay < 0ticks;",
            Fault::Type(Type::Ticks),
        ),
        (
            "let delay = 2ticks - 3ticks; return delay < 0ticks;",
            Fault::Type(Type::Ticks),
        ),
        (
            "let mut delay = 2ticks; delay -= 3ticks; return delay < 0ticks;",
            Fault::Type(Type::Ticks),
        ),
        (
            "let delay = 2147483647ticks + 1ticks; return delay == 0ticks;",
            Fault::Overflow,
        ),
        (
            "let delay = 1ticks / 0ticks; return delay == 0ticks;",
            Fault::DivisionByZero,
        ),
    ] {
        let program =
            Arc::new(build(&format!("script field; pub fn main() -> bool {{ {body} }}")).program);
        let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
        assert_eq!(
            vm.run(&mut TestHost::default(), &mut Memory::default(), 100)
                .unwrap_err()
                .fault,
            fault,
            "{body}"
        );
    }
    let (_, result) = execute(
        r#"
        script field;
        pub fn main() -> i32 {
            let mut delay = ticks(3) + 2ticks;
            delay *= 2ticks;
            delay /= 2ticks;
            delay -= 1ticks;
            let remainder = delay % 3ticks;
            return i32(remainder) + (!0 & 15) + i32(ticks(0));
        }
    "#,
    );
    assert_eq!(result, [16]);
}

#[test]
fn records_arrays_enums_and_options_have_value_semantics() {
    let (_, result) = execute(
        r#"
        script field;
        struct Point { x: i32, y: i32 }
        enum Result { Value(Point), Missing }
        pub fn main() -> i32 {
            let mut points = [Point { x: 2, y: 3 }, Point { x: 4, y: 5 }];
            let original = points;
            points[1].x = 12;
            let candidate = Result::Value(points[1]);
            let mut sum = 0;
            for point in original { sum += point.x; }
            let maybe: Option<i32> = Some(sum);
            match maybe {
                Some(value) => { sum = value + 1; },
                None => { sum = 0; },
            }
            match candidate {
                Result::Value(point) => { return sum + swap(point).y; },
                Result::Missing => { return 0; },
            }
        }
        fn swap(point: Point) -> Point {
            return Point { y: point.x, x: point.y };
        }
    "#,
    );
    assert_eq!(result, [19]);
}

#[test]
fn utf8_text_and_asset_references_are_module_data_not_vm_strings() {
    let compilation = build(
        r#"
        script field;
        use game;
        message greeting = "こんにちは、Lloyd!\nChosen’s journey.";
        asset bolt: game::Projectile = "effects/bolt";
        pub fn main() { game::text(greeting); game::emit(bolt); }
    "#,
    );
    assert_eq!(
        compilation.program.authored().unwrap().texts,
        ["こんにちは、Lloyd!\nChosen’s journey."]
    );
    assert_eq!(compilation.assets.len(), 1);
    assert_eq!(compilation.assets[0].path, "effects/bolt");
}

#[test]
fn imports_are_transitive_checked_and_recorded() {
    let sources = BTreeMap::from([
        (
            "main".into(),
            "script field; use helpers; pub fn main() -> i32 { return helpers::twice(6); }".into(),
        ),
        (
            "helpers".into(),
            "script library; pub fn twice(x: i32) -> i32 { return x * 2; }".into(),
        ),
    ]);
    let compiled = compile("main", &sources, &[]).unwrap();
    assert_eq!(compiled.sources.len(), 2);
    let program = Arc::new(compiled.program);
    let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
    vm.run(&mut TestHost::default(), &mut Memory::default(), 100)
        .unwrap();
    assert_eq!(vm.result(), Some(vec![12]));
}

#[test]
fn short_circuiting_loop_control_and_explicit_float_conversions() {
    let (_, result) = execute(
        r#"
        script field;
        pub fn main() -> i32 {
            let mut count = 0;
            let mut total = 0;
            while count < 10 {
                count += 1;
                if count == 2 { continue; }
                if count == 5 { break; }
                if true || 1 / 0 == 3 { total += count; }
                if false && 1 / 0 == 0 { return 99; }
            }
            return total + i32(f32(3) * 2.5);
        }
    "#,
    );
    assert_eq!(result, [15]);
}

#[test]
fn diagnostics_reject_unimplemented_or_unsafe_semantics() {
    for (source, expected) in [
        (
            "script field; use game; task main() { game::wait(1ticks); }",
            "requires await",
        ),
        (
            "script field; use game; fn main() { await game::wait(1ticks); }",
            "inside a task",
        ),
        (
            "script field; fn main() { let value = 1; value = 2; }",
            "mutable local",
        ),
        ("script field; fn main() { let x: f32 = 1; }", "expected"),
        (
            "script field; fn main() { let x = true + false; }",
            "operator",
        ),
        ("script field; fn main() { main(); }", "recursive"),
        (
            "script field; fn main() -> i32 { if true { return 1; } }",
            "every path",
        ),
        (
            "script field; enum E { A, B } fn main() { match E::A { E::A => {}, } }",
            "exhaustive",
        ),
        (
            "script field; fn main() { spawn child(); } task child() {}",
            "inside a task",
        ),
        ("script field; fn main() { let 日本 = 1; }", "ASCII"),
    ] {
        let result = compile(
            "main",
            &BTreeMap::from([("main".into(), source.into())]),
            &[WAIT],
        );
        let error = result.unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
        assert_eq!(error.location.file, "main");
        assert!(error.location.line > 0 && error.location.column > 0);
    }
}

#[test]
fn spawned_tasks_join_through_host_hooks_with_typed_aggregate_results() {
    #[derive(Default)]
    struct TaskHost {
        spawned: Vec<(u16, Vec<i32>)>,
        joins: Vec<i32>,
        ready: Option<Vec<i32>>,
    }
    impl Host for TaskHost {
        fn spawn(&mut self, function: u16, arguments: &[i32]) -> Result<i32, String> {
            self.spawned.push((function, arguments.to_vec()));
            Ok(16 + self.spawned.len() as i32)
        }
        fn join(&mut self, handle: i32) -> Result<Option<Vec<i32>>, String> {
            self.joins.push(handle);
            Ok(self.ready.take())
        }
    }
    let source = r#"
        script field;
        struct Pair { x: i32, y: i32 }
        pub task main() -> i32 {
            let child: Task<Pair> = spawn work(Pair { x: 4, y: 9 });
            let pair = await child;
            let finished: Task = spawn done();
            await finished;
            return pair.x + pair.y;
        }
        task work(pair: Pair) -> Pair { return Pair { x: pair.x * 2, y: pair.y }; }
        task done() {}
    "#;
    let compilation = compile(
        "main",
        &BTreeMap::from([("main".into(), source.into())]),
        &[],
    )
    .unwrap();
    let program = Arc::new(compilation.program);
    let mut parent = Vm::new(program.clone(), program.entry()).unwrap();
    let mut host = TaskHost::default();
    let mut memory = Memory::default();
    assert_eq!(
        parent.run(&mut host, &mut memory, 100).unwrap().event,
        RunEvent::SuspendedTask { handle: 17 }
    );
    let (function, arguments) = &host.spawned[0];
    let entry = program.authored().unwrap().functions[usize::from(*function)].entry;
    let mut child = Vm::with_arguments(program.clone(), entry, arguments).unwrap();
    assert_eq!(
        child.run(&mut host, &mut memory, 100).unwrap().event,
        RunEvent::Halted
    );
    assert_eq!(child.result(), Some(vec![8, 9]));
    parent.complete_task(&child.result().unwrap()).unwrap();
    // A completed unit child can join immediately without inventing a wait tick.
    host.ready = Some(vec![]);
    assert_eq!(
        parent.run(&mut host, &mut memory, 100).unwrap().event,
        RunEvent::Halted
    );
    assert_eq!(parent.result(), Some(vec![17]));
    assert_eq!(host.joins, [17, 18]);
    assert_eq!(host.spawned.len(), 2);
}

#[test]
fn task_handles_remain_typed_and_local_to_the_owning_task() {
    for (source, expected) in [
        (
            "script field; task main() { spawn helper(); } fn helper() {}",
            "requires a task",
        ),
        ("script field; task main() { await 3; }", "task handle"),
        (
            "script field; task main() { let child: Task<bool> = spawn work(); } task work() -> i32 { return 1; }",
            "expected",
        ),
        ("script field; task main(child: Task) {}", "task-local"),
        (
            "script field; task main() -> Task { return spawn work(); } task work() {}",
            "task-local",
        ),
        (
            "script field; struct State { child: Task } task main() {}",
            "task-local",
        ),
        (
            "script field; task main() { let children = [spawn work()]; } task work() {}",
            "task-local",
        ),
        (
            "script field; task main() { let child = Some(spawn work()); } task work() {}",
            "task-local",
        ),
        ("script field; task main() { spawn main(); }", "recursive"),
    ] {
        let error = compile(
            "main",
            &BTreeMap::from([("main".into(), source.into())]),
            &[],
        )
        .unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
    }
}

#[test]
fn defer_unwinds_lexical_scopes_in_reverse_order_on_every_orderly_exit() {
    let (host, result) = execute(
        r#"
        script field;
        use game;
        pub task main() -> i32 {
            let mut value = 1;
            defer { game::record(value); value = 99; }
            defer { game::record(20); }
            {
                let value = 2;
                defer { game::record(value); }
                defer { game::record(3); }
            }
            for index in 0..3 {
                defer { game::record(30 + index); }
                if index == 0 { continue; }
                if index == 1 { break; }
            }
            while value < 3 {
                defer { game::record(40 + value); }
                value += 1;
                if value == 2 { continue; }
            }
            {
                let mut value = 7;
                defer { game::record(value); value = 70; }
                return value;
            }
        }
    "#,
    );
    assert_eq!(host.records, [3, 2, 30, 31, 42, 43, 7, 20, 3]);
    // Evaluate the return value before cleanup mutates its original local slot.
    assert_eq!(result, [7]);
}

#[test]
fn cleanup_cannot_suspend_spawn_escape_or_capture_later_bindings() {
    for (source, expected) in [
        (
            "script field; use game; task main() { defer { await game::wait(1ticks); } }",
            "cleanup cannot suspend",
        ),
        (
            "script field; task main() { defer { spawn child(); } } task child() {}",
            "cleanup cannot spawn",
        ),
        (
            "script field; fn main() { defer { return; } }",
            "cleanup cannot return",
        ),
        (
            "script field; fn main() { for i in 0..2 { defer { break; } } }",
            "cleanup cannot exit",
        ),
        (
            "script field; fn main() { while true { defer { continue; } break; } }",
            "cleanup cannot exit",
        ),
        (
            "script field; use game; fn main() { defer { game::record(later); } let later = 3; }",
            "unknown value",
        ),
    ] {
        let error = compile(
            "main",
            &BTreeMap::from([("main".into(), source.into())]),
            &[WAIT, RECORD],
        )
        .unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
    }
    let (host, _) = execute(
        r#"
        script field;
        use game;
        fn main() {
            defer {
                defer { game::record(9); }
                for index in 0..3 {
                    if index == 0 { continue; }
                    game::record(index);
                    break;
                }
            }
        }
    "#,
    );
    assert_eq!(host.records, [1, 9]);
}

const CHARACTER_TYPE: Type = Type::TextReference {
    name: "game::text::Character",
    kind: TextReferenceKind::Character,
};
const ITEM_TYPE: Type = Type::TextReference {
    name: "game::text::Item",
    kind: TextReferenceKind::Item,
};
const CHARACTER: NativeDeclaration = NativeDeclaration {
    name: "game::text::character",
    opcode: 1,
    parameters: &[Type::I32],
    result: Some(CHARACTER_TYPE),
    suspends: false,
};
const ITEM: NativeDeclaration = NativeDeclaration {
    name: "game::text::item",
    opcode: 2,
    parameters: &[Type::I32],
    result: Some(ITEM_TYPE),
    suspends: false,
};
const REPORT: NativeDeclaration = NativeDeclaration {
    name: "game::report",
    opcode: 3,
    parameters: &[Type::I32, Type::Message, Type::I32],
    result: None,
    suspends: false,
};

#[test]
fn imported_templates_are_fixed_values_returned_from_functions_and_checked_at_native_boundaries() {
    #[derive(Default)]
    struct Messages(Vec<Vec<i32>>);
    impl Host for Messages {
        const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
            .register_typed(CHARACTER, |_, args, _| {
                Ok(NativeResult::Continue(Some(args[0])))
            })
            .register_typed(ITEM, |_, args, _| Ok(NativeResult::Continue(Some(args[0]))))
            .register_typed(REPORT, |host, args, _| {
                host.0.push(args.to_vec());
                Ok(NativeResult::Continue(None))
            });
    }
    let sources = BTreeMap::from([
        ("main".into(), r#"
        script field;
            use messages::found;
            use game::text;
            use game;
            pub fn main() {
                game::report(7, describe(text::character(1), text::item(2), -3), 9);
                game::report(11, "{{{who}}}: {count} × {item}.\n世界", 13);
            }
            fn describe(who: text::Character, item: text::Item, count: i32) -> Message {
                let message = found(who, item, count);
                return message;
            }
        "#.into()),
        ("messages".into(), r#"
        script field;
            use game::text;
            pub message found(who: text::Character, item: text::Item, count: i32) = "{{{who}}}: {count} × {item}.\n世界";
            pub message reordered(item: text::Item, who: text::Character, count: i32) = "{{{who}}}: {count} × {item}.\n世界";
        "#.into()),
    ]);
    let compilation = compile("main", &sources, &[CHARACTER, ITEM, REPORT]).unwrap();
    let program = Arc::new(compilation.program);
    let module = program.authored().unwrap();
    assert_eq!(module.texts.len(), 3);
    assert_eq!(module.templates.len(), 2);
    assert_eq!(module.templates[&0].parameters[0].ty, CHARACTER_TYPE);
    assert_eq!(module.templates[&1].parameters[0].ty, ITEM_TYPE);
    assert_eq!(
        module.templates[&0].parts,
        [
            MessagePart::Text("{".into()),
            MessagePart::Argument(0),
            MessagePart::Text("}: ".into()),
            MessagePart::Argument(2),
            MessagePart::Text(" × ".into()),
            MessagePart::Argument(1),
            MessagePart::Text(".\n世界".into()),
        ]
    );
    let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
    let mut host = Messages::default();
    vm.run(&mut host, &mut Memory::default(), 500).unwrap();
    assert_eq!(
        host.0,
        [
            vec![7, 0, 1, 2, -3, 0, 0, 0, 0, 0, 9],
            vec![11, 2, 0, 0, 0, 0, 0, 0, 0, 0, 13]
        ]
    );
    assert_eq!(
        format(
            "messages",
            &format("messages", &sources["messages"]).unwrap()
        )
        .unwrap(),
        format("messages", &sources["messages"]).unwrap()
    );
}

#[test]
fn template_placeholders_types_and_bounds_fail_before_activation() {
    for (source, expected) in [
        (
            "script field; message m(n: i32) = \"{missing}\"; fn main() {}",
            "unknown message placeholder",
        ),
        (
            "script field; message m(n: i32) = \"{n\"; fn main() {}",
            "unclosed message",
        ),
        (
            "script field; message m() = \"}\"; fn main() {}",
            "unmatched message",
        ),
        (
            "script field; message m(n: bool) = \"{n}\"; fn main() {}",
            "substitutions require",
        ),
        (
            "script field; message m(n: i32, n: i32) = \"{n}\"; fn main() {}",
            "duplicate message parameter",
        ),
        (
            "script field; message m(n: i32) = \"{n}\"; fn main() { m(); }",
            "expects 1 arguments",
        ),
        (
            "script field; message m(n: i32) = \"{n}\"; fn main() { m(true); }",
            "expected",
        ),
        (
            "script field; use game::text; message m(who: text::Character) = \"{who}\"; fn main() { m(text::item(1)); }",
            "expected",
        ),
        (
            "script field; message m(a:i32,b:i32,c:i32,d:i32,e:i32,f:i32,g:i32,h:i32,i:i32) = \"{a}\"; fn main() {}",
            "at most 8",
        ),
    ] {
        let error = compile(
            "main",
            &BTreeMap::from([("main".into(), source.into())]),
            &[CHARACTER, ITEM],
        )
        .unwrap_err();
        assert!(error.message.contains(expected), "{source}: {error}");
    }
    let invalid = NativeDeclaration {
        result: Some(Type::Message),
        ..REPORT
    };
    let error = compile(
        "main",
        &BTreeMap::from([("main".into(), "script field; fn main() {}".into())]),
        &[invalid],
    )
    .unwrap_err();
    assert!(error.message.contains("cannot return a Message"));
}

const VIEW: Type = Type::Collection {
    name: "view::Values",
    element: &Type::I32,
    count: 6,
    get: 7,
};
const VIEW_ALL: NativeDeclaration = NativeDeclaration {
    name: "view::all",
    opcode: 5,
    parameters: &[],
    result: Some(VIEW),
    suspends: false,
};
const VIEW_COUNT: NativeDeclaration = NativeDeclaration {
    name: "view::count",
    opcode: 6,
    parameters: &[VIEW],
    result: Some(Type::I32),
    suspends: false,
};
const VIEW_GET: NativeDeclaration = NativeDeclaration {
    name: "view::get",
    opcode: 7,
    parameters: &[VIEW, Type::I32],
    result: Some(Type::I32),
    suspends: false,
};

#[test]
fn host_collection_iteration_retains_one_view_and_propagates_lifetime_and_bounds_errors() {
    #[derive(Default)]
    struct ViewHost {
        values: Vec<i32>,
        valid: bool,
        created: usize,
        counted: usize,
        indices: Vec<i32>,
    }
    impl ViewHost {
        fn check(&self, handle: i32) -> Result<(), String> {
            if self.valid && handle == 42 {
                Ok(())
            } else {
                Err("stale view".into())
            }
        }
    }
    impl Host for ViewHost {
        const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
            .register_typed(VIEW_ALL, |host, _, _| {
                host.created += 1;
                Ok(NativeResult::Continue(Some(42)))
            })
            .register_typed(VIEW_COUNT, |host, args, _| {
                host.check(args[0])?;
                host.counted += 1;
                let count = i32::try_from(host.values.len()).map_err(|_| "view too large")?;
                Ok(NativeResult::Continue(Some(count)))
            })
            .register_typed(VIEW_GET, |host, args, _| {
                host.check(args[0])?;
                host.indices.push(args[1]);
                let value = host
                    .values
                    .get(args[1] as usize)
                    .ok_or("view index out of range")?;
                Ok(NativeResult::Continue(Some(*value)))
            })
            .register_typed(WAIT, |_, _, _| Ok(NativeResult::Suspend));
    }
    let source = r#"
        script field;
        use view;
        use game;
        pub task main() -> i32 {
            let mut sum = 0;
            for value in view::all() {
                await game::wait(1ticks);
                if value == 2 { continue; }
                sum += value;
                if sum >= 8 { break; }
            }
            return sum;
        }
    "#;
    let compiled = compile(
        "main",
        &BTreeMap::from([("main".into(), source.into())]),
        &[VIEW_ALL, VIEW_COUNT, VIEW_GET, WAIT],
    )
    .unwrap();
    let program = Arc::new(compiled.program);
    for failure in [None, Some("stale view"), Some("view index out of range")] {
        let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
        let mut host = ViewHost {
            values: vec![1, 2, 3, 4, 5],
            valid: true,
            ..Default::default()
        };
        let mut memory = Memory::default();
        assert!(matches!(
            vm.run(&mut host, &mut memory, 500).unwrap().event,
            RunEvent::Suspended { .. }
        ));
        match failure {
            Some("stale view") => host.valid = false,
            Some(_) => host.values.clear(),
            None => {}
        }
        loop {
            vm.complete(None, &mut memory).unwrap();
            let result = vm.run(&mut host, &mut memory, 500);
            if let Some(message) = failure {
                assert!(result.unwrap_err().to_string().contains(message));
                break;
            }
            if matches!(result.unwrap().event, RunEvent::Halted) {
                break;
            }
        }
        assert_eq!((host.created, host.counted), (1, 1));
        if failure.is_none() {
            assert_eq!(host.indices, [0, 1, 2, 3]);
            assert_eq!(vm.result(), Some(vec![8]));
        }
    }
}

#[test]
fn collection_descriptors_are_checked_before_lowering() {
    let source = BTreeMap::from([("main".into(), "script field; fn main() {}".into())]);
    for declarations in [
        vec![VIEW_ALL, VIEW_COUNT],
        vec![
            VIEW_ALL,
            VIEW_COUNT,
            NativeDeclaration {
                suspends: true,
                ..VIEW_GET
            },
        ],
        vec![
            VIEW_ALL,
            NativeDeclaration {
                result: Some(Type::Bool),
                ..VIEW_COUNT
            },
            VIEW_GET,
        ],
        vec![
            VIEW_ALL,
            VIEW_COUNT,
            NativeDeclaration {
                parameters: &[VIEW],
                ..VIEW_GET
            },
        ],
        vec![
            NativeDeclaration {
                result: Some(Type::Collection {
                    name: "view::Bad",
                    element: &Type::I32,
                    count: 6,
                    get: 6,
                }),
                ..VIEW_ALL
            },
            VIEW_COUNT,
            VIEW_GET,
        ],
        vec![
            NativeDeclaration {
                result: Some(Type::Collection {
                    name: "view::Bad",
                    element: &Type::Message,
                    count: 6,
                    get: 7,
                }),
                ..VIEW_ALL
            },
            VIEW_COUNT,
            VIEW_GET,
        ],
    ] {
        let error = compile("main", &source, &declarations).unwrap_err();
        assert!(error.message.contains("collection"), "{error}");
    }
}

#[test]
fn formatting_preserves_comments_and_utf8_and_is_idempotent() {
    let source = "// café\nscript field;\nuse game;pub task main(){/* keep 日本 */ await game::wait(1ticks);game::text(\"hello\\n世界\");}";
    let formatted = format("main", source).unwrap();
    assert!(formatted.contains("// café"));
    assert!(formatted.contains("/* keep 日本 */"));
    assert!(formatted.contains("世界"));
    assert_eq!(format("main", &formatted).unwrap(), formatted);
    build(&formatted);
}

#[test]
fn script_headers_are_required_unambiguous_and_preserved_by_formatting() {
    for kind in [ScriptKind::Field, ScriptKind::Model, ScriptKind::Library] {
        let source = format!("// Host declaration\nscript {kind};pub fn main(){{}}");
        assert_eq!(script_kind("main", &source).unwrap(), kind);
        assert_eq!(
            compile(
                "main",
                &BTreeMap::from([("main".into(), source.clone())]),
                &[]
            )
            .unwrap()
            .kind,
            kind
        );
        let formatted = format("main", &source).unwrap();
        assert!(formatted.starts_with(&format!("// Host declaration\nscript {kind};\n")));
        assert_eq!(format("main", &formatted).unwrap(), formatted);
    }
    for (source, expected) in [
        ("pub fn main() {}", "expected 'script field;'"),
        ("script battle; fn main() {}", "unknown script kind"),
        (
            "script model; script field; fn main() {}",
            "only be declared once",
        ),
        (
            "script model; fn main() {} script model;",
            "only be declared once",
        ),
        ("use game; script field; fn main() {}", "must precede"),
        ("script field fn main() {}", "expected ';'"),
    ] {
        let error = script_kind("main", source).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert!(
            compile(
                "main",
                &BTreeMap::from([("main".into(), source.into())]),
                &[]
            )
            .is_err()
        );
        assert!(format("main", source).is_err());
    }
}

#[test]
fn imports_respect_script_kinds_and_libraries_use_the_entry_hosts_natives() {
    for root in [ScriptKind::Field, ScriptKind::Model, ScriptKind::Library] {
        for imported in [ScriptKind::Field, ScriptKind::Model, ScriptKind::Library] {
            for import in ["helpers", "helpers::value"] {
                let sources = BTreeMap::from([
                    (
                        "main".into(),
                        format!("script {root}; use {import}; pub fn main() {{}}"),
                    ),
                    (
                        "helpers".into(),
                        format!("script {imported}; pub fn value() {{}}"),
                    ),
                ]);
                let result = compile("main", &sources, &[]);
                if root == imported || imported == ScriptKind::Library {
                    assert_eq!(result.unwrap().kind, root);
                } else {
                    let error = result.unwrap_err();
                    assert!(
                        error
                            .message
                            .contains(&format!("{root} script cannot use {imported} module")),
                        "{error}"
                    );
                    assert_eq!(error.location.file, "main");
                }
            }
        }
    }
    let sources = BTreeMap::from([
        (
            "main".into(),
            "script model; use shared; pub fn main() { shared::record(); }".into(),
        ),
        (
            "shared".into(),
            "script library; use game; pub fn record() { game::record(4); }".into(),
        ),
    ]);
    assert!(compile("main", &sources, &[RECORD]).is_ok());
    assert!(compile("main", &sources, &[]).is_err());
}
