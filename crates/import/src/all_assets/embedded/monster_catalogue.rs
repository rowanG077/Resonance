//! Monster book identities, location bindings and display text; combat data is separate.
use super::text::{FixedText, TextPool, TextRef, TextSource};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const FAMILY: &str = "monster-catalogue";
const RECORDS: u32 = 0x802113f4;
const RECORD_COUNT: usize = 251;
const LOCATIONS: u32 = 0x80211328;
const LOCATION_COUNT: usize = 51;
const STAT_LABELS: [u32; 2] = [0x8035d300, 0x8035d30c];
const DIRECT: [(TextKind, u32, usize); 9] = [
    (TextKind::UnavailableStats, 0x8035d2f8, 8),
    (TextKind::UnknownStats, 0x8035d304, 8),
    (TextKind::ListPosition, 0x8035d320, 8),
    (TextKind::Number, 0x8035d370, 4),
    (TextKind::Hp, 0x8035d374, 4),
    (TextKind::Tp, 0x8035d378, 4),
    (TextKind::EntryNumber, 0x8035d37c, 4),
    (TextKind::VariantPosition, 0x8035d380, 8),
    (TextKind::UnknownItem, 0x801aa9ec, 12),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextKind {
    UnavailableStats,
    UnknownStats,
    ListPosition,
    Number,
    Hp,
    Tp,
    EntryNumber,
    VariantPosition,
    UnknownItem,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub name: Option<TextRef>,
    pub description: Option<TextRef>,
    /// Groups the "count undiscovered monsters" script query, independently of species.
    pub unseen_count_group: u8,
    pub location: u8,
    pub storage: [u8; 2],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct StatLabels {
    /// The enemy variant has no statistics (its HP sentinel is -1).
    pub unavailable: Option<TextRef>,
    /// The player has not revealed the enemy's statistics.
    pub unknown: Option<TextRef>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub records: Vec<Record>,
    pub locations: Vec<Option<TextRef>>,
    pub direct: BTreeMap<TextKind, FixedText>,
    pub stat_labels: StatLabels,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required monster catalogue text")?))
    }
    pub(crate) fn direct_text(&self, kind: TextKind) -> &str {
        self.text(self.direct[&kind].text)
    }
    pub(crate) fn location(&self, selector: u8) -> Result<&str> {
        self.required_text(
            *self
                .locations
                .get(usize::from(selector))
                .context("monster location outside catalogue")?,
        )
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut text = TextPool::default();
    let records = dol::slice(executable, RECORDS, RECORD_COUNT * 12)?
        .chunks_exact(12)
        .map(|row| {
            Ok(Record {
                name: text.reference(executable, word(row, 0)?)?,
                description: text.reference(executable, word(row, 4)?)?,
                unseen_count_group: row[8],
                location: row[9],
                storage: row[10..12].try_into()?,
            })
        })
        .collect::<Result<_>>()?;
    let locations = text.table(executable, LOCATIONS, LOCATION_COUNT)?;
    let direct = DIRECT
        .into_iter()
        .map(|(kind, address, size)| Ok((kind, text.fixed(executable, address, size)?)))
        .collect::<Result<_>>()?;
    let [unavailable, unknown] = STAT_LABELS
        .map(|address| text.reference(executable, word(dol::slice(executable, address, 4)?, 0)?))
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    Ok((
        Catalogue {
            texts: text.values,
            records,
            locations,
            direct,
            stat_labels: StatLabels {
                unavailable,
                unknown,
            },
        },
        text.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, texts) = parse(executable)?;
    crate::embedded::write(
        file,
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "records":{"address":RECORDS,"count":RECORD_COUNT,"stride":12},
            "locations":{"address":LOCATIONS,"count":LOCATION_COUNT,"stride":4},
            "direct":DIRECT.map(|(kind,address,source_size)| serde_json::json!({"kind":kind,"address":address,"source_size":source_size})),
            "stat_labels":{"pointer_addresses":STAT_LABELS},"texts":texts,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;
    use std::fs;

    fn reconstruct(c: &Catalogue, sources: &[TextSource]) -> Result<Vec<(u32, Vec<u8>)>> {
        let pointer = |reference: Option<TextRef>| {
            reference
                .map_or(0, |id| sources[id.0].address)
                .to_be_bytes()
        };
        let mut records = Vec::new();
        for row in &c.records {
            records.extend(pointer(row.name));
            records.extend(pointer(row.description));
            records.extend([row.unseen_count_group, row.location]);
            records.extend(row.storage);
        }
        let mut spans = vec![
            (RECORDS, records),
            (
                LOCATIONS,
                c.locations.iter().copied().flat_map(pointer).collect(),
            ),
            (STAT_LABELS[0], pointer(c.stat_labels.unavailable).to_vec()),
            (STAT_LABELS[1], pointer(c.stat_labels.unknown).to_vec()),
        ];
        for (index, source) in sources.iter().enumerate() {
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(c.text(TextRef(index)));
            ensure!(!invalid, "monster text cannot reconstruct source encoding");
            let mut bytes = [encoded.as_ref(), &[0]].concat();
            assert_eq!(bytes.len() as u32, source.source_size);
            if let Some(&(kind, _, size)) = DIRECT
                .iter()
                .find(|(_, address, _)| *address == source.address)
            {
                bytes.extend(&c.direct[&kind].storage);
                assert_eq!(bytes.len(), size);
            }
            spans.push((source.address, bytes));
        }
        Ok(spans)
    }

    fn patch(executable: &mut [u8], address: u32, bytes: &[u8]) -> Result<()> {
        let offset = dol::slice(executable, address, bytes.len())?.as_ptr() as usize
            - executable.as_ptr() as usize;
        executable[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original executables; no codecs or devices"]
    fn original_monster_catalogue_reconstructs_complete_tables_and_publishes_shared_data()
    -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("monster-catalogue"));
        fs::create_dir(&output)?;
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let (c, sources) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&c)?)?;
                assert_eq!(c, restored);
                for (address, bytes) in reconstruct(&restored, &sources)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "span {address:#x}"
                    );
                }
                assert_eq!(c.records.len(), 251);
                assert_eq!(c.locations.len(), 51);
                assert!(
                    c.records
                        .iter()
                        .all(|row| usize::from(row.location) < c.locations.len())
                );
                assert_eq!(c.locations[2], c.locations[7]);
                assert_eq!(c.locations[16], c.locations[49]);
                assert_eq!(c.locations[36], c.locations[37]);
                assert_eq!(c.required_text(c.stat_labels.unavailable)?, "------");
                assert_eq!(c.required_text(c.stat_labels.unknown)?, "??????");
                assert_eq!(c.direct_text(TextKind::Number), "No.");
                assert_eq!(c.direct_text(TextKind::Hp), "HP");
                assert_eq!(c.direct_text(TextKind::Tp), "TP");
                assert_eq!(c.direct_text(TextKind::UnknownItem), "??????????");
                assert_eq!(c.direct_text(TextKind::ListPosition), "%d/%d");
                assert_eq!(c.direct_text(TextKind::EntryNumber), "%3u");
                assert_eq!(c.direct_text(TextKind::VariantPosition), "(%u/%u)");
                let paths = cook(&file, &executable, &output)?;
                assert_eq!(
                    crate::embedded::read::<Catalogue>(&output, FAMILY, "main.dol")?,
                    c
                );
                if let Some(expected) = &first {
                    assert_eq!(&paths[0], expected);
                } else {
                    first = Some(paths[0].clone());
                }
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(source["source_sha256"], crate::digest(&executable));
                assert_eq!(source["locations"]["count"], 51);

                let second_name =
                    sources[c.records[1].name.context("second monster name")?.0].address;
                let first_location = sources[c.locations[0].context("first location")?.0].address;
                let missing_stats =
                    sources[c.stat_labels.unavailable.context("missing-stat text")?.0].address;
                for (address, bytes) in [
                    (RECORDS, second_name.to_be_bytes().to_vec()),
                    (RECORDS + 4, vec![0; 4]),
                    (RECORDS + 8, vec![0xfe, 0xff, 0x5a, 0xa5]),
                    (
                        RECORDS + (RECORD_COUNT as u32 - 1) * 12 + 10,
                        vec![0x12, 0x34],
                    ),
                    (LOCATIONS + 4, first_location.to_be_bytes().to_vec()),
                    (LOCATIONS + 50 * 4, vec![0; 4]),
                    (STAT_LABELS[0], vec![0; 4]),
                    (STAT_LABELS[1], missing_stats.to_be_bytes().to_vec()),
                    (0x8035d320, b"\x0b\0X\0".to_vec()),
                    (0x8035d327, vec![0xac]),
                    (0x8035d377, vec![0xbd]),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let (changed, sources) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&changed)?)?;
                assert_eq!(changed, restored);
                assert_eq!(changed.records[0].name, changed.records[1].name);
                assert!(changed.records[0].description.is_none());
                assert_eq!(changed.records[0].storage, [0x5a, 0xa5]);
                assert_eq!(changed.locations[0], changed.locations[1]);
                assert!(changed.locations[50].is_none());
                assert!(changed.stat_labels.unavailable.is_none());
                assert_eq!(
                    changed.required_text(changed.stat_labels.unknown)?,
                    "------"
                );
                assert_eq!(changed.direct_text(TextKind::ListPosition), "\x0b\0X");
                assert!(changed.location(changed.records[0].location).is_err());
                assert!(changed.location(50).is_err());
                for (address, bytes) in reconstruct(&restored, &sources)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "changed span {address:#x}"
                    );
                }
                patch(&mut executable, RECORDS, &u32::MAX.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
