//! Source-wide identities and references, independent of cooking or execution support.
use super::{action_inventory::ActionInventory, audio::AudioInventory};
use crate::{monster::MONSTER_COUNT, validate_asset_path};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInventory {
    pub version: u32,
    pub sources: BTreeMap<String, SourceFile>,
    /// Includes duplicate/default rows. Duplication does not prove an entry unused.
    pub formations: Vec<FormationRecord>,
    pub enemies: Vec<EnemyPackage>,
    pub actions: ActionInventory,
    pub audio: AudioInventory,
    pub victory_groups: super::victory_group::Groups,
    pub items: super::items::BattleItems,
    /// Combination recipes reference the combined native programs in `actions.artes`.
    pub unison: super::unison::UnisonData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationRecord {
    pub id: u16,
    pub duplicate_of: Option<u16>,
    pub placement: Placement,
    pub escape_allowed: bool,
    pub victory_music: bool,
    pub victory_camera: bool,
    pub victory_celebration: bool,
    pub opening_event: bool,
    /// Preserved source bits whose consumer has not yet been identified.
    pub unresolved_flags: u8,
    /// Bit N replaces resource N's display name with fullwidth question marks.
    pub hidden_names: u8,
    pub resources: Vec<u16>,
    pub enemies: Vec<FormationActor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Generated,
    Explicit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationActor {
    pub resource: u8,
    pub variant: u8,
    /// Vertical palette-atlas cell selected by the model's palette transform.
    pub palette: u8,
    /// Alternate resources for the first two auxiliary models; zero keeps default.
    pub auxiliary_models: [u8; 2],
    pub position: [i16; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyPackage {
    pub monster: u8,
    pub name: String,
    pub compressed: SourceFile,
    pub decoded_bytes: u32,
    /// Includes the base statistics at index zero.
    pub variants: u8,
    pub palette_rows: u8,
    pub auxiliary_models: u8,
    pub auxiliary_resources: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormationIssue {
    MissingResource {
        formation: u16,
        actor: u8,
        slot: u8,
    },
    MissingEnemy {
        formation: u16,
        actor: u8,
        monster: u16,
    },
    MissingVariant {
        formation: u16,
        actor: u8,
        monster: u8,
        variant: u8,
    },
    MissingPalette {
        formation: u16,
        actor: u8,
        monster: u8,
        palette: u8,
    },
    MissingAuxiliaryModel {
        formation: u16,
        actor: u8,
        monster: u8,
        slot: u8,
        variant: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormationAudit {
    pub rows: usize,
    pub distinct_rows: usize,
    pub referenced_enemies: BTreeSet<u16>,
    pub issues: Vec<FormationIssue>,
}

impl SourceInventory {
    pub const VERSION: u32 = 1;

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION && self.formations.len() == 1000,
            "incomplete source formation inventory"
        );
        ensure!(
            self.enemies.len() == MONSTER_COUNT
                && self
                    .enemies
                    .iter()
                    .enumerate()
                    .all(|(id, package)| usize::from(package.monster) == id
                        && package.variants > 0
                        && package.decoded_bytes > 0),
            "incomplete source enemy inventory"
        );
        for (index, row) in self.formations.iter().enumerate() {
            ensure!(
                usize::from(row.id) == index
                    && (1..=4).contains(&row.resources.len())
                    && (1..=8).contains(&row.enemies.len())
                    && row.duplicate_of.is_none_or(|id| id < row.id),
                "invalid formation source row"
            );
        }
        for (path, source) in &self.sources {
            validate_asset_path(path)?;
            source.validate()?;
        }
        for enemy in &self.enemies {
            enemy.compressed.validate()?;
        }
        self.actions.validate()?;
        self.victory_groups.validate()?;
        self.items.validate()?;
        self.unison.validate()?;
        ensure!(
            self.unison.combinations.iter().all(|combination| self
                .actions
                .artes
                .native
                .iter()
                .any(|native| native.native_id == combination.native_id)),
            "missing combined Unison native inventory"
        );
        self.audio.validate()
    }

    /// Structural dependency checks cannot certify that a battle or effect executes.
    pub fn formation_audit(&self) -> FormationAudit {
        FormationAudit::new(&self.formations, &self.enemies)
    }
}

impl FormationAudit {
    pub fn new(formations: &[FormationRecord], enemies: &[EnemyPackage]) -> Self {
        let mut audit = Self {
            rows: formations.len(),
            distinct_rows: 0,
            referenced_enemies: BTreeSet::new(),
            issues: Vec::new(),
        };
        for row in formations {
            audit.distinct_rows += usize::from(row.duplicate_of.is_none());
            audit
                .referenced_enemies
                .extend(row.resources.iter().copied());
            for (index, actor) in row.enemies.iter().enumerate() {
                let (formation, slot) = (row.id, index as u8);
                let Some(&monster) = row.resources.get(usize::from(actor.resource)) else {
                    audit.issues.push(FormationIssue::MissingResource {
                        formation,
                        actor: slot,
                        slot: actor.resource,
                    });
                    continue;
                };
                let Some(enemy) = enemies.iter().find(|e| u16::from(e.monster) == monster) else {
                    audit.issues.push(FormationIssue::MissingEnemy {
                        formation,
                        actor: slot,
                        monster,
                    });
                    continue;
                };
                let monster = enemy.monster;
                if actor.variant >= enemy.variants {
                    audit.issues.push(FormationIssue::MissingVariant {
                        formation,
                        actor: slot,
                        monster,
                        variant: actor.variant,
                    });
                }
                if enemy.palette_rows != 0 && actor.palette >= enemy.palette_rows {
                    audit.issues.push(FormationIssue::MissingPalette {
                        formation,
                        actor: slot,
                        monster,
                        palette: actor.palette,
                    });
                }
                for (part, &variant) in actor.auxiliary_models.iter().enumerate() {
                    if part < usize::from(enemy.auxiliary_models)
                        && variant != 0
                        && u16::from(enemy.auxiliary_models) + u16::from(variant)
                            > u16::from(enemy.auxiliary_resources)
                    {
                        audit.issues.push(FormationIssue::MissingAuxiliaryModel {
                            formation,
                            actor: slot,
                            monster,
                            slot: part as u8,
                            variant,
                        });
                    }
                }
            }
        }
        audit
    }
}

impl SourceFile {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.bytes > 0
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|v| v.is_ascii_hexdigit()),
            "invalid source digest"
        );
        Ok(())
    }
}
