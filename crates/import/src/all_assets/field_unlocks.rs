//! World-map monuments unlock regional Long-range Mode through event flags.
use crate::{embedded, read::u16 as half, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::MapLocation;
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, num::NonZeroU16, path::Path};

const DATA: usize = 6;
const ROW_BYTES: usize = 12;
const TABLE_BYTES: usize = 13 * ROW_BYTES;
// The original event-bit region occupies 512 bytes before the party roster.
const EVENT_FLAGS: u16 = 512 * 8;

#[derive(Debug, PartialEq, Serialize)]
struct LongRangeUnlock {
    name: String,
    location: u16,
    /// The first flag also records discovery; empty extra slots do nothing.
    event_flags: [Option<NonZeroU16>; 3],
}

fn table(file: &Path) -> Option<usize> {
    match file.file_name()?.to_str()? {
        "US_r_Top2field.rel" | "US_m_Top2field.rel" | "US_Top2field.rel" => Some(0x8c),
        "r_Top2field.rel" | "m_Top2field.rel" | "Top2field.rel" => Some(0x20),
        "Top2fieldD.rel" => Some(0x37c),
        _ => None,
    }
}

fn decode(
    bytes: &[u8],
    locations: &BTreeMap<u16, MapLocation>,
    mut name: impl FnMut(usize) -> Result<String>,
) -> Result<Vec<LongRangeUnlock>> {
    ensure!(
        bytes.len() == TABLE_BYTES,
        "invalid long-range unlock table extent"
    );
    let (rows, terminator) = bytes.split_at(TABLE_BYTES - ROW_BYTES);
    ensure!(
        terminator == [0; ROW_BYTES],
        "invalid long-range unlock terminator"
    );
    rows.chunks_exact(ROW_BYTES)
        .enumerate()
        .map(|(index, row)| {
            let location = half(row, 4)?;
            ensure!(
                locations.contains_key(&location),
                "invalid unlock location {location}"
            );
            let flags = [half(row, 6)?, half(row, 8)?, half(row, 10)?];
            ensure!(
                flags[0] != 0 && flags.iter().all(|&flag| flag < EVENT_FLAGS),
                "invalid unlock event flags {flags:?}"
            );
            let name = name(index)?;
            ensure!(!name.is_empty(), "empty long-range region name");
            Ok(LongRangeUnlock {
                name,
                location,
                event_flags: flags.map(NonZeroU16::new),
            })
        })
        .collect()
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Option<Vec<String>>> {
    let Some(offset) = table(file) else {
        return Ok(None);
    };
    let rel = Rel::read(file)?;
    let world = crate::menu::world_map(
        &super::world_map::read(executable)?,
        &crate::field_catalogue::read(executable)?,
        &super::inventory_ui::read(executable)?,
    )?;
    let bytes = rel
        .at((DATA, offset))?
        .get(..TABLE_BYTES)
        .context("truncated long-range unlock table")?;
    ensure!(
        rel.pointers
            .range((DATA, offset)..(DATA, offset + TABLE_BYTES))
            .all(|(&(section, at), _)| section == DATA
                && (at - offset) % ROW_BYTES == 0
                && at < offset + TABLE_BYTES - ROW_BYTES),
        "unexpected long-range unlock relocation"
    );
    let mut names = Vec::new();
    let records = decode(bytes, &world.locations, |index| {
        let pointer = rel.pointer(DATA, offset + index * ROW_BYTES)?;
        names.push(pointer);
        rel.text(pointer)
    })?;
    embedded::write(
        file,
        output,
        "long-range-unlocks",
        &records,
        json!({
            "section": DATA, "offset": offset, "bytes": TABLE_BYTES, "names": names,
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both locally extracted original discs; no media conversion"]
    fn original_long_range_unlocks_cover_all_field_modules() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("long-range-unlocks"));
        let expected = [
            (0x2d, [0x385, 0, 0]),
            (0x2e, [0x386, 0, 0]),
            (0x2f, [0x388, 0x389, 0]),
            (0x30, [0x387, 0, 0]),
            (0x31, [0x38a, 0x38b, 0]),
            (0x32, [0x38d, 0, 0]),
            (0x33, [0x38c, 0, 0]),
            (0x126, [0x38f, 0x390, 0]),
            (0x127, [0x391, 0x392, 0x39a]),
            (0x128, [0x393, 0x395, 0]),
            (0x129, [0x398, 0x399, 0]),
            (0x12a, [0x394, 0x396, 0x397]),
        ];
        let mut publications = BTreeSet::new();
        for disc in [1, 2] {
            let extracted = root.join(format!("disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let world = crate::menu::world_map(
                &super::super::world_map::read(&executable)?,
                &crate::field_catalogue::read(&executable)?,
                &super::super::inventory_ui::read(&executable)?,
            )?;
            for module in [
                "US_r_Top2field.rel",
                "US_m_Top2field.rel",
                "US_Top2field.rel",
                "r_Top2field.rel",
                "m_Top2field.rel",
                "Top2field.rel",
                "Top2fieldD.rel",
            ] {
                let file = extracted.join("files").join(module);
                let rel = Rel::read(&file)?;
                let offset = table(&file).unwrap();
                let bytes = &rel.at((DATA, offset))?[..TABLE_BYTES];
                assert_eq!(
                    crate::digest(bytes),
                    "281b6c0b454731dbf895e2ad291e422d89d4d6e5fe79ef666d77c959a023accb"
                );
                let names = |index| rel.text(rel.pointer(DATA, offset + index * ROW_BYTES)?);
                let records = decode(bytes, &world.locations, names)?;
                assert_eq!(records.len(), expected.len());
                for (record, &(location, flags)) in records.iter().zip(&expected) {
                    assert_eq!(record.location, location);
                    assert_eq!(record.event_flags, flags.map(NonZeroU16::new));
                }
                if module.starts_with("US_") {
                    assert_eq!(records[0].name, "Triet Desert");
                    assert_eq!(records[11].name, "Altamira and Ymir");
                }
                let paths = cook(&file, &executable, &output)?.unwrap();
                assert_eq!(paths.len(), 2);
                let published: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[0]))?)?;
                assert_eq!(published, serde_json::to_value(&records)?);
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(source["module"], module);
                assert_eq!(source["offset"], offset);
                assert_eq!(source["source_sha256"], crate::digest(&fs::read(&file)?));
                assert_eq!(source["names"].as_array().unwrap().len(), records.len());
                publications.insert(paths[0].clone());
                assert!(decode(&bytes[..TABLE_BYTES - 1], &world.locations, names).is_err());
                for (at, value) in [
                    (4, 0u16),
                    (4, 0x100),
                    (6, 0),
                    (8, EVENT_FLAGS),
                    (10, EVENT_FLAGS),
                    (TABLE_BYTES - 2, 1),
                ] {
                    let mut damaged = bytes.to_vec();
                    damaged[at..at + 2].copy_from_slice(&value.to_be_bytes());
                    assert!(decode(&damaged, &world.locations, names).is_err());
                }
                assert!(decode(bytes, &world.locations, |_| Ok(String::new())).is_err());
            }
        }
        assert_eq!(
            publications.len(),
            2,
            "English and Japanese localized records"
        );
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
