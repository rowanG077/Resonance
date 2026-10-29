//! Validate local template names at the authoring boundary.
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

pub(super) fn validate(names: &[String], count: usize) -> Result<()> {
    ensure!(
        names.len() == count && count <= 256,
        "invalid effect template names"
    );
    let mut seen = BTreeSet::new();
    for name in names {
        ensure!(
            !name.is_empty()
                && name.len() <= 128
                && !name.chars().any(char::is_control)
                && seen.insert(name),
            "empty or duplicate effect template name"
        );
    }
    Ok(())
}

/// Runtime callers request this shared recipe directly, outside effect programs.
pub const STUN_BIRD: &str = "stun-bird";
