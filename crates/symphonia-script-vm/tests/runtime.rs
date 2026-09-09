use std::sync::Arc;
use symphonia_script::{Program, Width};
use symphonia_script_vm::*;

fn vm(words: &[u16]) -> Vm {
    let blob: Vec<_> = [4, 0, 0, 0]
        .iter()
        .chain(words)
        .flat_map(|w| u16::to_be_bytes(*w))
        .collect();
    let program = Arc::new(Program::decode(&blob).unwrap());
    Vm::new(program.clone(), program.entry()).unwrap()
}
#[derive(Default)]
struct TestHost {
    calls: Vec<(u8, Vec<i32>)>,
    suspend: bool,
}
impl Host for TestHost {
    const NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register(0x64, 2, false, |host, args, _| host.record(0x64, args))
        .register(0x66, 1, true, |host, args, _| host.record(0x66, args))
        .register(0xcc, 2, false, |host, args, _| host.record(0xcc, args));
}
impl TestHost {
    fn record(&mut self, opcode: u8, a: &[i32]) -> Result<NativeResult, String> {
        self.calls.push((opcode, a.to_vec()));
        Ok(if self.suspend || opcode == 0x64 {
            NativeResult::Suspend
        } else {
            NativeResult::Continue((opcode == 0x66).then_some(7))
        })
    }
}

#[test]
fn composed_arithmetic_updates_the_actual_stack_slot() {
    // ((2+3)*4) == 20; previously the + result was lost when * popped.
    let mut vm = vm(&[2, 3, 0x3031, 4, 0x3033, 0x3000, 0x20ff]);
    let mut memory = Memory::default();
    assert_eq!(
        vm.run(&mut TestHost::default(), &mut memory, 16)
            .unwrap()
            .event,
        RunEvent::Halted
    );
    assert_eq!(vm.expression(), Some(20));
    assert_eq!(vm.value_depth(), 0);
    assert_eq!(
        vm.run(&mut TestHost::default(), &mut memory, 0)
            .unwrap()
            .steps,
        0
    );
}
#[test]
fn unary_values_do_not_store_but_postfix_increments_do() {
    let mut memory = Memory::default();
    memory.write(0x100, Width::S32, 7).unwrap();
    let mut unary = vm(&[0x1200, 0x100, 0x3005, 0x3000, 0x20ff]);
    unary
        .run(&mut TestHost::default(), &mut memory, 16)
        .unwrap();
    assert_eq!(unary.expression(), Some(-7));
    assert_eq!(memory.read(0x100, Width::S32).unwrap(), 7);
    let mut postfix = vm(&[0x1200, 0x100, 0x3001, 0x0002, 0x3033, 0x3000, 0x20ff]);
    postfix
        .run(&mut TestHost::default(), &mut memory, 16)
        .unwrap();
    assert_eq!(postfix.expression(), Some(14));
    assert_eq!(memory.read(0x100, Width::S32).unwrap(), 8);
}
#[test]
fn nested_native_calls_preserve_outer_arguments() {
    let mut vm = vm(&[
        9, 0x3000, 0x4000, 3, 0x3000, 0x4000, 0x2066, 0x3000, 0x4000, 0x20cc, 0x20ff,
    ]);
    let mut host = TestHost::default();
    vm.run(&mut host, &mut Memory::default(), 32).unwrap();
    assert_eq!(host.calls, [(0x66, vec![3]), (0xcc, vec![9, 7])]);
    assert_eq!(vm.argument_depth(), 0);
}
#[test]
fn deferred_result_is_supplied_once_and_each_resume_gets_a_fresh_budget() {
    let mut vm = vm(&[1, 0x3000, 0x4000, 0x2066, 0x3000, 0x2001, 0]);
    let mut host = TestHost {
        suspend: true,
        ..Default::default()
    };
    let mut memory = Memory::default();
    vm.set_trace_limit(3);
    for _ in 0..100 {
        assert_eq!(
            vm.run(&mut host, &mut memory, 6).unwrap().event,
            RunEvent::Suspended { opcode: 0x66 }
        );
        assert_eq!(vm.value_depth(), 0);
        assert_eq!(
            vm.run(&mut host, &mut memory, 6).unwrap_err().fault,
            Fault::Suspended
        );
        assert_eq!(
            vm.complete(None, &mut memory).unwrap_err().fault,
            Fault::NativeResult
        );
        vm.complete(Some(42), &mut memory).unwrap();
        assert_eq!(
            vm.complete(Some(42), &mut memory).unwrap_err().fault,
            Fault::NotSuspended
        );
    }
    assert_eq!(vm.trace().len(), 3);
    assert_eq!(memory.read(0x20, Width::S32).unwrap(), 42);
}
#[test]
fn shared_memory_and_checked_indexing() {
    let mut memory = Memory::default();
    let mut writer = vm(&[0x1100, 0x100, 0x00ff, 0x3010, 0x3000, 0x20ff]);
    writer
        .run(&mut TestHost::default(), &mut memory, 16)
        .unwrap();
    let mut reader = vm(&[0x1000, 0x101, 0x3000, 0x20ff]);
    reader
        .run(&mut TestHost::default(), &mut memory, 16)
        .unwrap();
    assert_eq!(reader.expression(), Some(-1));
    let mut bad = vm(&[0x1200, 0xfffc, 1, 0x300f, 0x20ff]);
    assert_eq!(
        bad.run(&mut TestHost::default(), &mut memory, 16)
            .unwrap_err()
            .fault,
        Fault::Index
    );
    assert!(memory.read(0xffff, Width::S32).is_err());
}
#[test]
fn runaway_and_unsupported_natives_fail_with_pc() {
    let mut spin = vm(&[0x2001, 0]);
    let err = spin
        .run(&mut TestHost::default(), &mut Memory::default(), 10)
        .unwrap_err();
    assert_eq!(
        err,
        VmError {
            pc: 0,
            fault: Fault::Budget(10)
        }
    );
    let err = vm(&[0x20f0, 0x20ff])
        .run(&mut TestHost::default(), &mut Memory::default(), 10)
        .unwrap_err();
    assert_eq!(
        err,
        VmError {
            pc: 0,
            fault: Fault::Native(0xf0)
        }
    );
}
#[test]
fn branch_uses_popped_expression_and_call_returns() {
    let mut vm = vm(&[
        0, 0x3000, 0x2004, 6, 99, 0x20ff, 0x2002, 9, 0x20ff, 7, 0x3000, 0x2003,
    ]);
    vm.run(&mut TestHost::default(), &mut Memory::default(), 16)
        .unwrap();
    assert_eq!(vm.pc(), 8);
    assert_eq!(vm.expression(), Some(7));
}
#[test]
fn wide_shifts_follow_powerpc_count_semantics() {
    let mut vm = vm(&[1, 32, 0x3039, 0x3000, 0x20ff]);
    vm.run(&mut TestHost::default(), &mut Memory::default(), 16)
        .unwrap();
    assert_eq!(vm.expression(), Some(0));
}

#[test]
fn malformed_execution_cannot_escape_stack_or_arithmetic_limits() {
    let mut too_many_values = vec![1; 65];
    too_many_values.push(0x20ff);
    let mut too_many_args = Vec::new();
    for _ in 0..65 {
        too_many_args.extend([1, 0x3000, 0x4000]);
    }
    too_many_args.push(0x20ff);
    for (code, expected) in [
        (too_many_values, Fault::ValueOverflow),
        (too_many_args, Fault::ArgumentOverflow),
        (vec![0x2002, 0, 0x20ff], Fault::CallOverflow),
        (vec![0x20cc, 0x20ff], Fault::ArgumentUnderflow(2)),
        (vec![0x3000, 0x20ff], Fault::ValueUnderflow),
        (vec![1, 0, 0x3034, 0x20ff], Fault::DivisionByZero),
    ] {
        let mut runner = vm(&code);
        let error = runner
            .run(&mut TestHost::default(), &mut Memory::default(), 1024)
            .unwrap_err();
        assert_eq!(error.fault, expected);
        assert_eq!(
            runner
                .run(&mut TestHost::default(), &mut Memory::default(), 1)
                .unwrap_err()
                .fault,
            Fault::Failed
        );
    }
}

#[test]
fn registered_handlers_borrow_state_and_enforce_their_own_abi() {
    struct Borrowed<'a>(&'a mut Vec<i32>);
    impl Host for Borrowed<'_> {
        const NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
            .register(0x66, 1, true, |host, args, memory| {
                host.0.push(args[0]);
                memory
                    .write(0x100, Width::S32, args[0])
                    .map_err(|e| e.to_string())?;
                Ok(NativeResult::Continue(Some(args[0] * 2)))
            })
            .register(0xcc, 0, false, |_, _, _| {
                Ok(NativeResult::Continue(Some(1)))
            });
    }
    struct PureExpression;
    impl Host for PureExpression {}

    let code = [3, 0x3000, 0x4000, 0x2066, 0x3000, 0x20ff];
    let mut state = Vec::new();
    let mut memory = Memory::default();
    let mut runner = vm(&code);
    runner
        .run(&mut Borrowed(&mut state), &mut memory, 16)
        .unwrap();
    assert_eq!(state, [3]);
    assert_eq!(runner.expression(), Some(6));
    assert_eq!(memory.read(0x100, Width::S32).unwrap(), 3);
    assert_eq!(
        vm(&code)
            .run(&mut PureExpression, &mut memory, 16)
            .unwrap_err()
            .fault,
        Fault::Native(0x66)
    );
    assert_eq!(
        vm(&[0x20cc, 0x20ff])
            .run(&mut Borrowed(&mut state), &mut memory, 16)
            .unwrap_err()
            .fault,
        Fault::NativeResult
    );
}

#[test]
#[should_panic(expected = "duplicate native binding")]
fn duplicate_registration_cannot_replace_an_existing_handler() {
    TestHost::NATIVES.register(0x66, 0, false, |_, _, _| Ok(NativeResult::Continue(None)));
}
