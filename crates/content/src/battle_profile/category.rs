use serde::{Deserialize, Serialize};

/// Actor category matched by species-specific weapon bonuses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Species {
    #[default]
    Unknown,
    Plant,
    Undead,
    Beast,
    Magical,
    Machine,
    Human,
    Bird,
    Insect,
    Aquatic,
    Immaterial,
    Dragon,
    Angel,
}

/// Size affects formation spacing, Cyclone resistance and encounter framing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
    Huge,
}

impl Size {
    /// Occupied radius in the recovery-position grid's 100-unit cells.
    pub fn positioning_radius(self) -> u8 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
            Self::Huge => 3,
        }
    }
    pub fn resists_cyclone(self) -> bool {
        matches!(self, Self::Large | Self::Huge)
    }
    pub fn wide_camera(self) -> bool {
        self == Self::Huge
    }
}

/// Vertical hit response, separate from the actor's visual size or body mass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoilWeight {
    #[default]
    Normal,
    Light,
    Heavy,
    Grounded,
}
