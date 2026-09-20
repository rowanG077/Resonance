use super::*;

#[test]
fn positions_keep_signed_units_and_twelve_byte_stride() {
    let bytes: Vec<_> = [
        0i16, 45, -100, -80, 120, 777, 70, 47, 999, 80, -32768, 555, 70, 28, 136, 0, -1,
    ]
    .into_iter()
    .flat_map(i16::to_be_bytes)
    .collect();
    let (track, loops) = commands(&bytes).unwrap();
    assert!(!loops);
    assert!(matches!(
        track.as_slice(),
        [
            TimedCommand {
                tick: 0,
                command: ActionCommand::SetPosition([-100, -80, 120])
            },
            TimedCommand {
                tick: 70,
                command: ActionCommand::OffsetPosition {
                    height: 80,
                    retreat: -32768
                }
            },
            TimedCommand {
                tick: 70,
                command: ActionCommand::Sound(136)
            },
        ]
    ));
    assert!(commands(&bytes[..11]).is_err());
    assert!(commands(&bytes[..23]).is_err());
    let bytes: Vec<_> = [10i16, 46, 999, -80, 125, 555, -1]
        .into_iter()
        .flat_map(i16::to_be_bytes)
        .collect();
    assert!(matches!(
        commands(&bytes).unwrap().0[0],
        TimedCommand {
            tick: 10,
            command: ActionCommand::PositionFromTarget {
                height: -80,
                retreat: 125
            }
        }
    ));
    assert!(commands(&bytes[..11]).is_err());
}

#[test]
#[ignore = "requires the original extracted disc"]
fn original_wind_enemy_position_tracks_and_interpreter() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let begin = word(&usual, table + 205 * 4).unwrap() as usize;
    let end = word(&usual, table + 206 * 4).unwrap() as usize;
    let enemy = compression::decode(&archive[begin..end]).unwrap();
    let rows = usize::from(half(&enemy, 10).unwrap());
    assert_eq!(
        (usize::from(half(&enemy, 12).unwrap()) - rows) / ACTION_BYTES,
        7
    );
    let bank = usize::from(half(&enemy, 14).unwrap());
    for id in [2, 3] {
        let row = &enemy[rows + id * ACTION_BYTES..];
        assert_eq!(half(row, 0x1a).unwrap(), 52);
        let (track, loops) = commands(&enemy[bank + 104..]).unwrap();
        assert!(!loops);
        assert_eq!(
            track.iter().map(|step| step.tick).collect::<Vec<_>>(),
            [5, 10, 28, 28, 28, 28, 62, 62, 70, 70]
        );
        assert!(matches!(track[8].command, ActionCommand::Sound(136)));
        assert!(matches!(
            track[9].command,
            ActionCommand::OffsetPosition {
                height: 80,
                retreat: 0
            }
        ));
        assert_eq!(half(row, 14).unwrap(), 90);
    }
    // The third position track continues with opcode41, decoded independently.
    let row = &enemy[rows + 4 * ACTION_BYTES..];
    let track = &enemy[bank + usize::from(half(row, 0x1a).unwrap()) * 2..];
    assert_eq!(&track[16..28], &[0, 15, 0, 45, 0, 0, 0, 80, 0, 0, 0, 0]);
    assert_eq!(&track[28..34], &[0, 15, 0, 41, 7, 8]);
    let begin = word(&usual, table + 206 * 4).unwrap() as usize;
    let end = word(&usual, table + 207 * 4).unwrap() as usize;
    let enemy = compression::decode(&archive[begin..end]).unwrap();
    let rows = usize::from(half(&enemy, 10).unwrap());
    assert_eq!(
        (usize::from(half(&enemy, 12).unwrap()) - rows) / ACTION_BYTES,
        5
    );
    let row = &enemy[rows + 2 * ACTION_BYTES..];
    assert_eq!(half(row, 0x1a).unwrap(), 50);
    let source = &enemy[usize::from(half(&enemy, 14).unwrap()) + 100..];
    let (track, loops) = commands(source).unwrap();
    assert!(!loops);
    assert_eq!(
        track.iter().map(|step| step.tick).collect::<Vec<_>>(),
        [10, 10, 50, 54, 58, 70, 70]
    );
    assert!(
        matches!(track[1].command, ActionCommand::PositionFromTarget { height, retreat }
        if height == half(source, 8 + 6).unwrap() as i16 && retreat == half(source, 8 + 8).unwrap() as i16)
    );
    assert!(matches!(
        track[6].command,
        ActionCommand::OffsetPosition { .. }
    ));

    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    // Pin operand widths, cached bearing, origin, height write and stride.
    for (at, instruction) in [
        (0x2cc10, 0x3bff000c),
        (0x2cc50, 0x807c1990),
        (0x2cc5c, 0x386318c0),
        (0x2cc90, 0x2c04002f),
        (0x2cc98, 0xa87e0008),
        (0x2cca8, 0x7c0300d0),
        (0x2ccac, 0x387c18fc),
        (0x2ccd0, 0x387c18c0),
        (0x2cce0, 0xa89e0006),
        (0x2cd04, 0xd01c18c4),
        (0x2cd0c, 0xe01ed004),
        (0x2cd10, 0xd01c18c0),
        (0x2cd14, 0xe01ed006),
        (0x2cd18, 0xd01c18c4),
        (0x2cd1c, 0xe01ed008),
        (0x2cd20, 0xd01c18c8),
    ] {
        assert_eq!(
            word(rel.at((1, at)).unwrap(), 0).unwrap(),
            instruction,
            "{at:#x}"
        );
    }
    assert_eq!(float(rel.at((4, 0x2800)).unwrap(), 0).unwrap(), 0.5);
}
