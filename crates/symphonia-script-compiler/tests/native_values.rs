use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::{
    Op,
    authored::{NativeDeclaration, NativeField, Type},
};
use symphonia_script_compiler::compile;
use symphonia_script_vm::{Fault, Host, Memory, NativeBindings, NativeResult, Vm};

const ROW: Type = Type::Record {
    name: "data::Row",
    fields: &[
        NativeField {
            name: "age",
            ty: Type::Ticks,
        },
        NativeField {
            name: "enabled",
            ty: Type::Bool,
        },
        NativeField {
            name: "scale",
            ty: Type::F32,
        },
    ],
};
const ROWS: Type = Type::Array {
    element: &ROW,
    len: 2,
};
const SETTINGS: Type = Type::Record {
    name: "data::Settings",
    fields: &[
        NativeField {
            name: "rows",
            ty: ROWS,
        },
        NativeField {
            name: "count",
            ty: Type::I32,
        },
    ],
};
const GET: NativeDeclaration = NativeDeclaration {
    name: "data::settings",
    opcode: 1,
    parameters: &[],
    result: Some(SETTINGS),
    suspends: false,
};
const TABLE: NativeDeclaration = NativeDeclaration {
    name: "data::table",
    opcode: 2,
    parameters: &[],
    result: Some(ROWS),
    suspends: false,
};
const PUT: NativeDeclaration = NativeDeclaration {
    name: "data::put",
    opcode: 3,
    parameters: &[Type::I32, SETTINGS, Type::I32],
    result: None,
    suspends: false,
};

fn rows() -> Vec<i32> {
    vec![4, 1, 1.5f32.to_bits() as i32, 9, 0, 2.5f32.to_bits() as i32]
}
fn settings() -> Vec<i32> {
    let mut values = rows();
    values.push(2);
    values
}
#[derive(Default)]
struct Data {
    response: Option<NativeResult>,
    written: Vec<i32>,
}
impl Host for Data {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(GET, |host, _, _| {
            Ok(host
                .response
                .take()
                .unwrap_or_else(|| NativeResult::Values(settings())))
        })
        .register_typed(TABLE, |_, _, _| Ok(NativeResult::Values(rows())))
        .register_typed(PUT, |host, args, _| {
            host.written = args.to_vec();
            Ok(NativeResult::Continue(None))
        });
}
fn program(source: &str) -> Arc<symphonia_script::Program> {
    Arc::new(
        compile(
            "main",
            &BTreeMap::from([("main".into(), source.into())]),
            &[GET, TABLE, PUT],
        )
        .unwrap()
        .program,
    )
}

#[test]
fn native_records_and_arrays_use_existing_value_fields_indexing_and_flat_arguments() {
    let program = program(
        r#"
        script field;
        use data;
        use data::Row;
        pub fn main() -> i32 {
            let original = data::settings();
            let mut changed: data::Settings = original;
            changed.rows[0].age += 2ticks;
            changed.rows[1] = Row { age: 7ticks, enabled: true, scale: 3.0 };
            data::put(11, identity(changed), 13);
            let table = data::table();
            return i32(original.rows[0].age) + i32(table[1].age);
        }
        fn identity(value: data::Settings) -> data::Settings { return value; }
    "#,
    );
    let mut host = Data::default();
    let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
    vm.run(&mut host, &mut Memory::default(), 1000).unwrap();
    assert_eq!(vm.result(), Some(vec![13]));
    assert_eq!(
        host.written,
        [
            11,
            6,
            1,
            1.5f32.to_bits() as i32,
            7,
            1,
            3.0f32.to_bits() as i32,
            2,
            13
        ]
    );
}

#[test]
fn native_results_reject_wrong_shapes_domains_and_unexpected_suspension() {
    let program = program("script field; use data; pub fn main() { data::settings(); }");
    let mut bad_bool = settings();
    bad_bool[1] = 2;
    let mut bad_ticks = settings();
    bad_ticks[3] = -1;
    let mut bad_float = settings();
    bad_float[5] = f32::NAN.to_bits() as i32;
    for (response, expected) in [
        (NativeResult::Values(vec![0]), Fault::NativeResult),
        (NativeResult::Continue(Some(0)), Fault::NativeResult),
        (NativeResult::Values(bad_bool), Fault::Type(Type::Bool)),
        (NativeResult::Values(bad_ticks), Fault::Type(Type::Ticks)),
        (NativeResult::Values(bad_float), Fault::Type(Type::F32)),
        (NativeResult::Suspend, Fault::UnexpectedSuspend(GET.opcode)),
    ] {
        let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
        let mut host = Data {
            response: Some(response),
            ..Data::default()
        };
        assert_eq!(
            vm.run(&mut host, &mut Memory::default(), 100)
                .unwrap_err()
                .fault,
            expected
        );
        assert_eq!(vm.value_depth(), 0);
    }
}

#[test]
fn flattened_native_inputs_are_checked_before_dispatch() {
    let base =
        program("script field; use data; pub fn main() { data::put(11, data::settings(), 13); }");
    let mut module = base.authored().unwrap().clone();
    let mut words = vec![11];
    words.extend(settings());
    words.push(13);
    words[2] = 2; // bad Boolean in the first array element
    module.code = words
        .into_iter()
        .flat_map(|word| [Op::Push(word), Op::ArgumentValue])
        .collect();
    module
        .code
        .extend([Op::Native(PUT.opcode), Op::ReturnValues(0)]);
    module.locations.clear();
    let program = Arc::new(symphonia_script::Program::from_authored(module).unwrap());
    let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
    let mut host = Data::default();
    assert_eq!(
        vm.run(&mut host, &mut Memory::default(), 100)
            .unwrap_err()
            .fault,
        Fault::Type(Type::Bool)
    );
    assert!(host.written.is_empty());
}

#[test]
fn native_layouts_reject_duplicate_fields_oversize_and_suspendable_aggregate_results() {
    let source = BTreeMap::from([("main".into(), "script field; fn main() {}".into())]);
    for (native, expected) in [
        (
            NativeDeclaration {
                suspends: true,
                ..GET
            },
            "cannot suspend",
        ),
        (
            NativeDeclaration {
                result: Some(Type::Array {
                    element: &Type::I32,
                    len: 1025,
                }),
                ..GET
            },
            "1..=1024",
        ),
        (
            NativeDeclaration {
                result: Some(Type::Array {
                    element: &Type::I32,
                    len: 0,
                }),
                ..GET
            },
            "1..=1024",
        ),
        (
            NativeDeclaration {
                result: Some(Type::Record {
                    name: "data::Bad",
                    fields: &[
                        NativeField {
                            name: "same",
                            ty: Type::I32,
                        },
                        NativeField {
                            name: "same",
                            ty: Type::Bool,
                        },
                    ],
                }),
                ..GET
            },
            "duplicate field",
        ),
        (
            NativeDeclaration {
                result: Some(Type::Record {
                    name: "data::Bad",
                    fields: &[NativeField {
                        name: "text",
                        ty: Type::Message,
                    }],
                }),
                ..GET
            },
            "cannot return a Message",
        ),
    ] {
        let error = compile("main", &source, &[native]).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
    }
}
