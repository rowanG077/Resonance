//! Shared text colors, number/bar gradients, selection rows and UI symbols.
use super::text::{TextPool, TextRef};
use crate::{dol, read::Field};
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
const SYMBOLS: u32 = 0x8035d894;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    pub palette: [[u8; 4]; 11],
    pub number_colors: [[[u8; 4]; 2]; 18],
    pub bar_colors: [[[u8; 4]; 4]; 3],
    pub selection_rows: [i8; 9],
    pub symbols: Symbols,
}

impl Catalogue {
    pub(crate) fn text(&self, reference: TextRef) -> &str {
        &self.texts[reference.0]
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Symbols {
    /// Letter B drawn by the marker renderer.
    pub marker_b: TextRef,
    pub equipped: TextRef,
    pub positive_modifier: TextRef,
    pub negative_modifier: TextRef,
    pub category_format: TextRef,
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let rows = dol::slice(executable, SELECTION_ROWS, 9)?;
    let mut texts = TextPool::default();
    let symbols = Symbols {
        marker_b: texts.fixed(executable, SYMBOLS, 4)?,
        equipped: texts.fixed(executable, SYMBOLS + 4, 4)?,
        positive_modifier: texts.fixed(executable, SYMBOLS + 8, 4)?,
        negative_modifier: texts.fixed(executable, SYMBOLS + 12, 4)?,
        category_format: texts.fixed(executable, SYMBOLS + 16, 4)?,
    };
    Ok(Catalogue {
        texts: texts.values,
        palette: Field::read(dol::slice(executable, PALETTE, 44)?, 0)?,
        number_colors: Field::read(dol::slice(executable, NUMBER_COLORS, 144)?, 0)?,
        bar_colors: Field::read(dol::slice(executable, BAR_COLORS, 48)?, 0)?,
        selection_rows: Field::read(rows, 0)?,
        symbols,
    })
}

#[cfg(test)]
pub(super) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let catalogue = read(executable)?;
    crate::embedded::write(file, output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both extracted discs; publishes only UI metadata JSON"]
    fn ui_style_decodes_colors_selection_and_symbols() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let file = extracted.join(format!("disc{disc}/sys/main.dol"));
                let mut executable = fs::read(&file)?;
                let destination = output.join(format!("disc{disc}"));
                let catalogue = read(&executable)?;
                let paths = cook(&file, &executable, &destination)?;
                let restored: Catalogue = crate::embedded::read(&destination, FAMILY, "main.dol")?;
                assert_eq!(restored, catalogue);
                payloads.insert(paths[0].clone());
                assert_eq!(restored.text(restored.symbols.equipped), "E");
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                for (address, replacement) in [
                    (PALETTE + 43, vec![0x37]),
                    (NUMBER_COLORS + 143, vec![0x5a]),
                    (BAR_COLORS + 47, vec![0xa5]),
                    (SELECTION_ROWS, vec![128]),
                ] {
                    let source = dol::slice(&executable, address, replacement.len())?;
                    let at = source.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[at..at + replacement.len()].copy_from_slice(&replacement);
                }
                let changed = read(&executable)?;
                assert_eq!(changed.selection_rows[0], -128);
                assert_eq!(changed.palette[10][3], 0x37);
                assert_eq!(changed.number_colors[17][1][3], 0x5a);
                assert_eq!(changed.bar_colors[2][3][3], 0xa5);
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
