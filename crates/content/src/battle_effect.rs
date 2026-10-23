//! Effect recipes and timeline inputs prepared for battle loading.
pub mod declaration;
pub mod timeline;
pub mod visual;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use timeline::*;

pub const COMMON_PATH: &str = "battle/effects/common.json";
pub const TECHNIQUES_PATH: &str = "battle/effects/techniques.json";
pub const TINTS_PATH: &str = "battle/effects/tints.json";

/// Effect palettes, element colors and actor tint requests. Effect
/// emission uses RGB; actor requests copy all four color bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tints {
    pub palettes: [u8; 10],
    pub colors: [[u8; 4]; 10],
    /// Actor colors selected by shared combat feedback.
    pub actors: ActorColors,
    /// Common hit feedback selected by neutral or elemental impact.
    pub contact_effects: [u8; 10],
    pub contact_colors: [[u8; 4]; 10],
    /// Flash colors for enabled action families.
    pub admission_colors: AdmissionColors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorColors {
    pub recovery: [u8; 4],
    pub buff: [u8; 4],
    pub scan: [u8; 4],
    pub debuff: [u8; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionColors {
    pub basic: [u8; 4],
    pub advanced: [u8; 4],
    pub arcane: [u8; 4],
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EffectTint {
    pub enabled: bool,
    pub palette: u8,
    pub rgb: [u8; 3],
}

impl Tints {
    pub fn effect(&self, element: usize) -> Option<EffectTint> {
        let [r, g, b, _] = *self.colors.get(element)?;
        Some(EffectTint {
            enabled: element != 0,
            palette: self.palettes[element],
            rgb: [r, g, b],
        })
    }
}

/// Cooked recipes and schedules. Unsupported members remain cold until an
/// encounter requests them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBank {
    pub source_sha256: String,
    pub programs: Vec<Vec<ScheduledEvent>>,
    pub actors: Vec<declaration::Declaration>,
    /// Shared presentation dependencies; unused members stay cold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art: Option<Art>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Art {
    pub source_sha256: String,
    /// Native texture bank slot, then native texture index, then palette page.
    pub textures: BTreeMap<u8, Vec<EffectTexture>>,
    /// Effect model slots retained from the asset catalogue.
    pub models: BTreeMap<u8, crate::battle_model::ModelPart>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectTexture {
    pub images: Vec<crate::font::UiTexture>,
    pub sampler: crate::texture::Sampler,
    /// A binding selects a set; each palette selector resolves to a prepared image.
    pub palette_sets: Vec<Vec<Option<usize>>>,
}

impl EffectTexture {
    pub fn palette_image(&self, set: u8, selector: u8) -> Result<usize> {
        let image = self
            .palette_sets
            .get(usize::from(set))
            .and_then(|set| set.get(usize::from(selector)))
            .copied()
            .flatten()
            .context("effect palette selector is not prepared")?;
        ensure!(image < self.images.len(), "effect palette image is missing");
        Ok(image)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UvAnimation {
    Frames {
        frames: Vec<UvFrame>,
        /// Repeat from this frame, or hold the final frame when absent.
        loop_start: Option<usize>,
    },
    Scroll {
        origin: [i16; 2],
        step: [i16; 2],
        interval: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UvFrame {
    pub duration: u32,
    pub change: UvChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UvChange {
    Rectangle { rect: [i16; 4] },
    Palette { index: u8 },
}

/// Each model particle owns its playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBinding {
    pub slot: u8,
    pub animation: Option<u16>,
    pub repeat: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_at: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub program_animation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<ModelAttachment>,
}

/// Follow a joint on an earlier retained model in the same effect program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttachment {
    pub retained: u8,
    pub joint: u8,
}

use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParticleGeometry {
    /// Sprites sampled along an orbit. Segment steps affect drawing, not time.
    BillboardTrail {
        size: [f32; 2],
        radius: f32,
        radius_velocity: f32,
        segment_size_step: [f32; 3],
        segment_offset: [f32; 3],
        segment_angle_step: f32,
        steps_per_segment: u8,
    },
    /// Segmented ribbon; its shape samples are selected by phase at drawing.
    /// Length, width and jitter stay fixed during common particle updates.
    Ribbon {
        length: f32,
        width: f32,
        jitter: f32,
        phase: u8,
        phase_period: i8,
    },
    /// A segmented spiral with ordinary dimension integration. Modifiers may
    /// reverse its per-segment angle independently of its angular velocity.
    Spiral {
        value: [f32; 3],
        velocity: [f32; 3],
        acceleration: [f32; 3],
        segment_angle_step: f32,
    },
    Size {
        value: [f32; 3],
        velocity: [f32; 3],
        acceleration: [f32; 3],
    },
    Quad {
        vertices: [[f32; 3]; 4],
        velocity: [[f32; 3]; 4],
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleState {
    /// Birth overrides for the authored spiral segment displacement.
    #[serde(default, skip_serializing_if = "segment_offset_unset")]
    pub segment_offset: [Option<f32>; 3],
    /// Constructor modifier override; absent values retain the prepared declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<u8>,
    pub cull_back: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ribbon_depth: bool,
    pub offset: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angles: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub orbit: [f32; 3],
    pub geometry: ParticleGeometry,
    pub colors: [[i16; 4]; 2],
    pub brighten: [u8; 4],
    pub brighten_until: Option<u32>,
    pub uv: [i16; 4],
    pub palettes: [u8; 2],
    pub geometry_count: u8,
    /// Repeat this billboard at the current positions of the owner's tagged bones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_group: Option<u8>,
}

fn segment_offset_unset(value: &[Option<f32>; 3]) -> bool {
    value.iter().all(Option::is_none)
}

/// Detached child emission driven by the parent particle's ordinary age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodicEffect {
    pub member: u16,
    pub interval: u8,
}

/// Particle motion parameters; rendering resources are bound at loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleTemplate {
    /// Advance while a cinematic holds ordinary battle effects.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub during_pause: bool,
    pub lifetime: Option<u32>,
    /// Integrate and draw in the current pose of this owner bone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_bone: Option<u8>,
    /// Follow this bone's world position while retaining independent particle motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_bone: Option<u8>,
    /// Refresh the origin from the effect's attachment on each particle visit.
    pub follow_origin: bool,
    /// Face successive followed positions instead of integrating angular motion.
    #[serde(default)]
    pub follow_orientation: bool,
    /// Preserve authored angles and orbit instead of advancing their motion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed_orientation: bool,
    /// Move the emission origin; local displacement remains independent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub origin_motion: bool,
    /// Add this declaration value to the model's world Y coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_elevation: Option<f32>,
    /// Model height is local to the ground rather than the emission origin.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub model_ground_relative: bool,
    /// Submit this particle in its selected actor's ordered drawing lists.
    /// The visual recipe selects the side of the actor drawing pass.
    pub draw_after_target: bool,
    /// Reflect vertical velocity at the floor after integration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_restitution: Option<f32>,
    /// Keep the particle above the floor without changing its downward motion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ground_clamp: bool,
    /// Emit this member from the same bank at each floor bounce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounce_effect: Option<u16>,
    /// Emit once on crossing the floor without reflecting or clamping motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_effect: Option<u16>,
    /// Emit a detached member on the first particle visit and each interval thereafter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periodic_effect: Option<PeriodicEffect>,
    pub element_tint: bool,
    pub state: ParticleState,
    pub jerk: [f32; 3],
    pub orbit_velocity: [f32; 3],
    /// Integrate the local offset as XYZ rather than advancing its angle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub linear_orbit: bool,
    /// None keeps geometry acceleration active for the full lifetime.
    pub geometry_acceleration_until: Option<u32>,
    pub gradient: bool,
    pub fade: [u8; 4],
    pub fade_from: u32,
    pub uv_animation: Option<UvAnimation>,
}

impl ParticleState {
    pub fn finite(&self) -> bool {
        let finite = |v: &[f32]| v.iter().all(|v| v.is_finite());
        finite(&self.offset)
            && finite(&self.velocity)
            && finite(&self.acceleration)
            && finite(&self.angles)
            && finite(&self.angular_velocity)
            && self.segment_offset.iter().flatten().all(|v| v.is_finite())
            && finite(&self.orbit)
            && match &self.geometry {
                ParticleGeometry::BillboardTrail {
                    size,
                    radius,
                    radius_velocity,
                    segment_size_step,
                    segment_offset,
                    segment_angle_step,
                    ..
                } => {
                    finite(size)
                        && finite(&[*radius, *radius_velocity, *segment_angle_step])
                        && finite(segment_size_step)
                        && finite(segment_offset)
                }
                ParticleGeometry::Ribbon {
                    length,
                    width,
                    jitter,
                    ..
                } => finite(&[*length, *width, *jitter]),
                ParticleGeometry::Size {
                    value,
                    velocity,
                    acceleration,
                } => finite(value) && finite(velocity) && finite(acceleration),
                ParticleGeometry::Spiral {
                    value,
                    velocity,
                    acceleration,
                    segment_angle_step,
                } => {
                    finite(value)
                        && finite(velocity)
                        && finite(acceleration)
                        && segment_angle_step.is_finite()
                }
                ParticleGeometry::Quad { vertices, velocity } => {
                    vertices.iter().chain(velocity).all(|v| finite(v))
                }
            }
    }
}

impl ParticleTemplate {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.periodic_effect.is_none_or(|effect| effect.interval > 0
                && self.bounce_effect.is_none()
                && self.floor_effect.is_none()),
            "periodic effect requires a positive interval without floor emission"
        );
        ensure!(
            self.follow_bone.is_none()
                || (self.owner_bone.is_none()
                    && !self.follow_origin
                    && !self.follow_orientation
                    && !self.origin_motion
                    && self.ground_restitution.is_none()
                    && !self.ground_clamp
                    && self.floor_effect.is_none()),
            "bone position follow conflicts with another particle attachment"
        );
        ensure!(
            !self.ground_clamp || (self.ground_restitution.is_none() && !self.follow_origin),
            "floor clamp requires independent motion without restitution"
        );
        ensure!(
            self.bounce_effect.is_none()
                || (self.ground_restitution.is_some()
                    && !self.follow_origin
                    && self.owner_bone.is_none()),
            "bounce effect requires independent floor motion"
        );
        ensure!(
            self.floor_effect.is_none()
                || (!self.follow_origin
                    && self.owner_bone.is_none()
                    && self.ground_restitution.is_none()
                    && !self.ground_clamp),
            "floor effect requires independent motion without reflection"
        );
        ensure!(
            !(self.fixed_orientation && self.follow_orientation),
            "conflicting particle orientation"
        );
        ensure!(
            self.state.effect_group.is_none_or(|group| group < 16)
                && (self.state.effect_group.is_none() || self.owner_bone.is_none()),
            "invalid particle effect group"
        );
        ensure!(
            self.owner_bone.is_none()
                || (!self.follow_origin
                    && !self.follow_orientation
                    && self.model_elevation.is_none()
                    && !self.ground_clamp
                    && self.ground_restitution.is_none()),
            "bone particle has incompatible world motion"
        );
        ensure!(
            [
                self.lifetime,
                self.geometry_acceleration_until,
                self.state.brighten_until
            ]
            .into_iter()
            .all(|duration| duration.is_none_or(|duration| duration > 0)),
            "zero particle duration"
        );
        ensure!(
            self.state.blend.is_none_or(|blend| blend <= 1),
            "particle blend override is not prepared"
        );
        ensure!(
            self.state.finite()
                && self.model_elevation.is_none_or(f32::is_finite)
                && (!self.follow_orientation || self.follow_origin)
                && self
                    .ground_restitution
                    .is_none_or(|v| v.is_finite() && v >= 0.)
                && self
                    .jerk
                    .iter()
                    .chain(&self.orbit_velocity)
                    .all(|v| v.is_finite()),
            "invalid particle transform"
        );
        match &self.uv_animation {
            Some(UvAnimation::Frames { frames, loop_start }) => {
                ensure!(!frames.is_empty(), "empty particle UV animation");
                ensure!(
                    loop_start.is_none_or(|start| start < frames.len()),
                    "particle UV loop outside animation"
                );
                for frame in frames {
                    ensure!(frame.duration > 0, "zero particle UV frame duration");
                    if let UvChange::Rectangle { rect } = frame.change {
                        ensure!(rect[2] >= 0 && rect[3] >= 0, "negative particle UV extent");
                    }
                }
            }
            Some(UvAnimation::Scroll {
                origin, interval, ..
            }) => {
                ensure!(*interval > 0, "zero particle UV scroll interval");
                for (&origin, &extent) in origin.iter().zip(&self.state.uv[2..]) {
                    ensure!(extent > 0, "nonpositive particle UV scroll extent");
                    ensure!(
                        i32::from(origin) + i32::from(extent) - 1 <= i32::from(i16::MAX),
                        "particle UV scroll exceeds coordinate range"
                    );
                }
            }
            None => {}
        }
        Ok(())
    }
}

impl SourceBank {
    /// Borrow one admitted particle without copying its animation or geometry.
    pub fn particle(&self, member: usize) -> Result<&ParticleTemplate> {
        let template = self
            .actors
            .get(member)
            .context("unbound effect particle")?
            .template()?;
        template.validate()?;
        Ok(template)
    }

    /// Inspect only the timeline and recipes selected by this member.
    pub fn program(&self, member: usize) -> Result<&[ScheduledEvent]> {
        let timeline = self
            .programs
            .get(member)
            .with_context(|| format!("missing effect timeline {member}"))?;
        for event in timeline {
            match &event.operation {
                EffectOperation::Unsupported { reason } => anyhow::bail!("{reason}"),
                EffectOperation::Spawn {
                    particle, birth, ..
                } => {
                    self.particle(usize::from(*particle))
                        .with_context(|| format!("effect member {member} particle {particle}"))?;
                    birth.validate()?;
                }
                _ => {}
            }
        }
        Ok(timeline)
    }
}
