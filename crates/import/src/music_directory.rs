//! Music IDs select an arrangement and one of the original loading buffers.
use crate::{
    dol, embedded,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const ADDRESS: u32 = 0x801f982c;
const COUNT: usize = 113;
const STRIDE: usize = 8;
const FAMILY: &str = "music-directory";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Directory {
    /// Physical order, including the terminator and any rows after it.
    pub entries: Vec<Entry>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub id: i16,
    pub buffer: Buffer,
    /// Preserve authored casing, empty strings and null pointers distinctly.
    pub file: Option<String>,
}

/// The first three buffers each cache their last loaded music ID.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i16)]
pub(crate) enum Buffer {
    Buffer0 = 0,
    Buffer1 = 1,
    Buffer2 = 2,
    Resident = 3,
}

impl Directory {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let entries = dol::slice(executable, ADDRESS, COUNT * STRIDE)?
            .chunks_exact(STRIDE)
            .map(|row| {
                Ok(Entry {
                    id: half(row, 0)? as i16,
                    buffer: match half(row, 2)? as i16 {
                        0 => Buffer::Buffer0,
                        1 => Buffer::Buffer1,
                        2 => Buffer::Buffer2,
                        3 => Buffer::Resident,
                        value => bail!("unknown music loading buffer {value}"),
                    },
                    file: dol::optional_text(executable, word(row, 4)?)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            entries.iter().any(|entry| entry.id == -1),
            "unterminated music directory"
        );
        Ok(Self { entries })
    }

    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().take_while(|entry| entry.id != -1)
    }

    pub fn path(&self, id: u16) -> Result<String> {
        self.active()
            .find(|entry| entry.id == id as i16)
            .with_context(|| format!("music {id} is absent from the directory"))?
            .source_path()
    }

    #[cfg(test)]
    pub fn ids(&self, source: &str) -> Result<Vec<u16>> {
        let mut seen = std::collections::BTreeSet::new();
        let mut ids = Vec::new();
        for entry in self.active() {
            if seen.insert(entry.id) && entry.file.is_some() && entry.source_path()? == source {
                ids.push(entry.id as u16);
            }
        }
        Ok(ids)
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
        let file = self
            .file
            .as_deref()
            .context("music directory entry has no filename")?;
        crate::source_path(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn music_directory_preserves_physical_rows_and_first_match() -> Result<()> {
        let mut executable = vec![0; 0x100 + COUNT * STRIDE];
        executable.extend(b"s/First.song\0s/second.song\0");
        let size = (executable.len() - 0x100) as u32;
        for (at, value) in [(0, 0x100u32), (0x48, ADDRESS), (0x90, size)] {
            executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        for row in executable[0x100..0x100 + COUNT * STRIDE].chunks_exact_mut(STRIDE) {
            row[..2].copy_from_slice(&(-1i16).to_be_bytes());
        }
        for (index, id, buffer, string) in [
            (0, 7i16, 2i16, 0),
            (1, 7, 1, 13),
            (2, 8, 3, 0),
            (3, -1, 0, 12),
            (4, -2, 1, 13),
        ] {
            let at = 0x100 + index * STRIDE;
            executable[at..at + 2].copy_from_slice(&id.to_be_bytes());
            executable[at + 2..at + 4].copy_from_slice(&buffer.to_be_bytes());
            executable[at + 4..at + 8]
                .copy_from_slice(&(ADDRESS + (COUNT * STRIDE + string) as u32).to_be_bytes());
        }
        let directory = Directory::read(&executable)?;
        assert_eq!(directory.entries.len(), COUNT);
        assert_eq!(directory.active().count(), 3);
        assert_eq!(directory.path(7)?, "S/First.song");
        assert_eq!(directory.ids("S/First.song")?, [7, 8]);
        assert!(directory.ids("S/second.song")?.is_empty());
        assert!(directory.path((-2i16) as u16).is_err());
        assert_eq!(directory.entries[2].buffer, Buffer::Resident);
        assert_eq!(directory.entries[3].file.as_deref(), Some(""));
        assert_eq!(directory.entries[4].id, -2);
        assert_eq!(directory.entries[4].buffer, Buffer::Buffer1);
        assert_eq!(directory.entries[4].file.as_deref(), Some("s/second.song"));
        assert!(Directory::read(&executable[..0x100 + COUNT * STRIDE - 1]).is_err());
        let mut unterminated = executable.clone();
        for row in unterminated[0x100..0x100 + COUNT * STRIDE].chunks_exact_mut(STRIDE) {
            row[..2].copy_from_slice(&0i16.to_be_bytes());
        }
        assert!(Directory::read(&unterminated).is_err());
        executable[0x102..0x104].copy_from_slice(&4i16.to_be_bytes());
        assert!(Directory::read(&executable).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no synthesis or audio playback"]
    fn original_music_directories_reconstruct_rows_bindings_and_aliases() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("music-directory"));
        let result = (|| -> Result<()> {
            for disc in 1..=2 {
                let extracted = root.join(format!("disc{disc}"));
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let original = dol::slice(&executable, ADDRESS, COUNT * STRIDE)?;
                let directory = Directory::read(&executable)?;
                let mut names = BTreeMap::<String, Vec<u16>>::new();
                let mut aliases = BTreeMap::<String, Vec<u16>>::new();
                for (entry, row) in directory.entries.iter().zip(original.chunks_exact(STRIDE)) {
                    let pointer = word(row, 4)?;
                    assert_eq!(entry.file, dol::optional_text(&executable, pointer)?);
                    let mut bytes = entry.id.to_be_bytes().to_vec();
                    bytes.extend((entry.buffer as i16).to_be_bytes());
                    bytes.extend(pointer.to_be_bytes());
                    assert_eq!(bytes, row);
                }
                for row in original
                    .chunks_exact(STRIDE)
                    .take_while(|row| row[..2] != [255; 2])
                {
                    let id = half(row, 0)?;
                    let authored = dol::text(&executable, word(row, 4)?)?;
                    let (directory_name, name) = authored
                        .split_once('/')
                        .context("source song has no directory")?;
                    let path = format!("{}/{name}", directory_name.to_ascii_uppercase());
                    assert_eq!(directory.path(id)?, path);
                    assert_eq!(crate::media::music_path(&executable, id)?, path);
                    aliases
                        .entry(crate::digest(&fs::read(
                            extracted.join("files").join(&path),
                        )?))
                        .or_default()
                        .push(id);
                    names.entry(path).or_default().push(id);
                }
                for (path, ids) in &names {
                    assert_eq!(&directory.ids(path)?, ids);
                }
                let shared = crate::digest(&fs::read(extracted.join("files/S/bgm_c016.song"))?);
                aliases
                    .get_mut(&shared)
                    .context("source aliases missing")?
                    .sort_unstable();
                assert_eq!(aliases[&shared], [0, 60, 62, 64, 67, 84, 99, 102, 111]);
                assert_eq!(directory.active().count(), 112);
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
                    "disc {disc}: {} music directory rows, 112 active bindings",
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
