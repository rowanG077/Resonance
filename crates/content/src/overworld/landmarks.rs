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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShipCameraClearance {
    /// Distances beyond the landmark's footprint where camera clearance applies.
    pub near_margin: f32,
    pub far_margin: f32,
    pub height: f32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ship_camera_clearance: Option<ShipCameraClearance>,
    pub name: String,
    pub automatic: bool,
    pub entry_overrides: Vec<EntryOverride>,
}

/// Authored field-return placement. Directional perimeter geometry stays native.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryOverride {
    pub direction: Option<u8>,
    pub placement: EntryPlacement,
    pub mount: Option<super::Mount>,
    pub heading: Option<f32>,
    pub camera_yaw: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryPlacement {
    Fixed { position: [f32; 3] },
    LandmarkCenter { height: f32 },
    Perimeter { allow_disabled: bool },
}

impl EntryOverride {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.direction.is_none_or(|direction| direction < 8),
            "invalid entry direction"
        );
        ensure!(
            [self.heading, self.camera_yaw]
                .into_iter()
                .flatten()
                .all(|angle| angle.is_finite() && (0. ..std::f32::consts::TAU).contains(&angle)),
            "invalid entry angle"
        );
        match self.placement {
            EntryPlacement::Fixed { position } => {
                ensure!(
                    (0. ..WORLD_WIDTH).contains(&position[0])
                        && (0. ..WORLD_DEPTH).contains(&position[1])
                        && position[2].is_finite(),
                    "invalid fixed entry position"
                );
            }
            EntryPlacement::LandmarkCenter { height } => {
                ensure!(height.is_finite(), "invalid entry height")
            }
            EntryPlacement::Perimeter { .. } => {}
        }
        Ok(())
    }
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
                if let Some(clearance) = entry.ship_camera_clearance {
                    ensure!(
                        [
                            clearance.near_margin,
                            clearance.far_margin,
                            clearance.height
                        ]
                        .into_iter()
                        .all(|value| value.is_finite() && value > 0.)
                            && clearance.near_margin < clearance.far_margin,
                        "invalid landmark ship camera clearance"
                    );
                }
                ensure!(ids.insert(entry.id), "duplicate landmark");
                ensure!(
                    entry.entry_overrides.len() <= 8,
                    "too many landmark entry overrides"
                );
                let mut directions = BTreeSet::new();
                for rule in &entry.entry_overrides {
                    rule.validate()?;
                    ensure!(
                        directions.insert(rule.direction)
                            && (rule.direction.is_some() || entry.entry_overrides.len() == 1),
                        "ambiguous landmark entry overrides"
                    );
                }
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
