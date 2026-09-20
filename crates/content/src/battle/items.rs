//! Battle consumables are separate from field item operations.
use super::effects::{EffectBank, EffectId};
use crate::menu_data::Element;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleItems {
    pub recipes: Vec<ItemRecipe>,
    /// Local offsets from each character's effect anchor, before actor scaling.
    pub throw_origins: [[f32; 3]; 9],
    pub recovery_color: [u8; 4],
    pub enhancement_color: [u8; 4],
    pub scan_color: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRecipe {
    pub item: u16,
    pub action: ItemAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemAction {
    Recover {
        hp: u8,
        tp: u8,
        party: bool,
        enhanced: bool,
    },
    Revive,
    Cure {
        ailments: Ailments,
    },
    Enhance {
        enhancement: Enhancement,
    },
    Scan,
    HalveDamage,
    StopEnemies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ailments {
    Physical,
    Magical,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Enhancement {
    Attack,
    Defense,
    Accuracy,
    PhysicalProtection { retained: bool },
    MagicalProtection { retained: bool },
    Element { element: Element },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemTargets {
    Ally,
    AllAllies,
    Enemy,
    AllEnemies,
    User,
}

impl ItemRecipe {
    pub fn targets(self) -> ItemTargets {
        match self.action {
            ItemAction::Recover { party: true, .. } => ItemTargets::AllAllies,
            ItemAction::Scan => ItemTargets::Enemy,
            ItemAction::StopEnemies => ItemTargets::AllEnemies,
            ItemAction::HalveDamage => ItemTargets::User,
            _ => ItemTargets::Ally,
        }
    }
}

impl BattleItems {
    pub const THROW_EFFECT: EffectId = EffectId {
        bank: EffectBank::Common,
        id: 9,
    };
    pub const REVIVE_EFFECT: EffectId = EffectId {
        bank: EffectBank::Common,
        id: 42,
    };

    pub fn recipe(&self, item: u16) -> Option<ItemRecipe> {
        self.recipes
            .iter()
            .find(|recipe| recipe.item == item)
            .copied()
    }

    pub fn effects(&self) -> impl Iterator<Item = EffectId> + '_ {
        std::iter::once(Self::THROW_EFFECT).chain(
            self.recipes
                .iter()
                .any(|recipe| recipe.action == ItemAction::Revive)
                .then_some(Self::REVIVE_EFFECT),
        )
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.throw_origins.iter().flatten().all(|v| v.is_finite()),
            "invalid item throw origin"
        );
        for (index, recipe) in self.recipes.iter().enumerate() {
            ensure!(
                recipe.item != 0 && self.recipes[..index].iter().all(|r| r.item != recipe.item),
                "duplicate or empty battle item"
            );
            if let ItemAction::Recover { hp, tp, .. } = recipe.action {
                ensure!(
                    (hp != 0 || tp != 0) && hp <= 100 && tp <= 100,
                    "invalid item recovery"
                );
            }
        }
        Ok(())
    }
}
