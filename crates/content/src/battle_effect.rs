//! Original effect commands, retained as source records rather than executable code.
pub mod declaration;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const COMMON_PATH: &str = "battle/effects/common.json";
pub const TECHNIQUES_PATH: &str = "battle/effects/techniques.json";
pub const TINTS_PATH: &str = "battle/effects/tints.json";

/// Original effect palettes, element colors and actor tint requests. Effect
/// emission uses RGB; actor requests copy all four color bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tints {
    pub source_sha256: String,
    pub palettes: [u8; 10],
    pub colors: [[u8; 4]; 10],
    /// Actor color requests selected by the original common callbacks.
    pub actors: [[u8; 4]; 12],
    /// Common hit feedback selected by neutral/element source indices (3B370).
    pub contact_effects: [u8; 10],
    pub contact_colors: [[u8; 4]; 10],
    /// Enabled action-family flash table used by 1E12C/26980.
    pub admission_colors: [[u8; 4]; 4],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EffectTint {
    pub enabled: bool,
    pub palette: u8,
    pub rgb: [u8; 3],
}

/// Complete source declarations and original command inputs. Controller support
/// is checked only when an encounter requests a member during loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBank {
    pub source_sha256: String,
    pub programs: Vec<Vec<Record>>,
    pub actors: Vec<declaration::Declaration>,
    pub modifiers: BTreeMap<u16, Vec<u16>>,
    pub uv: Vec<UvRecord>,
    /// Original byte offsets in the UV row pool.
    pub uv_roots: Vec<u16>,
    /// Original ordinary-bank presentation dependencies; unused members stay cold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art: Option<Art>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Art {
    pub source_sha256: String,
    /// Native texture bank slot, then native texture index, then palette page.
    pub textures: BTreeMap<u8, Vec<EffectTexture>>,
    /// Common model bank 0 slots. Slot 0 is supplied by Colette's equipped weapon.
    pub models: BTreeMap<u8, crate::battle_model::ModelPart>,
    pub files: BTreeMap<String, crate::field_preload::File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectTexture {
    pub images: Vec<crate::font::UiTexture>,
    pub sampler: crate::texture::Sampler,
    /// Native palette index capacity; 0 for a texture without a palette.
    pub palette_step: u16,
    /// Distance between published palette windows; 0 for direct textures.
    pub page_step: u16,
}

impl EffectTexture {
    pub fn palette_page(&self, index: u8, stride: u8) -> anyhow::Result<usize> {
        use anyhow::ensure;
        if self.palette_step == 0 {
            ensure!(
                self.images.len() == 1 && self.page_step == 0,
                "nonindexed effect texture must have one image and no palette windows"
            );
            return Ok(0);
        }
        ensure!(
            matches!(self.palette_step, 16 | 256 | 16384),
            "unsupported effect palette capacity {}",
            self.palette_step
        );
        // 4AEFC honors a nonzero override; otherwise only CI8 uses 256.
        // The palette pointer moves by selector * stride entries, not pages.
        let stride = if stride != 0 {
            u16::from(stride)
        } else if self.palette_step == 256 {
            256
        } else {
            16
        };
        let offset = usize::from(index) * usize::from(stride);
        let step = usize::from(self.page_step);
        ensure!(
            step != 0 && usize::from(self.palette_step).is_multiple_of(step),
            "invalid published palette page spacing {step} for capacity {}",
            self.palette_step
        );
        ensure!(
            offset.is_multiple_of(step),
            "palette selector {index}, stride {stride}, entry offset {offset} is not represented by published page spacing {step} (capacity {}, {} pages)",
            self.palette_step,
            self.images.len()
        );
        let page = offset / step;
        ensure!(
            page < self.images.len(),
            "palette selector {index}, stride {stride}, entry offset {offset} exceeds {} published pages spaced by {step}",
            self.images.len()
        );
        Ok(page)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UvRecord {
    pub timing: u8,
    pub control: u8,
    pub values: [i16; 4],
}

/// Original control records and their data dependencies. Modifier words remain
/// original input; cooking does not publish their compiled execution form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramSource {
    pub records: Vec<Record>,
    pub particles: BTreeMap<u8, ParticleTemplate>,
    /// Model slot selected by each model-particle declaration.
    pub models: BTreeMap<u8, ModelBinding>,
    pub modifiers: BTreeMap<u16, Vec<u16>>,
}

/// Ordinary particles own playback; stored scenes share it by model slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBinding {
    pub slot: u8,
    pub shared: bool,
    pub animation: Option<u16>,
    pub repeat: bool,
}

/// One six-byte `ef1` timeline record. The unused operands of control records are
/// retained too, so publication does not silently rewrite the original input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub age: i16,
    pub command: u8,
    pub argument: u8,
    pub operand: u16,
}

impl Record {
    pub const fn from_bytes(bytes: [u8; 6]) -> Self {
        Self {
            age: i16::from_be_bytes([bytes[0], bytes[1]]),
            command: bytes[2],
            argument: bytes[3],
            operand: u16::from_be_bytes([bytes[4], bytes[5]]),
        }
    }

    pub const fn to_bytes(self) -> [u8; 6] {
        let age = self.age.to_be_bytes();
        let operand = self.operand.to_be_bytes();
        [
            age[0],
            age[1],
            self.command,
            self.argument,
            operand[0],
            operand[1],
        ]
    }
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
    pub cull_back: bool,
    pub offset: [f32; 3],
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub angles: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub orbit: [f32; 3],
    pub geometry: ParticleGeometry,
    pub colors: [[i16; 4]; 2],
    pub brighten: [u8; 4],
    pub brighten_until: u8,
    pub uv: [i16; 4],
    pub palettes: [u8; 2],
    pub geometry_count: u8,
}

/// Original particle motion parameters; rendering resources are bound at loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleTemplate {
    pub lifetime: i16,
    /// Original group 7 runs after late effect programs and during stored transitions.
    pub late: bool,
    /// Refresh the origin from the effect's attachment on each particle visit.
    pub follow_origin: bool,
    /// Submit this particle in its selected actor's ordered drawing lists.
    /// The original declaration distinguishes the before/after-body lists.
    pub draw_after_target: bool,
    /// Reflect vertical velocity at the floor after integration (403F4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_restitution: Option<f32>,
    pub element_tint: bool,
    pub state: ParticleState,
    pub jerk: [f32; 3],
    pub orbit_velocity: [f32; 3],
    /// Integrate the local offset as XYZ rather than advancing its angle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub linear_orbit: bool,
    /// Zero keeps geometry acceleration active for the full lifetime.
    pub geometry_acceleration_until: i16,
    pub gradient: bool,
    pub fade: [u8; 4],
    pub fade_from: u8,
    pub uv_track: Vec<UvRecord>,
}

impl ParticleState {
    pub fn finite(&self) -> bool {
        let finite = |v: &[f32]| v.iter().all(|v| v.is_finite());
        finite(&self.offset)
            && finite(&self.velocity)
            && finite(&self.acceleration)
            && finite(&self.angles)
            && finite(&self.angular_velocity)
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
                ParticleGeometry::Quad { vertices, velocity } => {
                    vertices.iter().chain(velocity).all(|v| finite(v))
                }
            }
    }
}

impl ParticleTemplate {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.state.finite()
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
        // The native row selector is a signed byte. Follow reachable rows only;
        // high-bit rows scroll indefinitely, and a 255 successor jumps once.
        let mut visited = [false; 128];
        let mut index = 0;
        if self.uv_track.is_empty() {
            return Ok(());
        }
        loop {
            ensure!(index < 128, "particle UV row outside signed selector range");
            if visited[index] {
                break;
            }
            visited[index] = true;
            let row = self
                .uv_track
                .get(index)
                .context("particle UV row outside track")?;
            if row.timing & 128 != 0 {
                break;
            }
            index += 1;
            let next = self
                .uv_track
                .get(index)
                .context("unterminated particle UV track")?;
            ensure!(index < 128, "particle UV row outside signed selector range");
            if next.timing == 255 {
                index = usize::from(next.control);
            }
        }
        Ok(())
    }
}

impl SourceBank {
    /// Prepare one directly allocated particle, including its original UV rows.
    pub fn particle(&self, member: usize) -> Result<ParticleTemplate> {
        let actor = self.actors.get(member).context("unbound effect particle")?;
        let uv = if actor.prefix.uv_track == -1 {
            &[][..]
        } else {
            // 426A4 takes a signed row index, not a UV table entry.
            self.uv
                .get(actor.prefix.uv_track as usize..)
                .filter(|rows| !rows.is_empty())
                .context("particle UV root outside row pool")?
        };
        actor.particle(uv)
    }

    /// Select the source inputs required by a requested member. No executable
    /// instructions are produced here, and unused controllers do not block it.
    pub fn program(&self, member: usize) -> Result<ProgramSource> {
        let records = self
            .programs
            .get(member)
            .context("missing effect program")?;
        let mut particles = BTreeMap::new();
        let mut models = BTreeMap::new();
        let mut modifiers = BTreeMap::new();
        for record in records {
            if record.command < 252 {
                ensure!(record.argument == 0, "effect attachment is not prepared");
                particles.insert(record.command, self.particle(usize::from(record.command))?);
                if let declaration::Body::Particle {
                    geometry:
                        declaration::GeometryOperands::Model {
                            index, animation, ..
                        },
                    ..
                } = &self.actors[usize::from(record.command)].body
                {
                    let prefix = &self.actors[usize::from(record.command)].prefix;
                    models.insert(
                        record.command,
                        ModelBinding {
                            slot: *index,
                            shared: prefix.resource_slot == 6,
                            animation: (prefix.flags_or_shake_amplitude & 4 != 0)
                                .then_some(u16::from(*animation)),
                            repeat: prefix.secondary.flags & 2 != 0,
                        },
                    );
                }
            }
            if record.command < 254 && record.command != 252 && record.operand != 0 {
                let words = self
                    .modifiers
                    .get(&record.operand)
                    .context("unbound effect modifier")?;
                modifiers.insert(record.operand, words.clone());
            }
        }
        Ok(ProgramSource {
            records: records.clone(),
            particles,
            models,
            modifiers,
        })
    }
}
