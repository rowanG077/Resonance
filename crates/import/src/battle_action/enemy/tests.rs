use super::super::tests::{put, storage};
use super::*;

fn action_source_bytes(row: &EnemyAction) -> [u8; 68] {
    let mut bytes = [0; 68];
    storage(&mut bytes, &row.storage);
    put(
        &mut bytes,
        0,
        [
            row.weight as u8,
            row.target_policy,
            row.recovery_ticks,
            row.recovery_clip,
        ],
    );
    for (offset, value) in [
        (4, row.recovery_rate.bits()),
        (8, row.requirements),
        (40, row.movement_speed.bits()),
        (44, row.movement_rate.bits()),
    ] {
        put(&mut bytes, offset, value.to_be_bytes());
    }
    for (offset, value) in [
        (12, row.target_state),
        (14, row.duration),
        (16, row.range[0] as u16),
        (18, row.range[1] as u16),
        (20, row.approach_range as u16),
        (22, row.approach_minimum as u16),
        (24, row.animation),
        (26, row.command),
        (28, row.hit),
        (30, row.combo_at),
        (36, row.vulnerable[0]),
        (38, row.vulnerable[1]),
        (54, row.recovery_command),
        (58, row.required_story_flag),
        (60, row.cast_voices[0]),
        (62, row.cast_voices[1]),
        (64, row.technique),
    ] {
        put(&mut bytes, offset, value.to_be_bytes());
    }
    for (offset, value) in [
        (32, row.followup_group),
        (34, row.effect),
        (35, row.guard_chance),
        (48, row.movement_clip),
        (49, row.stagger_threshold),
        (50, row.followup_chance),
        (51, row.required_monster),
        (52, row.tp),
        (53, row.hit_recovery_clip),
        (56, row.resource_decrement),
    ] {
        bytes[offset] = value;
    }
    bytes
}

fn package() -> Vec<u8> {
    let mut bytes = vec![0; 712];
    bytes[..4].copy_from_slice(b"em8\0");
    for (field, offset) in [
        (8, 488_u16),
        (10, 516),
        (12, 584),
        (14, 612),
        (16, 624),
        (18, 688),
    ] {
        bytes[field..field + 2].copy_from_slice(&offset.to_be_bytes());
    }
    bytes[516..520].copy_from_slice(&[35, 2, 10, 3]);
    bytes[530..532].copy_from_slice(&71_u16.to_be_bytes());
    bytes[540..542].copy_from_slice(&0_u16.to_be_bytes());
    bytes[550] = 33;
    bytes[551] = 15;
    bytes[573] = 99;
    bytes[582..584].copy_from_slice(&[91, 92]);
    bytes[607] = 1;
    bytes[612..624].copy_from_slice(&[0, 0, 0, 28, 0, 60, 0, 0, 255, 255, 255, 255]);
    bytes[624..628].copy_from_slice(&[0, 23, 25, 1]);
    bytes[656..658].copy_from_slice(&[255, 255]);
    bytes[690] = 30;
    bytes[700..702].copy_from_slice(&[255, 254]);
    bytes
}

#[test]
fn enemy_tables_preserve_source_roots_signed_values_and_inactive_storage() -> Result<()> {
    let mut bytes = package();
    bytes[516] = 255;
    bytes[520..524].copy_from_slice(&0x7fc1_2345_u32.to_be_bytes());
    bytes[532..536].copy_from_slice(&[0x80, 0, 0x7f, 0xff]);
    bytes[560..564].copy_from_slice(&0x8000_0000_u32.to_be_bytes());
    bytes[568] = 8;
    bytes[580..582].copy_from_slice(&237_u16.to_be_bytes());
    let parsed = read(&bytes)?;
    assert_eq!(parsed.rows.len(), 1);
    let row = &parsed.rows[0];
    assert_eq!(row.weight, -1);
    assert_eq!(row.range, [i16::MIN, i16::MAX]);
    assert_eq!((row.duration, row.tp, row.technique), (71, 8, 237));
    assert_eq!(
        row.storage
            .iter()
            .map(|s| (s.offset, s.bytes.clone()))
            .collect::<Vec<_>>(),
        [(57, vec![99]), (66, vec![91, 92])]
    );
    // Storage elides zero-only gaps. Reconstructing into a zeroed row must
    // nevertheless retain every source byte, including non-finite operands.
    let decoded: EnemyAction = serde_json::from_slice(&serde_json::to_vec(row)?)?;
    assert_eq!(action_source_bytes(&decoded), bytes[516..584]);
    let mut nonzero_gap = bytes[516..584].to_vec();
    nonzero_gap[33] = 0xa5;
    let decoded = action(&nonzero_gap)?;
    assert_eq!(decoded.storage[0].offset, 33);
    assert_eq!(decoded.storage[0].bytes, [0xa5]);
    assert_eq!(action_source_bytes(&decoded), nonzero_gap.as_slice());
    assert_eq!(parsed.policy.ordinary_count, 1);
    assert_eq!((parsed.hits[0].start, parsed.hits[0].emission), (23, 25));
    assert_eq!(parsed.hits[1].start, -1);
    assert_eq!(parsed.animations[0].clip, 30);
    assert_eq!(parsed.animations[1].time, -2);
    assert!(
        matches!(&parsed.commands[0].record, CommandRecord::Command { time: 0, opcode: 28, operands } if operands == &[60, 0])
    );
    assert_eq!(parsed.commands.len(), 2);
    assert_eq!(parsed.storage[0].offset, 622);
    assert_eq!(parsed.storage[0].bytes, [255, 255]);
    Ok(())
}

#[test]
fn enemy_tables_reject_bad_boundaries_and_truncated_programs() {
    for length in [0, 487, 624, 657, 699, 701] {
        assert!(read(&package()[..length]).is_err(), "length {length}");
    }
    for (at, value) in [(0, 0), (11, 5), (608, 5), (656, 0), (700, 0)] {
        let mut bytes = package();
        bytes[at] = value;
        assert!(read(&bytes).is_err(), "byte {at}");
    }
}

#[test]
#[ignore = "requires extracted original enemy packages; no media conversion"]
fn original_opening_enemy_actions_keep_every_row_and_program_root() -> Result<()> {
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut first = None;
    for disc in [1, 2] {
        let extracted = local.join(format!("disc{disc}"));
        let sources = crate::source_assets::Sources::read(&extracted)?;
        let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
        let archive = extracted.join("files").join(sources.enemy);
        let mut encoded = Vec::new();
        for id in [36, 49] {
            let bytes = crate::source_assets::enemy_package(&archive, &usual, id)?;
            let actions = read(&bytes)?;
            let (_, source) = section(&bytes, 10)?;
            for (row, original) in actions.rows.iter().zip(source.chunks_exact(68)) {
                let decoded: EnemyAction = serde_json::from_slice(&serde_json::to_vec(row)?)?;
                assert_eq!(action_source_bytes(&decoded), original);
            }
            assert_eq!(actions.policy.native, 0);
            if id == 36 {
                assert_eq!(
                    actions
                        .rows
                        .iter()
                        .map(|r| (
                            r.weight,
                            r.duration,
                            r.recovery_ticks,
                            r.animation,
                            r.command,
                            r.hit
                        ))
                        .collect::<Vec<_>>(),
                    [
                        (35, 71, 10, 2, 6, 2),
                        (45, 72, 10, 6, 12, 4),
                        (20, 110, 10, 10, 18, 6),
                        (10, 149, 10, 15, 30, 9),
                        (0, 59, 25, 0, 0, 0)
                    ]
                );
                assert_eq!(actions.rows[3].requirements, 0x80004);
                assert_eq!(actions.policy.ordinary_count, 5);
                assert_eq!(actions.hits.len(), 13);
                assert_eq!(actions.hit_rules.len(), 2);
                assert_eq!(actions.animations.len(), 22);
                assert_eq!(actions.hits[2].emission_operands, [1, 0, 0, 0]);
            } else {
                assert_eq!(actions.rows.len(), 2);
                assert_eq!(actions.rows[1].requirements, 0x20);
                assert_eq!(actions.policy.back_row_actions[0], 1);
                assert_eq!(actions.policy.back_row_weights[0], 30);
                assert_eq!(actions.hits[2].emission, -2);
                assert_eq!(actions.hit_rules[1].contact_cooldown, 240);
            }
            encoded.push(serde_json::to_vec(&actions)?);
        }
        if let Some(first) = &first {
            assert_eq!(&encoded, first);
        }
        first = Some(encoded);
    }
    Ok(())
}
