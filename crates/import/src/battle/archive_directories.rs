//! Native modules retain directories for several archive builds. Preserve them
//! all, but only use a directory whose declared EOF matches the shipped archive.
use super::{
    all::physical_ranges,
    embedded::{self, Layout},
};
use crate::{read::u32 as word, rel::Rel};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, ops::Range, path::Path};

const DATA: usize = 5;

#[derive(Clone, Copy, Serialize)]
pub(super) struct ArchiveLayout {
    pub magic: usize,
    pub skill: usize,
    pub arena: usize,
    pub weapon: usize,
    pub weapon_slots: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Archive {
    Magic,
    Skill,
    Arena,
    Weapon,
}

impl Archive {
    pub(crate) const ALL: [Self; 4] = [Self::Magic, Self::Skill, Self::Arena, Self::Weapon];

    pub(super) fn file(self) -> &'static str {
        match self {
            Self::Magic => "BTLmagic.dat",
            Self::Skill => "BTLskill.dat",
            Self::Arena => "BTLbg.dat",
            Self::Weapon => "BTLwepon.dat",
        }
    }

    fn table(self, layout: ArchiveLayout) -> (usize, usize, usize) {
        match self {
            Self::Magic => (layout.magic, 121, 1),
            // Four offsets are followed immediately by relocated native handlers.
            Self::Skill => (layout.skill, super::all::SKILL_ARCHIVE_ROWS, 1),
            Self::Arena => (layout.arena, 98, 0),
            Self::Weapon => (layout.weapon, layout.weapon_slots, 0),
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SlotKind {
    Unused,
    Resource,
    Alias { slot: u16 },
    End,
}

#[derive(Debug, PartialEq, Serialize)]
struct Slot {
    /// Exact authored offset, including every zero slot and alignment slot.
    offset: u32,
    #[serde(flatten)]
    kind: SlotKind,
}

#[derive(Serialize)]
pub(super) struct Directory {
    archive: Archive,
    /// Relative to the extracted files directory.
    path: String,
    first_resource_slot: usize,
    slots: Vec<Slot>,
    declared_eof: u32,
    shipped_bytes: u64,
    /// A length match is a prerequisite for using these physical ranges.
    compatible_length: bool,
    #[serde(skip)]
    ranges: Vec<(u16, Range<usize>)>,
}

impl Directory {
    pub(super) fn into_ranges(self) -> Result<Vec<(u16, Range<usize>)>> {
        self.check_length()?;
        Ok(self.ranges)
    }

    fn check_length(&self) -> Result<()> {
        ensure!(
            self.compatible_length,
            "{} directory declares {} bytes, shipped archive has {}",
            self.path,
            self.declared_eof,
            self.shipped_bytes
        );
        Ok(())
    }

    fn entry(&self, id: u16, zero_alias: bool) -> Result<&Slot> {
        self.check_length()?;
        let index = usize::from(id);
        let slot = self
            .slots
            .get(index)
            .context("archive package outside directory")?;
        ensure!(
            index >= self.first_resource_slot
                && index + 1 < self.slots.len()
                && !matches!(slot.kind, SlotKind::End),
            "invalid archive package {id}"
        );
        ensure!(
            !matches!(slot.kind, SlotKind::Unused)
                || (zero_alias
                    && slot.offset == 0
                    && self.slots[index + 1..]
                        .iter()
                        .any(|slot| matches!(slot.kind, SlotKind::End))),
            "missing archive package {id}"
        );
        Ok(slot)
    }

    /// A requested alias binds the same bounded physical package as its owner.
    pub(super) fn range(&self, id: u16, zero_alias: bool) -> Result<Range<usize>> {
        let start = self.entry(id, zero_alias)?.offset as usize;
        self.ranges
            .iter()
            .find(|(_, range)| range.start == start)
            .map(|(_, range)| range.clone())
            .context("archive package has no physical range")
    }

    /// Ordinary loaders scan forward to the next nonzero offset, preserving entry order.
    pub(super) fn load_range(&self, id: u16) -> Result<Range<usize>> {
        let start = self.entry(id, false)?.offset as usize;
        let end = self.slots[usize::from(id) + 1..]
            .iter()
            .find(|slot| slot.offset != 0)
            .context("unterminated archive package")?
            .offset as usize;
        ensure!(
            start < end && end as u64 <= self.shipped_bytes,
            "invalid archive package span"
        );
        Ok(start..end)
    }

    pub(super) fn from_offsets(
        archive: Archive,
        path: &str,
        offsets: &[u32],
        first: usize,
        shipped_bytes: u64,
    ) -> Result<Self> {
        let (slots, declared_eof) = slots(offsets, first)?;
        let ranges = physical_ranges(offsets, first, u64::from(declared_eof))?;
        Ok(Self {
            archive,
            path: path.into(),
            first_resource_slot: first,
            slots,
            declared_eof,
            shipped_bytes,
            compatible_length: u64::from(declared_eof) == shipped_bytes,
            ranges,
        })
    }
}

fn slots(offsets: &[u32], first: usize) -> Result<(Vec<Slot>, u32)> {
    let eof = offsets
        .iter()
        .rposition(|&offset| offset != 0)
        .context("empty archive directory")?;
    ensure!(eof > first, "archive directory has no resources");
    let mut seen = BTreeMap::new();
    let slots = offsets
        .iter()
        .enumerate()
        .map(|(index, &offset)| {
            let kind = if index == eof {
                SlotKind::End
            } else if index < first || index > eof || (offset == 0 && index != first) {
                ensure!(offset == 0, "nonzero offset outside archive resources");
                SlotKind::Unused
            } else if let Some(&slot) = seen.get(&offset) {
                SlotKind::Alias { slot }
            } else {
                seen.insert(offset, u16::try_from(index)?);
                SlotKind::Resource
            };
            Ok(Slot { offset, kind })
        })
        .collect::<Result<_>>()?;
    Ok((slots, offsets[eof]))
}

pub(super) fn read(
    rel: &Rel,
    layout: ArchiveLayout,
    archive: Archive,
    path: &str,
    shipped_bytes: u64,
) -> Result<Directory> {
    let (offset, count, first) = archive.table(layout);
    ensure!(
        rel.pointers
            .range((DATA, offset)..(DATA, offset + count * 4))
            .next()
            .is_none(),
        "native relocation inside archive directory"
    );
    let bytes = rel
        .at((DATA, offset))?
        .get(..count * 4)
        .context("truncated archive directory")?;
    let offsets = bytes
        .chunks_exact(4)
        .map(|row| word(row, 0))
        .collect::<Result<Vec<_>>>()?;
    Directory::from_offsets(archive, path, &offsets, first, shipped_bytes)
}

pub(super) fn enemy_package(path: &Path, usual: &[u8], id: u16) -> Result<Vec<u8>> {
    let directory = super::actions::member(usual, 10)?;
    let index = usize::from(id) * 4;
    let start = word(directory, index)? as usize;
    let end = word(directory, index + 4)? as usize;
    crate::compression::decode(&super::all::read_range(path, start..end)?)
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    let rel = Rel::read(file)?;
    let sources = super::all::Sources::for_module(file)?.context("missing battle source layout")?;
    let files = file
        .parent()
        .context("battle module has no parent directory")?;
    let directories = Archive::ALL
        .into_iter()
        .map(|archive| {
            read(
                &rel,
                layout.archives,
                archive,
                sources.archive(archive),
                files.join(sources.archive(archive)).metadata()?.len(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    embedded::write(
        file,
        output,
        "battle-archive-directories",
        &directories,
        json!({
            "section": DATA, "tables": layout.archives,
        }),
    )
    .map(Some)
}

#[test]
#[ignore = "requires both original extracted discs; no archive conversion"]
fn original_archive_directories_preserve_all_modules_and_gate_foreign_ranges() -> Result<()> {
    use std::{collections::BTreeSet, fs};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = crate::temporary_path(&std::env::temp_dir().join("battle-archive-directories"));
    let mut counts = [0; 2];
    let mut publications = BTreeSet::new();
    for disc in [1, 2] {
        let files = root.join(format!("disc{disc}/files"));
        let canonical = Rel::read(&files.join("US_r_Top2Btl.rel"))?;
        for module in [
            "US_r_Top2Btl.rel",
            "r_Top2Btl.rel",
            "US_Top2Btl.rel",
            "US_m_Top2Btl.rel",
            "Top2Btl.rel",
            "m_Top2Btl.rel",
            "Top2BtlD.rel",
        ] {
            let file = files.join(module);
            let rel = Rel::read(&file)?;
            let (_, layout) = Layout::identify(&file).unwrap();
            let sources =
                super::all::Sources::for_module(&file)?.context("missing battle source layout")?;
            let mut expected_json = Vec::new();
            for archive in Archive::ALL {
                let path = sources.archive(archive);
                let length = files.join(path).metadata()?.len();
                let directory = read(&rel, layout.archives, archive, path, length)?;
                let (offset, count, first) = archive.table(layout.archives);
                assert_eq!(directory.slots.len(), count);
                for (index, slot) in directory.slots.iter().enumerate() {
                    assert_eq!(slot.offset, word(rel.at((DATA, offset))?, index * 4)?);
                    match slot.kind {
                        SlotKind::Unused => assert_eq!(slot.offset, 0),
                        SlotKind::Resource => assert!(index >= first),
                        SlotKind::Alias { slot: owner } => {
                            assert!(usize::from(owner) < index);
                            assert_eq!(directory.slots[usize::from(owner)].offset, slot.offset);
                        }
                        SlotKind::End => assert_eq!(slot.offset, directory.declared_eof),
                    }
                }
                assert_eq!(
                    directory
                        .slots
                        .iter()
                        .filter(|slot| matches!(slot.kind, SlotKind::End))
                        .count(),
                    1
                );
                let compatible =
                    module.starts_with("US_") || matches!(archive, Archive::Magic | Archive::Skill);
                assert_eq!(
                    directory.compatible_length, compatible,
                    "{disc}/{module}/{archive:?}"
                );
                counts[usize::from(compatible)] += 1;
                expected_json.push(serde_json::to_value(&directory)?);
                if compatible {
                    let baseline =
                        read(&canonical, Layout::RETAIL.archives, archive, path, length)?
                            .into_ranges()?;
                    for (id, range) in &baseline {
                        assert_eq!(directory.range(*id, false)?, *range);
                    }
                    assert_eq!(directory.into_ranges()?, baseline);
                } else {
                    assert_eq!(
                        directory.declared_eof,
                        match archive {
                            Archive::Arena => 26_220_512,
                            Archive::Weapon => 2_730_048,
                            _ => unreachable!(),
                        }
                    );
                    assert!(directory.into_ranges().is_err());
                }
            }
            let paths = cook(&file, &output)?.unwrap();
            assert_eq!(paths.len(), 2);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&fs::read(output.join(&paths[0]))?)?,
                json!(expected_json)
            );
            let provenance: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
            assert_eq!(provenance["module"], module);
            assert_eq!(
                provenance["source_sha256"],
                crate::digest(&fs::read(&file)?)
            );
            publications.insert(paths[0].clone());
            if module == "Top2BtlD.rel" {
                assert!(
                    rel.at((
                        DATA,
                        layout.archives.weapon + layout.archives.weapon_slots * 4
                    ))?
                    .starts_with(b"./Btl/Btlwepon.dat\0")
                );
            }
            assert!(rel.pointer(DATA, layout.archives.skill + 4 * 4).is_ok());
        }
    }
    assert_eq!(
        counts,
        [16, 40],
        "56 source directories, including 16 foreign archive builds"
    );
    assert_eq!(
        publications.len(),
        3,
        "US, Japanese release and unpadded Japanese debug directories"
    );
    // The corpus uses zero sentinels extensively; an alias must retain its own slot too.
    let (alias, eof) = slots(&[0, 0, 16, 16, 32, 0], 1)?;
    assert_eq!(eof, 32);
    assert_eq!(alias[0].kind, SlotKind::Unused);
    assert_eq!(alias[1].kind, SlotKind::Resource);
    assert_eq!(alias[3].kind, SlotKind::Alias { slot: 2 });
    assert_eq!(alias[4].kind, SlotKind::End);
    assert_eq!(alias[5].kind, SlotKind::Unused);
    let directory =
        Directory::from_offsets(Archive::Magic, "renamed.dat", &[0, 0, 16, 16, 32, 0], 1, 32)?;
    assert_eq!(directory.range(2, false)?, 16..32);
    assert_eq!(directory.range(3, false)?, 16..32);
    assert!(directory.load_range(2).is_err());
    assert_eq!(directory.load_range(3)?, 16..32);
    assert!(directory.range(5, true).is_err());
    fs::remove_dir_all(output)?;
    Ok(())
}
