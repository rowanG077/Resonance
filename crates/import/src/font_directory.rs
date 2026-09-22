//! Native font bindings, character maps, proportional widths and RGB5A3 palette.
use crate::{
    dol, embedded,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

const STARTUP: u32 = 0x8017cc00;
const PALETTE: u32 = 0x801f88a0;
const BANKS: u32 = 0x801f88c0;
const ALTERNATE_CODES: u32 = 0x801f8900;
const ASCII_CODES: u32 = 0x801f8984;
const ADVANCES: u32 = 0x801f9680;
const COUNT: usize = 16;
const FAMILY: &str = "font-directory";
pub(crate) const ROW_BYTES: usize = 96;
pub(crate) const GLYPH_HEIGHT: usize = 24;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Directory {
    pub startup: String,
    pub banks: [Option<String>; COUNT],
    pub palette: [u16; COUNT],
    pub metrics: Metrics,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Metrics {
    /// All 96 slots, starting at byte 0x20 and including DEL.
    pub ascii_codes: Vec<u16>,
    /// Complete alternate single-byte mapping, including its final two entries.
    pub alternate_codes: Vec<u16>,
    /// Zero means the default width. The final five bytes lie outside the lookup range.
    pub advances: Vec<u8>,
}

impl Metrics {
    pub(crate) fn read(executable: &[u8]) -> Result<Self> {
        let codes = |address, count: usize| {
            dol::slice(executable, address, count * 2)?
                .chunks_exact(2)
                .map(|row| half(row, 0))
                .collect::<Result<Vec<_>>>()
        };
        Ok(Self {
            ascii_codes: codes(ASCII_CODES, 96)?,
            alternate_codes: codes(ALTERNATE_CODES, 66)?,
            advances: dol::slice(executable, ADVANCES, 288)?.to_vec(),
        })
    }

    pub(crate) fn code(&self, character: char) -> Result<u16> {
        if character == '^' {
            return Ok(0x81a7);
        }
        let character = character.to_string();
        let (bytes, _, invalid) = encoding_rs::SHIFT_JIS.encode(&character);
        ensure!(!invalid, "invalid font character {character:?}");
        match *bytes.as_ref() {
            [byte] => {
                let table = if byte & 0x80 != 0 {
                    &self.alternate_codes
                } else {
                    &self.ascii_codes
                };
                table
                    .get(usize::from((byte & 0x7f).max(32) - 32))
                    .copied()
                    .context("truncated font character table")
            }
            [high, low] => Ok(u16::from_be_bytes([high, low])),
            _ => anyhow::bail!("invalid font character {character:?}"),
        }
    }

    /// Width of an already-mapped glyph code, in the original 24-pixel font.
    pub(crate) fn advance(&self, code: u16) -> u32 {
        if !(0x8140..=0x829a).contains(&code) {
            return 24;
        }
        let index = usize::from((code >> 8) - 0x81) * 192 + usize::from(code & 255) - 64;
        match self.advances[index] {
            0 => 24,
            value => u32::from(value),
        }
    }
}

pub(crate) fn palette(executable: &[u8]) -> Result<[u16; COUNT]> {
    let bytes = dol::slice(executable, PALETTE, COUNT * 2)?;
    Ok(std::array::from_fn(|i| {
        u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]])
    }))
}

pub(crate) fn validate_size(size: u64) -> Result<()> {
    ensure!(
        size > 0 && size.is_multiple_of((ROW_BYTES * GLYPH_HEIGHT) as u64),
        "invalid physical font atlas length"
    );
    Ok(())
}

impl Directory {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let banks = dol::slice(executable, BANKS, COUNT * 4)?
            .chunks_exact(4)
            .map(|row| dol::optional_text(executable, word(row, 0)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            startup: dol::text(executable, STARTUP)?,
            banks: banks.try_into().expect("fixed-size font directory"),
            palette: palette(executable)?,
            metrics: Metrics::read(executable)?,
        })
    }

    pub fn paths(&self, files: &Path) -> Result<BTreeSet<String>> {
        std::iter::once(&self.startup)
            .chain(self.banks.iter().flatten())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|name| {
                let path = crate::field_resources::resolve_path(files, name)?;
                let source = files.join(&path);
                let metadata = fs::metadata(&source)?;
                ensure!(
                    metadata.is_file(),
                    "font source is not a regular file: {name}"
                );
                fs::File::open(source)?;
                validate_size(metadata.len())?;
                Ok(path)
            })
            .collect()
    }

    pub fn cook(extracted: &Path, output: &Path) -> Result<Vec<String>> {
        let file = extracted.join("sys/main.dol");
        embedded::write(&file, output, FAMILY, &Self::read(&fs::read(&file)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 0x420];
        bytes[0x160..0x176].copy_from_slice(b"Start.font\0OTHER.font\0");
        for (slot, offset, address, size) in [
            (0, 0x100_u32, PALETTE, 96_u32),
            (1, 0x160, STARTUP, 22),
            (2, 0x1a0, ALTERNATE_CODES, 324),
            (3, 0x300, ADVANCES, 288),
        ] {
            bytes[slot * 4..slot * 4 + 4].copy_from_slice(&offset.to_be_bytes());
            bytes[0x48 + slot * 4..0x4c + slot * 4].copy_from_slice(&address.to_be_bytes());
            bytes[0x90 + slot * 4..0x94 + slot * 4].copy_from_slice(&size.to_be_bytes());
        }
        for (i, row) in bytes[0x100..0x120].chunks_exact_mut(2).enumerate() {
            row.copy_from_slice(&(0x8100 + i as u16).to_be_bytes());
        }
        for index in [0, 3, 15] {
            bytes[0x120 + index * 4..0x124 + index * 4]
                .copy_from_slice(&(STARTUP + 11).to_be_bytes());
        }
        for (offset, count, base) in [(0x1a0, 66, 0x8240u16), (0x224, 96, 0x8140)] {
            for (index, row) in bytes[offset..offset + count * 2]
                .chunks_exact_mut(2)
                .enumerate()
            {
                row.copy_from_slice(&(base + index as u16).to_be_bytes());
            }
        }
        for (index, byte) in bytes[0x300..].iter_mut().enumerate() {
            *byte = if index % 5 == 0 {
                0
            } else {
                (index % 24 + 1) as u8
            };
        }
        bytes
    }

    #[test]
    fn declarations_preserve_aliases_palette_and_validate_sources() -> Result<()> {
        let mut executable = fixture();
        let directory = Directory::read(&executable)?;
        assert_eq!(directory.startup, "Start.font");
        assert_eq!(directory.banks[0], directory.banks[15]);
        assert_eq!(directory.banks[1], None);
        assert_eq!(
            directory.palette,
            std::array::from_fn(|i| 0x8100 + i as u16)
        );
        let metrics = &directory.metrics;
        assert_eq!(metrics.code('\0')?, 0x8140);
        assert_eq!(metrics.code('A')?, 0x8161);
        assert_eq!(metrics.code('^')?, 0x81a7);
        assert_eq!(metrics.ascii_codes[usize::from(b'^' - 32)], 0x817e);
        assert_eq!(metrics.code('\x7f')?, 0x819f);
        assert_eq!(metrics.code('あ')?, 0x82a0);
        for (character, code) in [('｡', 0x8241), ('･', 0x8245), ('ｵ', 0x8255), ('ﾟ', 0x827f)]
        {
            assert_eq!(metrics.code(character)?, code);
        }
        assert!(metrics.code('🦀').is_err());
        assert_eq!(metrics.alternate_codes[64..], [0x8280, 0x8281]);
        assert_eq!(metrics.advances[283..], executable[0x41b..]);
        assert_eq!(metrics.advances[0], 0);
        assert_eq!(metrics.advance(0x8140), 24);
        assert_eq!(metrics.advance(0x8141), 2);
        assert_eq!(metrics.advance(0x8200), 9);
        assert_eq!(metrics.advance(0x829a), 19);
        assert_eq!(metrics.advance(0x829b), 24);
        let files = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        fs::create_dir(&files)?;
        let result = (|| -> Result<()> {
            assert!(directory.paths(&files).is_err());
            fs::write(files.join("start.font"), vec![0; ROW_BYTES * GLYPH_HEIGHT])?;
            fs::create_dir(files.join("OTHER.font"))?;
            assert!(directory.paths(&files).is_err());
            fs::remove_dir(files.join("OTHER.font"))?;
            fs::write(files.join("OTHER.font"), [0; 1])?;
            assert!(directory.paths(&files).is_err());
            fs::write(
                files.join("OTHER.font"),
                vec![255; ROW_BYTES * GLYPH_HEIGHT],
            )?;
            assert_eq!(
                directory.paths(&files)?,
                BTreeSet::from(["start.font".into(), "OTHER.font".into()])
            );
            if !files.join("other.font").exists() {
                fs::write(files.join("other.font"), vec![0; ROW_BYTES * GLYPH_HEIGHT])?;
                assert!(directory.paths(&files).is_err());
            }
            Ok(())
        })();
        fs::remove_dir_all(&files)?;
        result?;
        assert!(Directory::read(&executable[..0x15f]).is_err());
        executable[0x120..0x124].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Directory::read(&executable).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; publishes JSON without media conversion"]
    fn original_font_directories_preserve_native_bindings_and_all_physical_banks() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            for disc in 1..=2 {
                let extracted = root.join(format!("disc{disc}"));
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let directory = Directory::read(&executable)?;
                assert_eq!(directory.startup, dol::text(&executable, STARTUP)?);
                for (row, path) in dol::slice(&executable, BANKS, COUNT * 4)?
                    .chunks_exact(4)
                    .zip(&directory.banks)
                {
                    assert_eq!(
                        path.as_deref(),
                        Some(dol::text(&executable, word(row, 0)?)?.as_str())
                    );
                }
                assert_eq!(
                    directory
                        .palette
                        .into_iter()
                        .flat_map(u16::to_be_bytes)
                        .collect::<Vec<_>>(),
                    dol::slice(&executable, PALETTE, COUNT * 2)?
                );
                for (address, count, codes) in [
                    (ASCII_CODES, 96, &directory.metrics.ascii_codes),
                    (ALTERNATE_CODES, 66, &directory.metrics.alternate_codes),
                ] {
                    assert_eq!(codes.len(), count);
                    assert_eq!(
                        codes
                            .iter()
                            .copied()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>(),
                        dol::slice(&executable, address, count * 2)?
                    );
                }
                assert_eq!(directory.metrics.advances.len(), 288);
                assert_eq!(
                    directory.metrics.advances,
                    dol::slice(&executable, ADVANCES, 288)?
                );
                let files = extracted.join("files");
                let declared = directory.paths(&files)?;
                assert_eq!(declared.len(), 8);
                let mut banks = BTreeSet::new();
                let mut glyphs = 0;
                for entry in fs::read_dir(&files)? {
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.ends_with("fontb0.dat") || name.ends_with("fontb1.dat") {
                        let size = entry.metadata()?.len();
                        validate_size(size)?;
                        glyphs += size / 144;
                        banks.insert(name);
                    }
                }
                assert!(declared.is_subset(&banks));
                assert_eq!((banks.len(), glyphs), (12, 17_280));
                assert_eq!(
                    banks
                        .difference(&declared)
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    [
                        "dep_fontb0.dat",
                        "dep_fontb1.dat",
                        "gage_fontb0.dat",
                        "yug_fontb0.dat"
                    ]
                );
                let paths = Directory::cook(&extracted, &output)?;
                assert_eq!(
                    embedded::read::<Directory>(&output, FAMILY, "main.dol")?,
                    directory
                );
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
            }
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
