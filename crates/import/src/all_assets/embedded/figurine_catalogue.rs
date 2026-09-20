//! Complete figurine records, before resource admission or preview-specific placement.
use super::text::{FixedText, TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{f32 as float, u32 as word},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "figurine-catalogue";
const RECORDS: u32 = 0x802280c0;
const COUNT: usize = 328;
const STRIDE: usize = 80;
const TITLE: u32 = 0x8019d6f4;
const ARCHIVE: u32 = 0x801aaa18;
const HIDDEN_PREFIX: u32 = 0x8035d3b8;
const DEFAULT_ELEVATION: u32 = 0x8035d350;
const EFREET_ELEVATION: u32 = 0x8035d3bc;
const RESOURCE_TAG: u32 = 0x20000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Resource {
    DirectNpc(u32),
    TaggedNpc(u32),
    Unavailable,
    /// Other negative selectors fail native archive admission but remain source data.
    Negative(i32),
}
impl Resource {
    fn read(value: u32) -> Self {
        if value == u32::MAX {
            Self::Unavailable
        } else if (value as i32) < 0 {
            Self::Negative(value as i32)
        } else if value < RESOURCE_TAG {
            Self::DirectNpc(value)
        } else {
            Self::TaggedNpc(value - RESOURCE_TAG)
        }
    }
    #[cfg(test)]
    fn source(self) -> u32 {
        match self {
            Self::DirectNpc(value) => value,
            Self::TaggedNpc(value) => value + RESOURCE_TAG,
            Self::Unavailable => u32::MAX,
            Self::Negative(value) => value as u32,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub name: Option<TextRef>,
    pub description: Option<TextRef>,
    pub resource: Resource,
    pub appearance_row: u32,
    /// Ordered source strings; a leading '-' hides a prefix and otherwise shows it.
    pub bone_rules: [Option<TextRef>; 16],
}
impl Record {
    pub(crate) fn is_null(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.resource == Resource::DirectNpc(0)
            && self.appearance_row == 0
            && self.bone_rules.iter().all(Option::is_none)
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preview {
    /// Hidden before applying the record's ordered show/hide overrides.
    pub hidden_prefix: FixedText,
    pub default_elevation: f32,
    pub tagged_efreet_elevation: f32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub title: Option<TextRef>,
    pub archive: FixedText,
    pub records: Vec<Record>,
    pub preview: Preview,
}
impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
    pub(crate) fn required_text(&self, reference: Option<TextRef>) -> Result<&str> {
        Ok(self.text(reference.context("null required figurine text")?))
    }
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut text = TextPool::default();
    let records = dol::slice(executable, RECORDS, COUNT * STRIDE)?
        .chunks_exact(STRIDE)
        .map(|row| {
            Ok(Record {
                name: text.reference(executable, word(row, 0)?)?,
                description: text.reference(executable, word(row, 4)?)?,
                resource: Resource::read(word(row, 8)?),
                appearance_row: word(row, 12)?,
                bone_rules: row[16..]
                    .chunks_exact(4)
                    .map(|pointer| text.reference(executable, word(pointer, 0)?))
                    .collect::<Result<Vec<_>>>()?
                    .try_into()
                    .unwrap(),
            })
        })
        .collect::<Result<_>>()?;
    let title = text.reference(executable, word(dol::slice(executable, TITLE, 4)?, 0)?)?;
    let archive = text.fixed(executable, ARCHIVE, 12)?;
    let preview = Preview {
        hidden_prefix: text.fixed(executable, HIDDEN_PREFIX, 4)?,
        default_elevation: float(dol::slice(executable, DEFAULT_ELEVATION, 4)?, 0)?,
        tagged_efreet_elevation: float(dol::slice(executable, EFREET_ELEVATION, 4)?, 0)?,
    };
    Ok((
        Catalogue {
            texts: text.values,
            title,
            archive,
            records,
            preview,
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
            "records":{"address":RECORDS,"count":COUNT,"stride":STRIDE},
            "title":{"pointer_address":TITLE},"archive":{"address":ARCHIVE,"source_size":12},
            "hidden_prefix":{"address":HIDDEN_PREFIX,"source_size":4},
            "default_elevation":{"address":DEFAULT_ELEVATION,"source_size":4},
            "tagged_efreet_elevation":{"address":EFREET_ELEVATION,"source_size":4},"texts":texts,
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
            records.extend(row.resource.source().to_be_bytes());
            records.extend(row.appearance_row.to_be_bytes());
            records.extend(row.bone_rules.into_iter().flat_map(pointer));
        }
        let mut spans = vec![
            (RECORDS, records),
            (TITLE, pointer(c.title).to_vec()),
            (
                DEFAULT_ELEVATION,
                c.preview.default_elevation.to_be_bytes().to_vec(),
            ),
            (
                EFREET_ELEVATION,
                c.preview.tagged_efreet_elevation.to_be_bytes().to_vec(),
            ),
        ];
        for (index, source) in sources.iter().enumerate() {
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(c.text(TextRef(index)));
            ensure!(!invalid, "figurine text cannot reconstruct source encoding");
            let mut bytes = [encoded.as_ref(), &[0]].concat();
            assert_eq!(bytes.len() as u32, source.source_size);
            if source.address == ARCHIVE {
                bytes.extend(&c.archive.storage);
            }
            if source.address == HIDDEN_PREFIX {
                bytes.extend(&c.preview.hidden_prefix.storage);
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
    fn original_figurine_catalogue_reconstructs_complete_tables_and_publishes_shared_data()
    -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("figurine-catalogue"));
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
                assert_eq!(c.records.len(), 328);
                assert!(c.records.iter().any(Record::is_null));
                assert!(
                    c.records
                        .iter()
                        .any(|r| r.resource == Resource::Unavailable)
                );
                assert_eq!(c.required_text(c.records[80].name)?, "Efreet");
                assert_eq!(c.records[80].resource, Resource::TaggedNpc(73));
                assert_eq!(c.text(c.preview.hidden_prefix.text), "kk");
                assert_eq!(c.preview.default_elevation, 0.);
                assert_eq!(c.preview.tagged_efreet_elevation, -80.);
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
                assert_eq!(source["records"]["count"], 328);

                let bone = c
                    .records
                    .iter()
                    .flat_map(|r| r.bone_rules)
                    .flatten()
                    .find(|&id| c.text(id).starts_with('-'))
                    .context("authored hidden bone prefix")?;
                let bone_address = sources[bone.0].address;
                let last = RECORDS + (COUNT as u32 - 1) * STRIDE as u32;
                for (address, bytes) in [
                    (RECORDS, vec![0; 80]),
                    (RECORDS + STRIDE as u32 + 8, 73u32.to_be_bytes().to_vec()),
                    (
                        RECORDS + 2 * STRIDE as u32 + 8,
                        0xffff_fffe_u32.to_be_bytes().to_vec(),
                    ),
                    (last + 4, vec![0; 4]),
                    (last + 12, 0x8000_0003u32.to_be_bytes().to_vec()),
                    (last + 16, bone_address.to_be_bytes().to_vec()),
                    (last + 20, vec![0; 4]),
                    (last + 76, bone_address.to_be_bytes().to_vec()),
                    (TITLE, vec![0; 4]),
                    (ARCHIVE, b"x\0".to_vec()),
                    (ARCHIVE + 11, vec![0xbe]),
                    (HIDDEN_PREFIX, b"\x0b\0X\0".to_vec()),
                    (DEFAULT_ELEVATION, 12.5f32.to_be_bytes().to_vec()),
                    (EFREET_ELEVATION, (-70f32).to_be_bytes().to_vec()),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let (changed, sources) = parse(&executable)?;
                let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&changed)?)?;
                assert_eq!(changed, restored);
                assert!(changed.records[0].is_null());
                assert_eq!(changed.records[1].resource, Resource::DirectNpc(73));
                assert_eq!(changed.records[2].resource, Resource::Negative(-2));
                assert_eq!(
                    changed.records[327].bone_rules[0],
                    changed.records[327].bone_rules[15]
                );
                assert!(changed.records[327].bone_rules[1].is_none());
                assert!(changed.title.is_none());
                assert_eq!(changed.text(changed.preview.hidden_prefix.text), "\x0b\0X");
                for (address, bytes) in reconstruct(&restored, &sources)? {
                    assert_eq!(
                        bytes,
                        dol::slice(&executable, address, bytes.len())?,
                        "changed span {address:#x}"
                    );
                }
                patch(&mut executable, last + 76, &u32::MAX.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
