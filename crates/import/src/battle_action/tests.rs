use super::*;

fn package() -> Vec<u8> {
    let mut bytes = vec![0; 688];
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
    bytes[516..518].copy_from_slice(&[35, 2]);
    bytes[551] = 15;
    bytes[607] = 1;
    bytes[624..628].copy_from_slice(&[0, 23, 25, 1]);
    bytes[656..658].copy_from_slice(&[255, 255]);
    bytes
}

#[test]
fn enemy_resources_keep_signed_values_and_action_choices() -> Result<()> {
    let mut bytes = package();
    bytes[551] = 255;
    bytes[532..536].copy_from_slice(&[0x80, 0, 0x7f, 0xff]);
    bytes[568] = 8;
    bytes[580..582].copy_from_slice(&237_u16.to_be_bytes());
    bytes[664..668].copy_from_slice(&f32::NAN.to_be_bytes());
    let parsed = read(&bytes, 36, &[])?;
    let row = &parsed.rows[0];
    assert_eq!(row.weight, 35);
    assert_eq!(row.guard_chance, -1);
    assert_eq!(row.range, [i16::MIN, i16::MAX]);
    assert_eq!((row.tp, row.attack), (8, Some(EnemyAttack::Right)));
    assert!(parsed.policy.unsupported_reason.is_none());
    assert!(row.unsupported_reason.is_none());
    assert_eq!(row.attack, Some(EnemyAttack::Right));
    serde_json::to_vec(&parsed)?;
    Ok(())
}

#[test]
fn enemy_tables_reject_bad_boundaries_and_truncated_resources() {
    for length in [0, 487, 584, 609] {
        assert!(
            read(&package()[..length], 36, &[]).is_err(),
            "length {length}"
        );
    }
    for (at, value) in [(0, 0), (11, 5), (608, 5)] {
        let mut bytes = package();
        bytes[at] = value;
        assert!(read(&bytes, 36, &[]).is_err(), "byte {at}");
    }
}

#[test]
fn enemy_decisions_decode_eligibility_targets_and_weighted_rows() -> Result<()> {
    let mut bytes = package();
    bytes[517] = 5;
    bytes[524..528].copy_from_slice(&(0x8_0000_u32 | 8 | 2 | 0x20).to_be_bytes());
    bytes[608] = 1;
    bytes[602] = 75;
    let parsed = read(&bytes, 36, &[])?;
    let row = &parsed.rows[0];
    assert_eq!(row.target_policy, Some(TargetPolicy::Flying));
    assert!(!row.return_to_formation);
    assert!(row.requirements.priority);
    assert_eq!(row.requirements.difficulty, 2..=2);
    assert_eq!(row.requirements.hp_percent, Some(50));
    assert_eq!(parsed.policy.back_row.len(), 1);
    assert_eq!(
        (
            parsed.policy.back_row[0].action,
            parsed.policy.back_row[0].weight
        ),
        (0, 75)
    );
    assert!(requirements(0x2000_0000 | 8)?.difficulty.is_empty());
    assert_eq!(requirements(0x60)?.difficulty, 0..=2);
    Ok(())
}

#[test]
fn unsupported_enemy_decisions_remain_explicit_after_import() -> Result<()> {
    for (at, value) in [
        (516, 255),
        (517, 255),
        (564, 1),
        (566, 1),
        (567, 1),
        (575, 1),
    ] {
        let mut bytes = package();
        bytes[at] = value;
        assert!(
            read(&bytes, 36, &[])?.rows[0].unsupported_reason.is_some(),
            "byte {at}"
        );
    }
    for value in [1., f32::INFINITY, f32::NAN] {
        let mut bytes = package();
        bytes[556..560].copy_from_slice(&value.to_be_bytes());
        assert!(read(&bytes, 36, &[])?.rows[0].unsupported_reason.is_some());
    }
    let mut bytes = package();
    bytes[524..528].copy_from_slice(&0x8000_0000_u32.to_be_bytes());
    assert!(read(&bytes, 36, &[])?.rows[0].unsupported_reason.is_some());
    for (at, value) in [(592, 1), (595, 1), (606, 1), (607, 0), (609, 1)] {
        let mut bytes = package();
        bytes[at] = value;
        assert!(
            read(&bytes, 36, &[])?.policy.unsupported_reason.is_some(),
            "byte {at}"
        );
    }
    let mut bytes = package();
    bytes[566] = 255;
    assert!(read(&bytes, 36, &[])?.rows[0].unsupported_reason.is_none());
    Ok(())
}
