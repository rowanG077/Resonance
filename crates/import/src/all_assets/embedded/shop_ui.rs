//! Shop actions, comparison labels and help bindings in their complete native order.
use super::text::{TextPool, TextRef};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
const FAMILY: &str = "shop-ui";
const TABLE: u32 = 0x801aadb0;

super::ordered! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
    #[repr(u8)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum Label {
        Buy,
        Sell,
        Equip,
        Exit,
        Status,
        Empty,
        Confirm,
        Yes,
        No,
        Total,
        Gald,
        SelectItem,
        Add,
        Reduce,
        Ok,
        StatusHelp,
        Info,
        Slash,
        Thrust,
        Defense,
        Accuracy,
        Evasion,
        Intelligence,
        Luck,
        Attack,
        CannotEquip,
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    /// Distinct bindings remain distinct even when they share a text pointer.
    labels: BTreeMap<Label, Option<TextRef>>,
}

impl Catalogue {
    pub(crate) fn label(&self, label: Label) -> Result<&str> {
        let reference = self
            .labels
            .get(&label)
            .copied()
            .flatten()
            .with_context(|| format!("missing shop label {label:?}"))?;
        Ok(&self.texts[reference.0])
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    let mut texts = TextPool::default();
    let labels = Label::ALL
        .into_iter()
        .zip(texts.array::<26>(executable, TABLE)?)
        .collect();
    Ok(Catalogue {
        texts: texts.values,
        labels,
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
    use crate::dol;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both extracted discs; publishes shop JSON without media conversion"]
    fn original_shop_ui_preserves_all_bindings_aliases_and_source_texts() -> Result<()> {
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
                assert_eq!(restored.labels.len(), 26);
                assert_eq!(
                    restored.labels[&Label::Status],
                    restored.labels[&Label::StatusHelp]
                );
                assert_eq!(restored.label(Label::CannotEquip)?, "Cannot equip");
                payloads.insert(paths[0].clone());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                let alias = crate::read::u32(
                    dol::slice(&executable, TABLE + Label::Status as u32 * 4, 4)?,
                    0,
                )?;
                for (index, pointer) in [(0, 0u32), (1, alias)] {
                    let bytes = dol::slice(&executable, TABLE + index * 4, 4)?;
                    let at = bytes.as_ptr() as usize - executable.as_ptr() as usize;
                    executable[at..at + 4].copy_from_slice(&pointer.to_be_bytes());
                }
                let changed = read(&executable)?;
                assert!(changed.label(Label::Buy).is_err());
                assert_eq!(changed.labels[&Label::Sell], changed.labels[&Label::Status]);
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
