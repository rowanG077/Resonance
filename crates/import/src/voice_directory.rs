//! Voice ID groups name archives; their low sixteen bits select an archive member.
use crate::{
    dol, embedded,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const ADDRESS: u32 = 0x802a30a0;
const COUNT: usize = 18;
const STRIDE: usize = 12;
const FAMILY: &str = "voice-directory";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Directory {
    /// Retain the complete physical table, including the lookup terminator.
    pub entries: Vec<Entry>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// Authored casing; extracted directory names are normalized on lookup.
    pub file: Option<String>,
    pub logical_base: u32,
    pub resource_id: u16,
    /// Final halfword, not read by archive lookup or playback dispatch.
    pub storage: u16,
}

impl Directory {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let entries = dol::slice(executable, ADDRESS, COUNT * STRIDE)?
            .chunks_exact(STRIDE)
            .map(|row| {
                Ok(Entry {
                    file: dol::optional_text(executable, word(row, 0)?)?,
                    logical_base: word(row, 4)?,
                    resource_id: half(row, 8)?,
                    storage: half(row, 10)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            entries.iter().any(|entry| entry.file.is_none()),
            "unterminated voice directory"
        );
        Ok(Self { entries })
    }

    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().take_while(|entry| entry.file.is_some())
    }

    pub fn path(&self, group: u32) -> Result<String> {
        self.active()
            .find(|entry| entry.logical_base == group)
            .with_context(|| format!("voice archive group {group:#x} is absent"))?
            .source_path()
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
        let source = self
            .file
            .as_deref()
            .context("voice directory terminator has no archive")?;
        crate::source_path(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut executable = vec![0; 0x100 + COUNT * STRIDE];
        executable[..4].copy_from_slice(&0x100u32.to_be_bytes());
        executable[0x48..0x4c].copy_from_slice(&ADDRESS.to_be_bytes());
        executable.extend(b"ev/First.afs\0cv/second.afs\0");
        let size = (executable.len() - 0x100) as u32;
        executable[0x90..0x94].copy_from_slice(&size.to_be_bytes());
        for (index, string, group, handle, storage) in [
            (0, 0, 0x10000u32, 0x78u16, 0xfedcu16),
            (1, 13, 0x10000, 0x1234, 7),
            (3, 13, 0x20000, 0x4321, 0x8000),
        ] {
            let at = 0x100 + index * STRIDE;
            executable[at..at + 4]
                .copy_from_slice(&(ADDRESS + (COUNT * STRIDE + string) as u32).to_be_bytes());
            executable[at + 4..at + 8].copy_from_slice(&group.to_be_bytes());
            executable[at + 8..at + 10].copy_from_slice(&handle.to_be_bytes());
            executable[at + 10..at + 12].copy_from_slice(&storage.to_be_bytes());
        }
        executable
    }

    #[test]
    fn voice_directory_preserves_storage_and_native_lookup_order() -> Result<()> {
        let executable = fixture();
        let directory = Directory::read(&executable)?;
        assert_eq!(directory.entries.len(), COUNT);
        assert_eq!(directory.active().count(), 2);
        assert_eq!(directory.path(0x10000)?, "EV/First.afs");
        assert!(directory.path(0x20000).is_err());
        assert!(directory.entries[2].source_path().is_err());
        assert_eq!(directory.entries[0].storage, 0xfedc);
        assert_eq!(directory.entries[3].resource_id, 0x4321);
        assert_eq!(directory.entries[3].storage, 0x8000);
        assert_eq!(directory.entries[3].file.as_deref(), Some("cv/second.afs"));
        assert_eq!(
            directory,
            serde_json::from_slice::<Directory>(&serde_json::to_vec(&directory)?)?
        );
        assert!(Directory::read(&executable[..0x100 + COUNT * STRIDE - 1]).is_err());
        let mut unterminated = executable.clone();
        for row in unterminated[0x100..0x100 + COUNT * STRIDE].chunks_exact_mut(STRIDE) {
            row[..4].copy_from_slice(&executable[0x100..0x104]);
        }
        assert!(Directory::read(&unterminated).is_err());
        let mut damaged = executable;
        damaged[0x100..0x104].copy_from_slice(&0xdeadbeefu32.to_be_bytes());
        assert!(Directory::read(&damaged).is_err());
        let mut unsafe_entry = directory.entries.into_iter().next().unwrap();
        unsafe_entry.file = Some("ev/../outside.afs".into());
        assert!(unsafe_entry.source_path().is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no archives or audio devices"]
    fn original_voice_directories_reconstruct_all_rows_on_both_discs() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("voice-directory"));
        let result = (|| -> Result<()> {
            for disc in 1..=2 {
                let extracted = root.join(format!("disc{disc}"));
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let original = dol::slice(&executable, ADDRESS, COUNT * STRIDE)?;
                let directory = Directory::read(&executable)?;
                let active = original
                    .chunks_exact(STRIDE)
                    .take_while(|row| row[..4] != [0; 4])
                    .count();
                assert_eq!(directory.active().count(), active);
                for (entry, row) in directory.entries.iter().zip(original.chunks_exact(STRIDE)) {
                    let pointer = word(row, 0)?;
                    assert_eq!(entry.file, dol::optional_text(&executable, pointer)?);
                    let mut reconstructed = pointer.to_be_bytes().to_vec();
                    reconstructed.extend(entry.logical_base.to_be_bytes());
                    reconstructed.extend(entry.resource_id.to_be_bytes());
                    reconstructed.extend(entry.storage.to_be_bytes());
                    assert_eq!(reconstructed, row);
                }
                let paths = Directory::cook(&extracted, &output)?;
                assert_eq!(
                    embedded::read::<Directory>(&output, FAMILY, "main.dol")?,
                    directory
                );
                let source: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(source["entries"]["count"], COUNT);
                assert_eq!(source["source_sha256"], crate::digest(&executable));
                eprintln!(
                    "disc {disc}: {} voice directory rows, {active} active",
                    directory.entries.len()
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
