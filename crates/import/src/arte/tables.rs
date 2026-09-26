//! Physical learning slots and Unison combinations, including inactive records.
use crate::{
    dol,
    read::{f32 as float, u16 as half, u32 as word},
};
use anyhow::{Result, ensure};
use resonance_content::arte::{Combination, LearningList};

const LEARNING: u32 = 0x80202dc8;
const COMBINATIONS: u32 = 0x80208688;

pub(crate) fn learning(executable: &[u8]) -> Result<Vec<LearningList>> {
    dol::slice(executable, LEARNING, 11 * 41)?
        .chunks_exact(41)
        .map(|row| {
            let list = LearningList {
                count: row[0],
                technique_slots: row[1..].to_vec(),
            };
            list.active()?;
            Ok(list)
        })
        .collect()
}

pub(crate) fn combinations(executable: &[u8]) -> Result<Vec<Combination>> {
    dol::slice(executable, COMBINATIONS, 20 * 64)?
        .chunks_exact(64)
        .map(|row| {
            let participant_count = half(row, 6)?;
            ensure!(
                participant_count <= 4,
                "Unison participant count exceeds recipe capacity"
            );
            let mut recipe_slots = [[0; 4]; 6];
            for (index, slot) in recipe_slots.iter_mut().flatten().enumerate() {
                *slot = half(row, 8 + index * 2)? as i16;
            }
            let name = word(row, 0)?;
            Ok(Combination {
                name: (name != 0)
                    .then(|| dol::text(executable, name))
                    .transpose()?,
                native_id: half(row, 4)? as i16,
                participant_count,
                recipe_slots,
                duration_ticks: half(row, 0x38)? as i16,
                storage: row[0x3a..0x3c].try_into()?,
                camera_pitch_offset_degrees: float(row, 0x3c)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    fn executable(address: u32, bytes: &[u8]) -> Vec<u8> {
        let mut dol = vec![0; 0x100];
        dol[..4].copy_from_slice(&0x100u32.to_be_bytes());
        dol[0x48..0x4c].copy_from_slice(&address.to_be_bytes());
        dol[0x90..0x94].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        dol.extend_from_slice(bytes);
        dol
    }

    #[test]
    fn learning_preserves_inactive_slots_and_bounds_the_active_prefix() -> Result<()> {
        let mut bytes = vec![0; 11 * 41];
        bytes[..3].copy_from_slice(&[2, 66, 98]);
        bytes[40] = 255;
        let lists = learning(&executable(LEARNING, &bytes))?;
        assert_eq!(lists.len(), 11);
        assert_eq!(lists[0].active()?, [66, 98]);
        assert_eq!(lists[0].technique_slots[39], 255);
        assert!(learning(&executable(LEARNING, &bytes[..bytes.len() - 1])).is_err());
        bytes[0] = 41;
        assert!(learning(&executable(LEARNING, &bytes)).is_err());
        assert!(
            LearningList {
                count: 0,
                technique_slots: vec![0; 39]
            }
            .active()
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn combinations_preserve_signed_fields_empty_names_and_inactive_ingredients() -> Result<()> {
        let mut bytes = vec![0; 20 * 64 + 1];
        bytes[..4].copy_from_slice(&(COMBINATIONS + 20 * 64).to_be_bytes());
        bytes[4..6].copy_from_slice(&(-300i16).to_be_bytes());
        bytes[0x36..0x38].copy_from_slice(&i16::MIN.to_be_bytes());
        bytes[0x38..0x3a].copy_from_slice(&(-1i16).to_be_bytes());
        bytes[0x3a..0x3c].copy_from_slice(&[1, 2]);
        bytes[0x3c..0x40].copy_from_slice(&5f32.to_be_bytes());
        let rows = combinations(&executable(COMBINATIONS, &bytes))?;
        assert_eq!(
            (rows[0].name.as_deref(), rows[1].name.as_deref()),
            (Some(""), None)
        );
        assert_eq!((rows[0].native_id, rows[0].duration_ticks), (-300, -1));
        assert_eq!(rows[0].recipe_slots[5][3], i16::MIN);
        assert_eq!(rows[0].storage, [1, 2]);
        assert_eq!(
            serde_json::from_slice::<Vec<Combination>>(&serde_json::to_vec(&rows)?)?,
            rows
        );
        bytes[7] = 5;
        assert!(combinations(&executable(COMBINATIONS, &bytes)).is_err());
        bytes[7] = 0;
        bytes[0x3c..0x40].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(combinations(&executable(COMBINATIONS, &bytes)).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no media conversion"]
    fn original_tables_include_every_learning_row_and_unison_slot() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let dol = fs::read(root.join(format!("disc{disc}/sys/main.dol")))?;
            let lists = learning(&dol)?;
            assert_eq!(
                lists.iter().map(|row| row.count).collect::<Vec<_>>(),
                [35, 31, 37, 29, 37, 29, 23, 27, 29, 2, 0]
            );
            assert_eq!(lists[9].active()?, [66, 98]);
            assert!(lists[10].active()?.is_empty());
            let bytes: Vec<_> = lists
                .iter()
                .flat_map(|row| {
                    std::iter::once(row.count).chain(row.technique_slots.iter().copied())
                })
                .collect();
            assert_eq!(bytes, dol::slice(&dol, LEARNING, 11 * 41)?);
            let rows = combinations(&dol)?;
            assert_eq!(rows.len(), 20);
            assert_eq!((rows[0].native_id, rows[0].participant_count), (300, 0));
            assert_eq!(rows[0].name.as_deref(), Some(""));
            assert_eq!(rows[0].recipe_slots, [[0; 4]; 6]);
            assert_eq!((rows[7].native_id, rows[7].duration_ticks), (306, 180));
            assert_eq!(rows[7].name.as_deref(), Some("Innocent Edge"));
            assert_eq!(rows[7].recipe_slots, [[0; 4]; 6]);
            assert_eq!(
                (
                    rows[6].camera_pitch_offset_degrees,
                    rows[12].camera_pitch_offset_degrees
                ),
                (5., 12.)
            );
            assert_eq!(
                rows.iter()
                    .flat_map(|row| row.recipe_slots)
                    .filter(|slots| slots[0] != 0)
                    .count(),
                68
            );
        }
        Ok(())
    }
}
