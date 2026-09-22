//! Instance-local script bindings; shared meshes and clips carry no behavior.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBehaviorBinding {
    pub module: String,
    pub function: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Node {
    pub part: u16,
    pub bone: u16,
}

impl ModelBehaviorBinding {
    pub fn validate(&self) -> Result<()> {
        let identifier = |value: &str| {
            let mut bytes = value.bytes();
            bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        ensure!(
            self.module.split("::").all(identifier) && identifier(&self.function),
            "invalid model behavior entry"
        );
        Ok(())
    }
}
