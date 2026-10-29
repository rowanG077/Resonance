use super::{WORLD_DEPTH, WORLD_WIDTH};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, num::NonZeroU16};

pub use symphonia_constants::WorldInteraction as Interaction;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marker {
    #[default]
    None,
    Model(u8),
    FieldPoint,
    Unmodeled,
}

impl Marker {
    fn is_none(&self) -> bool {
        *self == Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShipCameraClearance {
    /// Distances beyond the landmark's footprint where camera clearance applies.
    pub near_margin: f32,
    pub far_margin: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightFloor {
    /// Extends the landmark footprint by this distance.
    pub margin: f32,
    pub height: f32,
    #[serde(default = "active_only", skip_serializing_if = "is_active_only")]
    pub active_only: bool,
}
fn active_only() -> bool {
    true
}
fn is_active_only(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Landmark {
    /// World-event registry key: Sylvarant 1..255, Tethe'alla 257..511.
    pub id: u16,
    pub position: [f32; 2],
    /// None follows terrain; Some(0) is an authored sea-level placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    pub radius: f32,
    #[serde(default, skip_serializing_if = "Interaction::is_active")]
    pub interaction: Interaction,
    #[serde(default, skip_serializing_if = "Marker::is_none")]
    pub marker: Marker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ship_camera_clearance: Option<ShipCameraClearance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flight_floor: Option<FlightFloor>,
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub map_listed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visit_alias: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shops: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shop_variants: Vec<ShopVariant>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatic: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_overrides: Vec<EntryOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward_item: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_character: Option<PartyCharacter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidepost: Option<Guidepost>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopVariant {
    /// Word index in saved script_globals.
    pub global: usize,
    pub at_least: i32,
    pub shops: Vec<u8>,
}

impl Landmark {
    /// Position on the 384 × 288 world-map image, derived from travel coordinates.
    pub fn map_point(&self) -> [i16; 2] {
        self.position.map(|value| (value / 200.).trunc() as i16)
    }

    pub fn shops(&self, globals: &[i32]) -> &[u8] {
        self.shop_variants
            .iter()
            .rev()
            .find(|v| globals.get(v.global).is_some_and(|&g| g >= v.at_least))
            .map_or(&self.shops, |v| &v.shops)
    }
}

/// Authored field-return placement. Directional perimeter geometry stays native.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<u8>,
    pub placement: EntryPlacement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<super::Mount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[serde(from = "NamedWorlds", into = "NamedWorlds")]
pub struct Landmarks {
    /// Source order is significant when contact radii overlap.
    pub worlds: [Vec<Landmark>; 2],
}
impl Landmarks {
    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        for (world, entries) in self.worlds.iter().enumerate() {
            ensure!(!entries.is_empty(), "world has no landmarks");
            for entry in entries {
                ensure!(
                    usize::from(entry.id / 256) == world && entry.id % 256 != 0,
                    "landmark ID belongs to another world or uses its reserved zero ID"
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
                if let Marker::Model(id) = entry.marker {
                    ensure!((1..=17).contains(&id), "invalid landmark model");
                }
                if let Some(floor) = entry.flight_floor {
                    ensure!(
                        [floor.margin, floor.height]
                            .into_iter()
                            .all(|v| v.is_finite() && v > 0.),
                        "invalid landmark flight floor"
                    );
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
                    entry.reward_item.is_none_or(|item| item < 528),
                    "invalid landmark reward"
                );
                if let Some(post) = &entry.guidepost {
                    post.validate()?;
                }
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
        Ok(())
    }
}

/// A landmark's displayed region and the flags set when it is discovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guidepost {
    #[serde(rename = "region")]
    pub name: String,
    pub discovery_flag: NonZeroU16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlock_flags: Vec<NonZeroU16>,
}
impl Guidepost {
    pub fn flags(&self) -> impl Iterator<Item = u16> + '_ {
        std::iter::once(self.discovery_flag.get())
            .chain(self.unlock_flags.iter().map(|flag| flag.get()))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty() && self.flags().all(|flag| flag < 4096),
            "invalid world guidepost"
        );
        Ok(())
    }
}

pub use symphonia_constants::PartyCharacterId as PartyCharacter;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedWorlds {
    sylvarant: Vec<Landmark>,
    tethe_alla: Vec<Landmark>,
}
impl From<NamedWorlds> for Landmarks {
    fn from(worlds: NamedWorlds) -> Self {
        Self {
            worlds: [worlds.sylvarant, worlds.tethe_alla],
        }
    }
}
impl From<Landmarks> for NamedWorlds {
    fn from(landmarks: Landmarks) -> Self {
        let [sylvarant, tethe_alla] = landmarks.worlds;
        Self {
            sylvarant,
            tethe_alla,
        }
    }
}

#[cfg(test)]
mod tests;
