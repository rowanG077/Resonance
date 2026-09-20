use crate::read::u32 as word;
use anyhow::{Result, ensure};
use std::ops::Range;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Directory {
    pub count: usize,
    pub members: Vec<Option<usize>>,
}

impl Directory {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=65536).contains(&self.count)
                && self.members.len() == self.count
                && self.members.iter().enumerate().all(|(index, member)| {
                    member.is_none_or(|canonical| {
                        canonical <= index && self.members.get(canonical) == Some(&Some(canonical))
                    })
                }),
            "invalid physical resource directory or aliases"
        );
        Ok(())
    }

    pub fn new(ranges: &[Option<Range<usize>>]) -> Self {
        let mut canonical = std::collections::BTreeMap::new();
        Self {
            count: ranges.len(),
            members: ranges
                .iter()
                .enumerate()
                .map(|(index, range)| {
                    range
                        .as_ref()
                        .map(|range| *canonical.entry((range.start, range.end)).or_insert(index))
                })
                .collect(),
        }
    }
}

/// Only the physical walker checks uncovered bytes. MAP tables use FE fill to
/// align their first payload to 32 bytes; other unvisited bytes must be zero.
pub(super) fn padding(bytes: &[u8], header: usize, ranges: &[Option<Range<usize>>]) -> Result<()> {
    let mut ranges: Vec<_> = ranges.iter().flatten().collect();
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    ranges.dedup();
    let mut end = header;
    for range in ranges {
        ensure!(range.start >= end, "overlapping archive payloads");
        padding_range(bytes, end..range.start)?;
        end = range.end;
    }
    padding_range(bytes, end..bytes.len())
}

fn padding_range(bytes: &[u8], range: Range<usize>) -> Result<()> {
    let padding = &bytes[range.clone()];
    ensure!(
        padding.iter().all(|&byte| byte == 0)
            || (range.len() < 32
                && range.end.is_multiple_of(32)
                && padding.iter().all(|&byte| byte == 0xfe)),
        "unvisited non-padding archive bytes at {:#x}..{:#x}",
        range.start,
        range.end
    );
    Ok(())
}

/// Resource banks store offset/length pairs, including empty and aliased slots.
/// Validate the complete layout before treating an untagged resource as a bank.
pub(super) fn entries(bytes: &[u8]) -> Option<Vec<Option<Range<usize>>>> {
    let count = word(bytes, 0).ok()? as usize;
    if !(1..=65_536).contains(&count) {
        return None;
    }
    let header = 4 + count * 8;
    if header > bytes.len() {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    let mut ranges = Vec::new();
    for index in 0..count {
        let start = word(bytes, 4 + index * 8).ok()? as usize;
        let size = word(bytes, 8 + index * 8).ok()? as usize;
        if size == 0 {
            entries.push(None);
            continue;
        }
        let end = start.checked_add(size)?;
        if start < header || end > bytes.len() {
            return None;
        }
        entries.push(Some(start..end));
        ranges.push((start, end));
    }
    ranges.sort_unstable();
    ranges.dedup();
    let mut end = header;
    for (start, next) in ranges {
        if start < end || start - end >= 32 {
            return None;
        }
        end = next;
    }
    (bytes.len() - end < 32).then_some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_empty_slots_and_aliases_without_accepting_overlaps() {
        let mut bytes = vec![0; 96];
        for (index, value) in [4_u32, 64, 16, 0, 0, 64, 16, 80, 16].iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            entries(&bytes),
            Some(vec![Some(64..80), None, Some(64..80), Some(80..96)])
        );
        let ranges = entries(&bytes).unwrap();
        assert_eq!(
            Directory::new(&ranges).members,
            [Some(0), None, Some(0), Some(3)]
        );
        assert!(padding(&bytes, 36, &ranges).is_ok());
        bytes[36..64].fill(0xfe);
        assert!(padding(&bytes, 36, &ranges).is_ok());
        bytes[40] = 1;
        assert!(entries(&bytes).is_some());
        assert!(padding(&bytes, 36, &ranges).is_err());
        assert!(padding(&[0, 0, 1], 0, &[None]).is_err());
        bytes[28..32].copy_from_slice(&72_u32.to_be_bytes());
        assert!(entries(&bytes).is_none());
    }
}
