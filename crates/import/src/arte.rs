//! Original technique records; gameplay bindings select from these tables.
mod definition;
mod tables;

use crate::dol;
#[cfg(test)]
use crate::embedded;
use anyhow::Result;
use std::path::Path;

pub(crate) use resonance_content::arte::{Catalogue, Definition};

#[cfg(test)]
const FAMILY: &str = "arte-catalogue";

pub fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(Catalogue {
        definitions: definition::definitions(executable)?,
        learning: tables::learning(executable)?,
        combinations: tables::combinations(executable)?,
        learning_storage: dol::slice(executable, 0x80202f8b, 5)?.try_into()?,
    })
}

pub fn publish(catalogue: &Catalogue, output: &Path) -> Result<String> {
    let path = resonance_content::arte::PATH;
    crate::write_atomic(&output.join(path), &serde_json::to_vec(catalogue)?)?;
    Ok(path.into())
}

#[cfg(test)]
pub(crate) fn cooked(output: &Path) -> Result<Catalogue> {
    let catalogue: Catalogue = embedded::read(output, FAMILY, "main.dol")?;
    catalogue.validate()?;
    Ok(catalogue)
}

#[cfg(test)]
pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    embedded::write(file, output, FAMILY, &read(executable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both original extracted discs; only publishes JSON tables"]
    fn original_catalogue_publishes_all_rows_and_deduplicates_discs() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut payloads = BTreeSet::new();
            for disc in [1, 2] {
                let source = extracted.join(format!("disc{disc}/sys/main.dol"));
                let executable = fs::read(&source)?;
                let destination = output.join(format!("disc{disc}"));
                let paths = cook(&source, &executable, &destination)?;
                let restored = cooked(&destination)?;
                let shared = publish(&restored, &destination)?;
                assert_eq!(shared, resonance_content::arte::PATH);
                let shared: Catalogue =
                    serde_json::from_slice(&fs::read(destination.join(shared))?)?;
                shared.validate()?;
                assert_eq!(
                    serde_json::to_value(shared)?,
                    serde_json::to_value(&restored)?
                );
                assert_eq!(
                    serde_json::to_value(&restored)?,
                    serde_json::to_value(read(&executable)?)?
                );
                assert_eq!(restored.learned_by(10)?, [66, 98]);
                assert!(restored.learned_by(11)?.is_empty());
                assert_eq!(restored.learning_storage, [0; 5]);
                assert!(restored.learned_by(0).is_err() && restored.learned_by(12).is_err());
                assert!(restored.definition(253).is_err());
                let provenance: serde_json::Value =
                    serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                assert_eq!(provenance["source_sha256"], crate::digest(&executable));
                assert_eq!(provenance["data"], paths[0]);
                payloads.insert(paths[0].clone());
            }
            assert_eq!(payloads.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
