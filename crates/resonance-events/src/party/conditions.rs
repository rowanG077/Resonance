//! Persistent ailments and buffs queued for the next encounter.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Poison {
    #[default]
    None,
    Mild,
    Severe,
    Both,
}

impl Poison {
    pub fn has_mild(self) -> bool {
        matches!(self, Self::Mild | Self::Both)
    }
    pub fn has_severe(self) -> bool {
        matches!(self, Self::Severe | Self::Both)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ailments {
    pub poison: Poison,
    pub paralysis: bool,
    pub petrified: bool,
    pub curse: bool,
}

impl Ailments {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatBuff {
    AttackUp,
    DefenseUp,
    MagicAttackUp,
    MagicDefenseUp,
    AccuracyUp,
    AttackDown,
    DefenseDown,
    AccuracyDown,
    MagicAttackDown,
}
