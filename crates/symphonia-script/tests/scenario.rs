use symphonia_script::scenario;

fn header(code_base: u16, default_pc: u16, auxiliary: u16, records: u16) -> Vec<u8> {
    [code_base, default_pc, auxiliary, records]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect()
}

#[test]
fn canonical_encodings_round_trip() {
    let source = r".scenario
.code_base 4
.word 4
.word 0
.word 0
.word 0
entry:
    push.s8 -1
    push.s16 -2
    push.s32 -3
    push.u16 0xFFFE
    load.s8 0x0010
    load.s16 0x0020
    load.s32 0x0030
    arg
    calc 0x01
    proc 0x0A
    end
.byte 0xAB
";
    let data = scenario::assemble(source).unwrap();
    let (rendered, analysis) = scenario::disassemble(&data).unwrap();
    assert_eq!(scenario::assemble(&rendered).unwrap(), data);
    assert!(!analysis.has_errors());
    assert!(
        data.windows(6)
            .any(|bytes| bytes == [0x02, 0x00, 0xff, 0xfd, 0xff, 0xff])
    );
}

#[test]
fn control_flow_uses_absolute_word_pc_labels() {
    let source = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\nstart:\nbranch_false done\npush.s8 1\ndone:\nend\n";
    let data = scenario::assemble(source).unwrap();
    let (rendered, analysis) = scenario::disassemble(&data).unwrap();
    assert!(rendered.contains("branch_false L_0003"));
    assert!(rendered.contains("L_0003:"));
    assert_eq!(analysis.instructions[&0].operands, [3]);
    assert_eq!(scenario::assemble(&rendered).unwrap(), data);
}

#[test]
fn registry_is_an_overlay() {
    let mut data = header(0, 4, 0, 1);
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&0x20ff_0000_u32.to_be_bytes());
    data.extend_from_slice(&4_u32.to_be_bytes());
    let (rendered, analysis) = scenario::disassemble(&data).unwrap();
    assert!(analysis.records[0].active());
    assert_eq!(analysis.instructions[&6].mnemonic, "end");
    assert!(rendered.contains("registry[0]: kind=1"));
    assert_eq!(scenario::assemble(&rendered).unwrap(), data);
}

#[test]
fn noncanonical_encoding_stays_raw() {
    let mut data = header(4, 0, 0, 0);
    data.extend_from_slice(&0x2101_u16.to_be_bytes());
    data.extend_from_slice(&0x20ff_u16.to_be_bytes());
    let (rendered, analysis) = scenario::disassemble(&data).unwrap();
    assert!(rendered.contains(".word 0x2101"));
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|item| item.code == "noncanonical-procedure")
    );
    assert_eq!(scenario::assemble(&rendered).unwrap(), data);
}

#[test]
fn odd_trailing_byte_is_preserved() {
    let mut data = header(4, 0, 0, 0);
    data.extend_from_slice(&0x20ff_u16.to_be_bytes());
    data.push(0x7f);
    let (rendered, _) = scenario::disassemble(&data).unwrap();
    assert!(rendered.contains(".byte 0x7F"));
    assert_eq!(scenario::assemble(&rendered).unwrap(), data);
}

#[test]
fn unresolved_label_is_rejected() {
    let source = ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\njump missing\n";
    assert!(
        scenario::assemble(source)
            .unwrap_err()
            .to_string()
            .contains("invalid integer")
    );
}
