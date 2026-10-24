//! Combination records stored in the initialized technique catalogue.
use crate::{dol, read, rel::Rel};
use anyhow::{Result, ensure};
use resonance_content::battle_unison::{COMBINATION_COUNT, Catalogue, Combination};

mod host;
mod opener;

const ADDRESS: u32 = 0x802086c8;
const ROW_BYTES: usize = 64;
const RECIPE_BYTES: usize = 8;
const RECIPE_COUNT: usize = 6;

pub(crate) fn read(executable: &[u8], module: &Rel) -> Result<Catalogue> {
    let combinations = dol::slice(executable, ADDRESS, COMBINATION_COUNT * ROW_BYTES)?
        .chunks_exact(ROW_BYTES)
        .enumerate()
        .map(|(index, row)| {
            let participants = usize::from(read::u16(row, 6)?);
            ensure!(
                (2..=4).contains(&participants),
                "invalid Unison participant count"
            );
            let mut recipes = Vec::new();
            for bytes in row[8..8 + RECIPE_COUNT * RECIPE_BYTES].chunks_exact(RECIPE_BYTES) {
                if bytes.iter().all(|&byte| byte == 0) {
                    continue;
                }
                ensure!(
                    bytes[participants * 2..].iter().all(|&byte| byte == 0),
                    "Unison recipe exceeds participant count"
                );
                recipes.push(
                    (0..participants)
                        .map(|i| read::u16(bytes, i * 2))
                        .collect::<Result<_>>()?,
                );
            }
            ensure!(
                row[0x3a..0x3c] == [0, 0],
                "unexpected Unison record padding"
            );
            Ok(Combination {
                id: (index + 1) as u8,
                name: dol::text(executable, read::u32(row, 0)?)?,
                native_id: read::u16(row, 4)?,
                participants: participants as u8,
                recipes,
                duration: read::u16(row, 0x38)?,
                camera_pitch: read::f32(row, 0x3c)?,
            })
        })
        .collect::<Result<_>>()?;
    let catalogue = Catalogue {
        source_sha256: crate::digest(executable),
        host: host::read(module)?,
        openers: opener::read(module)?,
        combinations,
        placements: crate::arte::unison_placements(executable)?,
    };
    catalogue.validate()?;
    Ok(catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module() -> Rel {
        let (constants, records) = (0x100, 0x1100);
        let mut bytes = vec![0; records + 0xa44];
        let put = |bytes: &mut [u8], at: usize, value: u16| {
            bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
        };
        for (at, value) in [(0xf74, 25f32), (0xf44, 0.5), (0xf40, 150.), (0xf34, 1500.)] {
            bytes[constants + at..constants + at + 4].copy_from_slice(&value.to_be_bytes());
        }
        for (at, value) in [(0x960, 30), (0x962, 10), (0x974, 200), (0x978, 3)] {
            put(&mut bytes, records + at, value);
        }
        bytes[records + 0x978 + 13] = 1;
        bytes[records + 0x994 + 2..records + 0x994 + 8].copy_from_slice(&[25, 4, 0, 0, 8, 255]);
        bytes[records + 0x994 + 8..records + 0x994 + 12].copy_from_slice(&0.5f32.to_be_bytes());
        put(&mut bytes, records + 0x9a0, 65534);
        for (at, tick, groups) in [(0x9ac, 10, &[0, 1][..]), (0x9ec, 20, &[0][..])] {
            put(&mut bytes, records + at, tick);
            bytes[records + at + 2] = 8;
            bytes[records + at + 3] = groups.len() as u8;
            bytes[records + at + 4..records + at + 4 + groups.len()].copy_from_slice(groups);
            for offset in [8, 12] {
                bytes[records + at + offset..records + at + offset + 4]
                    .copy_from_slice(&30f32.to_be_bytes());
            }
            put(&mut bytes, records + at + 32, 65535);
        }
        for (at, words) in [
            (0xa2c, &[0, 13, 0, 90][..]),
            (0xa34, &[0, 13, 1, 90][..]),
            (0xa3c, &[8, 0, 40, 65535][..]),
        ] {
            for (index, &value) in words.iter().enumerate() {
                put(&mut bytes, records + at + index * 2, value);
            }
        }
        Rel {
            bytes,
            sections: vec![
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (constants, 0x1000),
                (records, 0xa44),
            ],
            pointers: Default::default(),
            local_targets: Default::default(),
        }
    }

    #[test]
    fn opening_contacts_keep_shared_geometry_and_later_mage_window() -> Result<()> {
        use resonance_content::{battle_action::ContactAttachments, battle_normal::Event};
        let openers = opener::read(&module())?;
        let contacts = |index: usize| {
            openers[index]
                .events
                .iter()
                .find_map(|(at, event)| match event {
                    Event::Contact(contact) => Some((*at, contact)),
                    _ => None,
                })
                .unwrap()
        };
        let (at, common) = contacts(0);
        assert_eq!(
            (at, common.duration, common.radius, common.height),
            (13, 9, 30., 30.)
        );
        assert!(
            matches!(&common.attachments, ContactAttachments::Groups(groups) if groups == &[0, 1])
        );
        let (at, late) = contacts(2);
        assert_eq!((at, late.duration), (23, 9));
        assert!(matches!(&late.attachments, ContactAttachments::Groups(groups) if groups == &[0]));
        assert_eq!(
            openers
                .iter()
                .map(|row| row.contact_ticks)
                .collect::<Vec<_>>(),
            [30, 30, 45, 45, 30, 30, 30, 30, 30]
        );
        assert!(
            openers[0]
                .events
                .iter()
                .any(|event| matches!(event, (11, Event::Forward(4.))))
        );
        Ok(())
    }

    #[test]
    fn preserves_order_empty_recipes_and_shared_native_ids() -> Result<()> {
        let table_bytes = COMBINATION_COUNT * ROW_BYTES;
        let name = b"Combination\0";
        let mut executable = vec![0; 0x100 + table_bytes + name.len()];
        for (at, value) in [
            (7 * 4, 0x100),
            (0x48 + 7 * 4, ADDRESS),
            (0x90 + 7 * 4, (table_bytes + name.len()) as u32),
        ] {
            executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        executable[0x100 + table_bytes..].copy_from_slice(name);
        for (index, row) in executable[0x100..0x100 + table_bytes]
            .chunks_exact_mut(ROW_BYTES)
            .enumerate()
        {
            row[..4].copy_from_slice(&(ADDRESS + table_bytes as u32).to_be_bytes());
            row[4..6].copy_from_slice(&303_u16.to_be_bytes());
            row[6..8].copy_from_slice(&2_u16.to_be_bytes());
            row[0x38..0x3a].copy_from_slice(&100_u16.to_be_bytes());
            if index != 6 {
                row[8..12].copy_from_slice(&[0, 6, 0, 40]);
            }
        }
        let arte_offset = executable.len();
        let arte_bytes = 253 * 88;
        executable.resize(arte_offset + arte_bytes, 0);
        for (at, value) in [
            (8 * 4, arte_offset as u32),
            (0x48 + 8 * 4, 0x80202f90),
            (0x90 + 8 * 4, arte_bytes as u32),
        ] {
            executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        let placement = arte_offset + 88;
        executable[placement..placement + 2].copy_from_slice(&204_u16.to_be_bytes());
        executable[placement + 0x48..placement + 0x4c].copy_from_slice(&40_u32.to_be_bytes());
        executable[placement + 0x4c..placement + 0x4e].copy_from_slice(&430_u16.to_be_bytes());
        executable[placement + 0x4e..placement + 0x50].copy_from_slice(&78_u16.to_be_bytes());
        let catalogue = read(&executable, &module())?;
        assert_eq!(catalogue.placements.len(), 253);
        assert_eq!(
            catalogue.placements[1],
            resonance_content::battle_unison::Placement {
                native_id: 204,
                altitude: 40,
                distance: 430,
                duration: 78,
            }
        );
        assert_eq!(catalogue.placements[0].duration, 0);
        assert_eq!(catalogue.combinations[0].recipes, [vec![6, 40]]);
        assert_eq!(
            catalogue.combinations[2].native_id,
            catalogue.combinations[3].native_id
        );
        assert!(catalogue.combinations[6].recipes.is_empty());
        assert_eq!(catalogue.combinations.last().unwrap().id, 19);
        executable[0x100 + 12] = 1;
        assert!(read(&executable, &module()).is_err());
        Ok(())
    }
}
