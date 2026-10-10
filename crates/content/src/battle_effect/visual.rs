//! Render choices resolved while importing an effect.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleVisual {
    pub geometry: ParticleShape,
    pub orientation: Orientation,
    pub texture: ParticleTexture,
    pub blend: u8,
    pub depth_test: bool,
    pub depth_write: bool,
    pub ground_relative: bool,
    pub anchored: bool,
    pub copies: u8,
    pub copy_rotation: u8,
    pub after_actor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    World,
    Billboard,
    Camera,
}

/// Texture-panel layout, independent of the geometry's tessellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UvLayout {
    Repeat,
    Advance,
    /// Wrap after the live column count; zero leaves the sequence unbounded.
    Cycle,
    /// Split each live column in half, using one full column when the count is zero.
    HalfWidthCycle,
    AlternatingHalves {
        panels_per_half: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParticleShape {
    Quad,
    Ring {
        segments: u8,
        flared: bool,
        hidden: bool,
        uv_layout: UvLayout,
    },
    Orbit,
    BillboardTrail,
    Spiral {
        radius_step: f32,
        segment_offset: [f32; 3],
        steps_per_segment: u8,
        uv_layout: UvLayout,
    },
    Disc {
        segments: u8,
    },
    Shell {
        segments: u8,
        elliptical: bool,
        uv_layout: UvLayout,
    },
    Sphere {
        columns: u8,
    },
    VertexQuad {
        copy_axis: u8,
        local_copies: bool,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParticleTexture {
    Untextured,
    Atlas {
        slot: u8,
        dual: bool,
        palette_set: u8,
    },
}

impl ParticleTexture {
    pub fn slot(self) -> Option<u8> {
        match self {
            Self::Untextured => None,
            Self::Atlas { slot, .. } => Some(slot),
        }
    }

    pub fn dual(self) -> bool {
        matches!(self, Self::Atlas { dual: true, .. })
    }

    pub fn palette_set(self) -> u8 {
        match self {
            Self::Untextured => 0,
            Self::Atlas { palette_set, .. } => palette_set,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVisual {
    pub blend: u8,
    pub lit: bool,
    pub depth_test: bool,
    pub cull_back: bool,
    pub before_actor: bool,
}
