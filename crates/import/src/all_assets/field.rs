//! Field metadata retains authored geometry, transforms and resource bindings.
use crate::read::{Field, u32 as word};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// The field loader reads fixed resource slots through slot 15.
pub(crate) const REQUIRED_SECTIONS: usize = 16;

/// Script and model-bank headers identify fields independently of archive names.
/// Leave child validation to the map reader so damaged code/models still fail.
pub(crate) fn is_map(bytes: &[u8], ranges: &[Option<Range<usize>>]) -> bool {
    let section = |index: usize| bytes.get(ranges.get(index)?.as_ref()?.clone());
    let (Some(script), Some(bank)) = (section(6), section(7)) else {
        return false;
    };
    ranges.len() >= REQUIRED_SECTIONS
        && super::script_header(script).is_some()
        && crate::character::field_model_count(bank).is_ok()
}

/// MAP ground/regions, actor package members 29/30 and FIELD terrain member 4 use
/// the same mesh format, in either byte-indexed or short-indexed form.
#[cfg(test)]
pub(crate) use crate::field::collision;

#[derive(Serialize, Deserialize)]
pub(crate) struct ModelBinding {
    pub archive_entry: usize,
    pub script_resource: u16,
    #[serde(skip)]
    pub range: Range<usize>,
}

/// Decode the whole MAP section 7, then cook each returned range as an actor
/// package. Entry zero is the ID table, not another physical model.
pub(crate) fn models(bytes: &[u8]) -> Result<Vec<ModelBinding>> {
    Ok(crate::character::field_model_entries(bytes)?
        .into_iter()
        .enumerate()
        .map(|(index, (script_resource, range))| ModelBinding {
            archive_entry: index + 1,
            script_resource,
            range,
        })
        .collect())
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CameraTrack {
    pub source_size: usize,
    /// Authored header word; targets begin twelve bytes beyond this offset.
    pub target_offset: u32,
    pub transforms: Vec<TransformKey>,
    pub targets: Vec<PositionKey>,
    /// Usually TARG; never consulted as a marker, but may alias another key.
    pub target_marker: Option<u32>,
    pub unreferenced_storage: Vec<crate::read::Storage>,
}

crate::read::record! {
    pub(crate) struct TransformKey(36) {
        pub time: f32 => 0,
        pub position: [f32; 3] => 4,
        pub rotation: [f32; 4] => 16,
        /// Neither camera constructor nor transform evaluator reads this word.
        pub unused_word: u32 => 32,
    }
}

crate::read::record! {
    pub(crate) struct PositionKey(20) {
        pub time: f32 => 0,
        pub position: [f32; 3] => 4,
        /// Neither camera constructor nor target evaluator reads this word.
        pub unused_word: u32 => 16,
    }
}

/// CAMM stores a linear position track and spherical quaternion track, plus an
/// optional independent target track. FOV and looping are supplied by events.
/// Target records begin at the offset in header word 1 plus 12; that offset can
/// point inside the final primary key and does not identify another header.
pub(crate) fn camera(bytes: &[u8]) -> Result<CameraTrack> {
    ensure!(bytes.starts_with(b"CAMM"), "expected CAMM camera track");
    let count = word(bytes, 8)? as usize;
    ensure!(
        count > 0 && count <= (bytes.len() - 12) / 36,
        "invalid camera key count"
    );
    let primary_end = 12 + count * 36;
    let mut covered = vec![0..primary_end];
    let transforms = (0..count)
        .map(|index| Field::read(bytes, 12 + index * 36))
        .collect::<Result<Vec<TransformKey>>>()?;
    ensure!(
        transforms
            .windows(2)
            .all(|keys| keys[0].time <= keys[1].time),
        "unordered camera keys"
    );
    let target_offset = word(bytes, 4)?;
    let mut target_marker = None;
    let targets = if target_offset == 0 {
        Vec::new()
    } else {
        let start = (target_offset as usize)
            .checked_add(12)
            .filter(|start| *start <= bytes.len())
            .ok_or_else(|| anyhow::anyhow!("invalid camera target range"))?;
        ensure!(
            count <= (bytes.len() - start) / 20,
            "truncated camera targets"
        );
        target_marker = Some(word(bytes, start - 4)?);
        covered.push(start - 4..start + count * 20);
        (0..count)
            .map(|index| Field::read(bytes, start + index * 20))
            .collect::<Result<Vec<PositionKey>>>()?
    };
    ensure!(
        targets.windows(2).all(|keys| keys[0].time <= keys[1].time),
        "unordered camera targets"
    );
    Ok(CameraTrack {
        source_size: bytes.len(),
        target_offset,
        transforms,
        targets,
        target_marker,
        unreferenced_storage: crate::read::unreferenced_storage(bytes, covered),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera_roundtrip(bytes: &[u8]) -> Result<CameraTrack> {
        let track: CameraTrack = serde_json::from_slice(&serde_json::to_vec(&camera(bytes)?)?)?;
        let mut restored = vec![0; track.source_size];
        let mut put = |offset: usize, words: Vec<u32>| {
            for (target, word) in restored[offset..].chunks_exact_mut(4).zip(words) {
                target.copy_from_slice(&word.to_be_bytes());
            }
        };
        put(
            0,
            vec![
                u32::from_be_bytes(*b"CAMM"),
                track.target_offset,
                track.transforms.len() as u32,
            ],
        );
        for (index, key) in track.transforms.iter().enumerate() {
            put(
                12 + index * 36,
                [key.time]
                    .into_iter()
                    .chain(key.position)
                    .chain(key.rotation)
                    .map(f32::to_bits)
                    .chain([key.unused_word])
                    .collect(),
            );
        }
        if let Some(marker) = track.target_marker {
            put(track.target_offset as usize + 8, vec![marker]);
        }
        for (index, key) in track.targets.iter().enumerate() {
            put(
                track.target_offset as usize + 12 + index * 20,
                [key.time]
                    .into_iter()
                    .chain(key.position)
                    .map(f32::to_bits)
                    .chain([key.unused_word])
                    .collect(),
            );
        }
        for storage in &track.unreferenced_storage {
            restored[storage.offset..storage.offset + storage.bytes.len()]
                .copy_from_slice(&storage.bytes);
        }
        assert_eq!(
            restored, bytes,
            "camera JSON must preserve the complete source"
        );
        Ok(track)
    }

    #[test]
    fn map_recognition_uses_fixed_slots_without_hiding_damaged_children() -> Result<()> {
        for count in [16, 24, 31, 40] {
            let mut bytes = vec![0; 4 + count * 4];
            bytes[..4].copy_from_slice(&(count as u32).to_be_bytes());
            let script = bytes.len();
            bytes[28..32].copy_from_slice(&(script as u32).to_be_bytes());
            bytes.extend([0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff, 0, 0]);
            let bank = bytes.len();
            bytes[32..36].copy_from_slice(&(bank as u32).to_be_bytes());
            bytes.extend([2_u32, 12, 16, 10 << 16, 0].map(u32::to_be_bytes).concat());
            let mut ranges = crate::field::sections(&bytes)?;
            assert!(is_map(&bytes, &ranges));
            assert!(!is_map(&bytes, &ranges[..REQUIRED_SECTIONS - 1]));
            assert_eq!(models(&bytes[bank..])?.len(), 1);

            // Recognition leaves opcode and model-pointer errors to strict readers.
            bytes[script + 8..script + 10].fill(255);
            bytes[bank + 8..bank + 12].fill(255);
            assert!(is_map(&bytes, &ranges));
            assert!(symphonia_script::Program::decode(&bytes[script..bank]).is_err());
            assert!(models(&bytes[bank..]).is_err());

            let declared_script = ranges[6].take();
            assert!(!is_map(&bytes, &ranges));
            ranges[6] = declared_script;
            bytes[script..script + 8].fill(0);
            assert!(!is_map(&bytes, &ranges));
            bytes[script..script + 8].copy_from_slice(&[0, 4, 0, 0, 0, 5, 0, 0]);
            bytes[bank..bank + 4].fill(0);
            assert!(!is_map(&bytes, &ranges));
        }
        Ok(())
    }

    #[test]
    fn collision_index_widths_preserve_the_same_mesh() {
        for compact in [false, true] {
            let mut bytes = Vec::new();
            let points = if compact { 20_u32 } else { 28 };
            if !compact {
                bytes.extend(0_u32.to_be_bytes());
            }
            bytes.extend(1_u32.to_be_bytes());
            bytes.extend(3_u16.to_be_bytes());
            bytes.extend(1_u16.to_be_bytes());
            bytes.extend(points.to_be_bytes());
            bytes.extend((points + 36).to_be_bytes());
            bytes.extend(11_u32.to_be_bytes());
            if !compact {
                bytes.extend(0_u32.to_be_bytes());
            }
            for value in [0_f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
                bytes.extend(value.to_be_bytes());
            }
            if compact {
                bytes.extend([0, 1, 2]);
            } else {
                for value in [0_u16, 1, 2] {
                    bytes.extend(value.to_be_bytes());
                }
            }
            let decoded = collision(&bytes).unwrap();
            assert_eq!(decoded[0].surface, 11);
            assert_eq!(decoded[0].vertices[2], [0., 1., 0.]);
            assert_eq!(decoded[0].triangles, [[0, 1, 2]]);
            if !compact {
                bytes[..4].copy_from_slice(&u32::MAX.to_be_bytes());
                assert_eq!(
                    crate::field::collision_data::Mesh::read(
                        &bytes,
                        crate::field::collision_data::Format::Short,
                    )
                    .unwrap()
                    .groups[0]
                        .triangles,
                    decoded[0].triangles
                );
                bytes[..4].fill(0);
            }
            *bytes.last_mut().unwrap() = 3;
            assert!(collision(&bytes).is_err());
        }
    }

    #[test]
    fn camera_keeps_rotation_and_independent_target_times() {
        let mut bytes = b"CAMM".to_vec();
        bytes.extend(76_u32.to_be_bytes());
        bytes.extend(2_u32.to_be_bytes());
        for time in [0_f32, 10.] {
            for value in [time, 1., 2., 3., 0., 0., 0., 1.] {
                bytes.extend(value.to_be_bytes());
            }
            bytes.extend(u32::MAX.to_be_bytes());
        }
        bytes.extend(b"TARG");
        for time in [0_f32, 8.] {
            for value in [time, 4., 5., 6.] {
                bytes.extend(value.to_be_bytes());
            }
            bytes.extend(0xeeee_eeee_u32.to_be_bytes());
        }
        let track = camera_roundtrip(&bytes).unwrap();
        assert_eq!(track.transforms[1].rotation, [0., 0., 0., 1.]);
        assert_eq!(track.transforms[1].time, 10.);
        assert_eq!(track.targets[1].time, 8.);
        assert_eq!(track.transforms[1].unused_word, u32::MAX);
        assert_eq!(track.targets[1].unused_word, 0xeeee_eeee);
        assert_eq!(track.target_marker, Some(u32::from_be_bytes(*b"TARG")));
        assert!(track.unreferenced_storage.is_empty());
        let mut aliased = bytes[..84].to_vec();
        aliased[4..8].copy_from_slice(&16_u32.to_be_bytes());
        let track = camera_roundtrip(&aliased).unwrap();
        assert_eq!(track.targets[0].position, [0., 0., 1.]);
        assert_eq!(track.targets[1].position, [1., 2., 3.]);
        assert_eq!(track.target_marker, Some(3_f32.to_bits()));
        assert!(track.unreferenced_storage.is_empty());
        bytes.splice(84..84, [0x55, 0, 0, 0]);
        bytes[4..8].copy_from_slice(&80_u32.to_be_bytes());
        bytes[88..92].copy_from_slice(&0x1234_5678_u32.to_be_bytes());
        bytes.extend([0, 0x23]);
        let track = camera_roundtrip(&bytes).unwrap();
        assert_eq!(track.target_marker, Some(0x1234_5678));
        assert_eq!(
            track.unreferenced_storage,
            [
                crate::read::Storage {
                    offset: 84,
                    bytes: vec![0x55, 0, 0, 0]
                },
                crate::read::Storage {
                    offset: 132,
                    bytes: vec![0, 0x23]
                },
            ]
        );
        bytes.extend([0; 6]);
        camera_roundtrip(&bytes).unwrap();
        assert!(camera(&bytes[..131]).is_err());
        bytes[4..8].fill(0);
        let track = camera_roundtrip(&bytes[..84]).unwrap();
        assert!(track.targets.is_empty());
        assert_eq!(track.target_marker, None);
        bytes.truncate(12);
        assert!(camera(&bytes).is_err());
    }

    #[test]
    #[ignore = "requires both original discs and their shared source index; camera records only"]
    fn original_camera_records_preserve_every_word_and_target_extent() -> Result<()> {
        use std::{collections::BTreeMap, fs, path::Path};

        fn camera_count(path: &Path) -> Result<usize> {
            if fs::metadata(path)?.is_file() {
                return Ok(usize::from(
                    path.file_name().is_some_and(|name| name == "camera.json"),
                ));
            }
            let mut count = 0;
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                count += if entry.file_type()?.is_dir() {
                    camera_count(&entry.path())?
                } else {
                    usize::from(entry.file_name() == "camera.json")
                };
            }
            Ok(count)
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let library = root.join("all-assets");
        let sources: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(library.join("sources.json"))?)?;
        let mut publications = BTreeMap::new();
        for path in sources
            .values()
            .flatten()
            .filter(|path| path.starts_with("assets/"))
        {
            if !publications.contains_key(path) {
                publications.insert(path.clone(), camera_count(&library.join(path))?);
            }
        }
        let mut checked = 0;
        for (source, paths) in sources {
            let expected: usize = paths.iter().filter_map(|path| publications.get(path)).sum();
            if expected == 0 {
                continue;
            }
            let (disc, file) = source.split_once('/').unwrap();
            let mut found = 0;
            {
                let mut bytes =
                    fs::read(root.join("extracted").join(disc).join("files").join(file))?;
                if bytes.starts_with(b"MSCF") {
                    bytes = crate::field::MapArchive::decode(&bytes)?.bytes;
                }
                let ranges = if let Some(entries) = super::super::archive::entries(&bytes) {
                    let directory = super::super::archive::Directory::new(&entries);
                    entries
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, range)| {
                            (directory.members[index] == Some(index))
                                .then_some(range)
                                .flatten()
                        })
                        .collect::<Vec<_>>()
                } else {
                    crate::field::sections(&bytes)?
                        .into_iter()
                        .flatten()
                        .collect()
                };
                for range in ranges {
                    let source = &bytes[range];
                    if !source.starts_with(b"CAMM") {
                        continue;
                    }
                    let track = camera_roundtrip(source)?;
                    assert_eq!(track.transforms.len(), word(source, 8)? as usize);
                    if track.target_offset != 0 {
                        assert_eq!(track.targets.len(), track.transforms.len());
                    }
                    found += 1;
                }
            }
            assert_eq!(
                found, expected,
                "camera publication count differs for {source}"
            );
            checked += found;
        }
        assert!(
            checked >= 138,
            "both discs must cover every published camera"
        );
        eprintln!("reconstructed {checked} camera resources from both discs");
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs"]
    fn original_field_metadata_decodes_every_map() -> Result<()> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut counts = [0; 4];
        let mut failures = Vec::new();
        for disc in ["disc1", "disc2"] {
            for directory in ["MAP", "FIELD"] {
                for entry in std::fs::read_dir(
                    root.join("local/extracted")
                        .join(disc)
                        .join("files")
                        .join(directory),
                )? {
                    let path = entry?.path();
                    if !path.extension().is_some_and(|extension| {
                        ["bin", "dat", "d"]
                            .iter()
                            .any(|expected| extension.eq_ignore_ascii_case(expected))
                    }) {
                        continue;
                    }
                    let mut data = std::fs::read(&path)?;
                    if data.starts_with(b"MSCF") {
                        data = crate::field::MapArchive::decode(&data)?.bytes;
                    }
                    let sections = crate::field::sections(&data)?;
                    if directory == "MAP" && sections[6].is_some() && sections[7].is_some() {
                        assert!(is_map(&data, &sections), "{}", path.display());
                    }
                    for (index, range) in sections.iter().enumerate() {
                        let Some(range) = range else {
                            continue;
                        };
                        let bytes = &data[range.clone()];
                        let result = if bytes.starts_with(b"CAMM") {
                            counts[0] += 1;
                            camera(bytes).map(|_| ())
                        } else if (directory == "MAP" && matches!(index, 4 | 5))
                            || (directory == "FIELD"
                                && sections.len() == 5
                                && index == 4
                                && path
                                    .extension()
                                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dat")))
                        {
                            counts[1] += 1;
                            collision(bytes).map(|_| ())
                        } else if directory == "MAP" && index == 7 {
                            counts[2] += 1;
                            models(bytes).and_then(|models| {
                                for model in models {
                                    let actor = &bytes[model.range];
                                    if crate::animation::is_animation(actor) {
                                        crate::animation::unbound(actor)?;
                                        continue;
                                    }
                                    if matches!(
                                        word(actor, 0x20).ok(),
                                        Some(0x005b_bc61 | 0x00b7_49e0)
                                    ) {
                                        continue;
                                    }
                                    let ranges = crate::field::sections(actor)?;
                                    assert!(
                                        !is_map(actor, &ranges),
                                        "actor package mistaken for map"
                                    );
                                    if ranges.len() != 31 {
                                        continue;
                                    }
                                    for index in [29, 30] {
                                        if let Some(range) = &ranges[index] {
                                            counts[3] += 1;
                                            let bytes = &actor[range.clone()];
                                            let result = if crate::animation::is_animation(bytes) {
                                                crate::animation::unbound_indexed(bytes).map(|_| ())
                                            } else {
                                                crate::field::collision_data::Mesh::read(
                                                    bytes,
                                                    crate::field::collision_data::Format::Short,
                                                )
                                                .map(|_| ())
                                            };
                                            result.map_err(|error| {
                                                anyhow::anyhow!(
                                                    "actor {}/{}: {error:#}",
                                                    model.archive_entry,
                                                    index
                                                )
                                            })?;
                                        }
                                    }
                                }
                                Ok(())
                            })
                        } else {
                            continue;
                        };
                        if let Err(error) = result {
                            failures.push(format!("{} section {index}: {error:#}", path.display()));
                        }
                    }
                }
            }
        }
        eprintln!("camera/collision/model-bank/actor-extra sections: {counts:?}");
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }
}
