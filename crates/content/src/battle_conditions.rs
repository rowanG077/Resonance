//! Named battle effects shared by prepared profiles and the simulation.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    AccuracyDown,
    AccuracyUp,
    Acuity,
    AilmentResistance,
    AttackDown,
    AttackUp,
    CastingSpeed,
    Curse,
    DefenseDown,
    DefenseHalved,
    DefenseUp,
    Enchanted,
    EvasionDown,
    Flare,
    Guard,
    Heavy,
    KillHpRecovery,
    KillTpRecovery,
    MagicalProtection,
    MagicAttackDown,
    MagicAttackUp,
    MagicDefenseDown,
    MagicDefenseUp,
    MovementBoost,
    Paralysis,
    Petrified,
    PhysicalAffliction,
    PhysicalProtection,
    PoisonMild,
    PoisonSevere,
    Quartz,
    ReduceItemEffect,
    RegenerateHp,
    RegenerateTp,
    Revive,
    ShortStun,
    Stun,
    TpHalf,
    TpThird,
    Weak,
}

impl From<crate::battle_action::Condition> for Condition {
    fn from(condition: crate::battle_action::Condition) -> Self {
        match condition {
            crate::battle_action::Condition::DefenseDown => Self::DefenseDown,
            crate::battle_action::Condition::Curse => Self::Curse,
            crate::battle_action::Condition::Paralysis => Self::Paralysis,
            crate::battle_action::Condition::Weak => Self::Weak,
        }
    }
}

const ALL: &[Condition] = &[
    Condition::AccuracyDown,
    Condition::AccuracyUp,
    Condition::Acuity,
    Condition::AilmentResistance,
    Condition::AttackDown,
    Condition::AttackUp,
    Condition::CastingSpeed,
    Condition::Curse,
    Condition::DefenseDown,
    Condition::DefenseHalved,
    Condition::DefenseUp,
    Condition::Enchanted,
    Condition::EvasionDown,
    Condition::Flare,
    Condition::Guard,
    Condition::Heavy,
    Condition::KillHpRecovery,
    Condition::KillTpRecovery,
    Condition::MagicalProtection,
    Condition::MagicAttackDown,
    Condition::MagicAttackUp,
    Condition::MagicDefenseDown,
    Condition::MagicDefenseUp,
    Condition::MovementBoost,
    Condition::Paralysis,
    Condition::Petrified,
    Condition::PhysicalAffliction,
    Condition::PhysicalProtection,
    Condition::PoisonMild,
    Condition::PoisonSevere,
    Condition::Quartz,
    Condition::ReduceItemEffect,
    Condition::RegenerateHp,
    Condition::RegenerateTp,
    Condition::Revive,
    Condition::ShortStun,
    Condition::Stun,
    Condition::TpHalf,
    Condition::TpThird,
    Condition::Weak,
];

/// Compact storage with a named, order-independent asset representation.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ConditionSet(u64);

impl ConditionSet {
    pub const EMPTY: Self = Self(0);

    pub const fn of(conditions: &[Condition]) -> Self {
        let mut result = 0;
        let mut index = 0;
        while index < conditions.len() {
            result |= 1 << conditions[index] as u8;
            index += 1;
        }
        Self(result)
    }

    pub const fn contains(self, condition: Condition) -> bool {
        self.0 & (1 << condition as u8) != 0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Condition> {
        ALL.iter()
            .copied()
            .filter(move |&condition| self.contains(condition))
    }
}

impl From<Condition> for ConditionSet {
    fn from(condition: Condition) -> Self {
        Self::of(&[condition])
    }
}

impl FromIterator<Condition> for ConditionSet {
    fn from_iter<T: IntoIterator<Item = Condition>>(conditions: T) -> Self {
        conditions
            .into_iter()
            .fold(Self::EMPTY, |set, condition| set.union(condition.into()))
    }
}

impl fmt::Debug for ConditionSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl Serialize for ConditionSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for ConditionSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<Condition>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_sets_serialize_names_and_reject_unknown_conditions() {
        let set = ConditionSet::of(&[Condition::Weak, Condition::PoisonMild]);
        assert_eq!(
            serde_json::to_string(&set).unwrap(),
            "[\"poison_mild\",\"weak\"]"
        );
        assert_eq!(
            serde_json::from_str::<ConditionSet>("[\"weak\",\"poison_mild\",\"weak\"]").unwrap(),
            set
        );
        for invalid in ["1", "[1,2]", "[\"unknown\"]"] {
            assert!(serde_json::from_str::<ConditionSet>(invalid).is_err());
        }
    }
}
