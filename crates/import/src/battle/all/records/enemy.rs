use super::*;
use crate::battle::{action_program, animation_table, enemy_inventory};

pub(in crate::battle::all) fn sections(bytes: &[u8]) -> Vec<(u16, Result<Value>)> {
    (4..20)
        .step_by(2)
        .map(|field| {
            let result = (|| {
                let data =
                    enemy_inventory::offset_section(bytes, usize::from(half(bytes, field)?))?;
                Ok(match field {
                    4 => super::actor::parse(data)?,
                    6 => statistics(data)?,
                    8 => action_program::hit_rule_pool(data)?,
                    10 => action_rows(data)?,
                    12 => policy(data)?,
                    14 => action_program::physical_command_table(data)?,
                    16 => {
                        validate_hit_roots(bytes, data)?;
                        hit_programs(data)?
                    }
                    18 => serde_json::to_value(animation_pool(bytes, data)?)?,
                    _ => unreachable!(),
                })
            })();
            (field as u16, result)
        })
        .chain(std::iter::once((
            0x14,
            EnemyResources::read(bytes).and_then(|record| Ok(serde_json::to_value(record)?)),
        )))
        .collect()
}

/// Source pointers retain explicit nulls and aliases independently of motion conversion.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EnemyResources {
    pub motion_count: u32,
    pub motion_offsets: Vec<u32>,
    pub variant_offset: u32,
}

impl EnemyResources {
    pub(crate) fn read(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            motion_count: word(bytes, 0x14)?,
            motion_offsets: (0x20..0x160)
                .step_by(4)
                .map(|at| word(bytes, at))
                .collect::<Result<_>>()?,
            variant_offset: word(bytes, 0x1e0)?,
        })
    }
}

fn validate_hit_roots(package: &[u8], pool: &[u8]) -> Result<()> {
    let extent = enemy_inventory::program_extent(pool, 32, -1)?;
    let program = &pool[..extent];
    for row in enemy_inventory::program_actions(package)? {
        let start = usize::try_from(half(row, 28)? as i16)? * 32;
        let tail = program.get(start..).context("hit entry outside table")?;
        enemy_inventory::program_extent(tail, 32, -1)?;
        ensure!(!tail.is_empty(), "hit entry outside table");
    }
    Ok(())
}

fn animation_pool(package: &[u8], pool: &[u8]) -> Result<animation_table::Parsed> {
    let roots = enemy_inventory::program_actions(package)?
        .into_iter()
        .map(|row| Ok(usize::try_from(half(row, 24)? as i8)? * 12))
        .collect::<Result<Vec<_>>>()?;
    let extent = enemy_inventory::program_extent(pool, 12, -2)?;
    let actions = enemy_inventory::offset_section(package, usize::from(half(package, 10)?))?;
    let declared = action_program::EnemyActionRecord::table(actions)?
        .into_iter()
        .filter_map(|row| {
            usize::try_from(row.animation_index as i8)
                .ok()
                .map(|index| index * 12)
        });
    animation_table::decode_with_records(pool, roots, (0..extent).step_by(12), declared)
}

fn hit_programs(bytes: &[u8]) -> Result<Value> {
    use action_program::{HIT_BYTES, HitRecord};
    let mut entries = Vec::new();
    let extent = enemy_inventory::program_extent(bytes, HIT_BYTES, -1)?;
    let mut covered = Vec::new();
    for cursor in (0..extent).step_by(HIT_BYTES) {
        let record = HitRecord::read(&bytes[cursor..])?;
        covered.push(cursor..cursor + record.size());
        entries.push(json!({"index":cursor/HIT_BYTES,"record":record}));
    }
    Ok(json!({
        "source_size":bytes.len(),
        "entries":entries,
        "unreferenced_storage":crate::read::unreferenced_storage(bytes, covered),
    }))
}

fn statistics(b: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(EnemyStatistics::read(b)?)?)
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EnemyStatistics {
    pub target_policy: u8,
    pub skill_magic_policy: u8,
    pub placement_row: u8,
    pub name: String,
    pub name_bytes: [u8; 24],
    pub hp: u32,
    pub initial_hp: u32,
    pub tp: i16,
    pub initial_tp: i16,
    #[serde(flatten)]
    pub combat: CombatStats,
    pub source_size: usize,
    pub uninterpreted_storage: Vec<crate::read::Storage>,
}

impl EnemyStatistics {
    pub(crate) fn read(b: &[u8]) -> Result<Self> {
        ensure!(b.len() >= 60, "truncated enemy statistics");
        let name_bytes: [u8; 24] = b[4..28].try_into()?;
        let length = name_bytes
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated enemy name")?;
        let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(&name_bytes[..length]);
        ensure!(!invalid, "invalid enemy name encoding");
        Ok(Self {
            target_policy: b[0],
            skill_magic_policy: b[1],
            placement_row: b[2],
            name: name.into_owned(),
            name_bytes,
            hp: word(b, 32)?,
            initial_hp: word(b, 36)?,
            tp: half(b, 40)? as i16,
            initial_tp: half(b, 42)? as i16,
            combat: CombatStats::read(b, 44, 29, 56)?,
            source_size: b.len(),
            uninterpreted_storage: crate::read::unreferenced_storage(
                b,
                vec![0..3, 4..28, 29..30, 32..57],
            ),
        })
    }
}

pub(in crate::battle::all) fn unresolved(bytes: &[u8], field: u16) -> Result<()> {
    if !matches!(field, 4 | 6 | 12) {
        return Ok(());
    }
    let data =
        enemy_inventory::offset_section(bytes, usize::from(half(bytes, usize::from(field))?))?;
    if field == 4 {
        let missing = super::actor::unresolved(data);
        ensure!(
            missing.is_empty(),
            "unresolved nonzero enemy fields {missing:x?}"
        );
        return Ok(());
    }
    let (stride, known): (usize, &[std::ops::Range<usize>]) = match field {
        6 => (data.len(), &[0..3, 4..28, 29..30, 32..57]),
        12 => (data.len(), &[8..9, 11..12, 14..26]),
        _ => return Ok(()),
    };
    let missing = data
        .iter()
        .enumerate()
        .filter_map(|(at, &value)| {
            (value != 0 && !known.iter().any(|range| range.contains(&(at % stride)))).then_some(at)
        })
        .collect::<Vec<_>>();
    ensure!(
        missing.is_empty(),
        "unresolved nonzero enemy fields {missing:x?}"
    );
    Ok(())
}

fn action_rows(bytes: &[u8]) -> Result<Value> {
    Ok(json!({"actions":action_program::EnemyActionRecord::table(bytes)?}))
}

fn policy(b: &[u8]) -> Result<Value> {
    ensure!(b.len() >= 26, "truncated enemy AI settings");
    ensure!(b[24] <= 4, "too many back-row actions");
    Ok(json!({"native_policy":b[8],"ordinary_action_count":b[23],
        "counter_chance":b[11],"counter_action_count":b[22],
        "back_row_count":b[24],
        "back_row":(0..4).map(|i|json!({"action":b[14+i],"weight":b[18+i]})).collect::<Vec<_>>(),
        "overlimit_first_action":b[25]!=0}))
}

#[cfg(test)]
mod tests {
    use super::super::actor::parse as metadata;
    use super::*;
    use crate::battle::actions;
    use std::{fs, path::Path};

    #[test]
    fn action_rows_preserve_inactive_operands_and_adjacent_fields() -> Result<()> {
        let mut package = vec![0; 0x200 + 68];
        package[10..12].copy_from_slice(&0x200_u16.to_be_bytes());
        package[0x238..0x23c].copy_from_slice(&[3, 255, 0x12, 0x34]);
        package[0x200] = 128;
        package[0x210..0x214].copy_from_slice(&[128, 1, 127, 255]);
        for offset in [24, 26, 28, 54] {
            package[0x200 + offset..0x202 + offset].copy_from_slice(&u16::MAX.to_be_bytes());
        }
        for (offset, bits) in [(4, 0x7fc12345_u32), (40, 0xff800000), (44, 0x80000000)] {
            package[0x200 + offset..0x204 + offset].copy_from_slice(&bits.to_be_bytes());
        }
        for offset in [33, 66, 67] {
            package[0x200 + offset] = offset as u8;
        }
        let table = action_rows(&package[0x200..])?;
        let row = &table["actions"][0];
        assert_eq!(row["resource_decrement"], 3);
        assert_eq!(row["required_story_flag"], 0x1234);
        assert_eq!(row["weight"], -128);
        assert_eq!(row["range"], json!([-32767, 32767]));
        assert_eq!(row["recovery_rate"], json!({"bits":0x7fc12345_u32}));
        assert_eq!(row["movement_speed"], json!({"bits":0xff800000_u32}));
        assert_eq!(
            row["uninterpreted_storage"],
            json!([
                {"offset":33,"bytes":[33]}, {"offset":57,"bytes":[255]},
                {"offset":66,"bytes":[66,67]},
            ])
        );
        let restored: action_program::EnemyActionRecord = serde_json::from_value(row.clone())?;
        assert_eq!(restored.source_bytes(), package[0x200..]);
        unresolved(&package, 10)?;
        assert!(action_program::EnemyActionRecord::read(&[0; 67]).is_err());
        assert!(action_rows(&[0; 69]).is_err());
        Ok(())
    }

    #[test]
    fn back_row_policy_retains_inactive_slots() -> Result<()> {
        let mut bytes = [0; 26];
        bytes[14..22].copy_from_slice(&[1, 2, 3, 255, 10, 20, 30, 40]);
        for count in 0..=4 {
            bytes[24] = count;
            let table = policy(&bytes)?;
            assert_eq!(table["back_row_count"], count);
            assert_eq!(table["back_row"].as_array().unwrap().len(), 4);
            assert_eq!(table["back_row"][3], json!({"action":255,"weight":40}));
        }
        bytes[24] = 5;
        assert!(policy(&bytes).is_err());
        assert!(policy(&bytes[..25]).is_err());

        // Statistics and policy are single records; trailing bytes must not
        // wrap into known fields when checking unresolved storage.
        bytes[24] = 4;
        for (field, trailing) in [(6, 64), (12, 34)] {
            let mut package = vec![0; 0x200 + trailing + 1];
            package[field..field + 2].copy_from_slice(&0x200_u16.to_be_bytes());
            if field == 12 {
                package[0x200..0x200 + bytes.len()].copy_from_slice(&bytes);
            }
            unresolved(&package, field as u16)?;
            package[0x200 + trailing] = 1;
            assert!(unresolved(&package, field as u16).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original enemy archives; no media conversion"]
    fn original_enemy_tables_reconstruct_rows_and_account_for_full_extents() -> Result<()> {
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}/files/BTL"));
            let usual = fs::read(root.join("BTLusual.dat"))?;
            let directory = actions::member(&usual, 10)?;
            let mut inactive = Vec::new();
            let mut storage = Vec::new();
            let mut action_storage = Vec::new();
            let mut remainders = Vec::new();
            let mut row_counts = [0; 2];
            for monster in 0..251 {
                let packed = crate::battle::all::read_range(
                    &root.join("BTLenemy.dat"),
                    word(directory, monster * 4)? as usize
                        ..word(directory, (monster + 1) * 4)? as usize,
                )?;
                let package = crate::compression::decode(&packed)?;
                let source =
                    enemy_inventory::offset_section(&package, usize::from(half(&package, 10)?))?;
                let table = action_rows(source)?;
                for (index, row) in source.chunks_exact(68).enumerate() {
                    let restored: action_program::EnemyActionRecord =
                        serde_json::from_value(table["actions"][index].clone())?;
                    assert_eq!(
                        restored.source_bytes(),
                        row,
                        "disc{disc} enemy{monster} action{index}"
                    );
                    if row[57] != 0 {
                        action_storage.push((monster, index));
                    }
                }
                unresolved(&package, 10)?;
                let source =
                    enemy_inventory::offset_section(&package, usize::from(half(&package, 4)?))?;
                let settings = metadata(source)?;
                assert_eq!(
                    settings["unused_storage"],
                    json!([
                        {"offset":0x52,"bytes":&source[0x52..0x55]},
                        {"offset":0x9a,"bytes":&source[0x9a..0x9b]}
                    ])
                );
                unresolved(&package, 4)?;
                let source =
                    enemy_inventory::offset_section(&package, usize::from(half(&package, 12)?))?;
                let table = policy(source)?;
                assert_eq!(table["back_row_count"], source[24]);
                assert_eq!(table["back_row"].as_array().unwrap().len(), 4);
                let mut pairs = [0; 8];
                for (index, row) in table["back_row"].as_array().unwrap().iter().enumerate() {
                    pairs[index] = u8::try_from(row["action"].as_u64().unwrap())?;
                    pairs[index + 4] = u8::try_from(row["weight"].as_u64().unwrap())?;
                    if index >= usize::from(source[24])
                        && (pairs[index] != 0 || pairs[index + 4] != 0)
                    {
                        inactive.push((monster, index, pairs[index], pairs[index + 4]));
                    }
                }
                assert_eq!(
                    pairs,
                    source[14..22],
                    "disc{disc} enemy{monster} back-row slots"
                );

                let settings = usize::from(half(&package, 4)?);
                for (kind, pointer, count_at, stride, key) in [
                    (0, 0x1d8, 0x1e6, 40, "instances"),
                    (1, 0x1e0, 0x1e7, 36, "variants"),
                ] {
                    let start = word(&package, pointer)? as usize;
                    let count = usize::from(package[settings + count_at]);
                    if start == 0 {
                        assert_eq!(count, 0, "disc{disc} enemy{monster} missing {key}");
                        continue;
                    }
                    let source = enemy_inventory::offset_section(&package, start)?;
                    let table = if kind == 0 {
                        super::super::enemy_auxiliary(source, count)?
                    } else {
                        super::super::enemy_variants(source, count)?
                    };
                    let rows = table[key].as_array().unwrap();
                    assert_eq!(rows.len(), count);
                    row_counts[kind] += count;
                    let mut restored = vec![0; count * stride];
                    for (index, (row, bytes)) in rows
                        .iter()
                        .zip(restored.chunks_exact_mut(stride))
                        .enumerate()
                    {
                        if kind == 0 {
                            for (at, name) in ["model", "animation", "bone", "flags"]
                                .into_iter()
                                .enumerate()
                            {
                                bytes[at] = u8::try_from(row[name].as_u64().unwrap_or(0))?;
                            }
                            for (at, name) in [(4, "translation"), (16, "rotation"), (28, "scale")]
                            {
                                for component in 0..3 {
                                    let value = row[name][component].as_f64().unwrap() as f32;
                                    bytes[at + component * 4..at + component * 4 + 4]
                                        .copy_from_slice(&value.to_be_bytes());
                                }
                            }
                        } else {
                            for (at, name) in [
                                (0, "hp"),
                                (4, "initial_hp"),
                                (12, "experience"),
                                (16, "gald"),
                            ] {
                                bytes[at..at + 4].copy_from_slice(
                                    &u32::try_from(row[name].as_u64().unwrap())?.to_be_bytes(),
                                );
                            }
                            for (at, name) in [
                                (8, "tp"),
                                (10, "initial_tp"),
                                (20, "attack"),
                                (22, "thrust"),
                                (24, "defense"),
                                (26, "intelligence"),
                                (28, "accuracy"),
                                (30, "evasion"),
                            ] {
                                bytes[at..at + 2].copy_from_slice(
                                    &i16::try_from(row[name].as_i64().unwrap())?.to_be_bytes(),
                                );
                            }
                            bytes[32] = u8::try_from(row["luck"].as_u64().unwrap())?;
                            bytes[33] = u8::try_from(row["level"].as_u64().unwrap())?;
                            for at in 0..2 {
                                bytes[34 + at] =
                                    u8::try_from(row["storage"][at].as_u64().unwrap())?;
                            }
                            if bytes[34..36] != [0, 0] {
                                storage.push((monster, index, bytes[34], bytes[35]));
                            }
                        }
                    }
                    assert_eq!(
                        restored,
                        source[..count * stride],
                        "disc{disc} enemy{monster} {key}"
                    );
                    let tail = &source[count * stride..];
                    let storage = if tail.iter().any(|&v| v != 0) {
                        json!([{"offset":count * stride,"bytes":tail}])
                    } else {
                        json!([])
                    };
                    assert_eq!(
                        table["unreferenced_storage"], storage,
                        "disc{disc} enemy{monster} {key} storage"
                    );
                    assert_eq!(table["source_size"], source.len());
                    restored.resize(source.len(), 0);
                    for storage in serde_json::from_value::<Vec<crate::read::Storage>>(
                        table["unreferenced_storage"].clone(),
                    )? {
                        restored[storage.offset..storage.offset + storage.bytes.len()]
                            .copy_from_slice(&storage.bytes);
                    }
                    assert_eq!(restored, source, "disc{disc} enemy{monster} full {key}");
                    if !tail.is_empty() {
                        remainders.push(json!({"monster":monster,"table":key,"bytes":tail.len(),"nonzero":tail.iter().filter(|&&v|v != 0).count()}));
                    }
                }
            }
            assert_eq!(action_storage, [(23, 2), (29, 2)]);
            assert!(row_counts.iter().all(|&count| count > 0));
            eprintln!(
                "disc{disc}: auxiliary/variant rows {row_counts:?}, inactive back-row slots {inactive:?}, variant storage {storage:?}, remaining extents {remainders:?}"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires the locally extracted original enemy archive"]
    fn original_enemy_metadata_preserves_affinities_camera_idle_and_casting() -> Result<()> {
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
        let published =
            crate::battle::visual::binding::Directory::open(&output, 1, "BTL/BTLenemy.dat")?;
        let extracted =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files/BTL");
        let directory = fs::read(extracted.join("BTLusual.dat"))?;
        let index = word(&directory, 0x2c)? as usize;
        let mut cameras = Vec::new();
        let mut idle = Vec::new();
        let mut casting = Vec::new();
        let mut ninth_affinity = Vec::new();
        let mut entry_commands = Vec::new();
        for monster in 0..251 {
            let packed = crate::battle::all::read_range(
                &extracted.join("BTLenemy.dat"),
                word(&directory, index + monster * 4)? as usize
                    ..word(&directory, index + (monster + 1) * 4)? as usize,
            )?;
            let package = crate::compression::decode(&packed)?;
            let start = usize::from(half(&package, 4)?);
            let source = &package[start..start + 0x1f0];
            let cooked = metadata(source)?;
            assert_eq!(
                serde_json::to_vec(&cooked)?,
                published
                    .resolve(&format!("battle/all/enemy-{monster}/header-4.json"))?
                    .1,
                "enemy {monster} settings changed"
            );
            let entry_command = half(source, 0xf2)? as i16;
            assert_eq!(cooked["model"]["entry_command_index"], entry_command);
            assert_eq!(cooked["model"]["entry_timeout_ticks"], 0);
            if entry_command != 0 {
                entry_commands.push((monster, entry_command));
            }
            // The hit resolver indexes metadata + 1 by element ID. ID 10
            // selects neutral; the ninth authored affinity remains distinct.
            assert_eq!(
                cooked["combat"]["element_affinities"],
                json!(&source[2..11])
            );
            if source[10] != 0 {
                ninth_affinity.push((monster, source[10]));
            }
            let distance = half(source, 0xf0)? as i16;
            let yaw = source[0xa7];
            assert_eq!(
                cooked["camera"],
                json!({"minimum_distance":distance,"yaw_offset_degrees":yaw})
            );
            assert_eq!(
                cooked["effects"]["idle"],
                json!({"interval":source[0xe8],"effect":source[0xe9]})
            );
            assert_eq!(cooked["casting"]["stored_recovery_clip"], source[0xa6]);
            if distance != 0 || yaw != 0 {
                cameras.push((monster, distance, yaw));
            }
            if source[0xe8] != 0 {
                idle.push((monster, source[0xe8], source[0xe9]));
            }
            if source[0xa6] != 0 {
                casting.push((monster, source[0xa6]));
            }
            unresolved(&package, 4)?;
        }
        assert_eq!(idle, [(30, 80, 1), (31, 60, 1)]);
        assert_eq!(entry_commands, [(196, 80)]);
        assert_eq!(
            ninth_affinity,
            [(54, 4), (73, 2), (208, 2), (209, 2), (210, 2), (240, 2)]
        );
        assert_eq!(
            casting,
            [(203, 37), (236, 39), (237, 39), (238, 39), (239, 35)]
        );
        assert_eq!(
            cameras,
            [
                (44, 3000, 0),
                (172, 2500, 0),
                (182, 2500, 0),
                (198, 2200, 0),
                (208, 2900, 0),
                (209, 2900, 0),
                (210, 2900, 0),
                (214, 0, 25),
                (240, 2900, 0),
                (246, 2750, 0),
            ]
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires the locally extracted original enemy archive"]
    fn original_enemy_metadata_preserves_book_preview_transform() -> Result<()> {
        let extracted =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files/BTL");
        let directory = fs::read(extracted.join("BTLusual.dat"))?;
        let index = word(&directory, 0x2c)? as usize;
        for (monster, scale, elevation) in [
            (91, 0.95f32, 5.0f32),
            (141, 1.0, 30.0),
            (147, 1.0, 35.0),
            (165, 0.9, 20.0),
            (236, 1.0, -15.0),
        ] {
            let packed = crate::battle::all::read_range(
                &extracted.join("BTLenemy.dat"),
                word(&directory, index + monster * 4)? as usize
                    ..word(&directory, index + (monster + 1) * 4)? as usize,
            )?;
            let package = crate::compression::decode(&packed)?;
            let start = usize::from(half(&package, 4)?);
            let source = &package[start..start + 0x1f0];
            assert_eq!(float(source, 0x11c)?, scale, "enemy {monster}");
            assert_eq!(float(source, 0x120)?, elevation, "enemy {monster}");
            assert_eq!(
                metadata(source)?["book_preview"],
                json!({"scale":scale,"elevation":elevation}),
                "enemy {monster}"
            );
            unresolved(&package, 4)?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires the locally extracted original enemy archive"]
    fn original_enemy_animation_pools_preserve_records_roots_and_storage() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let output = root.join("all-assets");
        let mut counts = [0; 3];
        for disc in [1, 2] {
            let published =
                crate::battle::visual::binding::Directory::open(&output, disc, "BTL/BTLenemy.dat")?;
            let extracted = root.join(format!("extracted/disc{disc}/files/BTL"));
            let directory = fs::read(extracted.join("BTLusual.dat"))?;
            let index = word(&directory, 0x2c)? as usize;
            for monster in 0..251 {
                let packed = crate::battle::all::read_range(
                    &extracted.join("BTLenemy.dat"),
                    word(&directory, index + monster * 4)? as usize
                        ..word(&directory, index + (monster + 1) * 4)? as usize,
                )?;
                let package = crate::compression::decode(&packed)?;
                for (field, result) in sections(&package)
                    .into_iter()
                    .filter(|(f, _)| matches!(*f, 8 | 16 | 18))
                {
                    let table = result
                        .with_context(|| format!("disc{disc} enemy{monster} field{field:x}"))?;
                    if field == 8 {
                        let source = enemy_inventory::offset_section(
                            &package,
                            usize::from(half(&package, 8)?),
                        )?;
                        let records: Vec<action_program::HitRuleRecord> =
                            serde_json::from_value(table["records"].clone())?;
                        assert_eq!(table["source_size"], source.len());
                        assert_eq!(
                            records
                                .iter()
                                .flat_map(action_program::HitRuleRecord::source_bytes)
                                .collect::<Vec<_>>(),
                            source,
                            "disc{disc} enemy{monster} hit rules"
                        );
                        continue;
                    }
                    if field == 16 {
                        let old: Value = serde_json::from_slice(
                            &published
                                .resolve(&format!(
                                    "battle/all/enemy-{monster}/header-{field:x}.json"
                                ))?
                                .1,
                        )?;
                        let views = |table: &Value| {
                            table["entries"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|entry| json!({"index":entry["index"],"record":entry["record"]}))
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(views(&table), views(&old));
                        let source = enemy_inventory::offset_section(
                            &package,
                            usize::from(half(&package, 16)?),
                        )?;
                        let mut recovered =
                            vec![0; table["source_size"].as_u64().unwrap() as usize];
                        for entry in table["entries"].as_array().unwrap() {
                            let record: action_program::HitRecord =
                                serde_json::from_value(entry["record"].clone())?;
                            let at = entry["index"].as_u64().unwrap() as usize
                                * action_program::HIT_BYTES;
                            let bytes = record.source_bytes();
                            assert_eq!(bytes, &source[at..at + bytes.len()]);
                            recovered[at..at + bytes.len()].copy_from_slice(&bytes);
                        }
                        for storage in serde_json::from_value::<Vec<crate::read::Storage>>(
                            table["unreferenced_storage"].clone(),
                        )? {
                            recovered[storage.offset..storage.offset + storage.bytes.len()]
                                .copy_from_slice(&storage.bytes);
                        }
                        assert_eq!(recovered, source, "disc{disc} enemy{monster} hit storage");
                        continue;
                    }
                    counts[0] += 1;
                    let source = enemy_inventory::offset_section(
                        &package,
                        usize::from(half(&package, 18)?),
                    )?;
                    let records = table["records"].as_array().unwrap();
                    let mut retained = vec![false; source.len()];
                    let mut mark = |at: usize, size: usize| retained[at..at + size].fill(true);
                    for record in records {
                        let at = record["offset"].as_u64().unwrap() as usize;
                        assert_eq!(record["time"], half(source, at)? as i16);
                        mark(at, 2);
                        if let Some(bind) = record.get("forced_bind") {
                            check_motion(bind, &source[at..at + 12])?;
                            mark(at, 12);
                        }
                        if let Some(dispatch) = record.get("dispatch") {
                            let time = half(source, at)? as i16;
                            let opcode = source.get(at + 2).copied().unwrap_or(0);
                            let expected = match (time, opcode) {
                                (-2, _) => "end",
                                (_, 255) => "texture",
                                (_, 254) => "rate",
                                (-1, _) => "loop",
                                (i16::MIN..=-5, _) => "stalled",
                                _ => "play",
                            };
                            assert_eq!(dispatch["kind"], expected);
                            match expected {
                                "end" => {}
                                "texture" => {
                                    assert_eq!(dispatch["layers"], json!(&source[at + 3..at + 5]));
                                    mark(at, 5);
                                }
                                "rate" => {
                                    assert_eq!(dispatch["rate"], json!(float(source, at + 8)?));
                                    mark(at, 3);
                                    mark(at + 8, 4);
                                }
                                "loop" => {
                                    assert_eq!(dispatch["target"], opcode);
                                    mark(at, 3);
                                }
                                "stalled" => {
                                    assert_eq!(dispatch["opcode"], opcode);
                                    mark(at, 3);
                                }
                                _ => {
                                    check_motion(&dispatch["command"], &source[at..at + 12])?;
                                    mark(at, 12);
                                }
                            }
                        }
                    }
                    for storage in table["unreferenced_storage"].as_array().unwrap() {
                        let at = storage["offset"].as_u64().unwrap() as usize;
                        let length = storage["bytes"].as_array().unwrap().len();
                        assert_eq!(storage["bytes"], json!(&source[at..at + length]));
                        mark(at, length);
                    }
                    assert!(retained.into_iter().all(|byte| byte));
                    let end = source
                        .chunks(12)
                        .rposition(|row| row.len() >= 2 && row[..2] == [255, 254])
                        .context("original animation pool has no terminal record")?;
                    for at in (0..=end * 12).step_by(12) {
                        assert!(
                            records
                                .iter()
                                .any(|row| row["offset"] == at && row.get("dispatch").is_some())
                        );
                        counts[1] += 1;
                    }
                    for action in enemy_inventory::program_actions(&package)? {
                        let at = usize::try_from(half(action, 24)? as i8)? * 12;
                        assert!(
                            records
                                .iter()
                                .any(|row| row["offset"] == at && row.get("forced_bind").is_some())
                        );
                    }
                    let actions = enemy_inventory::offset_section(
                        &package,
                        usize::from(half(&package, 10)?),
                    )?;
                    for action in actions.chunks_exact(68) {
                        // The only base-shifting callback is Item Thief/Rover;
                        // neither is selected by these original enemy actions.
                        assert!(!matches!(half(action, 64)?, 43 | 44));
                        counts[2] += 1;
                    }
                }
            }
        }
        assert_eq!(counts, [502, 4770, 2420]);
        Ok(())
    }

    fn check_motion(value: &Value, row: &[u8]) -> Result<()> {
        assert_eq!(
            value,
            &json!({
                "kind":"play","clip":row[2],"blend":row[3],"start":row[4],
                "end":(row[5] != 0).then_some(row[5]),"layer":row[6] & 63,
                "looping":row[6] & 64 != 0,"mirror":row[6] & 128 != 0,
                "resource":row[7] as i8,"rate":float(row,8)?,
            })
        );
        Ok(())
    }

    #[test]
    fn hit_programs_accept_short_terminators_but_require_complete_commands() -> Result<()> {
        let (stride, sentinel) = (32, -1i16);
        let mut bytes = vec![0; stride];
        bytes.extend(sentinel.to_be_bytes());
        let table = hit_programs(&bytes)?;
        assert_eq!(table["entries"][1]["record"]["kind"], "end");

        bytes.push(1);
        let table = hit_programs(&bytes)?;
        assert_eq!(
            table["entries"][1]["record"]["storage"],
            json!({"offset":2,"bytes":[1]})
        );
        bytes[stride..stride + 2].fill(0);
        assert!(hit_programs(&bytes).is_err());
        assert!(hit_programs(&bytes[..stride]).is_err());

        // An unused complete program after End is still authored data.
        bytes.resize(stride * 3, 0);
        bytes[stride..stride + 2].copy_from_slice(&sentinel.to_be_bytes());
        bytes.extend(sentinel.to_be_bytes());
        assert_eq!(
            hit_programs(&bytes)?["entries"].as_array().unwrap().len(),
            4
        );
        bytes[..2].copy_from_slice(&(-2i16).to_be_bytes());
        bytes[2] = 255;
        bytes[8..12].copy_from_slice(&f32::NAN.to_be_bytes());
        let table = hit_programs(&bytes)?;
        let record: action_program::HitRecord =
            serde_json::from_value(table["entries"][0]["record"].clone())?;
        assert_eq!(record.source_bytes(), bytes[..stride]);
        assert!(record.lower(&[0; 28]).is_err());
        Ok(())
    }

    #[test]
    fn animation_roots_use_signed_indices_and_keep_the_full_pool() -> Result<()> {
        let policy = 0x200 + 2 * 68;
        let mut package = vec![0; policy + 26];
        package[10..12].copy_from_slice(&0x200u16.to_be_bytes());
        package[12..14].copy_from_slice(&(policy as u16).to_be_bytes());
        let mut pool = vec![0; 4 * 12];
        pool[12..14].copy_from_slice(&(-1i16).to_be_bytes());
        pool[14] = 3;
        pool[24..26].copy_from_slice(&(-2i16).to_be_bytes());
        pool.extend((-2i16).to_be_bytes());
        let table = serde_json::to_value(animation_pool(&package, &pool)?)?;
        assert_eq!(table["records"][3]["offset"], 36);
        assert_eq!(table["records"][3]["forced_bind"]["kind"], "play");
        package[0x218..0x21a].copy_from_slice(&0x100u16.to_be_bytes());
        animation_pool(&package, &pool)?; // Only the signed low byte selects the root.
        package[0x218..0x21a].copy_from_slice(&0xffu16.to_be_bytes());
        assert!(animation_pool(&package, &pool).is_err());
        package[0x218..0x21a].fill(0);
        pool.resize(6 * 12, 0);
        pool.extend((-2i16).to_be_bytes());
        // A zero-weight declaration is still a valid program entry: Zombie's
        // fifth action has a separate entry that ordinary AI never selects.
        let dormant = 0x200 + 68 + 24;
        package[dormant..dormant + 2].copy_from_slice(&5u16.to_be_bytes());
        let parsed = animation_pool(&package, &pool)?;
        assert_eq!(
            serde_json::to_value(parsed.selected(60)?)?,
            serde_json::to_value(animation_table::selected_at(&pool, 60)?)?
        );
        // Invalid dormant operands remain declarations, not mandatory traversal.
        package[dormant..dormant + 2].copy_from_slice(&0xffu16.to_be_bytes());
        animation_pool(&package, &pool)?;
        package[dormant..dormant + 2].copy_from_slice(&120u16.to_be_bytes());
        animation_pool(&package, &pool)?;
        Ok(())
    }
}
