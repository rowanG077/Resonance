//! Enemy catalogue records, independent of battle state.
use crate::{menu_data::Element, model_preview::ModelPreview};
use serde::{Deserialize, Serialize};

pub const MONSTER_COUNT: usize = 251;
pub const MONSTER_VERSION: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterBook {
    pub records: Vec<Monster>,
    pub labels: std::collections::BTreeMap<String, String>,
}
impl MonsterBook {
    pub fn validate(&self, item_count: usize) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.records.len() == MONSTER_COUNT,
            "incomplete monster catalogue"
        );
        for (id, record) in self.records.iter().enumerate() {
            anyhow::ensure!(usize::from(record.id) == id, "unordered monster catalogue");
            record.validate(item_count)?;
        }
        for key in [
            "title",
            "number",
            "hp",
            "tp",
            "attack",
            "experience",
            "gald",
            "defense",
            "drops",
            "steal",
            "location",
            "attack_element",
            "weak",
            "strong",
            "battle_rank",
            "normal",
            "hard",
            "mania",
            "unknown_stat",
            "unknown_item",
        ] {
            anyhow::ensure!(self.labels.contains_key(key), "missing monster label {key}");
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.labels
            .values()
            .chain(
                self.records
                    .iter()
                    .flat_map(|r| [&r.name, &r.location, &r.category]),
            )
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monster {
    pub version: u32,
    pub id: u8,
    pub name: String,
    pub location: String,
    pub category: String,
    pub statistics: Vec<MonsterStats>,
    pub drops: [Option<u16>; 2],
    /// Both slots compare against one shared random roll per enemy instance.
    pub drop_chances: [u8; 2],
    /// Signed hundredths added in enemy roster order during victory rewards.
    pub grade: i16,
    pub steal: Option<u16>,
    pub attack_element: Option<Element>,
    /// Original neutral, water, wind, fire, earth, lightning, ice, light and dark responses.
    pub affinities: [i8; 9],
    pub weaknesses: Vec<Element>,
    pub resistances: Vec<Element>,
    pub preview: ModelPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterStats {
    pub hp: u32,
    pub tp: u16,
    /// A source value of zero starts the enemy at its maximum HP/TP.
    pub initial_hp: u32,
    pub initial_tp: u16,
    pub attack: u16,
    pub thrust: i16,
    pub defense: u16,
    pub intelligence: i16,
    pub accuracy: i16,
    pub evasion: i16,
    pub luck: u8,
    pub level: u8,
    pub experience: u32,
    pub gald: u32,
}

impl Monster {
    pub fn validate(&self, item_count: usize) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == MONSTER_VERSION
                && usize::from(self.id) < MONSTER_COUNT
                && !self.name.is_empty()
                && (1..=16).contains(&self.statistics.len())
                && self.statistics[0].hp > 0
                && self.drop_chances.iter().all(|&chance| chance <= 100)
                && self
                    .drops
                    .iter()
                    .chain([&self.steal])
                    .flatten()
                    .all(|&id| id > 0 && usize::from(id) < item_count),
            "invalid monster {}",
            self.id
        );
        self.preview.validate()
    }
}
