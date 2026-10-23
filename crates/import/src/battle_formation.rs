//! Decode `gp3` battle formation records.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_formation::{
    Actor, Formation, Formations, Resource, Settings, WeightedFormation,
};

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
            let actors = usize::from(row[4]);
            let resources = usize::from(row[5]);
            ensure!(
                actors <= 8 && resources <= 4,
                "invalid formation slot counts"
            );
            let half = |i| i16::from_be_bytes([row[i], row[i + 1]]);
            Ok(Formation {
                settings: Settings {
                    play_music: row[6] & 0x10 == 0,
                    celebrate: row[6] & 0x20 == 0,
                    entry_voice: row[6] & 0x40 == 0,
                    escape_restricted: row[6] & 1 != 0,
                },
                resources: (0..resources)
                    .map(|i| {
                        Ok(Resource {
                            enemy: u16::try_from(half(8 + i * 2))
                                .context("negative active formation resource")?,
                            hidden_name: row[7] & (1 << i) != 0,
                        })
                    })
                    .collect::<Result<_>>()?,
                actors: (0..actors)
                    .map(|i| Actor {
                        resource: row[16 + i],
                        variant: row[32 + i],
                        texture_variant: row[24 + i],
                        weapon_variant: row[40 + i],
                        position: (row[6] & 2 == 0).then(|| [half(56 + i * 4), half(58 + i * 4)]),
                        unsupported_reason: (row[48 + i] != 0)
                            .then(|| "encounter appearance override is not prepared".into()),
                    })
                    .collect(),
            })
        })
        .collect::<Result<_>>()?;
    let formations = Formations {
        source_sha256: crate::digest(usual),
        records,
        pools: pools(crate::source_assets::section(usual, 0)?)?,
    };
    formations.validate()?;
    Ok(formations)
}

fn pools(bytes: &[u8]) -> Result<Vec<Vec<WeightedFormation>>> {
    const RECORD_BYTES: usize = 32;
    const CHOICES: usize = 6;
    ensure!(
        bytes.len().is_multiple_of(RECORD_BYTES),
        "misaligned encounter pools"
    );
    bytes
        .chunks_exact(RECORD_BYTES)
        .map(|row| {
            let half = |at| u16::from_be_bytes([row[at], row[at + 1]]);
            let count = usize::from(half(2));
            ensure!(
                half(0) == 0 && (1..=CHOICES).contains(&count),
                "invalid encounter pool header"
            );
            Ok((0..count)
                .map(|slot| WeightedFormation {
                    formation: half(4 + slot * 2),
                    weight: half(16 + slot * 2),
                })
                .collect())
        })
        .collect()
}

pub fn publish(usual: &[u8], path: &std::path::Path) -> Result<()> {
    crate::write_atomic(path, &serde_json::to_vec(&read(usual)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usual(row: &[u8]) -> Vec<u8> {
        let mut bytes = vec![];
        for word in [2_u32, 12, 44] {
            bytes.extend(word.to_be_bytes());
        }
        let mut pool = [0; 32];
        pool[3] = 1;
        pool[17] = 100;
        bytes.extend(pool);
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
    fn encounter_pools_preserve_formations_and_weights() {
        let mut row = [0; 32];
        row[3] = 2;
        row[4..8].copy_from_slice(&[0, 7, 1, 44]);
        row[16..20].copy_from_slice(&[0, 60, 0, 50]);
        assert_eq!(
            pools(&row).unwrap(),
            vec![vec![
                WeightedFormation {
                    formation: 7,
                    weight: 60
                },
                WeightedFormation {
                    formation: 300,
                    weight: 50
                },
            ]]
        );
        assert!(pools(&row[..31]).is_err());
        row[3] = 7;
        assert!(pools(&row).is_err());
    }

    #[test]
    fn decodes_active_links_and_discards_inactive_slots() {
        let mut row = row();
        row[10..12].copy_from_slice(&(-1_i16).to_be_bytes());
        row[23] = 255;
        row[40] = 3;
        row[48] = 7;
        row[88..].fill(0xa5);
        let record = read(&usual(&row)).unwrap().records.remove(0);
        assert_eq!(
            record.resources,
            [Resource {
                enemy: 36,
                hidden_name: true
            }]
        );
        assert_eq!(record.actors.len(), 1);
        assert_eq!(record.actors[0].position, Some([-2, 40]));
        assert!(record.actors[0].unsupported_reason.is_some());
        assert!(record.settings.escape_restricted);
        assert!(
            record.settings.play_music && record.settings.celebrate && record.settings.entry_voice
        );
    }

    #[test]
    fn decodes_encounter_settings_and_marks_only_active_spawn_overrides() {
        let mut row = row();
        row[6] = 0x72;
        row[24 + 7] = 255;
        let record = read(&usual(&row)).unwrap().records.remove(0);
        assert_eq!(
            record.settings,
            Settings {
                play_music: false,
                celebrate: false,
                entry_voice: false,
                escape_restricted: false,
            }
        );
        assert!(record.actors[0].position.is_none());
        assert!(record.actors[0].unsupported_reason.is_none());
        row[40] = 1;
        let selected = read(&usual(&row)).unwrap().records.remove(0);
        assert_eq!(selected.actors[0].weapon_variant, 1);
        assert!(selected.actors[0].unsupported_reason.is_none());
        row[24] = 2;
        let selected = read(&usual(&row)).unwrap().records.remove(0);
        assert_eq!(selected.actors[0].texture_variant, 2);
        assert!(selected.actors[0].unsupported_reason.is_none());
        row[48] = 1;
        let record = read(&usual(&row)).unwrap().records.remove(0);
        assert!(record.actors[0].unsupported_reason.is_some());
    }
}
