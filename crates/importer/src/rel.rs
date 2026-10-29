//! Stored REL sections and local data relocations needed by world tables.
use crate::dol;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(crate) struct Rel {
    bytes: Vec<u8>,
    sections: Vec<(usize, usize)>,
    pub pointers: BTreeMap<(usize, usize), (usize, usize)>,
}

impl Rel {
    pub fn read(bytes: Vec<u8>) -> Result<Self> {
        let count = dol::u32(&bytes, 12)? as usize;
        ensure!(count <= 32, "invalid REL section count");
        let table = dol::u32(&bytes, 16)? as usize;
        let sections = (0..count)
            .map(|id| {
                let row = table + id * 8;
                Ok((
                    (dol::u32(&bytes, row)? & !3) as usize,
                    dol::u32(&bytes, row + 4)? as usize,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut pointers = BTreeMap::new();
        let module_id = dol::u32(&bytes, 0)?;
        let start = dol::u32(&bytes, 40)? as usize;
        let size = dol::u32(&bytes, 44)? as usize;
        ensure!(
            size.is_multiple_of(8) && size <= 4096,
            "invalid REL import table"
        );
        for entry in (start..start + size).step_by(8) {
            let module = dol::u32(&bytes, entry)?;
            let mut cursor = dol::u32(&bytes, entry + 4)? as usize;
            let (mut section, mut offset) = (0usize, 0usize);
            loop {
                let row = bytes
                    .get(cursor..cursor + 8)
                    .context("truncated REL relocation")?;
                let delta = dol::u16(row, 0)? as usize;
                let kind = row[2];
                let target = row[3] as usize;
                let addend = dol::u32(row, 4)? as usize;
                cursor += 8;
                match kind {
                    203 => break,
                    202 => {
                        section = target;
                        offset = 0;
                    }
                    _ => {
                        offset = offset
                            .checked_add(delta)
                            .context("REL relocation overflow")?;
                        if kind == 1 && module == module_id {
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
        })
    }

    pub fn slice(&self, section: usize, offset: usize, size: usize) -> Result<&[u8]> {
        let &(base, extent) = self.sections.get(section).context("missing REL section")?;
        ensure!(
            base > 0 && offset.checked_add(size).is_some_and(|end| end <= extent),
            "REL read outside stored section {section}:{offset:#x}"
        );
        self.bytes
            .get(base + offset..base + offset + size)
            .context("truncated REL section")
    }

    pub fn tail(&self, section: usize, offset: usize) -> Result<&[u8]> {
        let &(_, extent) = self.sections.get(section).context("missing REL section")?;
        ensure!(offset < extent, "REL offset exceeds stored section");
        self.slice(section, offset, extent - offset)
    }

    pub fn pointer(&self, section: usize, offset: usize) -> Result<(usize, usize)> {
        self.pointers
            .get(&(section, offset))
            .copied()
            .with_context(|| format!("missing REL data relocation {section}:{offset:#x}"))
    }

    pub fn text(&self, section: usize, offset: usize) -> Result<String> {
        let &(base, extent) = self.sections.get(section).context("missing REL section")?;
        ensure!(
            base > 0 && offset < extent,
            "REL text outside stored section"
        );
        let bytes = self
            .bytes
            .get(base + offset..base + extent)
            .context("truncated REL text")?;
        let end = bytes
            .iter()
            .take(4096)
            .position(|&byte| byte == 0)
            .context("unterminated REL string")?;
        let (decoded, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[..end]);
        ensure!(!invalid, "invalid REL string encoding");
        Ok(decoded.into_owned())
    }
}
