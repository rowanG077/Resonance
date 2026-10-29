//! Offset-table members used by native world and actor packages.
use crate::dol;
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub(crate) fn read_named(directory: &Path, filename: &str) -> Result<Vec<u8>> {
    ensure!(
        !filename.contains(['/', '\\']),
        "invalid native resource name {filename}"
    );
    let file = directory.join(filename);
    let file = if file.is_file() {
        file
    } else {
        let lowercase = directory.join(filename.to_ascii_lowercase());
        if lowercase.is_file() {
            lowercase
        } else {
            fs::read_dir(directory)?
                .filter_map(|entry| entry.ok())
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(filename)
                })
                .map(|entry| entry.path())
                .unwrap_or(lowercase)
        }
    };
    fs::read(file).with_context(|| format!("missing native resource {filename}"))
}

pub(crate) fn member(bytes: &[u8], index: usize, expected: usize) -> Result<Option<&[u8]>> {
    let count = dol::u32(bytes, 0)? as usize;
    ensure!(
        count == expected && index < count,
        "unexpected native archive"
    );
    let offsets = (0..count)
        .map(|slot| dol::u32(bytes, 4 + slot * 4).map(|at| at as usize))
        .collect::<Result<Vec<_>>>()?;
    let start = offsets[index];
    if start == 0 {
        return Ok(None);
    }
    let end = offsets
        .into_iter()
        .filter(|&offset| offset > start)
        .min()
        .unwrap_or(bytes.len());
    ensure!(start >= 4 + count * 4, "archive member overlaps directory");
    Ok(Some(
        bytes
            .get(start..end)
            .context("archive member exceeds file")?,
    ))
}
