//! `gp3` formation records selected by 5878 and loaded by 45238.
use anyhow::{Result, ensure};
use resonance_content::battle_formation::{Actor, Formation, Formations};

pub(crate) fn read(usual: &[u8]) -> Result<Formations> {
    let bytes = crate::source_assets::section(usual, 1)?;
    ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(96),
        "misaligned battle formation table"
    );
    let records = bytes
        .chunks_exact(96)
        .map(|row| {
            ensure!(row.starts_with(b"gp3\0"), "invalid battle formation record");
            let half = |i| i16::from_be_bytes([row[i], row[i + 1]]);
            Ok(Formation {
                actor_count: row[4],
                resource_count: row[5],
                flags: row[6],
                hidden_names: row[7],
                resources: std::array::from_fn(|i| half(8 + i * 2)),
                actors: std::array::from_fn(|i| Actor {
                    resource: row[16 + i],
                    appearance: row[24 + i],
                    variant: row[32 + i],
                    attachments: [row[40 + i], row[48 + i]],
                    position: [half(56 + i * 4), half(58 + i * 4)],
                }),
                storage: row[88..].try_into().unwrap(),
            })
        })
        .collect::<Result<_>>()?;
    let formations = Formations {
        source_sha256: crate::digest(usual),
        records,
    };
    formations.validate()?;
    Ok(formations)
}

pub fn publish(usual: &[u8], path: &std::path::Path) -> Result<()> {
    crate::write_atomic(path, &serde_json::to_vec(&read(usual)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usual(row: &[u8]) -> Vec<u8> {
        let mut bytes = vec![];
        for word in [2_u32, 12, 12] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend(row);
        bytes
    }

    fn row() -> [u8; 96] {
        let mut row = [0; 96];
        row[..4].copy_from_slice(b"gp3\0");
        row[4..8].copy_from_slice(&[1, 1, 5, 1]);
        row[8..10].copy_from_slice(&36_i16.to_be_bytes());
        row[56..60].copy_from_slice(&[0xff, 0xfe, 0, 40]);
        row
    }

    #[test]
    fn retains_inactive_signed_slots_attachments_and_storage() {
        let mut row = row();
        row[10..12].copy_from_slice(&(-1_i16).to_be_bytes());
        row[23] = 255;
        row[40] = 3;
        row[48] = 7;
        row[88..].fill(0xa5);
        let record = read(&usual(&row)).unwrap().records.remove(0);
        assert_eq!(record.resources, [36, -1, 0, 0]);
        assert_eq!(record.actors[0].position, [-2, 40]);
        assert_eq!(record.actors[0].attachments, [3, 7]);
        assert_eq!(record.actors[7].resource, 255);
        assert_eq!(record.storage, [0xa5; 8]);
    }

    #[test]
    fn rejects_truncation_bad_signatures_and_active_slot_references() {
        let bytes = usual(&row());
        for length in 0..bytes.len() {
            assert!(read(&bytes[..length]).is_err(), "length {length}");
        }
        for (at, value) in [(0, 0), (4, 9), (5, 5), (16, 1)] {
            let mut row = row();
            row[at] = value;
            assert!(read(&usual(&row)).is_err());
        }
    }

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_formations_roundtrip_every_slot_and_match_across_discs() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let sources = crate::source_assets::Sources::read(&extracted)?;
            let bytes = std::fs::read(extracted.join("files").join(sources.usual))?;
            let formations = read(&bytes)?;
            assert_eq!(formations.records.len(), 1000);
            for (record, original) in formations
                .records
                .iter()
                .zip(crate::source_assets::section(&bytes, 1)?.chunks_exact(96))
            {
                let mut row = [0; 96];
                row[..4].copy_from_slice(b"gp3\0");
                row[4..8].copy_from_slice(&[
                    record.actor_count,
                    record.resource_count,
                    record.flags,
                    record.hidden_names,
                ]);
                for (i, resource) in record.resources.iter().enumerate() {
                    row[8 + 2 * i..10 + 2 * i].copy_from_slice(&resource.to_be_bytes());
                }
                for (i, actor) in record.actors.iter().enumerate() {
                    for (at, value) in [
                        (16, actor.resource),
                        (24, actor.appearance),
                        (32, actor.variant),
                        (40, actor.attachments[0]),
                        (48, actor.attachments[1]),
                    ] {
                        row[at + i] = value;
                    }
                    row[56 + 4 * i..58 + 4 * i].copy_from_slice(&actor.position[0].to_be_bytes());
                    row[58 + 4 * i..60 + 4 * i].copy_from_slice(&actor.position[1].to_be_bytes());
                }
                row[88..].copy_from_slice(&record.storage);
                assert_eq!(row, original);
            }
            assert_eq!(formations.records[1].resources[0], 36);
            assert_eq!(&formations.records[2].resources[..2], &[49, 36]);
            if let Some(first) = &first {
                assert_eq!(&formations.records, first);
            }
            first = Some(formations.records);
        }
        Ok(())
    }
}
