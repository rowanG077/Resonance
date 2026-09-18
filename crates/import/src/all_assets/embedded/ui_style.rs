//! Shared text colors, number/bar gradients, cursor motion and UI symbols.
use super::text::{FixedText, TextPool, TextRef, TextSource};
use crate::{
    dol,
    read::{Field, FloatOperand},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "ui-style";
const PALETTE: u32 = 0x8026bdd0;
const NUMBER_COLORS: u32 = 0x801abf74;
const BAR_COLORS: u32 = 0x801abf44;
const SELECTION_ROWS: u32 = 0x801ac004;
const CURSOR: u32 = 0x8035d8a8;
const SYMBOLS: u32 = 0x8035d894;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub palette: [[u8; 4]; 11],
    pub number_colors: [[[u8; 4]; 2]; 18],
    pub bar_colors: [[[u8; 4]; 4]; 3],
    pub cursor: Cursor,
    pub symbols: Symbols,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cursor {
    /// Radians per phase unit before division. The first style doubles its phase.
    pub phase_scale: FloatOperand,
    pub phase_divisor: FloatOperand,
    /// First style, then the shared amplitude for the other two styles.
    pub amplitudes: [FloatOperand; 2],
    pub row_offsets: [i8; 9],
    /// Three bytes after the nine rendered selection rows.
    pub storage: [u8; 3],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Symbols {
    /// Letter B drawn by the marker renderer.
    pub marker_b: FixedText,
    pub equipped: FixedText,
    pub positive_modifier: FixedText,
    pub negative_modifier: FixedText,
    pub category_format: FixedText,
}

fn parse(executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let motion = dol::slice(executable, CURSOR, 16)?;
    let rows = dol::slice(executable, SELECTION_ROWS, 12)?;
    let mut texts = TextPool::default();
    let symbols = Symbols {
        marker_b: texts.fixed(executable, SYMBOLS, 4)?,
        equipped: texts.fixed(executable, SYMBOLS + 4, 4)?,
        positive_modifier: texts.fixed(executable, SYMBOLS + 8, 4)?,
        negative_modifier: texts.fixed(executable, SYMBOLS + 12, 4)?,
        category_format: texts.fixed(executable, SYMBOLS + 16, 4)?,
    };
    Ok((
        Catalogue {
            texts: texts.values,
            palette: Field::read(dol::slice(executable, PALETTE, 44)?, 0)?,
            number_colors: Field::read(dol::slice(executable, NUMBER_COLORS, 144)?, 0)?,
            bar_colors: Field::read(dol::slice(executable, BAR_COLORS, 48)?, 0)?,
            cursor: Cursor {
                phase_scale: FloatOperand::read(motion, 0)?,
                phase_divisor: FloatOperand::read(motion, 4)?,
                amplitudes: Field::read(motion, 8)?,
                row_offsets: Field::read(rows, 0)?,
                storage: rows[9..].try_into()?,
            },
            symbols,
        },
        texts.sources,
    ))
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(parse(executable)?.0)
}

#[cfg(test)]
pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, _) = parse(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    fn reconstruct(c: &Catalogue, sources: &[TextSource]) -> Vec<(u32, Vec<u8>)> {
        let cursor = &c.cursor;
        let mut spans = vec![
            (PALETTE, c.palette.iter().flatten().copied().collect()),
            (
                NUMBER_COLORS,
                c.number_colors
                    .iter()
                    .flatten()
                    .flatten()
                    .copied()
                    .collect(),
            ),
            (
                BAR_COLORS,
                c.bar_colors.iter().flatten().flatten().copied().collect(),
            ),
            (
                CURSOR,
                [
                    cursor.phase_scale,
                    cursor.phase_divisor,
                    cursor.amplitudes[0],
                    cursor.amplitudes[1],
                ]
                .into_iter()
                .flat_map(|value| value.bits().to_be_bytes())
                .collect(),
            ),
            (
                SELECTION_ROWS,
                cursor
                    .row_offsets
                    .map(|v| v as u8)
                    .into_iter()
                    .chain(cursor.storage)
                    .collect(),
            ),
        ];
        for symbol in [
            &c.symbols.marker_b,
            &c.symbols.equipped,
            &c.symbols.positive_modifier,
            &c.symbols.negative_modifier,
            &c.symbols.category_format,
        ] {
            let mut bytes = c.text(symbol.text).as_bytes().to_vec();
            bytes.push(0);
            bytes.extend(&symbol.storage);
            spans.push((sources[symbol.text.0].address, bytes));
        }
        spans
    }

    #[test]
    #[ignore = "requires both extracted discs; publishes only UI metadata JSON"]
    fn original_ui_style_reconstructs_complete_colors_cursor_and_symbols() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = extracted.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let (catalogue, sources) = parse(&executable)?;
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                payloads.insert(paths[0].clone());
                let spans = reconstruct(&restored, &sources);
                assert_eq!(
                    spans
                        .iter()
                        .map(|(_, bytes)| bytes.len())
                        .collect::<Vec<_>>(),
                    [44, 144, 48, 16, 12, 4, 4, 4, 4, 4]
                );
                for (address, bytes) in spans {
                    assert_eq!(bytes, dol::slice(&executable, address, bytes.len())?);
                }
                assert_eq!(restored.text(restored.symbols.equipped.text), "E");
                assert_eq!(
                    restored.cursor.amplitudes.map(|v| v.finite().unwrap()),
                    [2., 4.]
                );
                assert_eq!(sources.len(), 5);
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                for (address, replacement) in [
                    (PALETTE + 43, vec![0x37]),
                    (NUMBER_COLORS + 143, vec![0x5a]),
                    (BAR_COLORS + 47, vec![0xa5]),
                    (SELECTION_ROWS, vec![128]),
                    (SELECTION_ROWS + 9, vec![0x12, 0x34, 0x56]),
                    (CURSOR, 0x7fc01234u32.to_be_bytes().to_vec()),
                    (SYMBOLS + 2, vec![0xaa, 0x55]),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let at = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[at..at + replacement.len()].copy_from_slice(&replacement);
                }
                let (changed, sources) = parse(&executable)?;
                assert_eq!(changed.cursor.row_offsets[0], -128);
                assert_eq!(changed.cursor.storage, [0x12, 0x34, 0x56]);
                assert_eq!(changed.cursor.phase_scale.bits(), 0x7fc01234);
                assert!(changed.cursor.phase_scale.finite().is_err());
                assert_eq!(changed.symbols.marker_b.storage, [0xaa, 0x55]);
                for (address, bytes) in reconstruct(&changed, &sources) {
                    assert_eq!(bytes, dol::slice(&executable, address, bytes.len())?);
                }
                let modified = destination.join("modified.dol");
                fs::write(&modified, &executable)?;
                cook(&modified, &executable, &destination)?;
                assert_eq!(
                    crate::embedded::read::<Catalogue>(&destination, FAMILY, "modified.dol")?,
                    changed
                );
            }
            assert_eq!(payloads.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(&output)?;
        }
        result
    }
}
