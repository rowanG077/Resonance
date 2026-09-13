use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::{Op, Program, Width, authored::*};
use symphonia_script_vm::{Fault, Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

const WAIT: NativeDeclaration = NativeDeclaration {
    name: "wait",
    opcode: 1,
    parameters: &[Type::Ticks],
    result: Some(Type::I32),
    suspends: true,
};
const MESSAGE: NativeDeclaration = NativeDeclaration {
    name: "message",
    opcode: 2,
    parameters: &[Type::Message],
    result: None,
    suspends: false,
};

#[derive(Default)]
struct TestHost {
    calls: Vec<i32>,
}
impl Host for TestHost {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::new()
        .register_typed(WAIT, |host: &mut Self, args, _| {
            host.calls.push(args[0]);
            Ok(NativeResult::Suspend)
        })
        .register_typed(MESSAGE, |host: &mut Self, args, _| {
            host.calls.push(args[0]);
            Ok(NativeResult::Continue(None))
        });
}

fn function(name: &str, entry: u32, parameters: u16, locals: u16, results: u16) -> Function {
    Function {
        name: name.into(),
        entry,
        parameters,
        parameter_layout: ValueLayout::Sequence(vec![
            ValueLayout::Scalar(Type::I32);
            usize::from(parameters)
        ]),
        locals,
        results,
        is_task: true,
    }
}

fn program(code: Vec<Op>, locals: u16, results: u16) -> Arc<Program> {
    Arc::new(
        Program::from_authored(Module {
            code,
            functions: vec![function("main", 0, 0, locals, results)],
            ..Module::default()
        })
        .unwrap(),
    )
}

#[test]
fn external_parameter_metadata_rejects_empty_native_layouts_before_vm_activation() {
    const EMPTY: Type = Type::Record {
        name: "Empty",
        fields: &[],
    };
    for ty in [
        EMPTY,
        Type::Array {
            element: &EMPTY,
            len: 1,
        },
        Type::Array {
            element: &Type::Bool,
            len: 0,
        },
    ] {
        let mut entry = function("main", 0, 0, 0, 0);
        entry.parameter_layout = ValueLayout::Scalar(ty);
        assert!(
            Program::from_authored(Module {
                code: vec![Op::ReturnValues(0)],
                functions: vec![entry],
                ..Module::default()
            })
            .is_err()
        );
    }

    let mut entry = function("main", 0, 2, 2, 0);
    entry.parameter_layout = ValueLayout::Scalar(Type::Array {
        element: &Type::Bool,
        len: 2,
    });
    let program = Arc::new(
        Program::from_authored(Module {
            code: vec![Op::ReturnValues(0)],
            functions: vec![entry],
            ..Module::default()
        })
        .unwrap(),
    );
    assert!(Vm::with_arguments(program.clone(), 0, &[0, 1]).is_ok());
    assert!(Vm::with_arguments(program, 0, &[0, 2]).is_err());
}

#[test]
fn nested_call_frames_keep_private_locals_and_resume_the_existing_native_wait() {
    let mut code = vec![
        Op::Push(7),
        Op::ArgumentValue,
        Op::CallFunction(1),
        Op::StoreLocal(0),
    ];
    for _ in 0..Type::Message.slots() {
        code.extend([Op::Push(0), Op::ArgumentValue]);
    }
    code.extend([Op::Native(2), Op::LoadLocal(0), Op::ReturnValues(1)]);
    let helper_entry = code.len() as u32;
    let wait_pc = helper_entry + 6;
    code.extend([
        Op::LoadLocal(0),
        Op::Push(2),
        Op::Binary(BinaryOp::MulI32),
        Op::StoreLocal(1),
        Op::LoadLocal(1),
        Op::ArgumentValue,
        Op::Native(1),
        Op::StoreLocal(0),
        Op::LoadLocal(1),
        Op::LoadLocal(0),
        Op::Binary(BinaryOp::AddI32),
        Op::ReturnValues(1),
    ]);
    let shared = Arc::new(
        Program::from_authored(Module {
            code,
            functions: vec![
                function("main", 0, 0, 1, 1),
                function("double", helper_entry, 1, 2, 1),
            ],
            natives: vec![WAIT, MESSAGE],
            texts: vec!["Bonjour, コレット — café!".into()],
            locations: BTreeMap::from([
                (
                    2,
                    SourceLocation {
                        file: "event.sym".into(),
                        line: 3,
                        column: 5,
                    },
                ),
                (
                    wait_pc,
                    SourceLocation {
                        file: "event.sym".into(),
                        line: 10,
                        column: 5,
                    },
                ),
            ]),
            ..Module::default()
        })
        .unwrap(),
    );
    assert_eq!(
        shared.authored().unwrap().texts[0],
        "Bonjour, コレット — café!"
    );
    let mut first = Vm::new(shared.clone(), 0).unwrap();
    let mut second = Vm::new(shared, 0).unwrap();
    let mut memory = Memory::default();
    memory.write(0x20, Width::S32, 42).unwrap();
    let mut host = TestHost::default();
    for vm in [&mut first, &mut second] {
        assert_eq!(
            vm.run(&mut host, &mut memory, 100).unwrap().event,
            RunEvent::Suspended { opcode: 1 }
        );
        let trace = vm.source_trace(wait_pc);
        assert_eq!(
            trace
                .iter()
                .map(|frame| frame.function.as_str())
                .collect::<Vec<_>>(),
            ["double", "main"]
        );
        assert_eq!(trace[1].location.as_ref().unwrap().line, 3);
    }
    first.complete(Some(3), &mut memory).unwrap();
    second.complete(Some(99), &mut memory).unwrap();
    for vm in [&mut first, &mut second] {
        assert_eq!(
            vm.run(&mut host, &mut memory, 100).unwrap().event,
            RunEvent::Halted
        );
        assert_eq!(
            vm.complete(Some(0), &mut memory).unwrap_err().fault,
            Fault::NotSuspended
        );
    }
    assert_eq!(first.result(), Some(vec![17]));
    assert_eq!(second.result(), Some(vec![113]));
    assert_eq!(host.calls, [14, 14, 0, 0]);
    assert_eq!(memory.read(0x20, Width::S32).unwrap(), 42);
}

#[test]
fn fixed_arrays_and_aggregate_returns_share_the_same_frames() {
    let shared = Arc::new(
        Program::from_authored(Module {
            code: vec![
                Op::Push(9),
                Op::ArgumentValue,
                Op::Push(4),
                Op::ArgumentValue,
                Op::CallFunction(1),
                Op::ReturnValues(2),
                Op::Push(1),
                Op::Push(12),
                Op::StoreLocalIndexed { base: 0, len: 2 },
                Op::Push(0),
                Op::LoadLocalIndexed { base: 0, len: 2 },
                Op::Push(1),
                Op::LoadLocalIndexed { base: 0, len: 2 },
                Op::ReturnValues(2),
            ],
            functions: vec![function("main", 0, 0, 0, 2), function("pair", 6, 2, 2, 2)],
            ..Module::default()
        })
        .unwrap(),
    );
    let mut vm = Vm::new(shared, 0).unwrap();
    vm.run(&mut TestHost::default(), &mut Memory::default(), 100)
        .unwrap();
    assert_eq!(vm.result(), Some(vec![9, 12]));
}

#[test]
fn authored_arithmetic_and_indexing_fault_instead_of_inheriting_legacy_wrapping() {
    for (code, expected) in [
        (
            vec![
                Op::Push(i32::MAX),
                Op::Push(1),
                Op::Binary(BinaryOp::AddI32),
            ],
            Fault::Overflow,
        ),
        (
            vec![Op::Push(1), Op::Push(32), Op::Binary(BinaryOp::Shl)],
            Fault::Shift,
        ),
        (
            vec![Op::Push(1), Op::Push(0), Op::Binary(BinaryOp::DivI32)],
            Fault::DivisionByZero,
        ),
        (
            vec![
                Op::Push(f32::MAX.to_bits() as i32),
                Op::Push(2f32.to_bits() as i32),
                Op::Binary(BinaryOp::MulF32),
            ],
            Fault::NonFinite,
        ),
        (
            vec![
                Op::Push(2147483648f32.to_bits() as i32),
                Op::Convert(Conversion::F32ToI32),
            ],
            Fault::Overflow,
        ),
        (
            vec![Op::Push(2), Op::LoadLocalIndexed { base: 0, len: 2 }],
            Fault::Bounds,
        ),
    ] {
        let mut code = code;
        code.push(Op::ReturnValues(1));
        let mut vm = Vm::new(program(code, 2, 1), 0).unwrap();
        assert_eq!(
            vm.run(&mut TestHost::default(), &mut Memory::default(), 100)
                .unwrap_err()
                .fault,
            expected
        );
    }
}

#[test]
fn native_declarations_reject_abi_mismatch_and_invalid_deferred_values() {
    const CHOICE: NativeDeclaration = NativeDeclaration {
        result: Some(Type::Bool),
        ..WAIT
    };
    struct ChoiceHost;
    impl Host for ChoiceHost {
        const AUTHORED_NATIVES: NativeBindings<Self> =
            NativeBindings::new().register_typed(CHOICE, |_, _, _| Ok(NativeResult::Suspend));
    }
    let shared = Arc::new(
        Program::from_authored(Module {
            code: vec![
                Op::Push(1),
                Op::ArgumentValue,
                Op::Native(1),
                Op::ReturnValues(1),
            ],
            functions: vec![function("main", 0, 0, 0, 1)],
            natives: vec![CHOICE],
            ..Module::default()
        })
        .unwrap(),
    );
    let mut memory = Memory::default();
    assert_eq!(
        Vm::new(shared.clone(), 0)
            .unwrap()
            .run(&mut TestHost::default(), &mut memory, 100)
            .unwrap_err()
            .fault,
        Fault::NativeDeclaration(1)
    );
    let mut vm = Vm::new(shared, 0).unwrap();
    vm.run(&mut ChoiceHost, &mut memory, 100).unwrap();
    assert_eq!(
        vm.complete(Some(2), &mut memory).unwrap_err().fault,
        Fault::Type(Type::Bool)
    );
    vm.complete(Some(1), &mut memory).unwrap();
    vm.run(&mut ChoiceHost, &mut memory, 100).unwrap();
    assert_eq!(vm.result(), Some(vec![1]));
}

#[test]
fn cancelling_a_suspended_frame_invalidates_late_native_completion() {
    let shared = Arc::new(
        Program::from_authored(Module {
            code: vec![
                Op::Push(2),
                Op::ArgumentValue,
                Op::Native(1),
                Op::ReturnValues(1),
            ],
            functions: vec![function("main", 0, 0, 0, 1)],
            natives: vec![WAIT],
            ..Module::default()
        })
        .unwrap(),
    );
    let mut vm = Vm::new(shared, 0).unwrap();
    let mut memory = Memory::default();
    let mut host = TestHost::default();
    vm.run(&mut host, &mut memory, 100).unwrap();
    assert!(vm.cancel());
    assert!(!vm.cancel());
    assert_eq!(
        vm.complete(Some(5), &mut memory).unwrap_err().fault,
        Fault::Cancelled
    );
    assert_eq!(
        vm.run(&mut host, &mut memory, 100).unwrap_err().fault,
        Fault::Cancelled
    );
    assert_eq!(host.calls, [2]);
    assert_eq!(vm.value_depth(), 0);
    assert_eq!(vm.argument_depth(), 0);
    assert_eq!(vm.result(), None);
}

#[test]
fn joined_results_resume_once_and_cancellation_invalidates_the_join() {
    struct PendingChild;
    impl Host for PendingChild {
        fn join(&mut self, _: i32) -> Result<Option<Vec<i32>>, String> {
            Ok(None)
        }
    }
    let shared = program(
        vec![
            Op::Push(7),
            Op::JoinTask { results: 2 },
            Op::ReturnValues(2),
        ],
        0,
        2,
    );
    let mut memory = Memory::default();
    let mut vm = Vm::new(shared.clone(), 0).unwrap();
    assert_eq!(
        vm.run(&mut PendingChild, &mut memory, 100).unwrap().event,
        RunEvent::SuspendedTask { handle: 7 }
    );
    assert_eq!(vm.complete_task(&[1]).unwrap_err().fault, Fault::CallShape);
    assert_eq!(vm.value_depth(), 0);
    vm.complete_task(&[2, 3]).unwrap();
    assert_eq!(
        vm.complete_task(&[2, 3]).unwrap_err().fault,
        Fault::TaskCompletion
    );
    vm.run(&mut PendingChild, &mut memory, 100).unwrap();
    assert_eq!(vm.result(), Some(vec![2, 3]));
    let mut vm = Vm::new(shared, 0).unwrap();
    vm.run(&mut PendingChild, &mut memory, 100).unwrap();
    vm.cancel();
    assert_eq!(
        vm.complete_task(&[2, 3]).unwrap_err().fault,
        Fault::Cancelled
    );
}

#[test]
fn message_arguments_validate_resource_ids_typed_substitutions_and_padding() {
    let character = Type::TextReference {
        name: "Character",
        kind: TextReferenceKind::Character,
    };
    for (id, argument, padding, expected) in [
        (1, 0, 0, Fault::Type(Type::Message)),
        (0, -1, 0, Fault::Type(character)),
        (0, 1, 1, Fault::Type(Type::Message)),
    ] {
        let mut words = vec![0; Type::Message.slots()];
        words[0] = id;
        words[1] = argument;
        words[2] = padding;
        let mut code = Vec::new();
        for word in words {
            code.extend([Op::Push(word), Op::ArgumentValue]);
        }
        code.extend([Op::Native(MESSAGE.opcode), Op::ReturnValues(0)]);
        let program = Arc::new(
            Program::from_authored(Module {
                code,
                functions: vec![function("main", 0, 0, 0, 0)],
                natives: vec![MESSAGE],
                texts: vec!["{who}".into()],
                templates: BTreeMap::from([(
                    0,
                    MessageTemplate {
                        parameters: vec![MessageParameter {
                            name: "who".into(),
                            ty: character,
                        }],
                        parts: vec![MessagePart::Argument(0)],
                    },
                )]),
                ..Module::default()
            })
            .unwrap(),
        );
        let mut vm = Vm::new(program, 0).unwrap();
        let mut host = TestHost::default();
        assert_eq!(
            vm.run(&mut host, &mut Memory::default(), 100)
                .unwrap_err()
                .fault,
            expected
        );
        assert!(host.calls.is_empty());
    }
}

#[test]
fn immutable_strings_validate_separately_from_messages_across_calls_and_native_results() {
    const ECHO: NativeDeclaration = NativeDeclaration {
        name: "echo",
        opcode: 3,
        parameters: &[Type::String],
        result: Some(Type::String),
        suspends: false,
    };
    #[derive(Default)]
    struct Echo {
        result: Option<i32>,
        calls: usize,
    }
    impl Host for Echo {
        const AUTHORED_NATIVES: NativeBindings<Self> =
            NativeBindings::new().register_typed(ECHO, |host: &mut Self, args, _| {
                host.calls += 1;
                Ok(NativeResult::Continue(Some(host.result.unwrap_or(args[0]))))
            });
    }
    let module = || {
        let mut main = function("main", 0, 1, 1, 1);
        main.parameter_layout = ValueLayout::Scalar(Type::String);
        let mut helper = function("helper", 4, 1, 1, 1);
        helper.parameter_layout = ValueLayout::Scalar(Type::String);
        Module {
            code: vec![
                Op::LoadLocal(0),
                Op::ArgumentValue,
                Op::CallFunction(1),
                Op::ReturnValues(1),
                Op::LoadLocal(0),
                Op::ArgumentValue,
                Op::Native(ECHO.opcode),
                Op::ReturnValues(1),
            ],
            functions: vec![main, helper],
            natives: vec![ECHO],
            strings: vec!["dialogue".into(), "模型".into()],
            texts: vec!["dialogue".into()],
            ..Module::default()
        }
    };
    let program = Arc::new(Program::from_authored(module()).unwrap());
    for value in [-1, 2] {
        assert_eq!(
            Vm::validate_arguments(&program, 0, &[value])
                .unwrap_err()
                .fault,
            Fault::Type(Type::String)
        );
    }
    let mut vm = Vm::with_arguments(program.clone(), 0, &[1]).unwrap();
    let mut host = Echo::default();
    assert_eq!(
        vm.run(&mut host, &mut Memory::default(), 100)
            .unwrap()
            .event,
        RunEvent::Halted
    );
    assert_eq!(vm.result(), Some(vec![1]));
    assert_eq!(host.calls, 1);
    assert_eq!(program.authored().unwrap().strings[1], "模型");
    for result in [-1, 2] {
        let mut vm = Vm::with_arguments(program.clone(), 0, &[1]).unwrap();
        let mut host = Echo {
            result: Some(result),
            calls: 0,
        };
        assert_eq!(
            vm.run(&mut host, &mut Memory::default(), 100)
                .unwrap_err()
                .fault,
            Fault::Type(Type::String)
        );
        assert_eq!(host.calls, 1);
    }
    let mut invalid = module();
    invalid.code[0] = Op::Push(2);
    let mut vm =
        Vm::with_arguments(Arc::new(Program::from_authored(invalid).unwrap()), 0, &[0]).unwrap();
    let mut host = Echo::default();
    assert_eq!(
        vm.run(&mut host, &mut Memory::default(), 100)
            .unwrap_err()
            .fault,
        Fault::Type(Type::String)
    );
    assert_eq!(host.calls, 0);
    let mut duplicate = module();
    duplicate.strings.push("模型".into());
    assert!(Program::from_authored(duplicate).is_err());
}

#[test]
fn string_native_waits_reject_invalid_completion_without_consuming_the_wait() {
    const READ: NativeDeclaration = NativeDeclaration {
        name: "read",
        opcode: 3,
        parameters: &[],
        result: Some(Type::String),
        suspends: true,
    };
    struct Reader;
    impl Host for Reader {
        const AUTHORED_NATIVES: NativeBindings<Self> =
            NativeBindings::new().register_typed(READ, |_, _, _| Ok(NativeResult::Suspend));
    }
    let program = Arc::new(
        Program::from_authored(Module {
            code: vec![Op::Native(READ.opcode), Op::ReturnValues(1)],
            functions: vec![function("main", 0, 0, 0, 1)],
            natives: vec![READ],
            strings: vec!["ready".into()],
            ..Module::default()
        })
        .unwrap(),
    );
    let mut vm = Vm::new(program, 0).unwrap();
    let mut memory = Memory::default();
    assert_eq!(
        vm.run(&mut Reader, &mut memory, 10).unwrap().event,
        RunEvent::Suspended { opcode: 3 }
    );
    assert_eq!(
        vm.complete(Some(1), &mut memory).unwrap_err().fault,
        Fault::Type(Type::String)
    );
    vm.complete(Some(0), &mut memory).unwrap();
    vm.run(&mut Reader, &mut memory, 10).unwrap();
    assert_eq!(vm.result(), Some(vec![0]));
}
