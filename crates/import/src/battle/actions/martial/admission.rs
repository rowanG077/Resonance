//! A missing callback is valid only for a verified shared track dispatcher.
use super::*;

const SHARED: usize = 0x3823c;
const SELECTED: usize = 0x38118;
const UPDATE: usize = 0x37fd4;

const SHARED_BODY: &[u32] = &[
    0x9421ffe0, 0x7c0802a6, 0x38800000, 0x90010024, 0x93e1001c, 0x7c7f1b78, 0x8063000c, 0xa8630000,
    0x0, 0x907f0014, 0x7fe3fb78, 0x38800000, 0x815f0014, 0x800a0010, 0x90010008, 0x800a0018,
    0x9001000c, 0x800a000c, 0x90010010, 0xa8bf1556, 0xa8ca0000, 0xa8ea0002, 0xa90a0004, 0xa92a0006,
    0x814a0014, 0x0, 0x387f15b4, 0x0, 0x807f0014, 0x80030008, 0x2c000000, 0x4182003c, 0x7fe4fb78,
    0x387f15b4, 0x38df15ec, 0x38a00000, 0x38e00000, 0x0, 0x80bf0014, 0x387f15ec, 0x38800001,
    0x38c00000, 0x80050008, 0x7c050734, 0x0, 0x48000030, 0x387f15ec, 0x7c050734, 0x38800001,
    0x38c00000, 0x0, 0x7fe4fb78, 0x387f15b4, 0x38df15ec, 0x38a00000, 0x38e00000, 0x0, 0x38800000,
    0x3c600000, 0x909f19a0, 0x38000001, 0xc0030000, 0x909f199c, 0xd01f18b0, 0x887f0280, 0x50830fbc,
    0x987f0280, 0x981f01ad, 0xb09f01ae, 0x80010024, 0x83e1001c, 0x7c0803a6, 0x38210020, 0x4e800020,
];

const THREE_PHASES: &[u32] = &[
    0x9421fff0, 0x7c0802a6, 0x90010014, 0x8803107e, 0x5400e73e, 0x28000001, 0x4082000c, 0x0,
    0x48000020, 0x20000009, 0x0, 0x7c000034, 0x5405d97e, 0x38050001, 0x5405063e, 0x0, 0x80010014,
    0x7c0803a6, 0x38210010, 0x4e800020,
];

const TWO_PHASES: &[u32] = &[
    0x9421fff0, 0x7c0802a6, 0x0, 0x90010014, 0x8803107e, 0x5400e73e, 0x20000009, 0x7c000034,
    0x5405de3e, 0x0, 0x80010014, 0x7c0803a6, 0x38210010, 0x4e800020,
];

const LANDING: &[u32] = &[
    0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x7c7f1b78, 0x0, 0x3c600000, 0x38030000,
    0x901f0018, 0x83e1000c, 0x80010014, 0x7c0803a6, 0x38210010, 0x4e800020,
];

// Zero entries are checked separately as calls or native identity immediates.
fn body(rel: &Rel, entry: usize, expected: &[u32], calls: &[(usize, usize)]) -> Result<()> {
    let code = rel.at((1, entry))?;
    for (index, &instruction) in expected.iter().enumerate() {
        ensure!(
            instruction == 0 || word(code, index * 4)? == instruction,
            "unreviewed martial initializer instruction at {:#x}",
            entry + index * 4
        );
    }
    for &(offset, target) in calls {
        let instruction = word(code, offset)?;
        let displacement = ((instruction as i32) << 6) >> 6;
        ensure!(
            instruction & 0xfc000003 == 0x48000001
                && (entry + offset) as i64 + i64::from(displacement & !3) == target as i64,
            "unreviewed martial initializer call at {:#x}",
            entry + offset
        );
    }
    Ok(())
}

pub(super) fn validate(rel: &Rel, native: u16) -> Result<()> {
    let dispatch = rel.pointer(DATA, 0xd60 + usize::from(native) * 4)?;
    let entry = rel.pointer(dispatch.0, dispatch.1)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, UPDATE),
        "unimplemented martial update dispatcher for native {native}"
    );
    let initializer = match native {
        1 => 0x5b3f8,
        2 => 0x5b448,
        12 => 0x732f4,
        81 => 0x802e4,
        82 => 0x8031c,
        83 => 0x83060,
        84 => 0x649f0,
        86 => 0x87188,
        17 => 0x5f9dc,
        18 => 0x7252c,
        19 => 0x7316c,
        _ => SHARED,
    };
    ensure!(
        entry == (1, initializer),
        "unimplemented martial initializer for native {native}"
    );
    // The shared action binder clears both native callbacks before any initializer returns.
    let clear = rel.at((1, 0x3a784))?;
    ensure!(
        [0x38000000, 0x901e0018, 0x901e001c]
            .into_iter()
            .enumerate()
            .all(|(i, expected)| word(clear, i * 4).ok() == Some(expected)),
        "shared martial callbacks are not cleared"
    );
    match native {
        1 | 2 | 12 => {
            body(
                rel,
                initializer,
                THREE_PHASES,
                &[(0x1c, SHARED), (0x3c, SELECTED)],
            )?;
            ensure!(
                word(rel.at(entry)?, 0x28)? == 0x38800000 | u32::from(native),
                "changed three-phase native identity"
            );
        }
        81 | 82 | 83 | 84 | 86 => {
            body(rel, initializer, TWO_PHASES, &[(0x24, SELECTED)])?;
            ensure!(
                word(rel.at(entry)?, 8)? == 0x38800000 | u32::from(native),
                "changed two-phase native identity"
            );
        }
        17..=19 => {
            body(rel, initializer, LANDING, &[(0x14, SHARED)])?;
            let update = match native {
                17 => 0x5f934,
                18 => 0x72484,
                _ => 0x730c4,
            };
            ensure!(
                rel.local_targets().contains(&(1, update)),
                "missing supported Tempest landing callback"
            );
        }
        _ => body(
            rel,
            SHARED,
            SHARED_BODY,
            &[
                (0x20, 0xb87c),
                (0x64, 0x3a39c),
                (0x6c, 0x422b4),
                (0x94, 0x42308),
                (0xb0, 0x4273c),
                (0xc8, 0x4273c),
                (0xe0, 0x42308),
            ],
        )?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn original_none_callbacks_have_only_supported_entry_side_effects() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/US_r_Top2Btl.rel");
        let mut rel = Rel::read(&path).unwrap();
        let admitted = [
            1, 2, 3, 5, 7, 8, 9, 12, 13, 15, 16, 17, 18, 19, 21, 23, 24, 25, 26, 27, 28, 35, 36,
            38, 45, 46, 48, 62, 78, 81, 82, 83, 84, 86, 95, 96, 97, 98, 99, 100, 101, 103, 104,
            105, 108, 109, 111, 112, 113, 114, 115, 116, 117, 118, 120, 124, 125, 126, 133, 137,
        ];
        for native in admitted {
            validate(&rel, native).unwrap_or_else(|e| panic!("native {native}: {e:#}"));
        }
        // A native-specific dispatcher cannot become a track-only arte by falling through.
        for native in [
            4, 6, 10, 11, 14, 37, 39, 40, 41, 42, 63, 64, 65, 66, 67, 68, 69, 70, 71, 85, 87,
        ] {
            assert!(
                validate(&rel, native).is_err(),
                "native {native} bypassed its callback binding"
            );
        }
        let base = rel.sections[1].0;
        for (native, offset, instruction) in [
            (1, 0x5b414, 0x48000001_u32), // replace shared entry with an unaudited call
            (81, 0x802fc, 0x20000006),    // change the character-specific selector
            (17, 0x5f9fc, 0x901f001c),    // move landing logic to an end callback
            (3, 0x38328, 0x909f0018),     // add callback mutation in the generic entry
            (3, 0x3a78c, 0x901e0018),     // stop clearing the prior end callback
        ] {
            let at = base + offset;
            let old: [u8; 4] = rel.bytes[at..at + 4].try_into().unwrap();
            rel.bytes[at..at + 4].copy_from_slice(&instruction.to_be_bytes());
            assert!(
                validate(&rel, native).is_err(),
                "native {native} accepted changed side effect"
            );
            rel.bytes[at..at + 4].copy_from_slice(&old);
        }
    }
}
