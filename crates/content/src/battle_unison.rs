//! Ordered combination recipes and their presentation timing.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

mod host;
pub use host::{Host, Prelude};

pub const PATH: &str = "battle/unison.json";
pub const COMBINATION_COUNT: usize = 19;
pub const PLACEMENT_COUNT: usize = 253;

#[derive(Debug, Clone, Serialize)]
pub struct Catalogue {
    pub source_sha256: String,
    pub host: Host,
    /// Indexed by party character ID minus one.
    pub openers: Vec<Opener>,
    /// Selection priority follows the authored row order.
    pub combinations: Vec<Combination>,
    /// Indexed by catalogue technique ID, including the unused zero row.
    /// Placement values do not establish whether an arte is usable in Unison.
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opener {
    pub duration: u16,
    pub recovery: u16,
    pub reach: [u16; 2],
    /// The team-attack owner can wait longer than the shared action track.
    pub contact_ticks: u16,
    pub events: Vec<(u16, crate::battle_normal::Event)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub native_id: u16,
    pub altitude: u32,
    /// Planar placement contribution, not the complete actor-to-target distance.
    pub distance: u16,
    /// Authored execution interval; zero remains a value, not an eligibility gate.
    pub duration: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Combination {
    /// Stable row identity; distinct rows can select the same native action.
    pub id: u8,
    pub name: String,
    pub native_id: u16,
    pub participants: u8,
    /// Native arte IDs. An empty list provides no selectable recipe.
    pub recipes: Vec<Vec<u16>>,
    pub duration: u16,
    pub camera_pitch: f32,
}

impl Catalogue {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.combinations.len() == COMBINATION_COUNT
                && self.placements.len() == PLACEMENT_COUNT
                && self.openers.len() == 9,
            "invalid Unison catalogue count"
        );
        self.host.validate()?;
        for opener in &self.openers {
            ensure!(
                opener.duration > 0 && opener.contact_ticks > 0,
                "empty Unison opener"
            );
        }
        for (index, row) in self.combinations.iter().enumerate() {
            ensure!(
                usize::from(row.id) == index + 1
                    && !row.name.is_empty()
                    && (301..=318).contains(&row.native_id)
                    && (2..=4).contains(&row.participants)
                    && row.duration > 0
                    && row.camera_pitch.is_finite()
                    && row.recipes.len() <= 6
                    && row.recipes.iter().all(|recipe| {
                        recipe.len() == usize::from(row.participants) && !recipe.contains(&0)
                    }),
                "invalid Unison combination {}",
                row.id
            );
        }
        Ok(())
    }
}
