//! Gameplay meanings shared by editable collision meshes and encounter rules.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainSurface {
    Barrier,
    Grassland,
    Road,
    Wasteland,
    Snowfield,
    Desert,
    Forest,
    DeepForest,
    Beach,
    Water,
    Bridge,
    Pier,
}
impl TerrainSurface {
    pub const ALL: [Self; 12] = [
        Self::Barrier,
        Self::Grassland,
        Self::Road,
        Self::Wasteland,
        Self::Snowfield,
        Self::Desert,
        Self::Forest,
        Self::DeepForest,
        Self::Beach,
        Self::Water,
        Self::Bridge,
        Self::Pier,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    /// Generic collision geometry carries an internal index; files carry names.
    pub fn from_index(index: u32) -> Result<Self> {
        Self::ALL
            .get(index as usize)
            .copied()
            .context("invalid world surface")
    }
    pub fn walkable(self) -> bool {
        !matches!(self, Self::Barrier | Self::Water)
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Barrier => "barrier",
            Self::Grassland => "grassland",
            Self::Road => "road",
            Self::Wasteland => "wasteland",
            Self::Snowfield => "snowfield",
            Self::Desert => "desert",
            Self::Forest => "forest",
            Self::DeepForest => "deep_forest",
            Self::Beach => "beach",
            Self::Water => "water",
            Self::Bridge => "bridge",
            Self::Pier => "pier",
        }
    }
}
