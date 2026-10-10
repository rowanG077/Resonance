//! Battle technique and learning rules.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PATH: &str = "game/techniques.json";
pub const MAX_USES: u16 = 999;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub tp_cost: u8,
    pub element: u8,
    pub learning: LearningRules,
    pub capabilities: TechniqueCapabilities,
    pub admission_flash: Option<AdmissionFlash>,
    pub learn_on_level_up: bool,
    pub casting: Casting,
    /// Maximum target distance in world units.
    pub action_range: f32,
    pub cast_time_adjustment: i16,
    pub recovery_ticks: i16,
    pub required_level: u16,
}

/// Requirements evaluated when a technique is used. Every prerequisite group must
/// contain a currently learned technique with the requested usage count.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningRules {
    pub route: Option<LearningRoute>,
    pub parent: Option<u16>,
    pub technical_successor: Option<u16>,
    pub strike_successor: Option<u16>,
    pub parent_uses: u16,
    pub prerequisites: Vec<LearningPrerequisite>,
    pub excludes: Vec<u16>,
    pub requires_story_unlock: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningRoute {
    Technical,
    Strike,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningPrerequisite {
    pub any_of: Vec<u16>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechniqueCapabilities {
    pub family: Option<ArteFamily>,
    pub regal_family: Option<RegalArteFamily>,
    pub spell: bool,
    pub aerial: bool,
    pub uses_weapon_reach: bool,
    pub chains_without_contact: bool,
    pub target: TechniqueTarget,
    pub offensive: bool,
    pub revives: bool,
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
#[serde(deny_unknown_fields)]
pub struct Casting {
    pub support_target: bool,
    pub effects: CastingEffects,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastingEffects {
    #[default]
    Standard,
    Offensive,
    Healing,
}

impl CastingEffects {
    /// Pulse and release members in the common effect bank.
    pub fn members(self) -> [u16; 2] {
        match self {
            Self::Standard => [5, 7],
            Self::Offensive => [3, 7],
            Self::Healing => [4, 8],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub definitions: Vec<Definition>,
    pub learning: Vec<Vec<u8>>,
}

impl Catalogue {
    pub fn definition(&self, id: usize) -> Result<&Definition> {
        self.definitions
            .get(id)
            .context("arte ID outside catalogue")
    }

    pub fn learned_by(&self, character: u8) -> Result<&[u8]> {
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

    pub fn validate_learning(&self, character: u8) -> Result<&[u8]> {
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
