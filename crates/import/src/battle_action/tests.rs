use super::*;
use resonance_content::source::Storage;

pub(super) fn put(bytes: &mut [u8], at: usize, value: impl AsRef<[u8]>) {
    let value = value.as_ref();
    bytes[at..at + value.len()].copy_from_slice(value);
}

pub(super) fn storage(bytes: &mut [u8], spans: &[Storage]) {
    for span in spans {
        put(bytes, span.offset, &span.bytes);
    }
}

pub(super) fn source_bytes(bundle: &Bundle, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    storage(&mut bytes, &bundle.storage);
    let compact = bundle.phases.len() == 1;
    for (i, base) in bundle.pool_offsets.iter().enumerate() {
        put(
            &mut bytes,
            if compact { 12 } else { 0 } + i * 4,
            base.to_be_bytes(),
        );
    }
    for (i, phase) in bundle.phases.iter().enumerate() {
        let at = if compact { 0 } else { 16 + i * 28 };
        for (j, value) in [
            phase.duration,
            phase.recovery_ticks,
            phase.buffer_until,
            phase.combo_at,
        ]
        .into_iter()
        .enumerate()
        {
            put(&mut bytes, at + j * 2, value.to_be_bytes());
        }
        put(&mut bytes, at + 8, phase.startup_effect.to_be_bytes());
        if !compact {
            for (j, index) in phase.indices.iter().enumerate() {
                put(&mut bytes, at + 12 + j * 4, index.to_be_bytes());
            }
        }
    }
    let bases = bundle.pool_offsets.map(|v| v as usize);
    for (i, rule) in bundle.hit_rules.iter().enumerate() {
        let row = &mut bytes[bases[0] + i * 28..bases[0] + (i + 1) * 28];
        storage(row, &rule.storage);
        put(row, 0, rule.flags.to_be_bytes());
        put(
            row,
            2,
            [
                rule.element,
                rule.hitstun,
                rule.contact_cooldown,
                rule.stun_chance,
                rule.stagger,
                rule.guard_pressure,
            ],
        );
        put(row, 8, rule.conditions.to_be_bytes());
        put(row, 12, [rule.condition_chance, rule.power_mode]);
        put(row, 14, rule.power.to_be_bytes());
        put(row, 16, rule.sound.to_be_bytes());
        put(
            row,
            20,
            [
                rule.armor_damage,
                rule.knockback_delay,
                rule.impact_effect,
                rule.condition_parameter as u8,
                rule.impact_bank,
            ],
        );
    }
    for (i, hit) in bundle.hits.iter().enumerate() {
        let row = &mut bytes[bases[1] + i * 32..bases[1] + (i + 1) * 32];
        storage(row, &hit.storage);
        put(row, 0, hit.start.to_be_bytes());
        put(row, 2, [hit.emission as u8, hit.attachment_count as u8]);
        put(row, 4, hit.emission_operands);
        put(row, 8, hit.radius.bits().to_be_bytes());
        put(row, 12, hit.height.bits().to_be_bytes());
        put(
            row,
            16,
            [
                hit.shape,
                hit.damage_kind,
                hit.rule,
                hit.hit_class,
                hit.reaction,
            ],
        );
        put(row, 22, hit.projectile_modifier.to_be_bytes());
        put(row, 28, hit.inner_radius.bits().to_be_bytes());
    }
    for (i, animation) in bundle.animations.iter().enumerate() {
        let at = bases[2] + i * 12;
        put(&mut bytes, at, animation.time.to_be_bytes());
        put(
            &mut bytes,
            at + 2,
            [
                animation.clip,
                animation.blend,
                animation.start,
                animation.end,
                animation.layer_flags,
                animation.resource as u8,
            ],
        );
        put(&mut bytes, at + 8, animation.rate.bits().to_be_bytes());
    }
    for command in &bundle.commands {
        let at = bases[3] + command.word_index as usize * 2;
        match &command.record {
            CommandRecord::End => put(&mut bytes, at, (-1_i16).to_be_bytes()),
            CommandRecord::Loop => put(&mut bytes, at, (-2_i16).to_be_bytes()),
            CommandRecord::Command {
                time,
                opcode,
                operands,
            } => {
                put(&mut bytes, at, time.to_be_bytes());
                put(&mut bytes, at + 2, opcode.to_be_bytes());
                for (i, operand) in operands.iter().enumerate() {
                    put(&mut bytes, at + 4 + i * 2, operand.to_be_bytes());
                }
            }
        }
    }
    bytes
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 228];
    for (i, base) in [128_u32, 156, 190, 214].into_iter().enumerate() {
        put(&mut bytes, i * 4, base.to_be_bytes());
    }
    for i in 0..4 {
        put(&mut bytes, 16 + i * 28, (90_u16 + i as u16).to_be_bytes());
        put(&mut bytes, 24 + i * 28, (-1_i32).to_be_bytes());
    }
    // Overlapping command roots, including a root at the final terminator.
    put(&mut bytes, 68, 3_u32.to_be_bytes());
    put(&mut bytes, 124, 5_u32.to_be_bytes());
    put(&mut bytes, 128, [0x00, 0x20, 5, 20, 30, 20, 1]);
    put(&mut bytes, 141, [1, 0, 130]);
    put(&mut bytes, 156, [0, 20, 0xfd, 0xff]);
    put(&mut bytes, 164, 0x7fc12345_u32.to_be_bytes());
    put(&mut bytes, 188, (-1_i16).to_be_bytes());
    put(&mut bytes, 190, [0, 0, 4, 5, 6, 7, 0xff, 0x80]);
    put(&mut bytes, 198, 0xff800000_u32.to_be_bytes());
    put(&mut bytes, 202, (-2_i16).to_be_bytes());
    put(
        &mut bytes,
        214,
        [0, 2, 0, 0, 0, 15, 0, 3, 0xff, 0xfd, 0xff, 0xff, 0xab, 0xcd],
    );
    bytes
}

#[test]
fn source_records_preserve_overlapping_roots_nonfinite_values_and_unused_storage() -> Result<()> {
    let bytes = fixture();
    let cooked = serde_json::to_vec(&bundle(&bytes)?)?;
    let restored: Bundle = serde_json::from_slice(&cooked)?;
    assert_eq!(source_bytes(&restored, bytes.len()), bytes);
    assert_eq!(restored.commands.len(), 3);
    assert_eq!(restored.hit_rules[0].power, 130);
    assert_eq!(restored.hit_rules[0].contact_cooldown, 30);
    assert_eq!(restored.hits[0].radius.bits(), 0x7fc12345);
    assert_eq!(restored.animations[0].rate.bits(), 0xff800000);
    assert!(restored.storage.iter().any(|s| s.offset == 188));
    Ok(())
}

#[test]
fn invalid_boundaries_and_unterminated_referenced_commands_fail() {
    let bytes = fixture();
    for size in 0..226 {
        assert!(bundle(&bytes[..size]).is_err(), "size {size}");
    }
    for (at, value) in [
        (0, 124_u32),
        (4, 157),
        (8, 155),
        (12, 256),
        (28, u32::MAX),
        (40, u32::MAX),
    ] {
        let mut changed = bytes.clone();
        put(&mut changed, at, value.to_be_bytes());
        assert!(bundle(&changed).is_err(), "offset {at}");
    }
    let mut changed = bytes;
    put(&mut changed, 216, 11_i16.to_be_bytes());
    assert!(bundle(&changed).is_err());
}

#[test]
#[ignore = "requires both extracted original discs"]
fn original_action_bundles_roundtrip_every_member_on_both_discs() -> Result<()> {
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut previous = None;
    for disc in [1, 2] {
        let usual = std::fs::read(local.join(format!("disc{disc}/files/BTL/BTLusual.dat")))?;
        let output = tempfile::tempdir()?;
        let paths = publish(&usual, output.path(), "battle")?;
        assert_eq!(paths, [MARTIAL_PATH, SPELL_PATH]);
        let mut tables = Vec::new();
        for member in [8, 9] {
            let bytes = crate::source_assets::section(&usual, member)?;
            let table = read(bytes)?;
            assert_eq!(table.records.len(), 147);
            assert_eq!(
                table.records.iter().flatten().count(),
                if member == 8 { 114 } else { 42 }
            );
            let json = serde_json::to_vec(&table)?;
            assert_eq!(std::fs::read(output.path().join(&paths[member - 8]))?, json);
            let restored: Table = serde_json::from_slice(&json)?;
            for (index, (row, range)) in restored
                .records
                .iter()
                .zip(crate::field::sections(bytes)?)
                .enumerate()
            {
                if let Some(range) = range {
                    assert_eq!(
                        source_bytes(row.as_ref().unwrap(), range.len()),
                        &bytes[range],
                        "disc {disc} table {member} action {index}"
                    );
                } else {
                    assert!(row.is_none());
                }
            }
            if member == 9 {
                let lightning = restored.records[16].as_ref().unwrap();
                assert_eq!(lightning.phases[0].duration, 90);
                let rule = &lightning.hit_rules[0];
                assert_eq!(
                    (rule.flags, rule.element, rule.power_mode, rule.power),
                    (0x20, 5, 1, 130)
                );
                assert_eq!(
                    (
                        rule.hitstun,
                        rule.contact_cooldown,
                        rule.stun_chance,
                        rule.stagger
                    ),
                    (20, 30, 20, 1)
                );
                assert_eq!(restored.records[6].as_ref().unwrap().phases.len(), 1);
            }
            tables.push(json);
        }
        if let Some(previous) = previous {
            assert_eq!(tables, previous);
        }
        previous = Some(tables);
    }
    Ok(())
}
