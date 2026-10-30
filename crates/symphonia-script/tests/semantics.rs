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
        r#"{"schema_version":2,"game":"test","procedures":[{"opcode":10,"name":"play_sound","arguments":["sound_id"]}]}"#,
    )
    .unwrap();
    assert_eq!(registry.get(10).unwrap().name, "play_sound");
    assert!(
        NativeRegistry::from_json(
            r#"{"schema_version":2,"game":"test","procedures":[{"opcode":10,"name":"not valid"}]}"#
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
    assert_eq!(change_field.arguments.len(), 5);
    assert_eq!(retail.get(0xD7).unwrap().name, "motion_command");
    assert_eq!(retail.get(0xD7).unwrap().arguments[0], "operation");
    assert_eq!(retail.get_by_name("motion_command").unwrap().opcode, 0xD7);
    assert_eq!(retail.get(0xE1).unwrap().name, "configure_sound");
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

#[test]
fn recovered_native_names_keep_the_original_stack_contracts() {
    use symphonia_script::NativeCall;
    let registry = NativeRegistry::gqseaf();
    for (call, opcode, name, arity) in [
        (
            NativeCall::ReplacePartyMember,
            0x26,
            "replace_party_member",
            2,
        ),
        (
            NativeCall::StartBattleSimple,
            0x37,
            "start_battle_simple",
            3,
        ),
        (
            NativeCall::PlaySoundFullVolume,
            0x55,
            "play_sound_full_volume",
            2,
        ),
        (NativeCall::AdjustOverlimit, 0x76, "adjust_overlimit", 2),
        (NativeCall::OpenRenameMenu, 0x91, "open_rename_menu", 1),
        (NativeCall::FieldSystem, 0x92, "field_system", 2),
        (
            NativeCall::IsScriptResourceReady,
            0x99,
            "is_script_resource_ready",
            1,
        ),
        (
            NativeCall::SetCameraOrbitAngles,
            0xa5,
            "set_camera_orbit_angles",
            3,
        ),
        (NativeCall::GetCookingRank, 0xc9, "get_cooking_rank", 2),
        (NativeCall::GetSkitTicks, 0xf0, "get_skit_ticks", 0),
        (NativeCall::WaitSkitTicks, 0xf3, "wait_skit_ticks", 1),
    ] {
        assert_eq!(u8::from(call), opcode);
        let definition = registry.get_by_name(name).unwrap();
        assert_eq!(definition.opcode, opcode);
        assert_eq!(definition.arguments.len(), arity);
    }
    let cooking = registry.get(0xc9).unwrap();
    assert_eq!(cooking.arguments, ["character_id", "recipe_id"]);
    assert_eq!(cooking.returns_value, Some(true));
    assert!(registry.get(0x91).unwrap().yields);
    assert!(registry.get(0xf3).unwrap().yields);
}
