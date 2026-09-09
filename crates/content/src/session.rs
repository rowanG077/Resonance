//! Cooked definitions used by fresh-game initialization and party script calls.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub version: u32,
    pub executable_sha256: String,
    pub items: Vec<ItemDefinition>,
    pub characters: Vec<CharacterDefinition>,
    pub experience: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDefinition {
    /// Weapon, armor, head, shield/body, or accessory (two available slots).
    pub equipment_kind: Option<u8>,
    pub allowed_characters: u16,
    pub stack_limit: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterDefinition {
    pub affinity: i32,
    pub level: u8,
    pub experience: u32,
    /// HP, TP, and five growth statistics, in the authored growth-table order.
    pub base_stats: [u16; 7],
    pub luck: u8,
    pub overlimit: u8,
    /// Weapon, armor, head, accessory 1, accessory 2, shield/body.
    pub equipment: [u16; 6],
    pub techniques: Vec<u16>,
    pub allowed_techniques: Vec<u16>,
    pub shortcuts: [u16; 4],
    pub growth: [StatGrowth; 7],
    pub level_techniques: BTreeMap<u8, Vec<u16>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatGrowth {
    pub base: u8,
    pub random: u8,
    pub title_bonus: u8,
}

impl SessionData {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.characters.len() == 9,
            "invalid session data version/characters"
        );
        ensure!(
            (1..=4096).contains(&self.items.len()),
            "invalid item definition count"
        );
        ensure!(
            (2..=256).contains(&self.experience.len())
                && self.experience.windows(2).all(|p| p[0] <= p[1]),
            "invalid experience curve"
        );
        ensure!(
            self.executable_sha256.len() == 64
                && self
                    .executable_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit()),
            "invalid session source digest"
        );
        for item in &self.items {
            ensure!(
                item.equipment_kind.is_none_or(|kind| kind <= 4)
                    && item.allowed_characters < 512
                    && (1..=99).contains(&item.stack_limit),
                "invalid item definition"
            );
        }
        for character in &self.characters {
            ensure!(
                character.level > 0 && usize::from(character.level) < self.experience.len(),
                "invalid initial level"
            );
            ensure!(
                character
                    .equipment
                    .iter()
                    .all(|id| usize::from(*id) < self.items.len()),
                "initial equipment item is missing"
            );
            ensure!(
                character.techniques.len() <= 64
                    && character.allowed_techniques.len() <= 64
                    && character
                        .techniques
                        .iter()
                        .all(|id| character.allowed_techniques.contains(id))
                    && character
                        .level_techniques
                        .values()
                        .map(Vec::len)
                        .sum::<usize>()
                        <= 64,
                "too many character techniques"
            );
            ensure!(
                character
                    .level_techniques
                    .keys()
                    .all(|level| *level > 0 && usize::from(*level) < self.experience.len()),
                "invalid technique level"
            );
            ensure!(
                character.base_stats[0] <= 9999
                    && character.base_stats[1] <= 999
                    && character.base_stats.iter().all(|v| *v <= 32767),
                "invalid initial base stats"
            );
        }
        Ok(())
    }
}
