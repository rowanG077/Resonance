//! Optional CPU skinning recipes: ordered rigid, blended and accumulated writes.
//! Layout reference: https://github.com/Jaws-git/Sluggies-dat-tools/blob/main/_docs/_docs_model_format/skn_section.html
use crate::read::{u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::ops::Range;

const HEADER_SIZE: usize = 0x24;
const VERTEX_SIZE: usize = 12;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Binding {
    Attached,
    Unbound,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    Rigid,
    Blend,
    Accumulate,
}

#[derive(Serialize)]
pub(super) struct Skin {
    format_version: u32,
    binding: Binding,
    position_fraction_bits: u8,
    /// Relative to geometry object's zero position buffer, including scratch slots.
    clear_position_bytes: Range<u32>,
    groups: Vec<Group>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    retained_bind_vertices: Vec<Vertex>,
}

#[derive(Serialize)]
struct Group {
    operation: Operation,
    joints: Vec<u16>,
    vertices: Vec<Vertex>,
}

#[derive(Serialize)]
struct Vertex {
    destination: u32,
    position: [f32; 3],
    normal: [f32; 3],
    /// Exact authored bytes; rigid groups have implicit unit influence.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    weights: Vec<u8>,
}

/// The wrapper owns an exact optional extent. Some unused exports clear its
/// pointer while retaining the contiguous SKN payload and its declared size.
pub(super) fn model(bytes: &[u8], skeleton_end: usize) -> Result<Option<(Skin, Range<usize>)>> {
    let pointer = word(bytes, 0x10)? as usize;
    let size = word(bytes, 0x14)? as usize;
    // Game model headers reuse both words for texture selectors. The wrapper
    // describes a skin only when physical payload bytes follow the skeleton.
    if super::super::archive::padding(bytes, skeleton_end, &[]).is_ok() {
        return Ok(None);
    }
    if size == 0 {
        ensure!(pointer == 0, "skin pointer has no declared extent");
        return Ok(None);
    }
    let start = if pointer == 0 { skeleton_end } else { pointer };
    let end = start.checked_add(size).context("skin extent overflow")?;
    ensure!(start >= skeleton_end, "skin overlaps model resources");
    let data = bytes
        .get(start..end)
        .context("skin exceeds model resource")?;
    let binding = if pointer == 0 {
        Binding::Unbound
    } else {
        Binding::Attached
    };
    Ok(Some((decode(data, binding)?, start..end)))
}

fn decode(bytes: &[u8], binding: Binding) -> Result<Skin> {
    let header = bytes.get(..HEADER_SIZE).context("truncated skin header")?;
    let fraction_bits = header[6];
    ensure!(
        fraction_bits < 16,
        "unsupported skin quantization {fraction_bits:#x}"
    );
    ensure!(header[7] == 0, "nonzero skin header padding");
    ensure!(
        word(bytes, 0x1c)? == 0 && word(bytes, 0x20)? == 0,
        "unsupported skin cache-flush index table"
    );
    let clear_start = word(bytes, 0x14)?;
    let clear_end = clear_start
        .checked_add(word(bytes, 0x18)?)
        .context("skin clear range overflow")?;
    let mut reader = Reader {
        bytes,
        ranges: Vec::new(),
        scale: 2_f32.powi(-(fraction_bits as i32)),
    };
    let mut groups = Vec::new();
    let mut tables_end = HEADER_SIZE;
    for (kind, (operation, stride, matrices)) in [
        (Operation::Rigid, 0x40, 1),
        (Operation::Blend, 0x74, 2),
        (Operation::Accumulate, 0x44, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let count = half(bytes, kind * 2)? as usize;
        let table = word(bytes, 8 + kind * 4)? as usize;
        if count == 0 {
            ensure!(table == 0, "empty skin group has a table");
            continue;
        }
        reader.take(table, count * stride)?;
        tables_end = tables_end.max(table + count * stride);
        for index in 0..count {
            let record = &bytes[table + index * stride..table + (index + 1) * stride];
            // Matrices are runtime scratch; never discard unexpected authored data.
            ensure!(
                record[..matrices * 0x30].iter().all(|&b| b == 0),
                "skin contains an initialized runtime matrix"
            );
            let (source, destination, vertex_count, prefix, joints, weights, indices) =
                match operation {
                    Operation::Rigid => {
                        ensure!(
                            record[0x3d..].iter().all(|&b| b == 0),
                            "nonzero rigid skin padding"
                        );
                        (
                            word(record, 0x30)?,
                            word(record, 0x34)?,
                            half(record, 0x3a)?,
                            record[0x3c],
                            vec![half(record, 0x38)?],
                            None,
                            None,
                        )
                    }
                    Operation::Blend => {
                        ensure!(record[0x73] == 0, "nonzero blended skin padding");
                        (
                            word(record, 0x60)?,
                            word(record, 0x68)?,
                            half(record, 0x70)?,
                            record[0x72],
                            vec![half(record, 0x6c)?, half(record, 0x6e)?],
                            Some(word(record, 0x64)?),
                            None,
                        )
                    }
                    Operation::Accumulate => (
                        word(record, 0x30)?,
                        word(record, 0x38)?,
                        half(record, 0x42)?,
                        0,
                        vec![half(record, 0x40)?],
                        Some(word(record, 0x3c)?),
                        Some(word(record, 0x34)?),
                    ),
                };
            let count = vertex_count as usize;
            let source = reader.take(source as usize, prefix as usize + count * VERTEX_SIZE)?;
            ensure!(
                source[..prefix as usize].iter().all(|&b| b == 0),
                "nonzero skin array prefix"
            );
            let weights = weights
                .map(|at| reader.take(at as usize, count * joints.len()))
                .transpose()?;
            let indices = indices
                .map(|at| reader.take(at as usize, count * 2))
                .transpose()?;
            let first = destination
                .checked_add(prefix as u32)
                .context("skin destination overflow")?;
            ensure!(
                first.is_multiple_of(VERTEX_SIZE as u32),
                "unaligned skin destination"
            );
            let mut vertices = Vec::with_capacity(count);
            for index in 0..count {
                let at = prefix as usize + index * VERTEX_SIZE;
                let target = match indices {
                    Some(data) => half(data, index * 2)? as u32,
                    None => index as u32,
                };
                vertices.push(Vertex {
                    destination: (first / VERTEX_SIZE as u32)
                        .checked_add(target)
                        .context("skin vertex index overflow")?,
                    position: vector(source, at, reader.scale)?,
                    normal: vector(source, at + 6, reader.scale)?,
                    weights: weights.map_or_else(Vec::new, |data| {
                        data[index * joints.len()..(index + 1) * joints.len()].to_vec()
                    }),
                });
            }
            groups.push(Group {
                operation,
                joints,
                vertices,
            });
        }
    }
    let retained_bind_vertices =
        reader.retained_bind_vertices(tables_end, &binding, clear_start..clear_end, &groups)?;
    super::super::archive::padding(bytes, HEADER_SIZE, &reader.ranges)
        .context("uncooked skin bytes")?;
    Ok(Skin {
        format_version: 1,
        binding,
        position_fraction_bits: fraction_bits,
        clear_position_bytes: clear_start..clear_end,
        groups,
        retained_bind_vertices,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    ranges: Vec<Option<Range<usize>>>,
    scale: f32,
}

impl<'a> Reader<'a> {
    fn retained_bind_vertices(
        &mut self,
        tables_end: usize,
        binding: &Binding,
        clear: Range<u32>,
        groups: &[Group],
    ) -> Result<Vec<Vertex>> {
        let start = tables_end.next_multiple_of(32);
        let first_array = self
            .ranges
            .iter()
            .flatten()
            .map(|r| r.start)
            .filter(|&at| at >= tables_end)
            .min()
            .unwrap_or(self.bytes.len());
        if start >= first_array || self.bytes[start..first_array].iter().all(|&b| b == 0) {
            return Ok(Vec::new());
        }
        // Unbound exports can retain a complete copy of the bind-pose buffer
        // before the indexed sources. Its slots must match every source write;
        // bytes which merely resemble coordinates are not sufficient evidence.
        ensure!(
            matches!(binding, Binding::Unbound)
                && clear.start == 0
                && groups.iter().all(|g| g.operation == Operation::Accumulate),
            "unattributed bytes before skin sources"
        );
        let mut slots = std::collections::BTreeMap::new();
        for vertex in groups.iter().flat_map(|group| &group.vertices) {
            if let Some(previous) = slots.insert(vertex.destination, vertex) {
                ensure!(
                    previous.position == vertex.position && previous.normal == vertex.normal,
                    "skin sources disagree on retained bind vertex"
                );
            }
        }
        let size = slots.len() * VERTEX_SIZE;
        ensure!(
            size.next_multiple_of(32) == clear.end as usize
                && start + clear.end as usize == first_array
                && slots.keys().copied().eq(0..slots.len() as u32),
            "retained skin buffer extent does not match destination slots"
        );
        let source = self.take(start, size)?;
        slots
            .into_iter()
            .map(|(destination, vertex)| {
                let at = destination as usize * VERTEX_SIZE;
                let position = vector(source, at, self.scale)?;
                let normal = vector(source, at + 6, self.scale)?;
                ensure!(
                    position == vertex.position && normal == vertex.normal,
                    "retained skin buffer does not match its indexed sources"
                );
                Ok(Vertex {
                    destination,
                    position,
                    normal,
                    weights: Vec::new(),
                })
            })
            .collect()
    }

    fn take(&mut self, start: usize, size: usize) -> Result<&'a [u8]> {
        let end = start.checked_add(size).context("skin array overflow")?;
        ensure!(start >= HEADER_SIZE, "skin array overlaps header");
        let data = self
            .bytes
            .get(start..end)
            .context("skin array exceeds resource")?;
        if size != 0 {
            self.ranges.push(Some(start..end));
        }
        Ok(data)
    }
}

fn vector(bytes: &[u8], at: usize, scale: f32) -> Result<[f32; 3]> {
    Ok(
        [half(bytes, at)?, half(bytes, at + 2)?, half(bytes, at + 4)?]
            .map(|value| value as i16 as f32 * scale),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_word(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn put_half(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn recovers_all_skin_operations_without_merging_repeated_destinations() -> Result<()> {
        let mut bytes = vec![0; 512];
        for (kind, table) in [36, 100, 216].into_iter().enumerate() {
            put_half(&mut bytes, kind * 2, 1);
            put_word(&mut bytes, 8 + kind * 4, table);
        }
        bytes[6] = 8;
        for (at, value) in [
            (84, 320),
            (196, 352),
            (200, 384),
            (204, 12),
            (264, 416),
            (268, 448),
            (276, 480),
        ] {
            put_word(&mut bytes, at, value);
        }
        for (at, value) in [
            (92, 2),
            (94, 1),
            (208, 2),
            (210, 3),
            (212, 1),
            (280, 4),
            (282, 2),
            (448, 1),
            (450, 1),
            (320, 256),
            (326, 256),
        ] {
            put_half(&mut bytes, at, value);
        }
        bytes[384..386].copy_from_slice(&[127, 128]);
        bytes[480..482].copy_from_slice(&[255, 1]);
        let skin = decode(&bytes, Binding::Attached)?;
        assert_eq!(skin.groups.len(), 3);
        assert_eq!(skin.groups[0].vertices[0].position, [1., 0., 0.]);
        assert_eq!(skin.groups[0].vertices[0].normal, [1., 0., 0.]);
        assert!(skin.groups[0].vertices[0].weights.is_empty());
        assert_eq!(skin.groups[1].joints, [2, 3]);
        assert_eq!(skin.groups[1].vertices[0].weights, [127, 128]);
        assert_eq!(
            skin.groups[2]
                .vertices
                .iter()
                .map(|v| (v.destination, v.weights[0]))
                .collect::<Vec<_>>(),
            [(1, 255), (1, 1)]
        );
        bytes[500] = 1;
        assert!(decode(&bytes, Binding::Attached).is_err());
        bytes[500] = 0;
        assert!(decode(&bytes[..440], Binding::Attached).is_err());
        Ok(())
    }

    #[test]
    fn retained_bind_buffer_requires_exact_indexed_source_identity() -> Result<()> {
        let mut bytes = vec![0; 256];
        bytes[6] = 8;
        put_half(&mut bytes, 4, 1);
        for (at, value) in [(16, 36), (24, 32), (84, 160), (88, 192), (96, 224)] {
            put_word(&mut bytes, at, value);
        }
        for (at, value) in [(100, 7), (102, 2), (128, 256), (160, 256), (194, 1)] {
            put_half(&mut bytes, at, value);
        }
        bytes[224..226].copy_from_slice(&[127, 255]);
        let skin = decode(&bytes, Binding::Unbound)?;
        assert_eq!(skin.retained_bind_vertices.len(), 2);
        assert_eq!(skin.retained_bind_vertices[0].position, [1., 0., 0.]);
        assert!(decode(&bytes, Binding::Attached).is_err());
        bytes[129] = 1;
        assert!(decode(&bytes, Binding::Unbound).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires extracted original disc assets"]
    fn original_field_overlay_headers_are_material_selectors_not_skin_extents() -> Result<()> {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/FIELD");
        let check = |overlay: &[u8]| -> Result<[Option<u8>; 14]> {
            ensure!(
                matches!(word(overlay, 0x20)?, 0x005b_bc61 | 0x00b7_49e0),
                "not overlay geometry"
            );
            let end = crate::geometry::skeleton_range(overlay)?.end;
            assert!(model(overlay, end)?.is_none());
            let texture_count = word(overlay, word(overlay, 0)? as usize + 4)?;
            let selectors = super::super::texture_selectors(overlay, texture_count)?;
            assert!(selectors[3].is_none());
            super::super::super::archive::padding(overlay, end, &[])?;
            Ok(selectors)
        };
        let mut count = 0;
        for file in std::fs::read_dir(&source)? {
            let path = file?.path();
            if path.extension().is_none_or(|extension| extension != "dat") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            if word(&bytes, 0)? != 5 {
                continue;
            }
            let sections =
                crate::field::sections(&bytes).with_context(|| path.display().to_string())?;
            let Some(range) = &sections[2] else { continue };
            check(&bytes[range.clone()]).with_context(|| path.display().to_string())?;
            count += 1;
        }
        assert!(count >= 200, "only inspected {count} field overlays");
        for (file, members) in [("e03.d", [1, 2]), ("e04.d", [3, 4]), ("e05.d", [1, 3])] {
            let bytes = std::fs::read(source.join(file))?;
            let sections = crate::field::sections(&bytes).with_context(|| file.to_owned())?;
            for (variant, member) in members.into_iter().enumerate() {
                let actor = &bytes[sections[member].clone().context("missing field actor")?];
                let primary = crate::field::sections(actor)
                    .with_context(|| format!("{file}/{member}"))?[0]
                    .clone()
                    .context("missing field model")?;
                let model = &actor[primary];
                assert_ne!(word(model, 0x10)?, 0);
                assert_eq!(word(model, 0x14)?, 0);
                let selectors = check(model).with_context(|| format!("{file}/{member}/0"))?;
                assert_eq!(
                    (selectors[2], selectors[5]),
                    if variant == 0 {
                        (Some(1), Some(3))
                    } else {
                        (Some(0), Some(1))
                    }
                );
            }
        }
        Ok(())
    }

    #[test]
    fn selector_header_reuses_absent_skin_storage() -> Result<()> {
        let mut bytes = [0; 0x20];
        bytes[0x14] = 1;
        bytes[0x17] = 2;
        assert!(model(&bytes, bytes.len())?.is_none());
        let mut expected = [None; 14];
        expected[9] = Some(0);
        expected[12] = Some(1);
        assert_eq!(super::super::texture_selectors(&bytes, 2)?, expected);
        bytes[0x0c..0x19].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
        assert!(model(&bytes, bytes.len())?.is_none());
        assert_eq!(
            super::super::texture_selectors(&bytes, 13)?,
            [
                Some(0),
                Some(1),
                Some(2),
                None,
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                Some(8),
                Some(9),
                Some(10),
                Some(11),
                Some(12)
            ]
        );
        assert!(super::super::texture_selectors(&bytes, 12).is_err());
        let mut with_tail = bytes.to_vec();
        with_tail.extend_from_slice(&[1; 32]);
        assert!(model(&with_tail, bytes.len()).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires extracted original disc assets"]
    fn original_map_skins_recover_attached_and_unbound_groups() -> Result<()> {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/MAP");
        for (name, section, nested, counts, attached) in [
            ("ela_d03", 17, true, [2, 1, 0], true),
            ("tre_d05", 18, true, [0, 0, 25], false),
            ("tri_t01", 0, false, [9, 0, 6], false),
        ] {
            let map = crate::field::MapArchive::decode(&std::fs::read(
                source.join(format!("{name}.bin")),
            )?)?;
            let part = map.section(section)?;
            let part = if nested {
                &part[crate::field::sections(part)?[0]
                    .clone()
                    .context("missing main model")?]
            } else {
                part
            };
            let end = crate::geometry::skeleton_range(part)?.end;
            let (skin, range) = model(part, end)
                .with_context(|| name.to_string())?
                .context("missing SKN")?;
            assert_eq!(
                matches!(skin.binding, Binding::Attached),
                attached,
                "{name}"
            );
            assert_eq!(
                skin.retained_bind_vertices.len(),
                if name == "tre_d05" { 25 } else { 0 }
            );
            for (operation, expected) in [Operation::Rigid, Operation::Blend, Operation::Accumulate]
                .into_iter()
                .zip(counts)
            {
                assert_eq!(
                    skin.groups
                        .iter()
                        .filter(|g| g.operation == operation)
                        .count(),
                    expected,
                    "{name}"
                );
            }
            super::super::super::archive::padding(part, end, &[Some(range)])?;
        }
        Ok(())
    }
}
