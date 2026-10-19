//! Item and title descriptions and statistics shared by player menu pages.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod cooking;
pub mod crafting;
mod customize;
mod ex_skills;
mod manual;
mod presentation;
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
pub use presentation::*;
pub use rename::*;
pub use status::*;
pub use strategy::{STRATEGY_COUNTS, StrategyData, StrategyOption, StrategyPreset};
pub use synopsis::*;
pub use text::*;
pub use world_map::*;

pub const MANUAL_PATH: &str = "game/menu/manual.json";
pub const FIGURINES_PATH: &str = "game/menu/figurines.json";
pub const SYNOPSIS_PATH: &str = "game/menu/synopsis.json";
pub const CUSTOMIZE_PATH: &str = "game/menu/customize.json";
pub const RENAME_PATH: &str = "game/menu/rename.json";

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
    pub grade_shop: crate::grade::Shop,
    pub version: u32,
    pub items: Vec<Item>,
    pub titles: Vec<Vec<Title>>,
    pub initial_names: [String; 9],
    pub techniques: Vec<Technique>,
    pub strategy: StrategyData,
    pub cooking: CookingData,
    #[serde(default)]
    pub crafting: crafting::Data,
    pub status: StatusData,
    pub world_map: WorldMapData,
    pub ex_skills: ExSkillData,
    #[serde(default)]
    pub presentation: MenuPresentation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Technique {
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
    pub category: u8,
    #[serde(default)]
    pub field_usable: bool,
    /// Original item usage bit2, independent of the field-use effect.
    pub battle_usable: bool,
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
    /// A physical ailment curable by an ordinary remedy.
    Ailment,
    AllAilments,
    MagicalAilment,
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
    pub const VERSION: u32 = 34;
    /// Admit optional labels independently while keeping gameplay records typed.
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        Self::decode(&files.read("game/menu-data.json")?, files.diagnostics())
    }

    pub fn decode(bytes: &[u8], diagnostics: &crate::diagnostics::Diagnostics) -> Result<Self> {
        let mut source: serde_json::Value = serde_json::from_slice(bytes)?;
        let presentation = source
            .as_object_mut()
            .context("menu data must be an object")?
            .remove("presentation")
            .unwrap_or_default();
        let mut data: Self = serde_json::from_value(source)?;
        data.presentation = MenuPresentation::decode(presentation, diagnostics)?;
        Ok(data)
    }

    /// Complete cook-time validation, including pages a session may never open.
    pub fn validate(&self) -> Result<()> {
        self.grade_shop.validate()?;
        self.validate_gameplay()?;
        self.presentation.validate(self)?;
        for text in self.texts() {
            ensure!(
                text.len() <= 4096 && text.chars().all(|c| !c.is_control() || c == '\n'),
                "invalid menu text {text:?}"
            );
        }
        Ok(())
    }

    /// Shared inventory, character, technique and battle data. Optional page
    /// resources are validated when that page is selected.
    pub fn validate_gameplay(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION && !self.items.is_empty(),
            "invalid menu item data"
        );
        ensure!(
            self.titles.len() == 9
                && self
                    .titles
                    .iter()
                    .all(|titles| !titles.is_empty() && titles.len() <= usize::from(u8::MAX)),
            "invalid menu character data"
        );
        ensure!(
            self.initial_names.iter().all(|name| !name.is_empty()
                && name.len() <= 12
                && name.bytes().all(|b| (32..127).contains(&b))),
            "invalid initial character name"
        );
        for item in &self.items {
            ensure!(
                item.category < 48 && usize::from(item.transforms_to) < self.items.len(),
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
        self.strategy.validate_rules()?;
        self.cooking.validate(self.items.len())?;
        self.crafting.validate(self.items.len())?;
        self.ex_skills.validate_rules(self.items.len())?;
        Ok(())
    }

    pub fn label(&self, key: &str) -> Result<&str> {
        presentation::required_label(&self.presentation.labels, key)
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.initial_names
            .iter()
            .map(String::as_str)
            .chain(self.presentation.texts())
            .chain(self.crafting.texts())
    }
}
