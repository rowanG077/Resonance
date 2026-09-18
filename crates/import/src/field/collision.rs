//! Physical collision descriptors and all declared geometry, including aliases.
use crate::read::{f32 as float, u16 as half, u32 as word};
use anyhow::{Result, ensure};
use resonance_content::field::CollisionGroup;
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Detect,
    /// Actor packages use short indices regardless of their first word.
    Short,
}

pub(crate) struct Mesh {
    pub groups: Vec<CollisionGroup>,
    pub metadata: Metadata,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Metadata {
    index_bytes: usize,
    header_storage: Option<u32>,
    groups: Vec<GroupLayout>,
    /// Nonzero storage outside the bounded native vertex/index readers.
    unreferenced_ranges: Vec<Range<usize>>,
}

#[derive(Serialize, Deserialize)]
struct GroupLayout {
    vertices: Range<usize>,
    triangles: Range<usize>,
    /// The short descriptor's final word is not read by collision queries.
    storage: Option<u32>,
}

impl Mesh {
    pub(crate) fn read(bytes: &[u8], format: Format) -> Result<Self> {
        let compact = matches!(format, Format::Detect) && word(bytes, 0)? != 0;
        let (header, stride, index_bytes, count, header_storage) = if compact {
            (4, 16, 1, word(bytes, 0)? as usize, None)
        } else {
            (8, 20, 2, word(bytes, 4)? as usize, Some(word(bytes, 0)?))
        };
        ensure!(
            count <= (bytes.len() - header) / stride,
            "truncated collision groups"
        );
        let table_end = header + count * stride;
        let mut covered = vec![0..table_end];
        let mut groups = Vec::with_capacity(count);
        let mut layouts = Vec::with_capacity(count);
        for index in 0..count {
            let at = header + index * stride;
            let range = |count_at, offset_at, size| -> Result<Range<usize>> {
                let count = usize::from(half(bytes, at + count_at)?);
                let start = word(bytes, at + offset_at)? as usize;
                let end = start.checked_add(count * size);
                ensure!(
                    start >= table_end && end.is_some_and(|end| end <= bytes.len()),
                    "collision payload outside resource or overlapping its header"
                );
                Ok(start..end.unwrap())
            };
            let vertices = range(0, 4, 12)?;
            let triangles = range(2, 8, 3 * index_bytes)?;
            let group = CollisionGroup {
                surface: word(bytes, at + 12)?,
                vertices: vertices
                    .clone()
                    .step_by(12)
                    .map(|at| {
                        Ok([
                            float(bytes, at)?,
                            float(bytes, at + 4)?,
                            float(bytes, at + 8)?,
                        ])
                    })
                    .collect::<Result<_>>()?,
                triangles: triangles
                    .clone()
                    .step_by(3 * index_bytes)
                    .map(|at| {
                        if compact {
                            Ok([bytes[at] as u16, bytes[at + 1] as u16, bytes[at + 2] as u16])
                        } else {
                            Ok([half(bytes, at)?, half(bytes, at + 2)?, half(bytes, at + 4)?])
                        }
                    })
                    .collect::<Result<_>>()?,
            };
            group.validate()?;
            covered.extend([vertices.clone(), triangles.clone()]);
            groups.push(group);
            layouts.push(GroupLayout {
                vertices,
                triangles,
                storage: (!compact).then(|| word(bytes, at + 16)).transpose()?,
            });
        }
        // MAP/terrain loaders retain relative pointers. Native short, byte and
        // actor queries read only these arrays; gaps are not implicit records.
        Ok(Self {
            groups,
            metadata: Metadata {
                index_bytes,
                header_storage,
                groups: layouts,
                unreferenced_ranges: crate::read::unreferenced_ranges(bytes, covered),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use std::{
        fs,
        io::{Cursor, Read},
        path::Path,
    };

    fn reconstruct(bytes: &[u8], format: Format) -> Result<Metadata> {
        let mesh = Mesh::read(bytes, format)?;
        let groups: Vec<CollisionGroup> =
            serde_json::from_slice(&serde_json::to_vec(&mesh.groups)?)?;
        let metadata: Metadata = serde_json::from_slice(&serde_json::to_vec(&mesh.metadata)?)?;
        let mut rebuilt = vec![0; bytes.len()];
        let (header, stride) = if let Some(storage) = metadata.header_storage {
            rebuilt[..4].copy_from_slice(&storage.to_be_bytes());
            (8, 20)
        } else {
            (4, 16)
        };
        rebuilt[header - 4..header].copy_from_slice(&(groups.len() as u32).to_be_bytes());
        for (index, (group, layout)) in groups.iter().zip(&metadata.groups).enumerate() {
            let at = header + index * stride;
            rebuilt[at..at + 2].copy_from_slice(&(group.vertices.len() as u16).to_be_bytes());
            rebuilt[at + 2..at + 4].copy_from_slice(&(group.triangles.len() as u16).to_be_bytes());
            for (offset, value) in [
                (4, layout.vertices.start as u32),
                (8, layout.triangles.start as u32),
                (12, group.surface),
            ] {
                rebuilt[at + offset..at + offset + 4].copy_from_slice(&value.to_be_bytes());
            }
            if let Some(storage) = layout.storage {
                rebuilt[at + 16..at + 20].copy_from_slice(&storage.to_be_bytes());
            }
            let vertices: Vec<_> = group
                .vertices
                .iter()
                .flatten()
                .flat_map(|value| value.to_be_bytes())
                .collect();
            rebuilt[layout.vertices.clone()].copy_from_slice(&vertices);
            let indices: Vec<_> = group
                .triangles
                .iter()
                .flatten()
                .flat_map(|value| value.to_be_bytes()[2 - metadata.index_bytes..].to_vec())
                .collect();
            rebuilt[layout.triangles.clone()].copy_from_slice(&indices);
        }
        // Only explicitly identified unreferenced storage may differ. All
        // descriptors, declared geometry and zero padding reconstruct exactly.
        for range in &metadata.unreferenced_ranges {
            ensure!(
                bytes[range.clone()].iter().any(|&byte| byte != 0),
                "empty storage report"
            );
            rebuilt[range.clone()].copy_from_slice(&bytes[range.clone()]);
        }
        ensure!(rebuilt == bytes, "collision JSON lost source data");
        Ok(metadata)
    }

    #[test]
    fn physical_collision_retains_aliases_storage_and_checks_payloads() -> Result<()> {
        // Two short descriptors share geometry, including an unused vertex.
        let mut bytes = vec![0; 100];
        for (at, value) in [
            (0, 0x80000000u32),
            (4, 2),
            (12, 52),
            (16, 88),
            (20, 11),
            (24, 0xfedcba98),
            (32, 52),
            (36, 88),
            (40, 15),
            (44, 42),
            (48, 0x12345678),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        for at in [8, 28] {
            bytes[at..at + 2].copy_from_slice(&3u16.to_be_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&1u16.to_be_bytes());
        }
        for (index, value) in [1f32, 2., -3., 4., 5., 6., 7., 8., 9.]
            .into_iter()
            .enumerate()
        {
            bytes[52 + index * 4..56 + index * 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[90..92].copy_from_slice(&1u16.to_be_bytes());
        bytes[99] = 0x55;
        let metadata = reconstruct(&bytes, Format::Short)?;
        assert_eq!(metadata.unreferenced_ranges, [48..52, 94..100]);
        assert_eq!(metadata.groups[0].storage, Some(0xfedcba98));
        assert!(Mesh::read(&bytes, Format::Detect).is_err());
        assert!(Mesh::read(&bytes[..87], Format::Short).is_err());
        bytes[90..92].copy_from_slice(&3u16.to_be_bytes());
        assert!(Mesh::read(&bytes, Format::Short).is_err());
        bytes[90..92].copy_from_slice(&1u16.to_be_bytes());
        bytes[52..56].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(Mesh::read(&bytes, Format::Short).is_err());
        bytes[52..56].copy_from_slice(&0f32.to_be_bytes());
        bytes[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Mesh::read(&bytes, Format::Short).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs"]
    fn original_collision_reconstructs_all_field_sources() -> Result<()> {
        fn check(
            bytes: &[u8],
            format: Format,
            name: &str,
            counts: &mut [usize; 5],
            kind: usize,
        ) -> Result<()> {
            let metadata = reconstruct(bytes, format).with_context(|| name.to_owned())?;
            counts[kind] += 1;
            let storage: Vec<_> = metadata
                .groups
                .iter()
                .enumerate()
                .filter_map(|(index, group)| {
                    group
                        .storage
                        .filter(|&word| word != 0)
                        .map(|word| (index, word))
                })
                .collect();
            counts[3] += storage.len();
            counts[4] += metadata.unreferenced_ranges.len();
            if !storage.is_empty() || !metadata.unreferenced_ranges.is_empty() {
                eprintln!(
                    "{name}: suffix_words={storage:x?}, unreferenced_ranges={:x?}",
                    metadata.unreferenced_ranges
                );
            }
            Ok(())
        }
        fn visit(bytes: &[u8], name: &str, map: bool, counts: &mut [usize; 5]) -> Result<()> {
            if bytes.starts_with(b"MSCF") {
                let mut cabinet = cab::Cabinet::new(Cursor::new(bytes))?;
                let names: Vec<_> = cabinet
                    .folder_entries()
                    .flat_map(|folder| folder.file_entries())
                    .map(|entry| entry.name().to_owned())
                    .collect();
                for member in names {
                    let mut expanded = Vec::new();
                    cabinet
                        .read_file(&member)?
                        .take(64 * 1024 * 1024 + 1)
                        .read_to_end(&mut expanded)?;
                    ensure!(
                        expanded.len() <= 64 * 1024 * 1024,
                        "oversized collision archive"
                    );
                    visit(&expanded, &format!("{name}/{member}"), map, counts)?;
                }
                return Ok(());
            }
            let ranges = match crate::field::sections(bytes) {
                Ok(ranges) => ranges,
                Err(error) if map => return Err(error).with_context(|| name.to_owned()),
                Err(_) => return Ok(()),
            };
            if map {
                for index in [4, 5] {
                    if let Some(range) = ranges.get(index).and_then(Option::as_ref) {
                        check(
                            &bytes[range.clone()],
                            Format::Detect,
                            &format!("{name}/{index}"),
                            counts,
                            0,
                        )?;
                    }
                }
                if let Some(range) = ranges.get(7).and_then(Option::as_ref) {
                    let bank = &bytes[range.clone()];
                    for (index, (_, range)) in crate::character::field_model_entries(bank)?
                        .into_iter()
                        .enumerate()
                    {
                        visit(
                            &bank[range],
                            &format!("{name}/7/{}", index + 1),
                            false,
                            counts,
                        )?;
                    }
                }
            } else if ranges[0].as_ref().is_some_and(|range| {
                matches!(
                    word(&bytes[range.clone()], 0x20).ok(),
                    Some(0x005b_bc61 | 0x00b7_49e0)
                )
            }) {
                let (indices, format, kind): (&[usize], _, _) = match ranges.len() {
                    31 => (&[29, 30], Format::Short, 2),
                    5 if ranges[1].is_none() && ranges[3].is_none() => (&[4], Format::Detect, 1),
                    _ => return Ok(()),
                };
                for &index in indices {
                    if let Some(range) = &ranges[index] {
                        let part = &bytes[range.clone()];
                        if !crate::animation::is_animation(part) {
                            check(part, format, &format!("{name}/{index}"), counts, kind)?;
                        }
                    }
                }
            }
            Ok(())
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut counts = [0; 5];
        for disc in ["disc1", "disc2"] {
            for directory in ["", "MAP", "FIELD"] {
                for entry in fs::read_dir(root.join(disc).join("files").join(directory))? {
                    let path = entry?.path();
                    if !path.is_file()
                        || !path.extension().is_some_and(|extension| {
                            ["bin", "dat", "d", "cab"]
                                .iter()
                                .any(|expected| extension.eq_ignore_ascii_case(expected))
                        })
                    {
                        continue;
                    }
                    visit(
                        &fs::read(&path)?,
                        &path.display().to_string(),
                        directory == "MAP",
                        &mut counts,
                    )?;
                }
            }
        }
        ensure!(
            counts[..3].iter().all(|&count| count > 0),
            "missing collision source family"
        );
        eprintln!("collision map/terrain/actor sources, nonzero suffix words/ranges: {counts:?}");
        Ok(())
    }
}
