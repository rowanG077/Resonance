//! Render choices resolved while importing an effect.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleVisual {
    pub geometry: ParticleShape,
    pub orientation: Orientation,
    pub texture: ParticleTexture,
    /// Sample the completed scene through the texture's opacity mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_sample: Option<[i8; 2]>,
    #[serde(with = "super::labels::blend")]
    pub blend: u8,
    #[serde(
        default = "crate::defaults::enabled",
        skip_serializing_if = "crate::defaults::is_enabled"
    )]
    pub depth_test: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub depth_write: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub ground_relative: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub anchored: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub copies: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub copy_rotation: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub after_actor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    World,
    FixedWorld,
    FollowMotion,
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
    NoDraw,
    Quad,
    /// Three strips between concentric equilateral triangles.
    TriangleBand,
    Ring {
        segments: u8,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        flared: bool,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        hidden: bool,
        uv_layout: UvLayout,
    },
    Orbit,
    RadialQuads {
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        flat: bool,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        advance_u: bool,
    },
    BillboardTrail,
    Ribbon {
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        depth: bool,
    },
    Spiral {
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        radius_step: f32,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        segment_offset: [f32; 3],
        steps_per_segment: u8,
        uv_layout: UvLayout,
    },
    Disc {
        segments: u8,
    },
    Shell {
        segments: u8,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        elliptical: bool,
        uv_layout: UvLayout,
    },
    Sphere {
        columns: u8,
    },
    Hemisphere {
        columns: u8,
    },
    VertexQuad {
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        copy_axis: u8,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
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
        #[serde(with = "super::labels::atlas")]
        slot: u8,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        dual: bool,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
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
    #[serde(with = "super::labels::blend")]
    pub blend: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub lit: bool,
    #[serde(
        default = "crate::defaults::enabled",
        skip_serializing_if = "crate::defaults::is_enabled"
    )]
    pub depth_test: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub cull_back: bool,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub before_actor: bool,
}
