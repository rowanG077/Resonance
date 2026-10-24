//! Named enemy owners separate mandatory combat rules from optional artwork.
use super::*;
use crate::{diagnostics::Diagnostics, library::Snapshot, preparation::JSON_LIMIT};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 1;
pub const MAPPINGS: &str = "battle/enemies/mappings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub version: u32,
    pub rules: String,
    pub artwork: String,
    pub effects: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub version: u32,
    pub null_motions: BTreeSet<u16>,
    pub profile: Profile,
    pub weapon_visibility: BTreeMap<u8, WeaponVisibility>,
    pub projectiles: Vec<Projectile>,
    pub actions: EnemyActions,
    pub target_strategy: Option<TargetPolicy>,
    pub entry_row: EntryRow,
    pub guard_recovery_bonus: u8,
    pub presentation: Option<Presentation>,
}

impl From<Definition> for Rules {
    fn from(value: Definition) -> Self {
        Self {
            version: VERSION,
            null_motions: value.null_motions,
            profile: value.profile,
            weapon_visibility: value.weapon_visibility,
            projectiles: value.projectiles,
            actions: value.actions,
            target_strategy: value.target_strategy,
            entry_row: value.entry_row,
            guard_recovery_bonus: value.guard_recovery_bonus,
            presentation: value.presentation,
        }
    }
}

impl Rules {
    pub fn into_definition(self) -> Definition {
        Definition {
            source_sha256: String::new(),
            null_motions: self.null_motions,
            profile: self.profile,
            weapon_visibility: self.weapon_visibility,
            projectiles: self.projectiles,
            actions: self.actions,
            target_strategy: self.target_strategy,
            entry_row: self.entry_row,
            guard_recovery_bonus: self.guard_recovery_bonus,
            presentation: self.presentation,
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
    ensure!(
        entry.version == VERSION,
        "unsupported enemy version in {path}"
    );
    let rules: Rules = snapshot.json(&entry.rules, JSON_LIMIT)?;
    ensure!(
        rules.version == VERSION,
        "unsupported enemy rules in {}",
        entry.rules
    );
    let candidate = crate::battle_model::document::enemy::capture(
        snapshot,
        &entry.artwork,
        &rules.null_motions,
        diagnostics,
        cancelled,
    );
    ensure!(!cancelled(), "enemy preparation cancelled");
    let artwork = diagnostics.attempt(&entry.artwork, candidate)?;
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
        definition: rules.into_definition(),
        artwork,
        effects,
    })
}
