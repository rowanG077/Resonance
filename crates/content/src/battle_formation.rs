//! Active encounter participants and their resource dependencies.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/formations.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Formations {
    pub source_sha256: String,
    pub records: Vec<Formation>,
    pub pools: Vec<Vec<WeightedFormation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightedFormation {
    pub formation: u16,
    pub weight: u16,
}

/// Declared resources include reserves with no initial actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formation {
    pub settings: Settings,
    pub resources: Vec<Resource>,
    pub actors: Vec<Actor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub play_music: bool,
    pub celebrate: bool,
    pub entry_voice: bool,
    pub escape_restricted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub enemy: u16,
    pub hidden_name: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub resource: u8,
    pub variant: u8,
    /// Persistent row of the enemy's body atlas, independent of facial animation.
    #[serde(default)]
    pub texture_variant: u8,
    /// Select an alternate package for the first carried weapon.
    #[serde(default)]
    pub weapon_variant: u8,
    pub position: Option<[i16; 2]>,
    pub unsupported_reason: Option<String>,
}

impl Formation {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.actors
                .iter()
                .all(|actor| usize::from(actor.resource) < self.resources.len()),
            "formation refers to a missing enemy slot"
        );
        Ok(())
    }
}

impl Formations {
    pub fn select(&self, pool: u16, random: u32) -> Result<u16> {
        let entries = self
            .pools
            .get(usize::from(pool))
            .context("unknown encounter pool")?;
        let total: u32 = entries.iter().map(|entry| u32::from(entry.weight)).sum();
        ensure!(total > 0, "empty encounter pool {pool}");
        let mut choice = random % total;
        for entry in entries {
            if choice < u32::from(entry.weight) {
                return Ok(entry.formation);
            }
            choice -= u32::from(entry.weight);
        }
        unreachable!("choice is bounded by the total weight")
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.records.is_empty() && self.records.len() <= usize::from(u16::MAX) + 1,
            "invalid formation catalogue"
        );
        for record in &self.records {
            record.validate()?;
        }
        for pool in &self.pools {
            ensure!(
                !pool.is_empty()
                    && pool.iter().all(|entry| entry.weight > 0
                        && usize::from(entry.formation) < self.records.len()),
                "invalid encounter pool"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_encounters_use_the_actual_total_and_reject_empty_pools() {
        let table = Formations {
            source_sha256: String::new(),
            records: vec![],
            pools: vec![
                vec![
                    WeightedFormation {
                        formation: 7,
                        weight: 60,
                    },
                    WeightedFormation {
                        formation: 9,
                        weight: 50,
                    },
                ],
                vec![],
            ],
        };
        for (random, formation) in [(0, 7), (59, 7), (60, 9), (109, 9), (110, 7)] {
            assert_eq!(table.select(0, random).unwrap(), formation);
        }
        assert!(table.select(1, 0).is_err());
        assert!(table.select(2, 0).is_err());
    }
}
