//! Cooked definitions used by fresh-game initialization and party script calls.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// Shared gameplay rules bound from menu-data after loading; never saved twice.
    #[serde(skip)]
    pub rules: Option<std::sync::Arc<crate::menu_data::MenuData>>,
    pub version: u32,
    pub executable_sha256: String,
    pub items: Vec<ItemDefinition>,
    pub characters: Vec<CharacterDefinition>,
    pub experience: Vec<u32>,
}

/// Localized labels are independent of the statistics used for save identity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameText {
    #[serde(default)]
    pub characters: BTreeMap<i32, String>,
    pub items: BTreeMap<u16, String>,
    pub titles: BTreeMap<u16, String>,
    #[serde(default)]
    pub techniques: BTreeMap<u16, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDefinition {
    /// Weapon, armor, head, shield/body, or accessory (two available slots).
    pub equipment_kind: Option<u8>,
    pub allowed_characters: u16,
    pub stack_limit: u8,
}

pub const DEFAULT_ITEM_STACK_LIMIT: u8 = 20;
impl ItemDefinition {
    pub fn fits_slot(&self, character: usize, slot: usize) -> bool {
        // Persistent order: weapon, body, head, two accessories, arm.
        const SLOT_KINDS: [u8; 6] = [0, 1, 2, 4, 4, 3];
        SLOT_KINDS
            .get(slot)
            .is_some_and(|&kind| self.equipment_kind == Some(kind))
            && character < u16::BITS as usize
            && self.allowed_characters & (1 << character) != 0
    }
}

/// Key items are unique; ordinary inventory uses the base-game twenty-item cap.
pub const fn item_stack_limit(category: u8) -> u8 {
    if category == 45 {
        1
    } else {
        DEFAULT_ITEM_STACK_LIMIT
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterDefinition {
    #[serde(default, skip_serializing_if = "zeroes")]
    pub cooking: [u8; crate::menu_data::RECIPE_COUNT],
    #[serde(default, skip_serializing_if = "zeroes")]
    pub ex_skills: [u8; 4],
    #[serde(default, skip_serializing_if = "zeroes")]
    pub ex_gems: [u8; 4],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_ex_skills: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_compound_ex_skills: Vec<u8>,
    #[serde(default, skip_serializing_if = "neutral_technique")]
    pub technique_balance: i8,
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

fn neutral_technique(value: &i8) -> bool {
    *value == 0
}

fn zeroes<const N: usize>(values: &[u8; N]) -> bool {
    values.iter().all(|&v| v == 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatGrowth {
    pub base: u8,
    pub random: u8,
    pub title_bonus: u8,
}

impl SessionData {
    /// Bind and admit the shared rules before constructing a party or loading a save.
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let rules = crate::menu_data::MenuData::load(files)?;
        rules.validate_gameplay()?;
        let mut data: Self = files.json("game/session-data.json")?;
        data.rules = Some(std::sync::Arc::new(rules));
        data.validate()?;
        Ok(data)
    }

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
        for (index, character) in self.characters.iter().enumerate() {
            ensure!(
                character.overlimit <= 100,
                "initial Over Limit percentage exceeds 100"
            );
            ensure!(
                character.ex_gems.iter().all(|&level| level <= 5)
                    && character
                        .ex_gems
                        .iter()
                        .zip(character.ex_skills)
                        .all(|(&level, skill)| level != 0 || skill == 0)
                    && character
                        .recent_compound_ex_skills
                        .iter()
                        .all(|id| character.compound_ex_skills.contains(id)),
                "invalid initial EX skill state"
            );
            if character.ex_skills.iter().any(|&id| id != 0)
                || !character.compound_ex_skills.is_empty()
            {
                let rules = &self
                    .rules
                    .as_ref()
                    .context("initial EX skills require prepared rules")?
                    .ex_skills;
                let choices = rules
                    .characters
                    .get(index)
                    .context("missing initial character EX rules")?;
                ensure!(
                    character
                        .ex_skills
                        .iter()
                        .all(|&id| id == 0 || rules.skills.contains_key(&id))
                        && character
                            .compound_ex_skills
                            .iter()
                            .all(|&id| usize::from(id) < choices.compounds.len()),
                    "invalid initial EX skill reference"
                );
            }
            ensure!(
                character.cooking.iter().all(|&v| v <= 8),
                "invalid cooking experience"
            );
            ensure!(
                (-100..=100).contains(&character.technique_balance),
                "invalid technique balance"
            );
            ensure!(
                character.level > 0 && usize::from(character.level) < self.experience.len(),
                "invalid initial level"
            );
            ensure!(
                character
                    .equipment
                    .iter()
                    .enumerate()
                    .all(|(slot, &id)| id == 0
                        || self
                            .items
                            .get(usize::from(id))
                            .is_some_and(|item| item.fits_slot(index, slot))
                            && self
                                .rules
                                .as_ref()
                                .is_none_or(|rules| rules.items.get(usize::from(id)).is_some())),
                "initial equipment is missing, unprepared or ineligible"
            );
            ensure!(
                character
                    .techniques
                    .iter()
                    .chain(character.level_techniques.values().flatten())
                    .all(|id| character.allowed_techniques.contains(id)),
                "character technique is outside its catalogue"
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

#[test]
fn character_techniques_use_their_allowed_catalogue() -> Result<()> {
    use serde_json::json;
    let mut session: SessionData = serde_json::from_value(json!({
        "version":1, "executable_sha256":"0".repeat(64), "experience":[0,0],
        "items":[{"equipment_kind":null,"allowed_characters":0,"stack_limit":1}],
        "characters":vec![json!({
            "affinity":0,"level":1,"experience":0,"base_stats":[1,0,0,0,0,0,0],
            "luck":0,"overlimit":0,"equipment":vec![0;6],"shortcuts":vec![0;4],
            "techniques":[300],"allowed_techniques":[300],"level_techniques":{"1":[300]},
            "growth":vec![json!({"base":0,"random":0,"title_bonus":0});7]
        });9]
    }))?;
    session.validate()?;
    session.characters[0].techniques.push(301);
    assert!(session.validate().is_err());
    session.characters[0].techniques.clear();
    session.characters[0].level_techniques.insert(1, vec![301]);
    assert!(session.validate().is_err());
    Ok(())
}
