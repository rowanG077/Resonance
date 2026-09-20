//! Source declarations and bounded packages shared by menus, audio and physical recovery.
use crate::{
    all_assets::roles::declared_path, dol, field_resources::resolve_path, read::u32 as word,
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    ops::Range,
    path::Path,
};

pub(crate) struct Sources {
    pub usual: String,
    pub enemy: String,
    archives: [String; 4],
}

impl Sources {
    pub(crate) fn read(extracted: &Path) -> Result<Self> {
        Self::read_with(extracted, &fs::read(extracted.join("sys/main.dol"))?)
    }

    pub(crate) fn read_with(extracted: &Path, executable: &[u8]) -> Result<Self> {
        let files = extracted.join("files");
        let module = Rel::read(&files.join(resolve_path(&files, "US_r_Top2Btl.rel")?))?;
        let declaration = |offset| declared_path(&files, &module.text((4, offset))?);
        Ok(Self {
            usual: declared_path(&files, &dol::text(executable, 0x8017e53c)?)?,
            enemy: declaration(0x2404)?,
            archives: [
                declaration(0x1f34)?,
                declaration(0x1f4c)?,
                declaration(0x24b4)?,
                declaration(0xdfc)?,
            ],
        })
    }

    /// These mixed archives have general consumers, but complete battle preparation is deferred.
    pub(crate) fn deferred_paths(&self) -> BTreeSet<String> {
        [&self.usual, &self.enemy]
            .into_iter()
            .chain(&self.archives)
            .cloned()
            .collect()
    }
}

pub(crate) fn enemy_package(path: &Path, usual: &[u8], id: u16) -> Result<Vec<u8>> {
    let directory = section(usual, 10)?;
    let index = usize::from(id) * 4;
    let start = word(directory, index)? as usize;
    let end = word(directory, index + 4)? as usize;
    crate::compression::decode(&read_range(path, start..end)?)
}

pub(crate) fn section(bytes: &[u8], index: usize) -> Result<&[u8]> {
    let count = word(bytes, 0)? as usize;
    ensure!(
        count < 4096 && index < count,
        "invalid resource table member {index}"
    );
    let start = word(bytes, 4 + index * 4)? as usize;
    ensure!(
        start >= 4 + count * 4,
        "missing resource table member {index}"
    );
    let end = (0..count)
        .map(|i| word(bytes, 4 + i * 4).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&v| v > start)
        .min()
        .unwrap_or(bytes.len());
    bytes
        .get(start..end)
        .context("resource table member outside archive")
}

/// Skip null, end and alias table entries; each physical byte range is cooked once.
pub(crate) fn physical_ranges(
    offsets: &[u32],
    first: usize,
    length: u64,
) -> Result<Vec<(u16, Range<usize>)>> {
    let eof = offsets
        .iter()
        .position(|&v| u64::from(v) == length)
        .context("source archive has no EOF table entry")?;
    ensure!(eof >= first, "source archive ends before its first package");
    ensure!(
        offsets[eof + 1..].iter().all(|&v| v == 0),
        "nonempty source slot after archive EOF"
    );
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for id in first..eof {
        let start = offsets[id];
        if (start == 0 && id != first) || !seen.insert(start) {
            continue;
        }
        let end = offsets[id + 1..=eof]
            .iter()
            .copied()
            .find(|&v| v > start)
            .context("source package has no range end")?;
        ensure!(u64::from(end) <= length, "source package exceeds archive");
        result.push((id.try_into()?, start as usize..end as usize));
    }
    let mut ranges = result.iter().map(|(_, range)| range).collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    let mut cursor = 0;
    for range in ranges {
        ensure!(
            range.start == cursor,
            "archive coverage gap or overlap: expected offset {cursor:#x}, found {:#x}",
            range.start
        );
        cursor = range.end;
    }
    ensure!(cursor as u64 == length, "archive coverage stops before EOF");
    Ok(result)
}

pub(crate) fn read_range(path: &Path, range: Range<usize>) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    ensure!(
        range.start < range.end && range.end as u64 <= file.metadata()?.len(),
        "invalid source package range"
    );
    file.seek(SeekFrom::Start(range.start as u64))?;
    let mut bytes = vec![0; range.len()];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_archives_require_complete_nonoverlapping_coverage() {
        assert_eq!(
            physical_ranges(&[0, 0, 10, 10, 20, 0], 1, 20).unwrap(),
            vec![(1, 0..10), (2, 10..20)]
        );
        assert!(physical_ranges(&[5, 10, 20], 0, 20).is_err());
        assert!(physical_ranges(&[0, 10, 5, 20], 0, 20).is_err());
        assert!(physical_ranges(&[0, 10, 20, 5], 0, 20).is_err());
    }
}
