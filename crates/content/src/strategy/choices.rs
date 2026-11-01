//! Shared saved strategy choice identities and decoding.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Names follow the selectable strategy choices. Their numeric values are only
/// used at the existing save/menu and prepared-gameplay boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum TechniqueStrategy {
    AtOnce = 1,
    KeepReserve,
    Retain,
    Save,
    DoNotUse,
    Moderation,
    Heal,
    Support,
}

impl TechniqueStrategy {
    pub const ALL: [Self; 8] = [
        Self::AtOnce,
        Self::KeepReserve,
        Self::Retain,
        Self::Save,
        Self::DoNotUse,
        Self::Moderation,
        Self::Heal,
        Self::Support,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum PositionStrategy {
    Frontlines = 1,
    DontPursue,
    HoldPosition,
    LongRangeSkills,
    LongRangeMagic,
    SkillsAndMagic,
}

impl TryFrom<u8> for TechniqueStrategy {
    type Error = anyhow::Error;
    fn try_from(id: u8) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|choice| *choice as u8 == id)
            .context("invalid technique strategy")
    }
}

impl PositionStrategy {
    pub const ALL: [Self; 6] = [
        Self::Frontlines,
        Self::DontPursue,
        Self::HoldPosition,
        Self::LongRangeSkills,
        Self::LongRangeMagic,
        Self::SkillsAndMagic,
    ];

    pub fn is_ranged(self) -> bool {
        matches!(self, Self::LongRangeSkills | Self::LongRangeMagic)
    }
}

impl TryFrom<u8> for PositionStrategy {
    type Error = anyhow::Error;
    fn try_from(id: u8) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|choice| *choice as u8 == id)
            .context("invalid position strategy")
    }
}

/// Default is resolved here while its raw position choice stays distinguishable.
/// Target remains a choice ID: the imported table supplies its runtime handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedChoices {
    pub target: u8,
    pub techniques: TechniqueStrategy,
    pub position: PositionStrategy,
    pub explicit_position: bool,
}

impl ResolvedChoices {
    pub fn resolve(raw: [u8; 3], defaults: [u8; 3]) -> Result<Self> {
        ensure!(
            defaults
                .into_iter()
                .zip([8, 8, 6])
                .all(|(id, max)| (1..=max).contains(&id))
                && raw.into_iter().zip([8, 8, 6]).all(|(id, max)| id <= max),
            "invalid companion strategy choices"
        );
        let [target, techniques, position] =
            std::array::from_fn(|i| if raw[i] == 0 { defaults[i] } else { raw[i] });
        Ok(Self {
            target,
            techniques: techniques.try_into()?,
            position: position.try_into()?,
            explicit_position: raw[2] != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_choices_keep_numeric_ids_and_explicit_default_semantics() -> Result<()> {
        for value in PositionStrategy::ALL {
            assert_eq!(PositionStrategy::try_from(value as u8)?, value);
        }
        for value in TechniqueStrategy::ALL {
            assert_eq!(TechniqueStrategy::try_from(value as u8)?, value);
        }
        let default = ResolvedChoices::resolve([0; 3], [1, 6, 4])?;
        let explicit = ResolvedChoices::resolve([1, 6, 4], [1, 6, 4])?;
        assert_eq!(default.position, explicit.position);
        assert!(!default.explicit_position && explicit.explicit_position);
        assert!(PositionStrategy::try_from(0).is_err());
        assert!(TechniqueStrategy::try_from(9).is_err());
        assert!(ResolvedChoices::resolve([9, 0, 0], [1, 1, 1]).is_err());
        assert!(ResolvedChoices::resolve([0; 3], [1, 1, 0]).is_err());
        Ok(())
    }
}
