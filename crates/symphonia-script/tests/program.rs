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
