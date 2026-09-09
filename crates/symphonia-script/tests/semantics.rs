use symphonia_script::semantics::{self, NativeRegistry, OperatorForm};

#[test]
fn recovered_calculator_table_is_complete() {
    let specs = (0..=u8::MAX)
        .filter_map(semantics::calculator)
        .collect::<Vec<_>>();
    assert_eq!(specs.len(), 39);
    assert_eq!(semantics::calculator(0x10).unwrap().symbol, "=");
    assert_eq!(
        semantics::calculator(0x19).unwrap().form,
        OperatorForm::CompoundAssignment
    );
    assert_eq!(
        semantics::calculator(0x39).unwrap().form,
        OperatorForm::Binary
    );
    assert!(semantics::calculator(0x30).is_none());
    assert_eq!(
        semantics::calculator_by_name("bitwise_or").unwrap().opcode,
        0x37
    );
}

#[test]
fn native_registry_is_data_driven_and_validated() {
    let registry = NativeRegistry::from_json(
        r#"{"schema_version":1,"game":"test","procedures":[{"opcode":10,"name":"play_sound","arguments":["sound_id"]}]}"#,
    )
    .unwrap();
    assert_eq!(registry.get(10).unwrap().name, "play_sound");
    assert!(
        NativeRegistry::from_json(
            r#"{"schema_version":1,"game":"test","procedures":[{"opcode":10,"name":"not valid"}]}"#
        )
        .is_err()
    );

    let retail = NativeRegistry::gqseaf();
    assert_eq!(retail.iter().count(), 237);
    let expected = (1_u8..=0xF4)
        .filter(|opcode| !matches!(*opcode, 0x05..=0x09 | 0x5B | 0xEF))
        .collect::<Vec<_>>();
    assert_eq!(
        retail.iter().map(|call| call.opcode).collect::<Vec<_>>(),
        expected
    );
    assert!(retail.get(0x05).is_none());
    assert!(retail.get(0xEF).is_none());
    assert!(retail.get(0x01).unwrap().control_flow);
    assert_eq!(retail.get(0xC6).unwrap().arguments.len(), 21);
    let change_field = retail.get(0x40).unwrap();
    assert_eq!(change_field.handler, "fn_800500D4");
    assert_eq!(change_field.arguments.len(), 5);
    assert_eq!(retail.get(0xD7).unwrap().name, "motion_command");
    assert_eq!(retail.get(0xD7).unwrap().arguments[0], "operation");
    assert_eq!(retail.get_by_name("native_d7").unwrap().opcode, 0xD7);
    assert_eq!(retail.get(0xE1).unwrap().name, "configure_input_binding");
}

#[test]
fn control_surface_covers_every_dispatch_entry() {
    let registry = NativeRegistry::gqseaf();
    let catalog = registry.control_surface();
    assert_eq!(catalog.len(), 237);
    assert_eq!(catalog.first().map(|entry| entry.opcode), Some(1));
    assert!(
        catalog
            .iter()
            .any(|entry| entry.domain == "actors_and_objects")
    );
    assert!(
        catalog
            .iter()
            .any(|entry| entry.domain == "dialogue_and_yield")
    );
    assert!(
        catalog
            .iter()
            .any(|entry| entry.domain == "interpreter.control_flow")
    );
}

#[test]
fn vm_constant_catalog_is_valid_and_complete_at_the_encoding_layer() {
    let constants: serde_json::Value =
        serde_json::from_str(symphonia_script::GQSEAF_VM_CONSTANTS).unwrap();
    assert_eq!(constants["calculator"].as_array().unwrap().len(), 39);
    assert_eq!(constants["limits"]["argument_stack"], 64);
    assert!(
        constants["yield_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["value"] == 21 && entry["returns_value"] == true)
    );
}

#[test]
fn native_ids_cover_the_catalog_without_interpreter_control_flow() {
    use symphonia_script::NativeCall;
    assert_eq!(std::mem::size_of::<NativeCall>(), 1);
    let catalog = NativeRegistry::gqseaf();
    for opcode in 0..=u8::MAX {
        let expected = catalog.get(opcode).filter(|entry| !entry.control_flow);
        let call = NativeCall::try_from(opcode).ok();
        assert_eq!(call.map(u8::from), expected.map(|entry| entry.opcode));
    }
    assert_eq!(
        NativeCall::ALL.len(),
        catalog.iter().filter(|entry| !entry.control_flow).count()
    );
    assert_eq!(u8::from(NativeCall::ChangeField), 0x40);
    assert_eq!(u8::from(NativeCall::PlaySound), 0xe0);
}
