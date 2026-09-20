//! Checked access to stored REL sections and relocated local data pointers.
use crate::read::{u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) struct Rel {
    pub(crate) bytes: Vec<u8>,
    pub(crate) sections: Vec<(usize, usize)>,
    pub(crate) pointers: BTreeMap<(usize, usize), (usize, usize)>,
    pub(crate) local_targets: BTreeSet<(usize, usize)>,
}
impl Rel {
    pub(crate) fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        let count = word(&bytes, 12)? as usize;
        ensure!(count <= 32, "invalid REL section count");
        let table = word(&bytes, 16)? as usize;
        let sections = (0..count)
            .map(|i| {
                Ok((
                    (word(&bytes, table + i * 8)? & !3) as usize,
                    word(&bytes, table + i * 8 + 4)? as usize,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut pointers = BTreeMap::new();
        let mut local_targets = BTreeSet::new();
        let import = word(&bytes, 40)? as usize;
        let imports = word(&bytes, 44)? as usize;
        ensure!(
            imports.is_multiple_of(8) && imports <= 4096,
            "invalid REL import table"
        );
        for entry in (import..import + imports).step_by(8) {
            let module = word(&bytes, entry)?;
            let mut cursor = word(&bytes, entry + 4)? as usize;
            let (mut section, mut offset) = (0, 0usize);
            loop {
                let delta = usize::from(half(&bytes, cursor)?);
                let record = bytes
                    .get(cursor..cursor + 8)
                    .context("truncated REL relocation")?;
                let target = usize::from(record[3]);
                let addend = word(record, 4)? as usize;
                cursor += 8;
                match record[2] {
                    203 => break,
                    202 => {
                        section = target;
                        offset = 0;
                    }
                    kind => {
                        offset = offset
                            .checked_add(delta)
                            .context("REL relocation overflow")?;
                        if kind != 0 && kind < 201 && module == word(&bytes, 0)? {
                            local_targets.insert((target, addend));
                        }
                        if kind == 1 && module == word(&bytes, 0)? {
                            ensure!(
                                pointers
                                    .insert((section, offset), (target, addend))
                                    .is_none(),
                                "duplicate REL data pointer"
                            );
                        }
                    }
                }
            }
        }
        Ok(Self {
            bytes,
            sections,
            pointers,
            local_targets,
        })
    }
    pub(crate) fn local_targets(&self) -> &BTreeSet<(usize, usize)> {
        &self.local_targets
    }
    pub(crate) fn at(&self, pointer: (usize, usize)) -> Result<&[u8]> {
        let &(base, size) = self
            .sections
            .get(pointer.0)
            .context("missing REL section")?;
        ensure!(
            base != 0 && pointer.1 < size,
            "REL pointer outside stored section"
        );
        self.bytes
            .get(base + pointer.1..base + size)
            .context("truncated REL section")
    }
    pub(crate) fn pointer(&self, section: usize, offset: usize) -> Result<(usize, usize)> {
        self.pointers
            .get(&(section, offset))
            .copied()
            .with_context(|| format!("missing REL data relocation {section}:{offset:#x}"))
    }
    pub(crate) fn text(&self, pointer: (usize, usize)) -> Result<String> {
        let bytes = self.at(pointer)?;
        let end = bytes
            .iter()
            .take(4096)
            .position(|&byte| byte == 0)
            .context("unterminated REL string")?;
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[..end]);
        ensure!(!invalid, "invalid REL string encoding");
        Ok(text.into_owned())
    }
}
