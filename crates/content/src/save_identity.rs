//! One cooked identity binds saved state to native gameplay content.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "game/save-identity.json";
pub const SCHEMA: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub schema: u32,
    pub content: [u8; 32],
}
impl Identity {
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let identity: Self = files.json(PATH)?;
        identity.validate()?;
        Ok(identity)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == SCHEMA, "unsupported cooked save schema");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_identity_requires_only_its_descriptor_and_current_schema() -> Result<()> {
        let mut files = crate::prepared::Files::default();
        assert!(Identity::load(&files).is_err());
        let mut identity = Identity {
            schema: SCHEMA,
            content: [7; 32],
        };
        files.insert(PATH.into(), serde_json::to_vec(&identity)?.into());
        assert_eq!(Identity::load(&files)?, identity);
        identity.schema -= 1;
        files.insert(PATH.into(), serde_json::to_vec(&identity)?.into());
        assert!(Identity::load(&files).is_err());
        Ok(())
    }
}
