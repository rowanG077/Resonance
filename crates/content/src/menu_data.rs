//! Item and title descriptions and statistics shared by player menu pages.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod cooking;
mod customize;
mod ex_skills;
mod manual;
mod rename;
mod status;
mod strategy;
mod synopsis;
mod text;
mod world_map;
pub use cooking::*;
pub use customize::*;
pub use ex_skills::*;
pub use manual::*;
pub use rename::*;
pub use status::*;
pub use strategy::{STRATEGY_COUNTS, StrategyData, StrategyOption, StrategyPreset};
pub use synopsis::*;
pub use text::*;
pub use world_map::*;

pub const TECHNIQUE_COUNT: usize = 253;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemView {
    CollectorsBook,
    MonsterList,
    FigurineBook,
    TrainingManual,
    SylvarantMap,
    TetheallaMap,
}

impl Item {
    /// Inventory tabs omit the unclassified entries.
    pub fn inventory_category(&self) -> Option<usize> {
        Some(match self.category {
            1..=6 | 43 | 44 | 46 | 47 => 1,
            13..=22 => 2,
            23..=26 => 3,
            27..=30 => 4,
            31..=34 => 5,
            35..=42 => 6,
            7..=12 => 7,
            45 => 8,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuData {
    pub version: u32,
    pub items: Vec<Item>,
    pub titles: Vec<Vec<Title>>,
    pub full_names: Vec<String>,
    pub rename: RenameData,
    pub labels: BTreeMap<String, String>,
    pub item_categories: Vec<String>,
    pub inventory_categories: Vec<String>,
    pub item_group_prompt: MenuText,
    pub item_bottle_count: MenuText,
    pub techniques: Vec<Technique>,
    pub strategy: StrategyData,
    pub synopsis: SynopsisData,
    pub cooking: CookingData,
    pub customize: CustomizeData,
    pub status: StatusData,
    pub world_map: WorldMapData,
    pub monsters: crate::monster::MonsterBook,
    pub manual: TrainingManual,
    pub figurines: crate::figurine::FigurineBook,
    pub ex_skills: ExSkillData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Technique {
    pub name: String,
    pub description: String,
    pub tp: u8,
    pub tp_percent: bool,
    pub unison_usable: bool,
    /// Authored technique category marker.
    pub rank: u8,
    pub element: u8,
    pub level: u16,
    /// 0: either route, 1: technical, 2: strike, 3: learned by an event.
    pub route: u8,
    pub prerequisite: u16,
    pub alternatives: [u16; 4],
    pub field_use: Option<TechniqueUse>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TechniqueUse {
    Recover { hp: u8, party: bool },
    Cure { party: bool },
    Revive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub description: String,
    pub details: String,
    pub category: u8,
    #[serde(default)]
    pub field_usable: bool,
    #[serde(default)]
    pub view: Option<ItemView>,
    /// Slash, thrust, defense, intelligence, accuracy, evasion, luck.
    pub equipment_stats: [i16; 7],
    #[serde(default)]
    pub properties: EquipmentProperties,
    /// Base sale value; shop purchase prices apply the trade multiplier.
    pub price: u32,
    pub transforms_to: u16,
    pub field_use: Option<ItemUse>,
    pub attention: Option<ItemAttention>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemAttention {
    LowHp,
    LowTp,
    LowVitals,
    Knockout,
    Ailment,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemUse {
    Recover {
        hp: u8,
        tp: u8,
        party: bool,
    },
    Revive,
    Cure,
    /// Maximum HP/TP increase by a percentage; the other base stats use tenths.
    Herb {
        stat: usize,
        amount: u16,
        percent: bool,
    },
    Transform,
    EncounterRate {
        rate: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Title {
    pub name: String,
    pub description: String,
    /// Same order as the character's seven base growth statistics.
    pub growth: [u8; 7],
    /// Replaces the story-selected model variant while this title is equipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costume: Option<Costume>,
}

/// Character model variants; numbered title costumes differ between characters.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum Costume {
    #[default]
    Standard = 0,
    Variant1 = 1,
    Variant2 = 2,
    Story = 3,
    Variant4 = 4,
}

impl MenuData {
    pub const VERSION: u32 = 26;
    pub fn validate(&self) -> Result<()> {
        self.rename.validate()?;
        self.item_group_prompt.validate()?;
        self.item_bottle_count.validate()?;
        ensure!(
            self.item_group_prompt.lines.len() == 1 && self.item_bottle_count.lines.len() == 1,
            "invalid item prompt"
        );
        ensure!(
            self.version == Self::VERSION
                && self.items.len() == 528
                && self.item_categories.len() == 48
                && self.inventory_categories.len() == 9
                && self.techniques.len() == TECHNIQUE_COUNT,
            "invalid menu item data"
        );
        ensure!(
            self.full_names.len() == 9
                && self.titles.len() == 9
                && self
                    .titles
                    .iter()
                    .all(|titles| (1..32).contains(&titles.len())),
            "invalid menu character data"
        );
        for item in &self.items {
            ensure!(
                usize::from(item.category) < self.item_categories.len()
                    && usize::from(item.transforms_to) < self.items.len(),
                "invalid item category or transformation"
            );
            ensure!(
                match item.field_use {
                    Some(ItemUse::Recover { hp, tp, .. }) => hp <= 100 && tp <= 100,
                    Some(ItemUse::Herb { stat, amount, .. }) => stat < 7 && amount > 0,
                    Some(ItemUse::EncounterRate { rate }) => (1..=2).contains(&rate),
                    _ => true,
                },
                "invalid item action"
            );
        }
        for (id, tech) in self.techniques.iter().enumerate() {
            ensure!(
                tech.rank <= 2
                    && tech.route <= 3
                    && tech.element <= 8
                    && usize::from(tech.prerequisite) < self.techniques.len()
                    && tech
                        .alternatives
                        .iter()
                        .all(|id| usize::from(*id) < self.techniques.len()),
                "invalid technique {id}: {tech:?}"
            );
        }
        self.strategy.validate()?;
        self.synopsis.validate()?;
        self.cooking.validate(self.items.len())?;
        self.customize.validate()?;
        self.status.validate(&self.items)?;
        self.world_map.validate(self.items.len())?;
        self.monsters.validate(self.items.len())?;
        self.manual.validate()?;
        self.figurines.validate()?;
        self.ex_skills.validate(self.items.len())?;
        for text in self.texts() {
            ensure!(
                text.len() <= 4096 && text.chars().all(|c| !c.is_control() || c == '\n'),
                "invalid menu text {text:?}"
            );
        }
        for key in [
            "unison_title",
            "unison_player",
            "tech_unison",
            "tech_unison_title",
            "status",
            "next",
            "strength",
            "defense",
            "slash",
            "accuracy",
            "attack",
            "thrust",
            "evasion",
            "intelligence",
            "luck",
            "weapon",
            "body",
            "head",
            "arm",
            "accessory_1",
            "accessory_2",
            "optimal",
            "remove",
            "change_order",
            "optimal_selection",
            "optimal_slash",
            "optimal_thrust",
            "alphabetical",
            "parameter",
            "stat_arrow",
            "preview_loading",
            "item_defense",
            "item_accuracy",
            "item_evasion",
            "item_intelligence",
            "item_luck",
            "party_swap_target",
            "party_leader",
            "party_swap",
            "collectors_book",
            "transform_full",
            "transform_empty",
        ] {
            ensure!(
                self.labels.get(key).is_some_and(|s| !s.is_empty()),
                "missing menu label {key}"
            );
        }
        Ok(())
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.items
            .iter()
            .flat_map(|item| [&item.name, &item.description, &item.details])
            .chain(
                self.titles
                    .iter()
                    .flatten()
                    .flat_map(|title| [&title.name, &title.description]),
            )
            .chain(&self.full_names)
            .chain(self.labels.values())
            .chain(&self.item_categories)
            .chain(&self.inventory_categories)
            .chain(
                self.techniques
                    .iter()
                    .flat_map(|t| [&t.name, &t.description]),
            )
            .map(String::as_str)
            .chain(self.item_group_prompt.texts())
            .chain(self.item_bottle_count.texts())
            .chain(self.strategy.texts())
            .chain(self.rename.texts())
            .chain(self.synopsis.texts())
            .chain(self.cooking.texts())
            .chain(self.customize.texts())
            .chain(self.status.texts())
            .chain(self.world_map.texts())
            .chain(self.monsters.texts())
            .chain(self.manual.texts())
            .chain(self.figurines.texts())
            .chain(self.ex_skills.texts())
    }
}
