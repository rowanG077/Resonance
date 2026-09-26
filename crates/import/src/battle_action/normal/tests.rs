use super::*;
use crate::battle_action::tests::{put, source_bytes, storage};

#[test]
#[ignore = "requires both extracted original discs"]
fn original_normal_groups_roundtrip_all_nine_characters_on_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut previous = None;
    for disc in [1, 2] {
        let file = local.join(format!("disc{disc}/files/US_r_Top2Btl.rel"));
        let module = Rel::read(&file)?;
        let output = tempfile::tempdir()?;
        let path = publish(&file, output.path(), "battle")?;
        assert_eq!(path, NORMAL_PATH);
        let json = std::fs::read(output.path().join(path))?;
        let variant = publish(&file, output.path(), "battle/variants/test")?;
        assert_eq!(std::fs::read(output.path().join(variant))?, json);
        let table: NormalTable = serde_json::from_slice(&json)?;
        assert_eq!(table.weapon_flights.len(), 3);
        for (index, flight) in table.weapon_flights.iter().enumerate() {
            let mut row = [0; 16];
            storage(&mut row, &flight.storage);
            put(&mut row, 0, flight.outbound_ticks.to_be_bytes());
            put(&mut row, 4, flight.speed.bits().to_be_bytes());
            put(&mut row, 8, flight.return_speed.bits().to_be_bytes());
            put(&mut row, 12, flight.direction_y.bits().to_be_bytes());
            assert_eq!(row, module.at((5, 0x1cbc + index * 16))?[..16]);
        }
        assert_eq!(table.groups.len(), 9);
        for (index, group) in table.groups.iter().enumerate() {
            assert_eq!(group.actions.len(), 7);
            let root = 0x3a48 + index * 12;
            let bundles = module.pointer(5, root)?;
            let rules = module.pointer(5, root + 4)?;
            let selectors = module.pointer(5, root + 8)?;
            let bytes: Vec<_> = group
                .selectors
                .iter()
                .flat_map(|s| [s.action, s.allowed_directions, s.fallback, s.storage])
                .collect();
            assert_eq!(bytes, module.at(selectors)?[..28]);
            let mut starts = [usize::MAX; 3];
            for action in 0..7 {
                for (slot, start) in starts.iter_mut().enumerate() {
                    *start = (*start).min(
                        module
                            .pointer(bundles.0, bundles.1 + action * 16 + 4 + slot * 4)?
                            .1,
                    );
                }
            }
            let rules_size = group.hit_rules.len() * 28;
            let hits_size = group.hits.len() * 32;
            let animations_size = group.animations.len() * 12;
            let commands_size = bundles.1 - starts[2];
            // Reuse the common source-record encoder, independently of the
            // normal group's physical pool order and relocated stream roots.
            let offsets = [
                128,
                128 + rules_size,
                128 + rules_size + hits_size,
                128 + rules_size + hits_size + animations_size,
            ];
            let mut retained = group.command_storage.clone();
            for span in &mut retained {
                span.offset += offsets[3];
            }
            let encoded = source_bytes(
                &Bundle {
                    pool_offsets: offsets.map(|v| v as u32),
                    phases: vec![],
                    hit_rules: group.hit_rules.clone(),
                    hits: group.hits.clone(),
                    animations: group.animations.clone(),
                    commands: group.commands.clone(),
                    storage: retained,
                },
                offsets[3] + commands_size,
            );
            for (slot, original, length) in [
                (0, rules.1, rules_size),
                (1, starts[0], hits_size),
                (2, starts[1], animations_size),
                (3, starts[2], commands_size),
            ] {
                assert_eq!(
                    &encoded[offsets[slot]..offsets[slot] + length],
                    &module.at((rules.0, original))?[..length],
                    "disc {disc} group {index} pool {slot}"
                );
            }
            for (index, action) in group.descriptors.iter().enumerate() {
                let mut row = [0; 24];
                storage(&mut row, &action.storage);
                put(&mut row, 0, action.duration.to_be_bytes());
                put(&mut row, 2, action.recovery_ticks.to_be_bytes());
                put(&mut row, 4, action.combo_at[0].to_be_bytes());
                put(&mut row, 6, action.combo_at[1].to_be_bytes());
                put(&mut row, 8, [action.buffer_until, action.recovery_clip]);
                put(&mut row, 12, action.recovery_rate.bits().to_be_bytes());
                put(&mut row, 16, action.startup_effect.to_be_bytes());
                put(&mut row, 20, action.reach[0].to_be_bytes());
                put(&mut row, 22, action.reach[1].to_be_bytes());
                assert_eq!(
                    row,
                    module.at((rules.0, rules.1 + rules_size + index * 24))?[..24]
                );
            }
            for (index, action) in group.actions.iter().enumerate() {
                for (slot, (base, stride, offset)) in [
                    (rules.1 + rules_size, 24, action.descriptor),
                    (starts[0], 32, action.hit),
                    (starts[1], 12, action.animation),
                    (starts[2], 2, action.command),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(
                        base + offset as usize * stride,
                        module
                            .pointer(bundles.0, bundles.1 + index * 16 + slot * 4)?
                            .1
                    );
                }
            }
        }
        let lloyd = &table.groups[0];
        assert_eq!(
            (
                lloyd.descriptors[0].duration,
                lloyd.descriptors[0].recovery_ticks
            ),
            (30, 10)
        );
        assert_eq!(
            (
                lloyd.descriptors[4].duration,
                lloyd.descriptors[4].recovery_ticks
            ),
            (40, 8)
        );
        assert_eq!(
            (lloyd.animations[0].clip, lloyd.animations[0].rate.finite()?),
            (30, 0.5)
        );
        assert_eq!((lloyd.hits[0].start, lloyd.hits[0].emission), (8, 10));
        if let Some(previous) = previous {
            assert_eq!(json, previous);
        }
        previous = Some(json);
    }
    Ok(())
}

#[test]
#[ignore = "requires extracted original disc 1"]
fn invalid_normal_pools_fail_and_float_storage_is_lossless() -> Result<()> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/extracted/disc1/files/US_r_Top2Btl.rel");
    let mut module = Rel::read(&file)?;
    let bundles = module.pointer(5, 0x3a48)?;
    let descriptor = module.pointer(bundles.0, bundles.1)?;
    let at = module.sections[descriptor.0].0 + descriptor.1;
    for bits in [0x7fc12345_u32, 0x80000000, 0xff800000] {
        put(&mut module.bytes, at + 12, bits.to_be_bytes());
        assert_eq!(
            read(&module)?.groups[0].descriptors[0].recovery_rate.bits(),
            bits
        );
    }
    let key = (bundles.0, bundles.1 + 4);
    let original = module.pointers[&key];
    module.pointers.insert(key, (original.0, original.1 + 1));
    assert!(read(&module).is_err());
    module.pointers.insert(key, original);
    module.sections[5].1 = bundles.1 - 1;
    assert!(read(&module).is_err());
    Ok(())
}
