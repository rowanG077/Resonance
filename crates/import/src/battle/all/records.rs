//! Source settings become ordinary records, without requiring a live battle actor.
mod actor;
#[cfg(test)]
mod attachment_tests;
mod enemy;
mod native;
use crate::read::{f32 as float, u16 as half, u32 as word};
pub(crate) use actor::cook_party as cook_party_settings;
pub(crate) use actor::{ActorSettings, PartySettings};
use anyhow::{Context, Result, bail, ensure};
pub(super) use enemy::sections as enemy_sections;
pub(super) use enemy::unresolved as unresolved_enemy_fields;
pub(crate) use enemy::{EnemyResources, EnemyStatistics};
pub(crate) use native::NativeResources;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum AttachmentUv {
    Disabled,
    Frames,
    ScrollV,
    Reserved,
    Sequence,
    ScrollU,
    Inactive(i8),
}

pub(crate) fn attachment_recipe(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() == 64, "invalid attachment settings size");
    // The two scroll accumulators share storage with the frame sequence.
    // Preserve every slot, including values beyond the active sequence length.
    let sequence_length = usize::from(bytes[11]);
    ensure!(
        !(0..2).any(|channel| bytes[channel] != 0 && bytes[4 + channel] == 4)
            || (1..=36).contains(&sequence_length),
        "active attachment sequence must contain 1..=36 frames"
    );
    let channels = (0..2)
        .map(|index| -> Result<_> {
            let mode = match bytes[4 + index] {
                0 => AttachmentUv::Disabled,
                1 => AttachmentUv::Frames,
                2 => AttachmentUv::ScrollV,
                3 => AttachmentUv::Reserved,
                4 => AttachmentUv::Sequence,
                5 => AttachmentUv::ScrollU,
                other => AttachmentUv::Inactive(other as i8),
            };
            Ok(json!({
                "count": bytes[index], "texture": bytes[2 + index] as i8,
                "mode": mode, "frames_or_step": bytes[6 + index] as i8,
                "period": bytes[8 + index] as i8,
                "initial_scroll": half(bytes, 28 + index * 2)? as i16,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "uv_channels": channels, "frame_order": &bytes[28..],
        "sequence_length": sequence_length,
        "unused_storage": [{"offset": 12, "bytes": &bytes[12..13]}],
        "effect_interval": bytes[10],
        "trail": {
            "texture": bytes[13] as i8, "palette": bytes[14],
            "flags": bytes[15], "color": &bytes[16..20],
            "uv": [half(bytes,20)? as i16, half(bytes,22)? as i16,
                half(bytes,24)? as i16, half(bytes,26)? as i16]
        }
    }))
}

pub(super) fn enemy_auxiliary(bytes: &[u8], count: usize) -> Result<Value> {
    let rows = bytes
        .get(
            ..count
                .checked_mul(40)
                .context("auxiliary instance count overflow")?,
        )
        .context("truncated enemy auxiliary instances")?;
    let instances = rows
        .chunks_exact(40)
        .map(|row| -> Result<_> {
            let vector = |at| -> Result<_> {
                Ok([float(row, at)?, float(row, at + 4)?, float(row, at + 8)?])
            };
            Ok(json!({
                "model":row[0], "animation":(row[1] != 0).then_some(row[1]),
                "bone":row[2], "flags":row[3],
                "translation":vector(4)?, "rotation":vector(16)?, "scale":vector(28)?
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"source_size":bytes.len(),"instances":instances,
        "unreferenced_storage":crate::read::unreferenced_storage(bytes, vec![0..rows.len()])}))
}

pub(super) fn enemy_variants(bytes: &[u8], count: usize) -> Result<Value> {
    Ok(serde_json::to_value(EnemyVariants::read(bytes, count)?)?)
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CombatStats {
    pub level: u8,
    pub attack: i16,
    pub thrust: i16,
    pub defense: i16,
    pub intelligence: i16,
    pub accuracy: i16,
    pub evasion: i16,
    pub luck: u8,
}

impl CombatStats {
    fn read(bytes: &[u8], stats: usize, level: usize, luck: usize) -> Result<Self> {
        Ok(Self {
            level: *bytes.get(level).context("missing enemy level")?,
            attack: half(bytes, stats)? as i16,
            thrust: half(bytes, stats + 2)? as i16,
            defense: half(bytes, stats + 4)? as i16,
            intelligence: half(bytes, stats + 6)? as i16,
            accuracy: half(bytes, stats + 8)? as i16,
            evasion: half(bytes, stats + 10)? as i16,
            luck: *bytes.get(luck).context("missing enemy luck")?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EnemyVariant {
    pub hp: u32,
    pub initial_hp: u32,
    pub tp: i16,
    pub initial_tp: i16,
    pub experience: u32,
    pub gald: u32,
    #[serde(flatten)]
    pub combat: CombatStats,
    pub storage: [u8; 2],
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EnemyVariants {
    pub source_size: usize,
    pub variants: Vec<EnemyVariant>,
    pub unreferenced_storage: Vec<crate::read::Storage>,
}

impl EnemyVariants {
    pub(crate) fn read(bytes: &[u8], count: usize) -> Result<Self> {
        let rows = bytes
            .get(..count.checked_mul(36).context("variant count overflow")?)
            .context("truncated enemy variants")?;
        Ok(Self {
            source_size: bytes.len(),
            variants: rows
                .chunks_exact(36)
                .map(|row| {
                    Ok(EnemyVariant {
                        hp: word(row, 0)?,
                        initial_hp: word(row, 4)?,
                        tp: half(row, 8)? as i16,
                        initial_tp: half(row, 10)? as i16,
                        experience: word(row, 12)?,
                        gald: word(row, 16)?,
                        combat: CombatStats::read(row, 20, 33, 32)?,
                        storage: row[34..36].try_into()?,
                    })
                })
                .collect::<Result<_>>()?,
            unreferenced_storage: crate::read::unreferenced_storage(bytes, vec![0..rows.len()]),
        })
    }
}

pub(super) fn usual_table(bytes: &[u8], index: usize) -> Result<Value> {
    Ok(match index {
        0 => {
            ensure!(
                bytes.len().is_multiple_of(32),
                "misaligned encounter selection table"
            );
            let rows = bytes
                .chunks_exact(32)
                .map(|row| -> Result<_> {
                    let count = half(row, 2)?;
                    ensure!(count <= 6, "too many weighted encounter choices");
                    ensure!(
                        half(row, 0)? == 0 && row[29..].iter().all(|&v| v == 0),
                        "unresolved encounter selection fields"
                    );
                    let choices = (0..6)
                        .map(|i| -> Result<_> {
                            Ok(json!({"formation":half(row,4+i*2)?,"weight":half(row,16+i*2)?}))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(json!({"count":count,"choices":choices,"victory_streak_group":row[28]}))
                })
                .collect::<Result<Vec<_>>>()?;
            json!({"encounters":rows})
        }
        1 => serde_json::to_value(super::super::formations::FormationTable::read(bytes)?)?,
        5 => {
            ensure!(
                bytes.len().is_multiple_of(4),
                "misaligned trigonometry lookup table"
            );
            json!({"sine":bytes.chunks_exact(4).map(|r|float(r,0)).collect::<Result<Vec<_>>>()?,"cosine_start":90})
        }
        11 => {
            json!({"voice_count":bytes.len()*8,"streamed_voices": bytes.iter().enumerate().flat_map(|(index, byte)| {
            (0..8).filter_map(move |bit| (byte & (1 << bit) != 0).then_some(index * 8 + bit))
        }).collect::<Vec<_>>()})
        }
        12 => {
            ensure!(
                bytes.len().is_multiple_of(2),
                "misaligned voice duration table"
            );
            json!({"duration_ticks": bytes.chunks_exact(2).map(|b| half(b,0)).collect::<Result<Vec<_>>>()?})
        }
        10 | 13 => {
            ensure!(
                bytes.len().is_multiple_of(4),
                "misaligned archive offset table"
            );
            let offsets = bytes
                .chunks_exact(4)
                .map(|b| word(b, 0))
                .collect::<Result<Vec<_>>>()?;
            json!({"offsets": offsets})
        }
        _ => bail!("unsupported shared battle table {index}"),
    })
}

pub(super) fn arena_metadata(bytes: &[u8], sections: usize) -> Result<Value> {
    let settings = crate::texture_animation::ArenaSettings::read(bytes)?;
    let mut value = serde_json::to_value(&settings)?;
    value["section_count"] = json!(sections);
    value["scenery_model_count"] = json!(settings.scenery_model_count(sections));
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemy_tables_account_for_storage_and_reject_truncated_records() -> Result<()> {
        let mut package = [0; 0x220];
        package[0x14..0x18].copy_from_slice(&40_u32.to_be_bytes());
        for slot in [20, 22, 79] {
            package[0x20 + slot * 4..0x24 + slot * 4].copy_from_slice(&0x200_u32.to_be_bytes());
        }
        package[0x1e0..0x1e4].copy_from_slice(&0x210_u32.to_be_bytes());
        let resources: EnemyResources =
            serde_json::from_slice(&serde_json::to_vec(&EnemyResources::read(&package)?)?)?;
        assert_eq!(resources.motion_count, 40);
        assert_eq!(resources.motion_offsets.len(), 80);
        assert_eq!(resources.motion_offsets[21], 0);
        assert_eq!(
            [
                resources.motion_offsets[20],
                resources.motion_offsets[22],
                resources.motion_offsets[79]
            ],
            [0x200; 3]
        );
        assert_eq!(resources.variant_offset, 0x210);
        assert!(EnemyResources::read(&package[..0x1e3]).is_err());

        let mut statistics = [0; 64];
        statistics[4..9].copy_from_slice(b"AB\0xy");
        statistics[3] = 7;
        statistics[28] = 8;
        statistics[30..32].copy_from_slice(&[9, 10]);
        statistics[57..].fill(11);
        statistics[46..48].copy_from_slice(&(-2_i16).to_be_bytes());
        let stats = EnemyStatistics::read(&statistics)?;
        let decoded: EnemyStatistics = serde_json::from_slice(&serde_json::to_vec(&stats)?)?;
        assert_eq!(decoded.name, "AB");
        assert_eq!(decoded.name_bytes, statistics[4..28]);
        assert_eq!(decoded.combat.thrust, -2);
        assert_eq!(decoded.source_size, 64);
        assert_eq!(
            serde_json::to_value(decoded.uninterpreted_storage)?,
            json!([
                {"offset":3,"bytes":[7]}, {"offset":28,"bytes":[8]},
                {"offset":30,"bytes":[9,10]}, {"offset":57,"bytes":&statistics[57..]},
            ])
        );
        assert!(EnemyStatistics::read(&statistics[..59]).is_err());
        statistics[4..28].fill(b'x');
        assert!(EnemyStatistics::read(&statistics).is_err());

        let mut auxiliary = [0; 41];
        auxiliary[0] = 5;
        auxiliary[40] = 9;
        let table = enemy_auxiliary(&auxiliary, 1)?;
        assert_eq!(table["instances"][0]["model"], 5);
        assert!(table["instances"][0]["animation"].is_null());
        assert_eq!(table["source_size"], auxiliary.len());
        assert_eq!(
            table["unreferenced_storage"],
            json!([{"offset":40,"bytes":[9]}])
        );
        assert!(enemy_auxiliary(&auxiliary[..39], 1).is_err());
        assert!(enemy_auxiliary(&[], usize::MAX).is_err());

        let mut variants = [0; 73];
        variants[22..24].copy_from_slice(&(-123_i16).to_be_bytes());
        variants[34..36].copy_from_slice(&[127, 128]);
        variants[72] = 9;
        let table = enemy_variants(&variants, 1)?;
        assert_eq!(table["variants"][0]["storage"], json!([127, 128]));
        assert_eq!(table["variants"][0]["thrust"], -123);
        // A whole spare stride does not establish another authored record.
        assert_eq!(table["variants"].as_array().unwrap().len(), 1);
        assert_eq!(table["source_size"], variants.len());
        assert_eq!(
            table["unreferenced_storage"],
            json!([{"offset":36,"bytes":&variants[36..]}])
        );
        assert!(enemy_variants(&variants[..35], 1).is_err());
        assert!(enemy_variants(&[], usize::MAX).is_err());
        for table in [enemy_auxiliary(&[0; 8], 0)?, enemy_variants(&[0; 8], 0)?] {
            assert_eq!(table["source_size"], 8);
            assert_eq!(table["unreferenced_storage"], json!([]));
        }
        Ok(())
    }

    #[test]
    fn formations_keep_inactive_slots_and_reject_invalid_extents() -> Result<()> {
        use crate::battle::formations::{FormationTable, RECORD_SIZE};
        let mut bytes = [0; RECORD_SIZE];
        bytes[..4].copy_from_slice(b"gp3\0");
        bytes[14..16].copy_from_slice(&u16::MAX.to_be_bytes());
        bytes[23] = 255;
        bytes[47] = 7;
        bytes[55] = 9;
        bytes[84..86].copy_from_slice(&i16::MIN.to_be_bytes());
        bytes[86..88].copy_from_slice(&i16::MAX.to_be_bytes());
        bytes[88..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let table: FormationTable = serde_json::from_value(usual_table(&bytes, 1)?)?;
        let row = &table.formations[0];
        assert_eq!((row.actor_count, row.resource_count), (0, 0));
        assert_eq!(row.resources, [0, 0, 0, -1]);
        assert_eq!(row.actors[7].resource, 255);
        assert_eq!(row.actors[7].attachments, [7, 9]);
        assert_eq!(row.actors[7].position, [i16::MIN, i16::MAX]);
        assert_eq!(row.storage, [1, 2, 3, 4, 5, 6, 7, 8]);
        for (at, value) in [(0, 0), (4, 9), (5, 5)] {
            let mut invalid = bytes;
            invalid[at] = value;
            assert!(usual_table(&invalid, 1).is_err());
        }
        assert!(usual_table(&bytes[..RECORD_SIZE - 1], 1).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original shared battle archives; no media conversion"]
    fn original_formations_reconstruct_all_physical_slots_on_both_discs() -> Result<()> {
        use crate::battle::formations::{FormationTable, RECORD_SIZE};
        let mut first = None;
        for disc in [1, 2] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../local/extracted/disc{disc}/files/BTL/BTLusual.dat"
            ));
            let archive = std::fs::read(path)?;
            let bytes = crate::battle::actions::member(&archive, 1)?;
            let table: FormationTable = serde_json::from_value(usual_table(bytes, 1)?)?;
            let rows = &table.formations;
            assert_eq!(rows.len(), 1000);
            let mut reconstructed = Vec::with_capacity(bytes.len());
            let mut inactive_positions = Vec::new();
            for (index, row) in rows.iter().enumerate() {
                let mut record = [0; RECORD_SIZE];
                record[..4].copy_from_slice(b"gp3\0");
                record[4..8].copy_from_slice(&[
                    row.actor_count,
                    row.resource_count,
                    row.flags,
                    row.hidden_names,
                ]);
                for (slot, resource) in row.resources.iter().enumerate() {
                    record[8 + slot * 2..10 + slot * 2].copy_from_slice(&resource.to_be_bytes());
                }
                for (slot, actor) in row.actors.iter().enumerate() {
                    for (at, value) in [
                        (16, actor.resource),
                        (24, actor.appearance),
                        (32, actor.variant),
                    ] {
                        record[at + slot] = value;
                    }
                    for axis in 0..2 {
                        record[40 + axis * 8 + slot] = actor.attachments[axis];
                        let coordinate = actor.position[axis];
                        let at = 56 + slot * 4 + axis * 2;
                        record[at..at + 2].copy_from_slice(&coordinate.to_be_bytes());
                    }
                    if slot >= usize::from(row.actor_count) && actor.position != [0; 2] {
                        inactive_positions.push((index, slot, actor.position));
                    }
                }
                record[88..].copy_from_slice(&row.storage);
                reconstructed.extend(record);
            }
            assert_eq!(reconstructed, bytes);
            if let Some(first) = &first {
                assert_eq!(&reconstructed, first);
            } else {
                first = Some(reconstructed);
            }
            assert_eq!(
                inactive_positions,
                [
                    (125, 1, [250, -200]),
                    (125, 2, [275, 215]),
                    (126, 1, [225, 200]),
                    (126, 2, [175, -215]),
                    (173, 3, [300, 250]),
                    (207, 2, [300, 100]),
                    (492, 1, [225, -300]),
                    (492, 2, [280, 250]),
                    (493, 1, [100, -300]),
                    (493, 2, [250, 175]),
                    (493, 3, [150, -200]),
                ]
            );
        }
        Ok(())
    }
}
