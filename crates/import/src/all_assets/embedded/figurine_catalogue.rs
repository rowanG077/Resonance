//! Complete figurine records, before resource admission or preview-specific placement.
use super::text::{TextPool, TextRef};
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
    pub hidden_prefix: TextRef,
    pub default_elevation: f32,
    pub tagged_efreet_elevation: f32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub title: Option<TextRef>,
    pub archive: TextRef,
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

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
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
    Ok(Catalogue {
        texts: text.values,
        title,
        archive,
        records,
        preview,
    })
}

pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::embedded::write(file, output, FAMILY, &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn patch(executable: &mut [u8], address: u32, bytes: &[u8]) -> Result<()> {
        let offset = dol::slice(executable, address, bytes.len())?.as_ptr() as usize
            - executable.as_ptr() as usize;
        executable[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original executables; no codecs or devices"]
    fn original_figurine_catalogue_preserves_records_and_publishes_shared_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("figurine-catalogue"));
        fs::create_dir(&output)?;
        let result = (|| -> Result<()> {
            let mut first = None;
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let c = read(&executable)?;
                assert_eq!(c.records.len(), 328);
                assert!(c.records.iter().any(Record::is_null));
                assert!(
                    c.records
                        .iter()
                        .any(|r| r.resource == Resource::Unavailable)
                );
                assert_eq!(c.required_text(c.records[80].name)?, "Efreet");
                assert_eq!(c.records[80].resource, Resource::TaggedNpc(73));
                assert_eq!(c.text(c.preview.hidden_prefix), "kk");
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

                let bone_address = dol::slice(&executable, RECORDS, COUNT * STRIDE)?
                    .chunks_exact(STRIDE)
                    .flat_map(|row| row[16..].chunks_exact(4))
                    .map(|pointer| word(pointer, 0))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .find(|&address| {
                        address != 0
                            && dol::slice(&executable, address, 1).is_ok_and(|s| s[0] == b'-')
                    })
                    .context("authored hidden bone prefix")?;
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
                    (HIDDEN_PREFIX, b"\x0b\0X\0".to_vec()),
                    (DEFAULT_ELEVATION, 12.5f32.to_be_bytes().to_vec()),
                    (EFREET_ELEVATION, (-70f32).to_be_bytes().to_vec()),
                ] {
                    patch(&mut executable, address, &bytes)?;
                }
                let changed = read(&executable)?;
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
                assert_eq!(changed.text(changed.archive), "x");
                assert_eq!(changed.records[327].appearance_row, 0x8000_0003);
                assert_eq!(changed.preview.default_elevation, 12.5);
                assert_eq!(changed.preview.tagged_efreet_elevation, -70.);
                assert_eq!(changed.text(changed.preview.hidden_prefix), "\x0b\0X");
                patch(&mut executable, last + 76, &u32::MAX.to_be_bytes())?;
                assert!(read(&executable).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(output)?;
        result
    }
}
