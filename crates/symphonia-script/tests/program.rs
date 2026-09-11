use symphonia_script::Program;
fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_be_bytes()).collect()
}
#[test]
fn execution_rejects_unverified_control_flow_and_headers() {
    for words in [
        vec![4, 0, 0, 1, 0x20ff],         // truncated registry
        vec![4, 0, 0, 0, 0x2001, 1],      // jump into its own operand
        vec![4, 0, 0, 0, 0x3030, 0x20ff], // unknown calculator
        vec![4, 0, 0, 0, 0x0200, 1],      // truncated literal
        vec![4, 0, 99, 0, 0x20ff],        // auxiliary outside input
    ] {
        assert!(Program::decode(&bytes(&words)).is_err(), "{words:x?}");
    }
}

#[test]
fn registry_overlay_is_valid_executable_code() {
    let data = bytes(&[0, 4, 0, 1, 0, 1, 0x20ff, 0, 0, 4]);
    let program = Program::decode(&data).unwrap();
    assert_eq!(program.entry(), 4);
    assert_eq!(program.event(1, 0x20ff0000), Some(4));
}

#[test]
fn duplicate_registry_keys_keep_the_first_entry() {
    let data = bytes(&[
        16, 0, 0, 2, 0, 2, 0, 3000, 0, 1, 0, 2, 0, 3000, 0, 2, 0x20ff, 0x20ff, 0x20ff,
    ]);
    assert_eq!(Program::decode(&data).unwrap().event(2, 3000), Some(1));
    let mut invalid = data;
    invalid[30..32].copy_from_slice(&100u16.to_be_bytes());
    assert!(
        Program::decode(&invalid).is_err(),
        "unreachable duplicates must still have valid targets"
    );
}
