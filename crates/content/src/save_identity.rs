//! Saved-state compatibility derived from the effective gameplay documents.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
mod battle;
mod field;
mod projection;
pub use projection::gameplay;

pub const SCHEMA: u32 = 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub schema: u32,
    pub content: [u8; 32],
}
impl Identity {
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let identity = files
            .identity()
            .context("save identity was not derived from prepared content")?;
        identity.validate()?;
        Ok(identity.clone())
    }

    /// Capture global gameplay identity from the same view used for preparation.
    /// Resource paths, message tables and visual/media payloads are not identity.
    /// Rigs, attachment bounds and motion curves used by gameplay participate.
    /// Native scripts bind by original IDs; authored modules bind by logical name.
    pub fn capture(
        snapshot: &mut crate::library::Snapshot,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        use crate::{
            field::document::Definition,
            library::FIELDS,
            preparation::{self, JSON_LIMIT},
            skit,
        };
        ensure!(!cancelled(), "save identity preparation cancelled");
        let session: crate::session::SessionData =
            snapshot.json("game/session-data.json", JSON_LIMIT)?;
        let mut rules: serde_json::Value = snapshot.json("game/menu-data.json", JSON_LIMIT)?;
        let monsters = rules
            .get("presentation")
            .and_then(|value| value.get("monsters"))
            .filter(|value| !value.is_null())
            .map(projection::monsters)
            .transpose()?;
        rules
            .as_object_mut()
            .context("menu data must be an object")?
            .remove("presentation");
        let rules: crate::menu_data::MenuData = serde_json::from_value(rules)?;
        let mut hash = Sha256::new();
        hash.update(b"Resonance editable saved state");
        hash.update(SCHEMA.to_be_bytes());
        record(&mut hash, "game", snapshot.descriptor().game.as_bytes());
        record(&mut hash, "gameplay", &gameplay(&session, rules)?);
        if let Some(monsters) = monsters {
            record(&mut hash, "monsters", &monsters);
        }
        let fields: Vec<_> = snapshot
            .mappings(FIELDS)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        for id in fields {
            ensure!(!cancelled(), "save identity preparation cancelled");
            let path = snapshot.field(id)?;
            let field: Definition = snapshot.json(&path, 1024 * 1024)?;
            field.validate()?;
            record(
                &mut hash,
                &format!("field/{id}"),
                &snapshot.read(&field.script, 4 * 1024 * 1024)?,
            );
            field::capture(snapshot, &mut hash, id, &field, cancelled)?;
        }
        let skits: Vec<_> = snapshot
            .mappings(skit::document::MAPPINGS)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        for id in skits {
            ensure!(!cancelled(), "save identity preparation cancelled");
            let path = snapshot.resolve(skit::document::MAPPINGS, id, Some("skit.json"))?;
            let skit: skit::SkitResourcePaths = snapshot.json(&path, 1024 * 1024)?;
            record(
                &mut hash,
                &format!("skit/{id}"),
                &snapshot.read(&skit.script, 4 * 1024 * 1024)?,
            );
        }
        if let Some(world) = crate::overworld::document::gameplay(snapshot, cancelled)? {
            record(&mut hash, "world", &world);
        }
        battle::capture(snapshot, &mut hash, cancelled)?;
        for (module, source) in preparation::scripts(snapshot, cancelled)? {
            ensure!(!cancelled(), "save identity preparation cancelled");
            let canonical = symphonia_script_compiler::canonical_source(&module, &source)?;
            record(&mut hash, &format!("source/{module}"), &canonical);
        }
        ensure!(!cancelled(), "save identity preparation cancelled");
        Ok(Self {
            schema: SCHEMA,
            content: hash.finalize().into(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == SCHEMA,
            "unsupported save schema {}; expected {SCHEMA}; start a new game",
            self.schema
        );
        Ok(())
    }

    /// Extend an editable library's gameplay identity with one captured authored
    /// project. Callers supply canonical bindings and source semantics, never an
    /// absolute project path or a second read of sources used for compilation.
    pub fn with_authored_project(&self, canonical: &[u8]) -> Result<Self> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"Resonance editable authored project");
        hash.update(SCHEMA.to_be_bytes());
        record(&mut hash, "library", &self.content);
        record(&mut hash, "project", canonical);
        Ok(Self {
            schema: SCHEMA,
            content: hash.finalize().into(),
        })
    }
}

fn record(hash: &mut Sha256, name: &str, bytes: &[u8]) {
    hash.update((name.len() as u64).to_be_bytes());
    hash.update(name.as_bytes());
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

fn record_json(hash: &mut Sha256, name: &str, value: &impl Serialize) -> Result<()> {
    record(hash, name, &serde_json::to_vec(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_identity_requires_current_derived_state() -> Result<()> {
        use crate::prepared::Files;
        let identity = Identity {
            schema: SCHEMA,
            content: [7; 32],
        };
        let mut files = Files::default();
        assert!(Identity::load(&files).is_err());
        files.insert(
            "game/save-identity.json".into(),
            serde_json::to_vec(&identity)?.into(),
        );
        assert!(
            Identity::load(&files).is_err(),
            "stored fingerprints never authorize saves"
        );
        let files = files.with_identity(identity.clone())?;
        assert_eq!(Identity::load(&files)?, identity);
        for schema in [SCHEMA - 1, SCHEMA + 1] {
            let unsupported = Identity {
                schema,
                ..identity.clone()
            };
            assert!(unsupported.validate().is_err());
            assert!(Files::default().with_identity(unsupported).is_err());
        }
        Ok(())
    }

    #[test]
    fn changing_prepared_inputs_discards_derived_identity_without_changing_retained_view()
    -> Result<()> {
        use crate::prepared::Files;
        let identity = Identity {
            schema: SCHEMA,
            content: [13; 32],
        };
        let original = Files::default().with_identity(identity.clone())?;
        assert_eq!(Identity::load(&original)?, identity);
        let mut edited = original.clone();
        edited.insert("game/session-data.json".into(), b"changed".to_vec().into());
        assert!(Identity::load(&edited).is_err());
        let mut removed = original.clone();
        removed.remove("game/session-data.json");
        assert!(Identity::load(&removed).is_err());
        let mut cleared = original.clone();
        cleared.clear();
        assert!(Identity::load(&cleared).is_err());
        assert_eq!(Identity::load(&original)?, identity);
        assert!(
            Files::default()
                .with_identity(Identity {
                    schema: SCHEMA - 1,
                    content: [13; 32]
                })
                .is_err()
        );
        Ok(())
    }
}
