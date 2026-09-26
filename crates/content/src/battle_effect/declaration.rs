//! Original effect declarations, independent of runtime controller support.
use serde::{Deserialize, Serialize};

use crate::source::FloatOperand as Number;

pub type Vector = [Number; 3];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Declaration {
    pub prefix: Prefix,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefix {
    pub kind: u8,
    pub blend: u8,
    pub resource_slot: u8,
    pub palettes: [u8; 2],
    pub retained_slot: i8,
    pub palette_stride: u8,
    pub reserved_header: u8,
    pub uv: [i16; 4],
    pub lifetime: i16,
    pub geometry_count: u8,
    pub geometry_phase: u8,
    pub flags_or_shake_amplitude: u32,
    pub colors: [[i16; 4]; 2],
    pub brighten: [u8; 4],
    pub darken: [u8; 4],
    pub brighten_until: u8,
    pub darken_from: u8,
    /// Native signed selector; only -1 disables the track.
    pub uv_track: i8,
    pub copy_axis_or_phase_period: u8,
    pub position: Vector,
    pub velocity: Vector,
    pub acceleration: Vector,
    pub angles: Vector,
    pub angular_velocity: Vector,
    pub acceleration_change_or_segment_offset: Vector,
    pub reserved_motion: [u8; 8],
    pub angle_step: Number,
    pub dimension_acceleration_until: i16,
    pub reserved_dimensions: [u8; 2],
    pub bone: u8,
    pub texture_frame_or_joint_slot: u8,
    pub additional_copies: u8,
    pub copy_rotation: u8,
    pub reserved_secondary: u8,
    pub secondary: Secondary,
    pub reserved_attachment: [u8; 2],
    pub local_offset: Vector,
    pub local_velocity: Vector,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Secondary {
    pub flags: u8,
    pub periodic_program: u8,
    pub period: u8,
    pub animation_change_age: u8,
    pub ground_program: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
// Nearly every declaration uses the particle body. Keep its data inline instead
// of allocating each geometry separately while loading the source bank.
#[allow(clippy::large_enum_variant)]
pub enum Body {
    Particle {
        geometry: GeometryOperands,
        context: ContextSeed,
    },
    Camera {
        elevation: Number,
        reserved_elevation: [u8; 4],
        distance: Number,
        storage: Vec<u8>,
    },
    Caption {
        /// Text storage, including terminator and unused suffix.
        text_storage: Vec<u8>,
    },
    Opaque {
        storage: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeometryOperands {
    Parameters {
        dimensions: Dimensions,
        radius_step: Number,
        /// Signed words used as a screen-texture offset by resource slot 10.
        texture_offset_words: [i32; 2],
        storage: Vec<u8>,
        stored_elevation: Number,
    },
    Model {
        dimensions: Dimensions,
        index: u8,
        animation: u8,
        stored_animation_change: u8,
        reserved_animation: u8,
        /// Replaced when the model animation is initialized.
        animation_state: Vec<u8>,
        stored_model_binding: u32,
        elevation: Number,
    },
    VertexQuad {
        vertices: [Vector; 4],
        velocities: [Vector; 4],
        storage: Vec<u8>,
        stored_elevation: Number,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimensions {
    pub value: Vector,
    pub velocity: Vector,
    pub acceleration: Vector,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSeed {
    /// Replaced by caller context when an effect is constructed.
    pub stored_origin: Vector,
    pub stored_uv_binding: u32,
    pub stored_heading: Number,
    pub stored_program_binding: u32,
}

impl Declaration {
    /// Admit a declaration to the common motion controller. This does not prepare
    /// its textures/models, which belong to the presentation resource loader.
    pub fn particle(
        &self,
        uv_track: &[super::UvRecord],
    ) -> anyhow::Result<super::ParticleTemplate> {
        use super::{ParticleGeometry, ParticleState, ParticleTemplate};
        use anyhow::{bail, ensure};
        let p = &self.prefix;
        ensure!(
            matches!(p.kind, 3 | 4 | 5 | 7 | 8 | 10 | 11 | 12 | 15 | 18),
            "particle controller {} is not prepared",
            p.kind
        );
        let flags = p.flags_or_shake_amplitude;
        if p.kind == 3 {
            ensure!(
                ((flags == 0x20 && p.resource_slot == 6)
                    || (flags & !0x404 == 0 && matches!(p.resource_slot, 0 | 2)))
                    && p.uv_track == -1,
                "model particle requires an unprepared binding or attachment"
            );
        }
        ensure!(
            flags & (0x2000_0000 | 0x0800_0000 | 0x8000 | 0x80) == 0
                && p.secondary.flags & !(if p.kind == 3 { 2 } else { 0 }) == 0,
            "particle requires an unprepared attachment or child controller"
        );
        ensure!(
            p.uv_track >= -1 && (p.uv_track == -1) == uv_track.is_empty(),
            "particle UV track binding does not match declaration"
        );
        let Body::Particle { geometry, .. } = &self.body else {
            bail!("particle controller has incompatible declaration");
        };
        let vector = |v: Vector| v.map(|value| f32::from_bits(value.bits()));
        let geometry = match geometry {
            GeometryOperands::Model { dimensions, .. } if p.kind == 3 => ParticleGeometry::Size {
                value: vector(dimensions.value),
                velocity: vector(dimensions.velocity),
                acceleration: vector(dimensions.acceleration),
            },
            GeometryOperands::Parameters { dimensions, .. } if p.kind == 8 => {
                let [height, width, radius] = vector(dimensions.value);
                ParticleGeometry::BillboardTrail {
                    size: [height, width],
                    radius,
                    radius_velocity: vector(dimensions.velocity)[2],
                    segment_size_step: vector(dimensions.acceleration),
                    segment_offset: vector(p.acceleration_change_or_segment_offset),
                    segment_angle_step: f32::from_bits(p.angle_step.bits()),
                    steps_per_segment: p.geometry_phase,
                }
            }
            GeometryOperands::Parameters { dimensions, .. } if p.kind == 18 => {
                ParticleGeometry::Ribbon {
                    length: vector(dimensions.value)[0],
                    width: vector(dimensions.value)[1],
                    jitter: vector(dimensions.velocity)[1],
                    phase: p.geometry_phase,
                    phase_period: p.copy_axis_or_phase_period as i8,
                }
            }
            GeometryOperands::VertexQuad {
                vertices,
                velocities,
                ..
            } if p.kind == 15 => ParticleGeometry::Quad {
                vertices: vertices.map(vector),
                velocity: velocities.map(vector),
            },
            GeometryOperands::Parameters { dimensions, .. } if p.kind != 15 => {
                ParticleGeometry::Size {
                    value: vector(dimensions.value),
                    velocity: vector(dimensions.velocity),
                    acceleration: vector(dimensions.acceleration),
                }
            }
            _ => bail!("particle controller has incompatible geometry"),
        };
        let data = ParticleTemplate {
            lifetime: p.lifetime,
            late: flags & 0x100 != 0,
            follow_origin: flags & 0x400 != 0,
            draw_after_target: flags & 0x202000 != 0,
            // Original REL rodata2104 is 0xbf266666 (-0.65).
            ground_restitution: (flags & 1 != 0).then_some(0.65),
            element_tint: flags & 0x400000 != 0,
            state: ParticleState {
                cull_back: flags & 0x1000_0000 != 0,
                offset: vector(p.position),
                velocity: vector(p.velocity),
                acceleration: vector(p.acceleration),
                angles: vector(p.angles),
                angular_velocity: vector(p.angular_velocity),
                orbit: vector(p.local_offset),
                geometry,
                colors: p.colors,
                brighten: p.brighten,
                brighten_until: p.brighten_until,
                uv: p.uv,
                palettes: p.palettes,
                geometry_count: p.geometry_count,
            },
            // 403F4 excludes 8 and 10: +0x70 is a rendered segment offset.
            jerk: if matches!(p.kind, 8 | 10) {
                [0.; 3]
            } else {
                vector(p.acceleration_change_or_segment_offset)
            },
            orbit_velocity: vector(p.local_velocity),
            linear_orbit: p.kind == 5,
            geometry_acceleration_until: p.dimension_acceleration_until,
            gradient: flags & 8 != 0,
            fade: p.darken,
            fade_from: p.darken_from,
            uv_track: uv_track.to_vec(),
        };
        data.validate()?;
        Ok(data)
    }
}
