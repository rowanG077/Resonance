use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::authored::{NativeDeclaration, Type};
use symphonia_script_compiler::{compile, format};
use symphonia_script_vm::{Fault, Host, Memory, NativeBindings, NativeResult, Vm};

const TAKE: NativeDeclaration = NativeDeclaration {
    name: "native::take",
    opcode: 1,
    parameters: &[Type::String, Type::I32, Type::Message],
    result: None,
    suspends: false,
};
#[derive(Default)]
struct Capture(Vec<i32>);
impl Host for Capture {
    const AUTHORED_NATIVES: NativeBindings<Self> =
        NativeBindings::new().register_typed(TAKE, |host, args, _| {
            host.0 = args.to_vec();
            Ok(NativeResult::Continue(None))
        });
}

#[test]
fn imported_constants_use_scalar_arguments_and_strings_stay_separate_from_messages() {
    let sources = BTreeMap::from([
        (
            "symbols".into(),
            r#"script library;
            pub const Tail: string = "Bone_sippo01";
            pub const Wing: string = "羽";
            pub const TailAlias: string = Tail;
            pub const TailVisible: i32 = 147;
        "#
            .into(),
        ),
        (
            "main".into(),
            r#"script model;
            use symbols;
            use native;
            pub fn main() -> i32 {
                let text: string = name();
                let mut flag = symbols::TailVisible;
                flag += 1;
                native::take(text, symbols::TailVisible, "Bone_sippo01");
                if text != "Bone_sippo01" || symbols::Tail != symbols::TailAlias {
                    return 0;
                }
                return flag;
            }
            fn name() -> string { return symbols::Tail; }
        "#
            .into(),
        ),
    ]);
    let program = Arc::new(compile("main", &sources, &[TAKE]).unwrap().program);
    let mut host = Capture::default();
    let mut vm = Vm::new(program.clone(), program.entry()).unwrap();
    vm.run(&mut host, &mut Memory::default(), 1000).unwrap();
    assert_eq!(vm.result().unwrap(), [148]);
    let module = program.authored().unwrap();
    assert_eq!(module.strings, ["Bone_sippo01", "羽"]);
    assert_eq!(module.texts, ["Bone_sippo01"]);
    assert_eq!(host.0[0..2], [0, 147]);
    assert_eq!(host.0[2..], [0; Type::Message.slots()]);
    for (name, source) in &sources {
        let formatted = format(name, source).unwrap();
        assert_eq!(format(name, &formatted).unwrap(), formatted);
    }
}

#[test]
fn declaration_only_libraries_validate_constants_without_synthetic_executable_code() {
    let source =
        "script library; pub const TailVisible: i32 = 147; pub const Tail: string = \"tail\";";
    let program = Arc::new(
        compile(
            "catalogue",
            &BTreeMap::from([("catalogue".into(), source.into())]),
            &[],
        )
        .unwrap()
        .program,
    );
    let module = program.authored().unwrap();
    assert!(module.code.is_empty() && module.functions.is_empty());
    assert_eq!(module.strings, ["tail"]);
    assert_eq!(
        Vm::new(program.clone(), program.entry())
            .err()
            .unwrap()
            .fault,
        Fault::Pc
    );
    for source in [
        "script library; pub const Tail: i32 = \"bad\";",
        "script library; pub const Tail: string = 147;",
        "script field; pub const TailVisible: i32 = 147;",
        "script model; pub const Tail: string = \"tail\";",
        "script library; const Tail: string = \"tail\"; fn main() { let value: Message = Tail; }",
        "script library; fn main() { let value = \"tail\" + \"wing\"; }",
    ] {
        assert!(
            compile(
                "catalogue",
                &BTreeMap::from([("catalogue".into(), source.into())]),
                &[]
            )
            .is_err()
        );
    }
}
