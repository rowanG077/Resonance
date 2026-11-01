//! Battle technique and learning rules.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod document;
pub mod elemental;

pub const PATH: &str = "game/techniques.json";
pub const MAX_USES: u16 = 999;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    default,
    deny_unknown_fields,
    bound(
        serialize = "I: Serialize + Default + PartialEq, E: Serialize + Default + PartialEq",
        deserialize = "I: Deserialize<'de> + Default, E: Deserialize<'de> + Default"
    )
)]
pub struct Definition<I = u16, E = u8> {
    /// Complete asset-authored action, independent of the original arte IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub tp_cost: u8,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub element: E,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub learning: LearningRules<I>,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub capabilities: TechniqueCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admission_flash: Option<AdmissionFlash>,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub learn_on_level_up: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub casting: Casting,
    /// Maximum target distance in world units.
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub action_range: f32,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub cast_time_adjustment: i16,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub recovery_ticks: i16,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub required_level: u16,
}

/// Requirements evaluated when a technique is used. Every prerequisite group must
/// contain a currently learned technique with the requested usage count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    default,
    deny_unknown_fields,
    bound(
        serialize = "I: Serialize",
        deserialize = "I: Deserialize<'de> + Default"
    )
)]
pub struct LearningRules<I = u16> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<LearningRoute>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<I>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technical_successor: Option<I>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike_successor: Option<I>,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub parent_uses: u16,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<LearningPrerequisite<I>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<I>,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub requires_story_unlock: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningRoute {
    Technical,
    Strike,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningPrerequisite<I = u16> {
    pub any_of: Vec<I>,
    pub minimum_uses: u16,
}

impl LearningRules {
    fn validate(&self, definitions: usize) -> Result<()> {
        let valid = |id: u16| id != 0 && usize::from(id) < definitions;
        ensure!(
            self.parent
                .into_iter()
                .chain(self.technical_successor)
                .chain(self.strike_successor)
                .chain(self.excludes.iter().copied())
                .all(valid)
                && self.prerequisites.iter().all(|group| {
                    !group.any_of.is_empty() && group.any_of.iter().copied().all(valid)
                }),
            "invalid technique learning rule"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArteFamily {
    Basic,
    Advanced,
    Arcane,
    Finisher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegalArteFamily {
    Ground,
    AntiAir,
    Aerial,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueTarget {
    Unavailable,
    #[default]
    Enemy,
    Ally,
    SelfTarget,
}

/// Autonomous admission scope, independent of a spell's eventual effect area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTargetScope {
    #[default]
    Single,
    Group,
    Either,
}

impl AiTargetScope {
    pub fn single(self) -> bool {
        self != Self::Group
    }
    pub fn group(self) -> bool {
        self != Self::Single
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TechniqueCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<ArteFamily>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regal_family: Option<RegalArteFamily>,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub spell: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub aerial: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub uses_weapon_reach: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub chains_without_contact: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub target: TechniqueTarget,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub ai_target_scope: AiTargetScope,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub summon: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub offensive: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub revives: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub healing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionFlash {
    Basic,
    Advanced,
    Arcane,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Casting {
    /// Preserve the authored cast duration despite equipment or Over Limit.
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub fixed_timing: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub support_target: bool,
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub effects: CastingEffects,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastingEffects {
    #[default]
    Standard,
    Angel,
    Offensive,
    Healing,
}

impl CastingEffects {
    pub fn opening_member(self) -> u16 {
        match self {
            Self::Angel => 51,
            _ => 37,
        }
    }

    /// Pulse and release members in the common effect bank.
    pub fn members(self) -> [u16; 2] {
        match self {
            Self::Standard | Self::Angel => [5, 7],
            Self::Offensive => [3, 7],
            Self::Healing => [4, 8],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub definitions: Vec<Definition>,
    pub learning: Vec<Vec<u16>>,
}

impl Catalogue {
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        Ok(files.json::<document::Document>(PATH)?.prepare()?.catalogue)
    }

    pub fn definition(&self, id: usize) -> Result<&Definition> {
        self.definitions
            .get(id)
            .context("arte ID outside catalogue")
    }

    pub fn learned_by(&self, character: u8) -> Result<&[u16]> {
        self.learning
            .get(usize::from(
                character.checked_sub(1).context("zero character ID")?,
            ))
            .map(Vec::as_slice)
            .context("character has no learning list")
    }

    /// Complete cooked-catalogue admission. Runtime learning admits only its member's list.
    pub fn validate(&self) -> Result<()> {
        for character in 1..=self.learning.len() {
            self.validate_learning(character.try_into().context("too many learning lists")?)?;
        }
        for row in &self.definitions {
            row.learning.validate(self.definitions.len())?;
        }
        Ok(())
    }

    pub fn validate_learning(&self, character: u8) -> Result<&[u16]> {
        let list = self.learned_by(character)?;
        let unique: BTreeSet<_> = list.iter().copied().collect();
        ensure!(
            unique.len() == list.len()
                && list
                    .iter()
                    .all(|&id| id != 0 && usize::from(id) < self.definitions.len()),
            "invalid technique learning list"
        );
        for &id in list {
            self.definitions[usize::from(id)]
                .learning
                .validate(self.definitions.len())?;
        }
        Ok(list)
    }
}
