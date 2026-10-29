//! Named enemy owners separate mandatory combat rules from optional artwork.
use super::*;
use crate::{diagnostics::Diagnostics, library::Snapshot, preparation::JSON_LIMIT};
use anyhow::{Context, Result, ensure};

pub const MAPPINGS: &str = "battle/enemies/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub rules: Rules,
    /// Decode optional artwork under the cosmetic diagnostic policy, rather than
    /// letting a malformed visual setting reject otherwise valid gameplay rules.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub artwork: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<String>,
}

pub use super::rules::Rules;

impl Entry {
    pub fn decode_artwork(
        &self,
    ) -> Result<Option<crate::battle_model::document::enemy::Definition>> {
        if self.artwork.is_null() {
            Ok(None)
        } else {
            serde_json::from_value(self.artwork.clone())
                .map(Some)
                .context("decode enemy artwork")
        }
    }
}

#[derive(Debug)]
pub struct Captured {
    pub definition: Definition,
    /// The preview and battle rig are refreshed together, never taken from a
    /// field's earlier menu catalogue generation.
    pub artwork: Option<crate::battle_model::document::enemy::Captured>,
    pub effects: Option<(String, crate::battle_effect::SourceBank)>,
}

pub fn capture(
    snapshot: &mut Snapshot,
    id: u8,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Captured> {
    ensure!(!cancelled(), "enemy preparation cancelled");
    let path = snapshot.resolve(MAPPINGS, u32::from(id), Some("enemy.json"))?;
    let entry: Entry = snapshot.json(&path, JSON_LIMIT)?;
    let artwork = entry.decode_artwork();
    let definition = entry.rules.into_definition()?;
    let candidate = artwork.and_then(|artwork| {
        artwork
            .map(|artwork| {
                crate::battle_model::document::enemy::capture(
                    snapshot,
                    &path,
                    artwork,
                    &definition.null_motions,
                    diagnostics,
                    cancelled,
                )
            })
            .transpose()
    });
    ensure!(!cancelled(), "enemy preparation cancelled");
    let artwork = diagnostics.attempt(&path, candidate)?.flatten();
    let effects = if let Some(path) = entry.effects {
        let candidate =
            crate::battle_effect::document::capture(snapshot, &path, diagnostics, cancelled);
        ensure!(!cancelled(), "enemy preparation cancelled");
        diagnostics
            .attempt(&path, candidate)?
            .map(|bank| (path, bank))
    } else {
        None
    };
    ensure!(!cancelled(), "enemy preparation cancelled");
    Ok(Captured {
        definition,
        artwork,
        effects,
    })
}
