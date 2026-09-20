//! Event-bank selectors bind a source filename to its MusyX project group.
use crate::{dol, embedded, read::u32 as word};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const ADDRESS: u32 = 0x801f97e0;
const COUNT: usize = 8;
const STRIDE: usize = 8;
const FAMILY: &str = "event-bank-directory";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Directory {
    pub entries: [Entry; COUNT],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub file: Option<String>,
    pub group: u8,
    /// Three bytes following the group ID, not read by bank selection.
    pub storage: [u8; 3],
}

impl Directory {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let entries = dol::slice(executable, ADDRESS, COUNT * STRIDE)?
            .chunks_exact(STRIDE)
            .map(|row| {
                Ok(Entry {
                    file: dol::optional_text(executable, word(row, 0)?)?,
                    group: row[4],
                    storage: row[5..8].try_into()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            entries: entries.try_into().expect("fixed-size event bank directory"),
        })
    }

    pub fn entry(&self, selector: i32) -> &Entry {
        &self.entries[(selector as usize) & (COUNT - 1)]
    }

    pub fn cook(extracted: &Path, output: &Path) -> Result<Vec<String>> {
        let file = extracted.join("sys/main.dol");
        embedded::write(
            &file,
            output,
            FAMILY,
            &Self::read(&fs::read(&file)?)?,
            serde_json::json!({"entries":{"address":ADDRESS,"count":COUNT,"stride":STRIDE}}),
        )
    }
}

impl Entry {
    pub fn source_path(&self) -> Result<String> {
        crate::source_path(
            self.file
                .as_deref()
                .context("event bank entry has no filename")?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::u16 as half;

    #[test]
    fn event_bank_selectors_preserve_every_slot_and_storage_byte() -> Result<()> {
        let mut executable = vec![0; 0x100 + COUNT * STRIDE];
        executable.extend(b"s/Absent.SND\0");
        let size = (executable.len() - 0x100) as u32;
        for (at, value) in [(0, 0x100u32), (0x48, ADDRESS), (0x90, size)] {
            executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        for (index, row) in executable[0x100..0x100 + COUNT * STRIDE]
            .chunks_exact_mut(STRIDE)
            .enumerate()
        {
            row[4] = 200 + index as u8;
            row[5..8].copy_from_slice(&[0xfe, index as u8, 0x81]);
        }
        executable[0x100..0x104]
            .copy_from_slice(&(ADDRESS + (COUNT * STRIDE) as u32).to_be_bytes());
        let directory = Directory::read(&executable)?;
        assert_eq!(directory.entry(0).file.as_deref(), Some("s/Absent.SND"));
        assert_eq!(directory.entry(0).source_path()?, "S/Absent.SND");
        assert!(directory.entry(1).source_path().is_err());
        for selector in [i32::MIN, -9, -8, -1, 0, 1, 7, 8, 15, i32::MAX] {
            let index = (selector as u32 & 7) as u8;
            let entry = directory.entry(selector);
            assert_eq!(entry.group, 200 + index);
            assert_eq!(entry.storage, [0xfe, index, 0x81]);
        }
        assert!(Directory::read(&executable[..0x100 + COUNT * STRIDE - 1]).is_err());
        executable[0x100..0x104].copy_from_slice(&0xdeadbeefu32.to_be_bytes());
        assert!(Directory::read(&executable).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; checks source project groups without audio playback"]
    fn original_event_bank_directories_reconstruct_rows_and_project_bindings() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("event-bank-directory"));
        let result = (|| -> Result<()> {
            for disc in 1..=2 {
                let extracted = root.join(format!("disc{disc}"));
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let directory = Directory::read(&executable)?;
                let rows = dol::slice(&executable, ADDRESS, COUNT * STRIDE)?;
                for (index, row) in rows.chunks_exact(STRIDE).enumerate() {
                    let entry = directory.entry(index as i32);
                    let pointer = word(row, 0)?;
                    let authored = dol::text(&executable, pointer)?;
                    assert_eq!(entry.file.as_deref(), Some(authored.as_str()));
                    let mut reconstructed = pointer.to_be_bytes().to_vec();
                    reconstructed.push(entry.group);
                    reconstructed.extend(entry.storage);
                    assert_eq!(reconstructed, row);
                    let (dir, file) = authored
                        .split_once('/')
                        .context("source bank has no directory")?;
                    let path = format!("{}/{file}", dir.to_ascii_uppercase());
                    assert_eq!(entry.source_path()?, path);
                    let bytes = fs::read(extracted.join("files").join(path))?;
                    let project = word(&bytes, 4)? as usize;
                    assert_eq!(half(&bytes, project + 4)?, u16::from(row[4]));
                    assert_eq!(
                        resonance_audio_cook::bank::Bank::parse(&bytes)?.group()?,
                        u16::from(entry.group)
                    );
                }
                let paths = Directory::cook(&extracted, &output)?;
                assert_eq!(
                    embedded::read::<Directory>(&output, FAMILY, "main.dol")?,
                    directory
                );
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(
                    source["entries"],
                    serde_json::json!({"address":ADDRESS,"count":COUNT,"stride":STRIDE})
                );
                assert_eq!(source["source_sha256"], crate::digest(&executable));
                eprintln!(
                    "disc {disc}: eight event bank directory rows and project groups reconstructed"
                );
            }
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
