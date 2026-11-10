use std::sync::Arc;
use symphonia_script::{Program, Width, message::Expression};
use symphonia_script_vm::{Host, Memory, RunEvent, Vm, message::evaluate};

struct EmptyHost;
impl Host for EmptyHost {}

#[test]
fn edited_expressions_share_script_arithmetic_reference_and_write_semantics() {
    let expression: Expression = serde_json::from_str(
        r#"{
        "add":[{"post_increment":[{"state_i16":100}]},{"state_i16":100}]
    }"#,
    )
    .unwrap();
    let mut memory = Memory::default();
    memory.write(100, Width::S16, 32767).unwrap();
    assert_eq!(evaluate(&expression, &mut memory).unwrap(), -1);
    assert_eq!(memory.read(100, Width::S16).unwrap(), -32768);
    // Indexing uses the referenced value's signed width and an element offset.
    let expression: Expression =
        serde_json::from_str(r#"{"assign":[{"indexed_reference":[{"state_i16":100},2]},123]}"#)
            .unwrap();
    assert_eq!(evaluate(&expression, &mut memory).unwrap(), 123);
    assert_eq!(memory.read(104, Width::S16).unwrap(), 123);
    for (text, expected) in [
        (r#"{"divide":[-7,0]}"#, -1),
        (r#"{"add":[2147483647,1]}"#, i32::MIN),
        (r#"{"shift_left":[1,32]}"#, 0),
        (r#"{"shift_right":[-4,32]}"#, -1),
    ] {
        let expression = serde_json::from_str(text).unwrap();
        assert_eq!(evaluate(&expression, &mut memory).unwrap(), expected);
    }
}

#[test]
#[ignore = "requires dialogue audit; compares every original expression without cooking media"]
fn all_imported_message_expressions_match_the_original_vm() {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_DIALOGUE_REVIEW").expect("set RESONANCE_DIALOGUE_REVIEW"),
    );
    let audit: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("audit.json")).unwrap()).unwrap();
    let mut checked = 0;
    let mut errors = 0;
    let mut state_expressions = 0;
    fn uses_state(expression: &Expression) -> bool {
        match expression {
            Expression::State { .. } => true,
            Expression::Calculate { arguments, .. } | Expression::Call { arguments, .. } => {
                arguments.iter().any(uses_state)
            }
            Expression::Constant(_) => false,
        }
    }
    for row in audit["expressions"].as_array().unwrap() {
        let bytes: Vec<u8> = serde_json::from_value(row["bytes"].clone()).unwrap();
        let expression = Expression::from_native(&bytes).unwrap();
        let encoded = serde_json::to_vec(&expression).unwrap();
        let expression: Expression = serde_json::from_slice(&encoded).unwrap();
        let mut source = vec![0, 4, 0, 0, 0, 0, 0, 0];
        source.extend_from_slice(&bytes);
        let program = Arc::new(Program::decode(&source).unwrap());
        let state = uses_state(&expression);
        state_expressions += usize::from(state);
        for &pattern in if state {
            &[0u8, 1, 0x7f, 0xff][..]
        } else {
            &[0u8][..]
        } {
            let mut original = Memory::default();
            let mut edited = Memory::default();
            // Deliberately include negative and oversized indices, so bounds and
            // signed reads are compared as well as ordinary successful values.
            for offset in if state { 0..65536u32 } else { 0..0 } {
                let offset = offset as u16;
                original
                    .write(offset, Width::S8, i32::from(pattern))
                    .unwrap();
                edited.write(offset, Width::S8, i32::from(pattern)).unwrap();
            }
            let mut vm = Vm::new(program.clone(), 0).unwrap();
            let before = vm.run(&mut EmptyHost, &mut original, 1024);
            let after = evaluate(&expression, &mut edited);
            match before {
                Ok(result) => {
                    assert_eq!(result.event, RunEvent::Halted);
                    assert_eq!(after.unwrap(), vm.expression().unwrap(), "{expression:?}");
                }
                Err(_) => {
                    assert!(after.is_err(), "{expression:?}");
                    errors += 1;
                }
            }
            for offset in if state { 0..65536u32 } else { 0..0 } {
                let offset = offset as u16;
                assert_eq!(
                    original.read(offset, Width::S8).unwrap(),
                    edited.read(offset, Width::S8).unwrap()
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 8543 + state_expressions * 3);
    println!(
        "All {checked} expression/memory comparisons preserve values, errors and every memory byte ({state_expressions} state expressions; {errors} error cases)."
    );
}

#[test]
fn native_message_calls_keep_eager_argument_order_and_the_shared_result_cache() {
    use symphonia_script_vm::{NativeResult, message::evaluate_with_host};
    let call = |name: &str, argument| Expression::Call {
        function: name.into(),
        arguments: vec![argument],
    };
    let expression = Expression::Calculate {
        operator: 0x31,
        arguments: vec![
            call(
                "first",
                Expression::Calculate {
                    operator: 1,
                    arguments: vec![Expression::State {
                        offset: 100,
                        width: Width::S16,
                    }],
                },
            ),
            call(
                "second",
                Expression::State {
                    offset: 100,
                    width: Width::S16,
                },
            ),
        ],
    };
    let mut memory = Memory::default();
    memory.write(100, Width::S16, 7).unwrap();
    let mut calls = Vec::new();
    let value = evaluate_with_host(&expression, &mut memory, &mut |name, args, _| {
        calls.push((name.to_owned(), args.to_vec()));
        Ok(NativeResult::Continue(Some(args[0] * 10)))
    })
    .unwrap();
    assert_eq!(value, 150);
    assert_eq!(
        calls,
        [("first".into(), vec![7]), ("second".into(), vec![8])]
    );
    assert_eq!(memory.read(100, Width::S16).unwrap(), 8);
    assert_eq!(memory.read(0x20, Width::S32).unwrap(), 80);
}

#[test]
fn a_resultless_native_message_root_uses_the_terminal_depth_alias() {
    use symphonia_script_vm::{Fault, NativeResult, message::evaluate_with_host};
    let call = Expression::Call {
        function: "no_result".into(),
        arguments: vec![],
    };
    let mut memory = Memory::default();
    memory.write(0x20, Width::S32, 77).unwrap();
    assert_eq!(
        evaluate_with_host(&call, &mut memory, &mut |_, _, _| Ok(
            NativeResult::LegacyNoResult
        ))
        .unwrap(),
        -1
    );
    assert_eq!(memory.read(0x20, Width::S32).unwrap(), 77);
    let nested = Expression::Calculate {
        operator: 0x31,
        arguments: vec![Expression::Constant(5), call],
    };
    assert!(matches!(
        evaluate_with_host(&nested, &mut memory, &mut |_, _, _| Ok(
            NativeResult::LegacyNoResult
        )),
        Err(Fault::ValueUnderflow)
    ));
}
