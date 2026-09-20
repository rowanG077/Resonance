//! Complete 352-byte effect actor declarations, before constructor writes.
//!
//! Controllers dispatch before particle allocation copies the declaration.
//! Model animation state, vertex motion and caption bytes therefore share storage,
//! not a common set of active floating-point parameters.
use super::*;
use crate::read::FloatOperand;
use serde::{Deserialize, Serialize};

type Vector = [FloatOperand; 3];

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub prefix: Prefix,
    pub body: Body,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Prefix {
    pub kind: u8,
    pub blend: u8,
    pub resource_slot: u8,
    pub palettes: [u8; 2],
    pub retained_slot: i8,
    pub palette_stride: u8,
    pub storage_07: u8,
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
    pub storage_7c: [u8; 8],
    pub angle_step: FloatOperand,
    pub dimension_acceleration_until: i16,
    pub storage_8a: [u8; 2],
    pub bone: u8,
    pub texture_frame_or_joint_slot: u8,
    pub additional_copies: u8,
    pub copy_rotation: u8,
    pub storage_90: u8,
    pub secondary: Secondary,
    pub storage_96: [u8; 2],
    pub local_offset: Vector,
    pub local_velocity: Vector,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Secondary {
    pub flags: u8,
    pub periodic_program: u8,
    pub period: u8,
    pub animation_change_age: u8,
    pub ground_program: u8,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Body {
    Particle {
        geometry: Box<GeometryOperands>,
        context: ContextSeed,
    },
    Camera {
        elevation: FloatOperand,
        storage_b4: [u8; 4],
        distance: FloatOperand,
        storage_bc: Vec<u8>,
    },
    Caption {
        /// Entire B0..160 storage, including terminator and unused suffix.
        text_storage: Vec<u8>,
    },
    UnusedController {
        storage_b0: Vec<u8>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum GeometryOperands {
    Parameters {
        dimensions: Dimensions,
        radius_step: FloatOperand,
        /// Signed words used as a screen-texture offset by resource slot 10.
        texture_offset_words: [i32; 2],
        storage_e0: Vec<u8>,
        stored_elevation: FloatOperand,
    },
    Model {
        dimensions: Dimensions,
        index: u8,
        animation: u8,
        stored_animation_change: u8,
        storage_d7: u8,
        /// D8..140 is initialized by 8006EB68 before model use.
        animation_state: Vec<u8>,
        stored_model_binding: u32,
        elevation: FloatOperand,
    },
    VertexQuad {
        vertices: [Vector; 4],
        velocities: [Vector; 4],
        storage_110: Vec<u8>,
        stored_elevation: FloatOperand,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Dimensions {
    pub value: Vector,
    pub velocity: Vector,
    pub acceleration: Vector,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ContextSeed {
    /// 413B8/418B4 replace these declaration operands with caller context.
    pub stored_origin: Vector,
    pub stored_uv_binding: u32,
    pub stored_heading: FloatOperand,
    pub stored_program_binding: u32,
}

impl Record {
    pub(crate) fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == ACTOR_BYTES,
            "truncated authored effect actor"
        );
        let mut r = Reader { bytes, at: 0 };
        let prefix = Prefix {
            kind: r.byte(),
            blend: r.byte(),
            resource_slot: r.byte(),
            palettes: r.bytes(),
            retained_slot: r.byte() as i8,
            palette_stride: r.byte(),
            storage_07: r.byte(),
            uv: r.color(),
            lifetime: r.short(),
            geometry_count: r.byte(),
            geometry_phase: r.byte(),
            flags_or_shake_amplitude: r.word(),
            colors: [r.color(), r.color()],
            brighten: r.bytes(),
            darken: r.bytes(),
            brighten_until: r.byte(),
            darken_from: r.byte(),
            uv_track: r.byte() as i8,
            copy_axis_or_phase_period: r.byte(),
            position: r.vector(),
            velocity: r.vector(),
            acceleration: r.vector(),
            angles: r.vector(),
            angular_velocity: r.vector(),
            acceleration_change_or_segment_offset: r.vector(),
            storage_7c: r.bytes(),
            angle_step: r.float(),
            dimension_acceleration_until: r.short(),
            storage_8a: r.bytes(),
            bone: r.byte(),
            texture_frame_or_joint_slot: r.byte(),
            additional_copies: r.byte(),
            copy_rotation: r.byte(),
            storage_90: r.byte(),
            secondary: Secondary {
                flags: r.byte(),
                periodic_program: r.byte(),
                period: r.byte(),
                animation_change_age: r.byte(),
                ground_program: r.byte(),
            },
            storage_96: r.bytes(),
            local_offset: r.vector(),
            local_velocity: r.vector(),
        };
        debug_assert_eq!(r.at, 0xb0);
        let body = match prefix.kind {
            22 | 24 => Body::UnusedController {
                storage_b0: r.storage(176),
            },
            23 => Body::Camera {
                elevation: r.float(),
                storage_b4: r.bytes(),
                distance: r.float(),
                storage_bc: r.storage(164),
            },
            25 => Body::Caption {
                text_storage: r.storage(176),
            },
            kind => {
                let geometry = match kind {
                    3 => GeometryOperands::Model {
                        dimensions: r.dimensions(),
                        index: r.byte(),
                        animation: r.byte(),
                        stored_animation_change: r.byte(),
                        storage_d7: r.byte(),
                        animation_state: r.storage(104),
                        stored_model_binding: r.word(),
                        elevation: r.float(),
                    },
                    15 => GeometryOperands::VertexQuad {
                        vertices: std::array::from_fn(|_| r.vector()),
                        velocities: std::array::from_fn(|_| r.vector()),
                        storage_110: r.storage(52),
                        stored_elevation: r.float(),
                    },
                    _ => GeometryOperands::Parameters {
                        dimensions: r.dimensions(),
                        radius_step: r.float(),
                        texture_offset_words: [r.word() as i32, r.word() as i32],
                        storage_e0: r.storage(100),
                        stored_elevation: r.float(),
                    },
                };
                debug_assert_eq!(r.at, 0x148);
                Body::Particle {
                    geometry: Box::new(geometry),
                    context: ContextSeed {
                        stored_origin: r.vector(),
                        stored_uv_binding: r.word(),
                        stored_heading: r.float(),
                        stored_program_binding: r.word(),
                    },
                }
            }
        };
        debug_assert_eq!(r.at, ACTOR_BYTES);
        Ok(Self { prefix, body })
    }

    #[cfg(test)]
    pub(crate) fn source_bytes(&self) -> Result<Vec<u8>> {
        let p = &self.prefix;
        let mut w = Writer(Vec::with_capacity(ACTOR_BYTES));
        w.bytes(&[
            p.kind,
            p.blend,
            p.resource_slot,
            p.palettes[0],
            p.palettes[1],
            p.retained_slot as u8,
            p.palette_stride,
            p.storage_07,
        ]);
        w.color(p.uv);
        w.short(p.lifetime);
        w.bytes(&[p.geometry_count, p.geometry_phase]);
        w.word(p.flags_or_shake_amplitude);
        p.colors.iter().for_each(|&color| w.color(color));
        w.bytes(&p.brighten);
        w.bytes(&p.darken);
        w.bytes(&[
            p.brighten_until,
            p.darken_from,
            p.uv_track as u8,
            p.copy_axis_or_phase_period,
        ]);
        for vector in [
            p.position,
            p.velocity,
            p.acceleration,
            p.angles,
            p.angular_velocity,
            p.acceleration_change_or_segment_offset,
        ] {
            w.vector(vector);
        }
        w.bytes(&p.storage_7c);
        w.float(p.angle_step);
        w.short(p.dimension_acceleration_until);
        w.bytes(&p.storage_8a);
        w.bytes(&[
            p.bone,
            p.texture_frame_or_joint_slot,
            p.additional_copies,
            p.copy_rotation,
            p.storage_90,
            p.secondary.flags,
            p.secondary.periodic_program,
            p.secondary.period,
            p.secondary.animation_change_age,
            p.secondary.ground_program,
        ]);
        w.bytes(&p.storage_96);
        w.vector(p.local_offset);
        w.vector(p.local_velocity);
        match &self.body {
            Body::UnusedController { storage_b0 } => {
                ensure!(matches!(p.kind, 22 | 24), "actor body disagrees with kind");
                w.storage(storage_b0, 176)?;
            }
            Body::Camera {
                elevation,
                storage_b4,
                distance,
                storage_bc,
            } => {
                ensure!(p.kind == 23, "actor body disagrees with kind");
                w.float(*elevation);
                w.bytes(storage_b4);
                w.float(*distance);
                w.storage(storage_bc, 164)?;
            }
            Body::Caption { text_storage } => {
                ensure!(p.kind == 25, "actor body disagrees with kind");
                w.storage(text_storage, 176)?;
            }
            Body::Particle { geometry, context } => {
                ensure!(!matches!(p.kind, 22..=25), "actor body disagrees with kind");
                match geometry.as_ref() {
                    GeometryOperands::Parameters {
                        dimensions,
                        radius_step,
                        texture_offset_words,
                        storage_e0,
                        stored_elevation,
                    } => {
                        ensure!(
                            !matches!(p.kind, 3 | 15),
                            "actor geometry body disagrees with kind"
                        );
                        w.dimensions(dimensions);
                        w.float(*radius_step);
                        texture_offset_words
                            .iter()
                            .for_each(|&value| w.word(value as u32));
                        w.storage(storage_e0, 100)?;
                        w.float(*stored_elevation);
                    }
                    GeometryOperands::Model {
                        dimensions,
                        index,
                        animation,
                        stored_animation_change,
                        storage_d7,
                        animation_state,
                        stored_model_binding,
                        elevation,
                    } => {
                        ensure!(p.kind == 3, "actor geometry body disagrees with kind");
                        w.dimensions(dimensions);
                        w.bytes(&[*index, *animation, *stored_animation_change, *storage_d7]);
                        w.storage(animation_state, 104)?;
                        w.word(*stored_model_binding);
                        w.float(*elevation);
                    }
                    GeometryOperands::VertexQuad {
                        vertices,
                        velocities,
                        storage_110,
                        stored_elevation,
                    } => {
                        ensure!(p.kind == 15, "actor geometry body disagrees with kind");
                        vertices.iter().chain(velocities).for_each(|&v| w.vector(v));
                        w.storage(storage_110, 52)?;
                        w.float(*stored_elevation);
                    }
                }
                w.vector(context.stored_origin);
                w.word(context.stored_uv_binding);
                w.float(context.stored_heading);
                w.word(context.stored_program_binding);
            }
        }
        ensure!(w.0.len() == ACTOR_BYTES, "invalid actor record extent");
        Ok(w.0)
    }
}

// Reads are bounded by the exact record length and the fixed branch extents above.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let value = self.bytes[self.at..self.at + N].try_into().unwrap();
        self.at += N;
        value
    }
    fn byte(&mut self) -> u8 {
        self.bytes::<1>()[0]
    }
    fn short(&mut self) -> i16 {
        i16::from_be_bytes(self.bytes())
    }
    fn word(&mut self) -> u32 {
        u32::from_be_bytes(self.bytes())
    }
    fn float(&mut self) -> FloatOperand {
        FloatOperand::from_bits(self.word())
    }
    fn vector(&mut self) -> Vector {
        std::array::from_fn(|_| self.float())
    }
    fn color(&mut self) -> [i16; 4] {
        std::array::from_fn(|_| self.short())
    }
    fn storage(&mut self, count: usize) -> Vec<u8> {
        let value = self.bytes[self.at..self.at + count].to_vec();
        self.at += count;
        value
    }
    fn dimensions(&mut self) -> Dimensions {
        Dimensions {
            value: self.vector(),
            velocity: self.vector(),
            acceleration: self.vector(),
        }
    }
}

#[cfg(test)]
struct Writer(Vec<u8>);
#[cfg(test)]
impl Writer {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
    fn short(&mut self, value: i16) {
        self.bytes(&value.to_be_bytes());
    }
    fn word(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }
    fn float(&mut self, value: FloatOperand) {
        self.word(value.bits());
    }
    fn vector(&mut self, vector: Vector) {
        vector.iter().for_each(|&value| self.float(value));
    }
    fn color(&mut self, color: [i16; 4]) {
        color.iter().for_each(|&value| self.short(value));
    }
    fn storage(&mut self, storage: &[u8], expected: usize) -> Result<()> {
        ensure!(
            storage.len() == expected,
            "invalid actor storage extent at {:#x}",
            self.0.len()
        );
        self.bytes(storage);
        Ok(())
    }
    fn dimensions(&mut self, dimensions: &Dimensions) {
        self.vector(dimensions.value);
        self.vector(dimensions.velocity);
        self.vector(dimensions.acceleration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_actor_kinds_preserve_the_complete_union_through_json() {
        for kind in 0..=u8::MAX {
            let mut bytes: [u8; ACTOR_BYTES] =
                std::array::from_fn(|i| (i as u8).wrapping_mul(53).wrapping_add(17));
            bytes[0] = kind;
            bytes[0x32] = 254; // A negative selector other than the -1 sentinel.
            bytes[0x34..0x38].copy_from_slice(&0x7fa1_2345u32.to_be_bytes());
            bytes[0xb8..0xbc].copy_from_slice(&0xff80_0000u32.to_be_bytes());
            let record = Record::read(&bytes).unwrap();
            assert_eq!(record.prefix.uv_track, -2);
            assert_eq!(record.prefix.position[0].bits(), 0x7fa1_2345);
            let emitted = serde_json::to_vec(&record).unwrap();
            let restored: Record = serde_json::from_slice(&emitted).unwrap();
            assert_eq!(restored.source_bytes().unwrap(), bytes, "actor kind {kind}");
            assert_eq!(restored.prefix.kind, kind);
            assert_eq!(restored.prefix.position[0].bits(), 0x7fa1_2345);
        }
    }

    #[test]
    fn caption_storage_and_geometry_discriminants_are_structural_not_admission_guards() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 25;
        bytes[0xb0..].fill(0x81); // Unclosed Shift-JIS text is still a recoverable physical record.
        let mut record = Record::read(&bytes).unwrap();
        assert_eq!(record.source_bytes().unwrap(), bytes);
        assert!(authored_controller(&record).is_err());
        let Body::Caption { text_storage } = &mut record.body else {
            unreachable!()
        };
        text_storage[..3].copy_from_slice(b"ok\0");
        assert!(
            matches!(authored_controller(&record).unwrap(), Some(EffectController::Caption { text }) if text == "ok")
        );
        assert_eq!(&record.source_bytes().unwrap()[0xb3..], &bytes[0xb3..]);
        record.prefix.kind = 3;
        assert!(record.source_bytes().is_err());
        record.prefix.kind = 25;
        let Body::Caption { text_storage } = &mut record.body else {
            unreachable!()
        };
        text_storage.pop();
        assert!(record.source_bytes().is_err());
        assert!(Record::read(&bytes[..ACTOR_BYTES - 1]).is_err());
    }
}
