//! Original 352-byte controller union. Runtime admission belongs to loading.
use anyhow::{Result, ensure};
use resonance_content::{battle_effect::declaration::*, source::FloatOperand as Number};
const ACTOR_BYTES: usize = 352;

pub(super) fn read(bytes: &[u8]) -> Result<Declaration> {
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
        reserved_header: r.byte(),
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
        reserved_motion: r.bytes(),
        angle_step: r.float(),
        dimension_acceleration_until: r.short(),
        reserved_dimensions: r.bytes(),
        bone: r.byte(),
        texture_frame_or_joint_slot: r.byte(),
        additional_copies: r.byte(),
        copy_rotation: r.byte(),
        reserved_secondary: r.byte(),
        secondary: Secondary {
            flags: r.byte(),
            periodic_program: r.byte(),
            period: r.byte(),
            animation_change_age: r.byte(),
            ground_program: r.byte(),
        },
        reserved_attachment: r.bytes(),
        local_offset: r.vector(),
        local_velocity: r.vector(),
    };
    debug_assert_eq!(r.at, 0xb0);
    let body = match prefix.kind {
        22 | 24 => Body::Opaque {
            storage: r.storage(176),
        },
        23 => Body::Camera {
            elevation: r.float(),
            reserved_elevation: r.bytes(),
            distance: r.float(),
            storage: r.storage(164),
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
                    reserved_animation: r.byte(),
                    animation_state: r.storage(104),
                    stored_model_binding: r.word(),
                    elevation: r.float(),
                },
                15 => GeometryOperands::VertexQuad {
                    vertices: std::array::from_fn(|_| r.vector()),
                    velocities: std::array::from_fn(|_| r.vector()),
                    storage: r.storage(52),
                    stored_elevation: r.float(),
                },
                _ => GeometryOperands::Parameters {
                    dimensions: r.dimensions(),
                    radius_step: r.float(),
                    texture_offset_words: [r.word() as i32, r.word() as i32],
                    storage: r.storage(100),
                    stored_elevation: r.float(),
                },
            };
            debug_assert_eq!(r.at, 0x148);
            Body::Particle {
                geometry,
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
    Ok(Declaration { prefix, body })
}

#[cfg(test)]
pub(super) fn source_bytes(record: &Declaration) -> Result<Vec<u8>> {
    let p = &record.prefix;
    let mut w = Writer(Vec::with_capacity(ACTOR_BYTES));
    w.bytes(&[
        p.kind,
        p.blend,
        p.resource_slot,
        p.palettes[0],
        p.palettes[1],
        p.retained_slot as u8,
        p.palette_stride,
        p.reserved_header,
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
    w.bytes(&p.reserved_motion);
    w.float(p.angle_step);
    w.short(p.dimension_acceleration_until);
    w.bytes(&p.reserved_dimensions);
    w.bytes(&[
        p.bone,
        p.texture_frame_or_joint_slot,
        p.additional_copies,
        p.copy_rotation,
        p.reserved_secondary,
        p.secondary.flags,
        p.secondary.periodic_program,
        p.secondary.period,
        p.secondary.animation_change_age,
        p.secondary.ground_program,
    ]);
    w.bytes(&p.reserved_attachment);
    w.vector(p.local_offset);
    w.vector(p.local_velocity);
    match &record.body {
        Body::Opaque { storage } => {
            ensure!(matches!(p.kind, 22 | 24), "actor body disagrees with kind");
            w.storage(storage, 176)?;
        }
        Body::Camera {
            elevation,
            reserved_elevation,
            distance,
            storage,
        } => {
            ensure!(p.kind == 23, "actor body disagrees with kind");
            w.float(*elevation);
            w.bytes(reserved_elevation);
            w.float(*distance);
            w.storage(storage, 164)?;
        }
        Body::Caption { text_storage } => {
            ensure!(p.kind == 25, "actor body disagrees with kind");
            w.storage(text_storage, 176)?;
        }
        Body::Particle { geometry, context } => {
            ensure!(!matches!(p.kind, 22..=25), "actor body disagrees with kind");
            match geometry {
                GeometryOperands::Parameters {
                    dimensions,
                    radius_step,
                    texture_offset_words,
                    storage,
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
                    w.storage(storage, 100)?;
                    w.float(*stored_elevation);
                }
                GeometryOperands::Model {
                    dimensions,
                    index,
                    animation,
                    stored_animation_change,
                    reserved_animation,
                    animation_state,
                    stored_model_binding,
                    elevation,
                } => {
                    ensure!(p.kind == 3, "actor geometry body disagrees with kind");
                    w.dimensions(dimensions);
                    w.bytes(&[
                        *index,
                        *animation,
                        *stored_animation_change,
                        *reserved_animation,
                    ]);
                    w.storage(animation_state, 104)?;
                    w.word(*stored_model_binding);
                    w.float(*elevation);
                }
                GeometryOperands::VertexQuad {
                    vertices,
                    velocities,
                    storage,
                    stored_elevation,
                } => {
                    ensure!(p.kind == 15, "actor geometry body disagrees with kind");
                    vertices.iter().chain(velocities).for_each(|&v| w.vector(v));
                    w.storage(storage, 52)?;
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
    fn float(&mut self) -> Number {
        Number::from_bits(self.word())
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
    fn float(&mut self, value: Number) {
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
    fn controller_unions_preserve_every_byte_through_json() {
        for kind in 0..=u8::MAX {
            let mut bytes: [u8; ACTOR_BYTES] =
                std::array::from_fn(|i| (i as u8).wrapping_mul(53).wrapping_add(17));
            bytes[0] = kind;
            bytes[0x32] = 254;
            bytes[0x34..0x38].copy_from_slice(&0x7fa1_2345u32.to_be_bytes());
            bytes[0xb8..0xbc].copy_from_slice(&0xff80_0000u32.to_be_bytes());
            let record = read(&bytes).unwrap();
            let restored: Declaration =
                serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
            assert_eq!(source_bytes(&restored).unwrap(), bytes, "controller {kind}");
            assert_eq!(restored.prefix.uv_track, -2);
        }
        assert!(read(&[0; ACTOR_BYTES - 1]).is_err());
    }
}
