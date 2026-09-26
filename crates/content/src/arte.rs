//! Original technique and learning records shared by menus and battle loading.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "game/techniques.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub native_id: i16,
    pub storage02: u16,
    pub auxiliary_text: Option<String>,
    pub tp_cost: u8,
    pub storage09: [u8; 3],
    pub description: Option<String>,
    pub name: Option<String>,
    pub menu_category: u8,
    pub element: u8,
    pub target_preference: u8,
    pub learning_route: u8,
    pub learning_parent: i16,
    pub technical_successor: i16,
    pub strike_successor: i16,
    pub mutually_exclusive: [i16; 4],
    pub required_learned: [i16; 4],
    pub forbidden_learned: [i16; 2],
    pub storage32: u16,
    pub flags: u32,
    pub cast_time_adjustment: i16,
    pub recovery_ticks: i16,
    pub required_uses: u16,
    pub required_level: u16,
    pub target_condition_mask: u64,
    pub storage48: [u8; 3],
    pub unison_altitude: u8,
    pub unison_distance: i16,
    pub unison_duration: i16,
    pub skill_archive_index: u32,
    pub storage54: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub definitions: Vec<Definition>,
    pub learning: Vec<LearningList>,
    pub combinations: Vec<Combination>,
    pub learning_storage: [u8; 5],
}

impl Catalogue {
    pub fn definition(&self, id: usize) -> Result<&Definition> {
        self.definitions
            .get(id)
            .context("arte ID outside catalogue")
    }

    pub fn learned_by(&self, character: u8) -> Result<&[u8]> {
        self.learning
            .get(usize::from(
                character.checked_sub(1).context("zero character ID")?,
            ))
            .context("character has no learning list")?
            .active()
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.definitions.len() == crate::menu_data::TECHNIQUE_COUNT
                && self.learning.len() == 11
                && self.combinations.len() == 20,
            "incomplete arte catalogue"
        );
        for list in &self.learning {
            list.active()?;
        }
        ensure!(
            self.combinations.iter().all(
                |row| row.participant_count <= 4 && row.camera_pitch_offset_degrees.is_finite()
            ),
            "invalid Unison combination"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningList {
    pub count: u8,
    pub technique_slots: Vec<u8>,
}

impl LearningList {
    pub fn active(&self) -> Result<&[u8]> {
        ensure!(
            self.technique_slots.len() == 40,
            "invalid learning slot capacity"
        );
        self.technique_slots
            .get(..usize::from(self.count))
            .context("learning count exceeds slot capacity")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Combination {
    pub name: Option<String>,
    pub native_id: i16,
    pub participant_count: u16,
    pub recipe_slots: [[i16; 4]; 6],
    pub duration_ticks: i16,
    pub storage: [u8; 2],
    pub camera_pitch_offset_degrees: f32,
}
