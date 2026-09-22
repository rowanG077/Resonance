use super::{WORLD_DEPTH, WORLD_WIDTH};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU16,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interaction {
    Disabled,
    Active,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Marker {
    None,
    Model { id: u8 },
    FieldPoint,
    Unmodeled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Landmark {
    /// World-event registry key: Sylvarant 1..98, Tethe'alla 257..337.
    pub id: u16,
    pub position: [f32; 2],
    /// None follows terrain; Some(0) is an authored sea-level placement.
    pub height: Option<f32>,
    pub radius: f32,
    pub interaction: Interaction,
    pub marker: Marker,
    pub name: String,
    pub automatic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Landmarks {
    /// Source order is significant when contact radii overlap.
    pub worlds: [Vec<Landmark>; 2],
    pub item_rewards: BTreeMap<u16, u16>,
    pub party_requirements: BTreeMap<u16, u8>,
}
impl Landmarks {
    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        for (world, entries) in self.worlds.iter().enumerate() {
            ensure!(!entries.is_empty(), "world has no landmarks");
            for (index, entry) in entries.iter().enumerate() {
                ensure!(
                    usize::from(entry.id) == world * 256 + index + 1
                        && index < if world == 0 { 98 } else { 81 },
                    "invalid world landmark order"
                );
                ensure!(
                    (0.0..WORLD_WIDTH).contains(&entry.position[0])
                        && (0.0..WORLD_DEPTH).contains(&entry.position[1])
                        && entry.height.is_none_or(f32::is_finite)
                        && entry.radius.is_finite()
                        && entry.radius >= 0.
                        && entry.radius < 3200.
                        && !entry.name.is_empty(),
                    "invalid world landmark geometry or name"
                );
                if let Marker::Model { id } = entry.marker {
                    ensure!((1..=17).contains(&id), "invalid landmark model");
                }
                ensure!(ids.insert(entry.id), "duplicate landmark");
            }
        }
        ensure!(
            self.item_rewards
                .iter()
                .all(|(id, item)| ids.contains(id) && *item < 528)
                && self
                    .party_requirements
                    .iter()
                    .all(|(id, character)| ids.contains(id) && (1..=9).contains(character)),
            "invalid landmark reward or party requirement"
        );
        Ok(())
    }
}

/// Region guideposts retain the original published long-range-unlocks format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guidepost {
    pub name: String,
    pub name_id: usize,
    pub location: u16,
    /// The first flag also records discovery; empty slots have no effect.
    pub event_flags: [Option<NonZeroU16>; 3],
}
impl Guidepost {
    pub fn validate(&self, landmarks: &Landmarks) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.event_flags[0].is_some()
                && self.event_flags.iter().flatten().all(|v| v.get() < 4096)
                && landmarks
                    .worlds
                    .iter()
                    .flatten()
                    .any(|l| l.id == self.location),
            "invalid world guidepost"
        );
        Ok(())
    }
}
