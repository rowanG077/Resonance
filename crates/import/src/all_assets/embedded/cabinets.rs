//! Cabinets may be unaligned inside native data sections. Their own headers
//! delimit each archive; members use the same decoders as filesystem assets.
use crate::{all_assets::geometry, digest, write_atomic};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::Cursor,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
struct Source {
    offset: usize,
    bytes: usize,
    data: String,
}

pub(super) struct Scanner {
    output: PathBuf,
    converted: BTreeMap<String, bool>,
}

impl Scanner {
    pub(super) fn new(output: &Path) -> Self {
        Self {
            output: output.into(),
            converted: BTreeMap::new(),
        }
    }

    pub(super) fn scan(
        &mut self,
        bytes: &[u8],
        name: &str,
        paths: &mut Vec<String>,
        report: &mut impl FnMut(&str, Result<()>),
    ) {
        let mut sources = Vec::new();
        for (offset, result) in envelopes(bytes) {
            let label = format!("{name}/cabinet-{offset:x}");
            let result = (|| {
                let payload = result?;
                let data = format!("embedded/cabinets/{}", digest(payload));
                let complete = self.converted.entry(data.clone()).or_insert_with(|| {
                    let mut complete = true;
                    geometry::cook(
                        payload,
                        &data,
                        &self.output,
                        None,
                        geometry::Input::File,
                        &mut |child, result| {
                            complete &= result.is_ok();
                            report(child, result);
                        },
                    );
                    complete
                });
                ensure!(*complete, "embedded cabinet has failed members");
                paths.push(data.clone());
                sources.push(Source {
                    offset,
                    bytes: payload.len(),
                    data,
                });
                Ok(())
            })();
            report(&label, result);
        }
        if !sources.is_empty() {
            let path = format!("{name}/cabinets.json");
            let result = serde_json::to_vec(&sources)
                .map_err(Into::into)
                .and_then(|bytes| write_atomic(&self.output.join(&path), &bytes));
            if result.is_ok() {
                paths.push(path.clone());
            }
            report(&path, result);
        }
    }
}

fn envelopes(bytes: &[u8]) -> Vec<(usize, Result<&[u8]>)> {
    let mut entries = Vec::new();
    let mut end = 0;
    for (at, magic) in bytes.windows(4).enumerate() {
        if at < end || magic != b"MSCF" {
            continue;
        }
        let Some(header) = bytes.get(at..at + 36) else {
            continue;
        };
        // Distinguish cabinet headers from a literal signature in native data.
        if header[4..8] != [0; 4] || header[12..16] != [0; 4] || header[20..24] != [0; 4] {
            continue;
        }
        let result = (|| {
            let size = u32::from_le_bytes(header[8..12].try_into()?) as usize;
            ensure!(size >= header.len(), "invalid embedded cabinet length");
            let payload = bytes
                .get(at..at.checked_add(size).context("cabinet extent overflow")?)
                .context("embedded cabinet exceeds its data section")?;
            cab::Cabinet::new(Cursor::new(payload))?;
            end = at + size;
            Ok(payload)
        })();
        entries.push((at, result));
    }
    entries
}

#[test]
fn cabinet_extents_do_not_depend_on_alignment() -> Result<()> {
    let mut builder = cab::CabinetBuilder::new();
    builder
        .add_folder(cab::CompressionType::None)
        .add_file("empty");
    let mut writer = builder.build(Cursor::new(Vec::new()))?;
    while writer.next_file()?.is_some() {}
    let cabinet = writer.finish()?.into_inner();
    let mut section = vec![0; 3];
    section.extend(&cabinet);
    section.push(0);
    section.extend(&cabinet);
    let entries = envelopes(&section);
    assert_eq!(entries.len(), 2);
    for ((at, payload), expected) in entries.into_iter().zip([3, 4 + cabinet.len()]) {
        assert_eq!(at, expected);
        assert_eq!(payload?, cabinet);
    }
    section.pop();
    assert!(envelopes(&section)[1].1.is_err());
    section[11..15].copy_from_slice(&35u32.to_le_bytes());
    assert!(envelopes(&section)[0].1.is_err());
    assert!(envelopes(b"this is a literal MSCF signature in text").is_empty());
    Ok(())
}

#[test]
#[ignore = "requires both extracted original discs; no media conversion"]
fn original_cabinets_cover_all_field_modules() -> Result<()> {
    use std::collections::BTreeSet;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let expected = [
        (
            "fsky.GTA",
            0xcc11,
            "db6598c1d6364cfe854dcf87c4fe44670324da6503bc30486f9a2d0085845cac",
        ),
        (
            "deli.GTA",
            0x1d82d,
            "dab812a3887c1174905920d14148aafc876e4573b47e78c09fb2fec65ae9f341",
        ),
    ];
    let mut distinct = BTreeSet::new();
    for disc in [1, 2] {
        for module in [
            "Top2field.rel",
            "Top2fieldD.rel",
            "US_Top2field.rel",
            "US_m_Top2field.rel",
            "US_r_Top2field.rel",
            "m_Top2field.rel",
            "r_Top2field.rel",
        ] {
            let file = root.join(format!("disc{disc}/files/{module}"));
            let rel = crate::rel::Rel::read(&file)?;
            let data = rel.at((6, 0))?;
            let entries = envelopes(data);
            assert_eq!(entries.len(), expected.len());
            let offsets = if module == "Top2fieldD.rel" {
                [0x512, 0xd123]
            } else if module.starts_with("US_") {
                [0x128, 0xcd3c]
            } else {
                [0xbc, 0xccd0]
            };
            for (((at, result), &(name, size, hash)), offset) in
                entries.into_iter().zip(&expected).zip(offsets)
            {
                let payload = result?;
                assert_eq!(at, offset);
                assert_eq!(payload.len(), size);
                assert_eq!(digest(payload), hash);
                distinct.insert(hash);
                let cabinet = cab::Cabinet::new(Cursor::new(payload))?;
                let names: Vec<_> = cabinet
                    .folder_entries()
                    .flat_map(|folder| folder.file_entries())
                    .map(|entry| entry.name())
                    .collect();
                assert_eq!(names, [name]);
            }
        }
    }
    assert_eq!(distinct.len(), 2);
    Ok(())
}
