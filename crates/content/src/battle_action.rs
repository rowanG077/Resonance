//! Action definitions used during battle preparation.
use crate::menu_data::Element;
use serde::{Deserialize, Serialize};

/// Enemy choices and their resolved contact resources.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyActions {
    pub rows: Vec<EnemyAction>,
    pub policy: EnemyPolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnemyPolicy {
    pub back_row: Vec<EnemyBackRow>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnemyBackRow {
    pub action: u8,
    pub weight: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnemyRequirements {
    pub difficulty: std::ops::RangeInclusive<u8>,
    pub hp_percent: Option<u8>,
    pub priority: bool,
}

impl Default for EnemyRequirements {
    fn default() -> Self {
        Self {
            difficulty: 0..=2,
            hp_percent: None,
            priority: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetPolicy {
    Nearest,
    Farthest,
    Leader,
    Spread,
    Flying,
    Casting,
    LowestHp,
    Protect,
    SelfTarget,
}

impl TryFrom<u8> for TargetPolicy {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> anyhow::Result<Self> {
        Ok(match value {
            1 => Self::Nearest,
            2 => Self::Farthest,
            3 => Self::Leader,
            4 => Self::Spread,
            5 => Self::Flying,
            6 => Self::Casting,
            7 => Self::LowestHp,
            8 => Self::Protect,
            9 => Self::SelfTarget,
            _ => anyhow::bail!("unsupported target policy {value}"),
        })
    }
}

/// Supported native actions. Their timing and execution belong to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyAttack {
    Tail,
    Pounce,
    Right,
    Push,
    Double,
    Triple,
    Counter,
    Cross,
    Strike,
    Spit,
    Swing,
    Scatter,
    Lob,
    FireBall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemyAction {
    pub weight: u8,
    pub target_policy: Option<TargetPolicy>,
    pub requirements: EnemyRequirements,
    pub return_to_formation: bool,
    pub range: [i16; 2],
    pub approach_range: i16,
    pub approach_minimum: i16,
    pub guard_chance: i8,
    pub tp: u8,
    pub attack: Option<EnemyAttack>,
    pub projectile: Option<crate::battle_projectile::Projectile>,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitElement {
    #[default]
    Inherited,
    Neutral,
    Element(Element),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    #[default]
    Normal,
    Percent(u16),
    Fixed(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    DefenseDown,
    Curse,
    Paralysis,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HitCondition {
    pub condition: Condition,
    pub chance: u8,
    pub value: i8,
}
