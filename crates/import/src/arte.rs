//! Complete authored arte records; gameplay bindings select from these tables.
mod definition;
mod tables;

use crate::{dol, embedded};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) use definition::Definition;
use tables::{Combination, LearningList};

const FAMILY: &str = "arte-catalogue";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalogue {
    pub(crate) definitions: Vec<Definition>,
    pub(crate) learning: Vec<LearningList>,
    pub(crate) combinations: Vec<Combination>,
    learning_storage: [u8; 5],
}

impl Catalogue {
    pub(crate) fn definition(&self, id: usize) -> Result<&Definition> {
        self.definitions
            .get(id)
            .context("arte ID outside catalogue")
    }

    pub(crate) fn learned_by(&self, character: u8) -> Result<&[u8]> {
        self.learning
            .get(usize::from(
                character.checked_sub(1).context("zero character ID")?,
            ))
            .context("character has no learning list")?
            .active()
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.definitions.len() == resonance_content::menu_data::TECHNIQUE_COUNT
                && self.learning.len() == 11
                && self.combinations.len() == 20,
            "incomplete arte catalogue"
        );
        for list in &self.learning {
            list.active()?;
        }
        ensure!(
            self.combinations.iter().all(
                |row| row.participant_count <= 4 && row.camera_pitch_offset_degrees.is_finite()
            ),
            "invalid Unison combination"
        );
        Ok(())
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Catalogue> {
    Ok(Catalogue {
        definitions: definition::definitions(executable)?,
        learning: tables::learning(executable)?,
        combinations: tables::combinations(executable)?,
        learning_storage: dol::slice(executable, 0x80202f8b, 5)?.try_into()?,
    })
}

pub(crate) fn cooked(output: &Path) -> Result<Catalogue> {
    let catalogue: Catalogue = embedded::read(output, FAMILY, "main.dol")?;
    catalogue.validate()?;
    Ok(catalogue)
}

pub(crate) fn cook(file: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    embedded::write(
        file,
        output,
        FAMILY,
        &read(executable)?,
        serde_json::json!({
            "definitions":{"address":0x80202f90u32,"count":253,"stride":88},
            "learning":{"address":0x80202dc8u32,"count":11,"stride":41,"trailing_bytes":5},
            "combinations":{"address":0x80208688u32,"count":20,"stride":64},
        }),
    )
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
