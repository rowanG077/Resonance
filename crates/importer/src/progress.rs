//! Spoiler-free events for the interactive importer. Detailed diagnostics stay on stderr.
use serde::{Deserialize, Serialize};

/// Hidden from the runtime content index; distinguishes an interrupted full cook.
pub const IMPORT_STATE: &str = ".resonance-import-state";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportProgress {
    Extracting {
        disc: u8,
        files: usize,
        total: usize,
    },
    Cooking {
        stage: usize,
        total: usize,
    },
    Publishing,
    Finished,
}
