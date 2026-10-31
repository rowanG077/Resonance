//! Direct reader for the native `ef1` battle-effect program bank.
//! Conversion stays strict: unsupported commands cannot silently become a
//! partial cooked definition.
use crate::{battle_effect_media, battle_effect_tints, dol};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    BirthField, BirthSum, EffectOperation, ModelAnimation, ModelAttachment, ModelBinding,
    ModelParent, MotionChange, ParticleBirth, PeriodicEffect, PolarSpread, ScheduledEvent,
    UvAnimation, UvChange, UvFrame, ValueRange,
};
use resonance_content::battle_effect::{
    ParticleGeometry, ParticleState, ParticleTemplate,
    declaration::Declaration,
    document::{Artwork, Definition, STUN_BIRD},
    visual::{ModelVisual, Orientation, ParticleShape, ParticleTexture, ParticleVisual, UvLayout},
};
use std::collections::HashSet;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

struct Row<'a>(&'a [u8]);

impl Row<'_> {
    fn short(&self, at: usize) -> i16 {
        i16::from_be_bytes(self.0[at..at + 2].try_into().unwrap())
    }

    fn word(&self, at: usize) -> u32 {
        u32::from_be_bytes(self.0[at..at + 4].try_into().unwrap())
    }

    fn float(&self, at: usize) -> f32 {
        f32::from_bits(self.word(at))
    }

    fn vector(&self, at: usize) -> [f32; 3] {
        std::array::from_fn(|axis| self.float(at + axis * 4))
    }

    fn color(&self, at: usize) -> [i16; 4] {
        std::array::from_fn(|axis| self.short(at + axis * 2))
    }
}

fn duration(value: i16) -> Result<Option<u32>> {
    ensure!(value >= 0, "negative effect duration");
    Ok((value > 0).then_some(value as u32))
}

struct EditState {
    scratch: [Option<ValueRange>; 4],
    integers: [Option<ValueRange>; 4],
    draws: i64,
}
impl Default for EditState {
    fn default() -> Self {
        Self {
            scratch: [Some(ValueRange::fixed(0.)); 4],
            integers: [Some(ValueRange::fixed(0.)); 4],
            draws: 0,
        }
    }
}

/// Ordinary procedural declarations are shared by resident and scene banks.
/// More specialized native controllers are deliberately rejected here.
fn uv_animation(rows: &[u8], index: usize) -> Result<UvAnimation> {
    let rows = rows
        .get(index * 10..)
        .context("effect UV track exceeds pool")?;
    let first = rows.get(..10).context("effect UV track is empty")?;
    let values = |row: &[u8]| -> Result<[i16; 4]> {
        Ok(std::array::from_fn(|axis| {
            i16::from_be_bytes([row[2 + axis * 2], row[3 + axis * 2]])
        }))
    };
    if (128..255).contains(&first[0]) {
        let interval = u32::from(first[0] & 127);
        ensure!(interval > 0, "zero effect UV scroll interval");
        let value = values(first)?;
        return Ok(UvAnimation::Scroll {
            origin: [value[0], value[1]],
            step: [value[2], value[3]],
            interval,
        });
    }
    let mut frames = Vec::new();
    for row in rows.chunks_exact(10) {
        let value = values(row)?;
        if row[0] == 255 {
            let loop_start = usize::from(row[1]);
            ensure!(loop_start < frames.len(), "effect UV loop exceeds frames");
            return Ok(UvAnimation::Frames {
                frames,
                loop_start: Some(loop_start),
            });
        }
        if row[0] == 254 && value == [0; 4] {
            frames.push(UvFrame {
                duration: 1,
                change: UvChange::Rectangle { rect: [0; 4] },
            });
            return Ok(UvAnimation::Frames {
                frames,
                loop_start: None,
            });
        }
        ensure!(
            (1..128).contains(&row[0]),
            "unsupported UV frame-to-scroll transition"
        );
        frames.push(UvFrame {
            duration: u32::from(row[0]),
            change: if value[0] == -32000 {
                UvChange::Palette {
                    index: value[1].try_into()?,
                }
            } else {
                UvChange::Rectangle { rect: value }
            },
        });
    }
    anyhow::bail!("unterminated effect UV track")
}

fn simple_particle(bytes: &[u8], uv: &[u8]) -> Result<Declaration> {
    let row = Row(bytes);
    if bytes[0] == 22 {
        return Ok(Declaration::CameraShake {
            duration: u32::try_from(row.short(16)).context("negative effect shake duration")?,
            amplitude: row.word(20),
        });
    }
    if bytes[0] == 24 {
        let duration = row.short(16);
        let color = row.color(24);
        ensure!(
            duration > 0 && row.word(20) == 0 && color[3] == 64,
            "invalid effect stage tint"
        );
        return Ok(Declaration::StageTint {
            color: std::array::from_fn(|i| color[i] as u8),
            duration: duration as u16,
        });
    }
    if bytes[0] == 25 {
        let end = bytes[176..]
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated effect notice")?;
        let (text, invalid) =
            encoding_rs::SHIFT_JIS.decode_without_bom_handling(&bytes[176..176 + end]);
        ensure!(
            !invalid && !text.is_empty() && !text.chars().any(char::is_control),
            "invalid effect notice"
        );
        return Ok(Declaration::Notice {
            text: text.into_owned(),
        });
    }
    ensure!(
        matches!(
            bytes[0],
            3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 15 | 18 | 19
        ),
        "effect actor needs a specialized controller"
    );
    let flags = row.word(20);
    let kind = bytes[0];
    let group_quad = kind == 5
        && flags & 0x200 != 0
        && flags & !(0x200 | 0x40 | 0x0400_0000 | 0x400 | 0x20_2000 | 0x80000) == 0;
    let bone_ring = kind == 4
        && flags & 0x4000_0000 != 0
        && flags & !(0x4000_0000 | 0x2000_0000 | 0x0400_0000 | 0x40000 | 0x10 | 8 | 2) == 0
        && bytes[2] != 10
        && bytes[142] == 0;
    let unsupported_bone = kind == 5 && flags == 0x4400_0040;
    let owner_bone_model = kind == 3
        && bytes[2] == 2
        && bytes[142] == 0
        && ((flags == 0x4000_0000 | 0x400 | 0x20 && bytes[145] == 0)
            || (flags == 0x4000_0000 | 4 && bytes[140..150] == [10, 0, 0, 0, 0, 2, 0, 0, 0, 0]));
    const ALLOWED: u32 = 0x0400_0000
        | 0x40
        | 0x100
        | 0x400
        | 0x8
        | 0x20_2000
        | 1
        | 0x0800_0000
        | 0x400000
        | 0x1000_0000
        | 0x80000
        | 0x800000
        | 0x1000
        | 2
        | 0x2000
        | 0x0200_0000
        | 0x100000
        | 0x1000000
        | 0x800
        | 0x20
        | 0x10
        | 0x80
        | 0x4
        | 0x8
        | 0x40000
        | 0x20000
        | 0x2000_0000
        | 0x4000_0000
        | 0x200;
    ensure!(
        flags & !ALLOWED == 0,
        "particle uses an unsupported native controller"
    );
    let follow_bone = flags & 0x2000_0000 != 0 && !bone_ring;
    if follow_bone {
        ensure!(
            bytes[142] == 0
                && (matches!(kind, 4 | 5) || (kind == 3 && flags & !(0x2000_0000 | 0x400) == 0)),
            "unsupported effect bone follower"
        );
    }
    ensure!(
        flags & 0x200 == 0 || group_quad || kind == 3,
        "unsupported effect group controller"
    );
    ensure!(
        flags & 0x4000_0000 == 0 || bone_ring || owner_bone_model || unsupported_bone,
        "unsupported effect owner-bone controller"
    );
    let periodic = kind == 3
        && flags == 0x406
        && bytes[2] == 2
        && bytes[145] == 10
        && bytes[146] != 0
        && bytes[147] > 0
        && bytes[148..150] == [0; 2];
    let child_effect = if bytes[145..150] == [0; 5] || periodic {
        None
    } else if bytes[0] == 3 && bytes[145] & !3 == 0 && bytes[146..148] == [0; 2] && bytes[149] == 0
    {
        // Model playback uses these bytes for repeat/reverse, not a child.
        None
    } else if matches!(bytes[0], 5 | 15) && bytes[145..149] == [4, 0, 0, 0] {
        Some(u16::from(bytes[149]))
    } else {
        anyhow::bail!("particle needs child controller")
    };
    let mut rectangle = std::array::from_fn(|axis| row.short(8 + axis * 2));
    let uv_animation = if bytes[0] == 3 && bytes[50] != 255 {
        let track = uv
            .get(usize::from(bytes[50]) * 10..usize::from(bytes[50]) * 10 + 10)
            .context("model effect UV scroll is missing")?;
        ensure!(
            track[0] == 128 && track[1] == 0,
            "unsupported model effect UV controller"
        );
        let values: [i16; 4] = std::array::from_fn(|axis| {
            i16::from_be_bytes([track[2 + axis * 2], track[3 + axis * 2]])
        });
        ensure!(
            values[2] > 0 && values[3] > 0,
            "invalid model effect UV scroll extent"
        );
        rectangle = [0, 0, values[2], values[3]];
        Some(UvAnimation::Scroll {
            origin: [0; 2],
            step: [values[0], values[1]],
            interval: 1,
        })
    } else {
        (bytes[50] != 255)
            .then(|| uv_animation(uv, usize::from(bytes[50])))
            .transpose()?
    };
    if bytes[0] == 3
        && flags & 0x20000 == 0
        && !follow_bone
        && !owner_bone_model
        && uv_animation.is_none()
    {
        let (rows, frame) = (bytes[140], bytes[141]);
        ensure!(
            rows == 0 && frame == 0 || frame < rows,
            "invalid model texture frame"
        );
        rectangle = [0, i16::from(frame), 0, i16::from(rows)];
    }
    let lifetime = duration(row.short(16))?.map(|last_age| last_age + 1);
    let template = ParticleTemplate {
        during_pause: flags & 0x100 != 0,
        lifetime,
        owner_bone: (bone_ring || owner_bone_model).then_some(bytes[140]),
        follow_bone: follow_bone.then_some(bytes[140]),
        follow_origin: flags & 0x400 != 0 && !follow_bone && !owner_bone_model,
        follow_orientation: matches!(bytes[0], 3 | 15) && flags & 0x80 != 0,
        fixed_orientation: bytes[0] == 4 && flags & 0x80 != 0,
        origin_motion: false,
        model_elevation: (bytes[0] == 3 && flags & 2 != 0).then(|| row.float(324)),
        model_ground_relative: bytes[0] == 3 && flags & 0x1000 != 0,
        draw_after_target: flags & 0x20_2000 != 0,
        ground_restitution: (flags & 1 != 0).then_some(0.65),
        ground_clamp: flags & 0x0800_0000 != 0,
        bounce_effect: (flags & 1 != 0).then_some(child_effect).flatten(),
        floor_effect: (flags & 1 == 0).then_some(child_effect).flatten(),
        periodic_effect: periodic.then_some(PeriodicEffect {
            member: u16::from(bytes[146]),
            interval: bytes[147],
        }),
        element_tint: flags & 0x400000 != 0,
        state: ParticleState {
            segment_offset: [None; 3],
            blend: None,
            cull_back: flags & 0x1000_0000 != 0,
            ribbon_depth: false,
            offset: row.vector(52),
            velocity: row.vector(64),
            acceleration: row.vector(76),
            angles: row.vector(88),
            angular_velocity: row.vector(100),
            orbit: row.vector(152),
            geometry: if bytes[0] == 15 {
                ParticleGeometry::Quad {
                    vertices: std::array::from_fn(|i| row.vector(176 + i * 12)),
                    velocity: std::array::from_fn(|i| row.vector(224 + i * 12)),
                }
            } else if bytes[0] == 8 {
                let value = row.vector(176);
                ParticleGeometry::BillboardTrail {
                    size: [value[0], value[1]],
                    radius: value[2],
                    radius_velocity: row.float(196),
                    segment_size_step: row.vector(200),
                    segment_offset: row.vector(112),
                    segment_angle_step: row.float(132),
                    steps_per_segment: bytes[19],
                }
            } else if bytes[0] == 10 {
                ParticleGeometry::Spiral {
                    value: row.vector(176),
                    velocity: row.vector(188),
                    acceleration: row.vector(200),
                    segment_angle_step: row.float(132),
                }
            } else if bytes[0] == 18 {
                let value = row.vector(176);
                let velocity = row.vector(188);
                ParticleGeometry::Ribbon {
                    length: value[0],
                    width: value[1],
                    jitter: velocity[1],
                    phase: bytes[19],
                    phase_period: bytes[51] as i8,
                }
            } else {
                ParticleGeometry::Size {
                    value: row.vector(176),
                    velocity: row.vector(188),
                    acceleration: row.vector(200),
                }
            },
            colors: [row.color(24), row.color(32)],
            brighten: bytes[40..44].try_into()?,
            brighten_until: (bytes[48] != 0).then_some(u32::from(bytes[48])),
            uv: rectangle,
            palettes: [bytes[3], bytes[4]],
            geometry_count: bytes[18],
            effect_group: group_quad.then_some(bytes[140]),
        },
        jerk: if matches!(bytes[0], 8 | 10) {
            [0.; 3]
        } else {
            row.vector(112)
        },
        orbit_velocity: row.vector(164),
        linear_orbit: bytes[0] == 5,
        geometry_acceleration_until: duration(row.short(136))?,
        gradient: flags & 8 != 0,
        fade: bytes[44..48].try_into()?,
        fade_from: u32::from(bytes[49]),
        uv_animation,
    };
    template.validate()?;
    if bytes[0] == 3 {
        return Ok(Declaration::ModelParticle {
            template,
            binding: ModelBinding {
                slot: bytes[212],
                animation: (flags & 4 != 0).then_some(u16::from(bytes[213])),
                repeat: bytes[145] & 2 != 0,
                reverse_at: (bytes[145] & 1 != 0).then_some(bytes[148]),
                program_animation: flags & 0x20 != 0,
                attachment: (flags & 0x20000 != 0).then_some(ModelAttachment {
                    retained: bytes[141],
                    joint: bytes[140],
                }),
            },
            visual: ModelVisual {
                blend: bytes[1],
                lit: flags & 0x4000 != 0,
                depth_test: flags & 0x82000 == 0,
                cull_back: bytes[1] == 0 || flags & 0x10000000 != 0,
                before_actor: flags & 0x200000 != 0,
            },
        });
    }
    Ok(Declaration::Particle {
        template,
        visual: ParticleVisual {
            geometry: if unsupported_bone {
                ParticleShape::Unsupported {
                    reason:
                        "particle requires an unsupported bone or point-history drawing binding"
                            .into(),
                }
            } else {
                match bytes[0] {
                    4 => ParticleShape::Ring {
                        segments: if flags & 0x100000 != 0 {
                            32
                        } else if flags & 0x1000000 != 0 {
                            8
                        } else {
                            16
                        },
                        flared: flags & 0x800 != 0,
                        hidden: flags & 0x20 != 0,
                        uv_layout: if flags & 0x100000 != 0 {
                            UvLayout::HalfWidthCycle
                        } else if flags & 0x10 != 0 {
                            UvLayout::Cycle
                        } else {
                            UvLayout::Repeat
                        },
                    },
                    5 => ParticleShape::Quad,
                    14 => ParticleShape::NoDraw,
                    6 => ParticleShape::Hemisphere { columns: 16 },
                    7 => ParticleShape::Orbit,
                    8 => ParticleShape::BillboardTrail,
                    9 => ParticleShape::RadialQuads {
                        flat: flags & 0x20 != 0,
                        advance_u: flags & 0x10 != 0,
                    },
                    10 => ParticleShape::Spiral {
                        radius_step: row.float(212),
                        segment_offset: row.vector(112),
                        steps_per_segment: bytes[19],
                        uv_layout: if flags & 0x10 != 0 {
                            UvLayout::Advance
                        } else {
                            UvLayout::Repeat
                        },
                    },
                    11 => ParticleShape::Disc {
                        segments: if flags & 0x1000000 != 0 { 8 } else { 16 },
                    },
                    12 => ParticleShape::Shell {
                        segments: if flags & 0x100000 != 0 { 16 } else { 8 },
                        elliptical: flags & 0x20 != 0,
                        uv_layout: if flags & 0x100000 != 0 {
                            UvLayout::AlternatingHalves { panels_per_half: 4 }
                        } else if flags & 0x10 != 0 {
                            UvLayout::Cycle
                        } else {
                            UvLayout::Repeat
                        },
                    },
                    13 => ParticleShape::Sphere {
                        columns: if flags & 0x1000000 != 0 { 8 } else { 16 },
                    },
                    15 => ParticleShape::VertexQuad {
                        copy_axis: bytes[51],
                        local_copies: flags & 0x20 != 0,
                    },
                    18 => ParticleShape::Ribbon {
                        depth: flags & 0x20 != 0,
                    },
                    19 => ParticleShape::TriangleBand,
                    _ => unreachable!(),
                }
            },
            orientation: if bytes[0] == 4 && flags & 0x80 != 0 {
                Orientation::FixedWorld
            } else if bytes[0] == 15 && flags & 0x80 != 0 {
                Orientation::FollowMotion
            } else if flags & 0x40 != 0 {
                Orientation::Billboard
            } else if flags & 0x0200_0000 != 0 {
                Orientation::Camera
            } else {
                Orientation::World
            },
            texture: match bytes[2] {
                255 => ParticleTexture::Untextured,
                10 => ParticleTexture::Atlas {
                    slot: 0,
                    dual: true,
                    palette_set: 0,
                },
                slot => ParticleTexture::Atlas {
                    slot,
                    dual: flags & 0x0400_0000 != 0,
                    palette_set: u8::from(slot == 1 && bytes[6] == 32),
                },
            },
            scene_sample: (bytes[2] == 10).then(|| [row.word(216) as i8, row.word(220) as i8]),
            blend: bytes[1],
            depth_test: flags & 0x80000 == 0,
            depth_write: flags & 0x800000 != 0,
            ground_relative: flags & 0x1000 != 0,
            anchored: flags & 2 != 0,
            copies: bytes[142],
            copy_rotation: bytes[143],
            after_actor: flags & 0x2000 != 0,
        },
    })
}

pub(crate) struct NativeBank<'a> {
    bytes: &'a [u8],
    pub actors: &'a [u8],
    uv: &'a [u8],
    commands: &'a [u8],
    roots: &'a [u8],
    count: usize,
}

impl<'a> NativeBank<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.starts_with(b"ef1\0") && bytes.len() >= 20,
            "invalid effect bank"
        );
        let offsets = (8..20)
            .step_by(2)
            .map(|at| dol::u16(bytes, at).map(usize::from))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            offsets
                .iter()
                .all(|&offset| (20..=bytes.len()).contains(&offset)),
            "effect section exceeds bank"
        );
        let section = |index: usize| -> Result<&[u8]> {
            let start = offsets[index];
            let end = offsets
                .iter()
                .copied()
                .filter(|&at| at > start)
                .min()
                .unwrap_or(bytes.len());
            bytes.get(start..end).context("effect section exceeds bank")
        };
        let actors = section(0)?;
        let remainder = actors.len() % 352;
        let tail = &actors[actors.len() - remainder..];
        ensure!(
            remainder <= 4 && (tail.iter().all(|&byte| byte == 0) || tail == [255, 255, 0, 0]),
            "misaligned effect actor pool"
        );
        let actors = &actors[..actors.len() - remainder];
        let uv = if offsets[3] == offsets[4] {
            &[][..]
        } else {
            section(3)?
        };
        ensure!(uv.len().is_multiple_of(10), "misaligned effect UV pool");
        let count = usize::from(bytes[4]);
        let roots = section(4)?
            .get(..count * 2)
            .context("effect program table is truncated")?;
        Ok(Self {
            bytes,
            actors,
            uv,
            commands: section(1)?,
            roots,
            count,
        })
    }

    pub fn actor(&self, index: u8) -> Result<&[u8]> {
        self.actors
            .get(usize::from(index) * 352..(usize::from(index) + 1) * 352)
            .context("effect actor is absent")
    }

    pub fn program_count(&self) -> usize {
        self.count
    }

    pub fn simple_particle(&self, index: u8) -> Result<Declaration> {
        simple_particle(self.actor(index)?, self.uv)
    }

    fn scalar_slot<'b>(
        birth: &'b mut ParticleBirth,
        target: u16,
        kind: u8,
    ) -> Result<&'b mut Option<ValueRange>> {
        if kind == 15 {
            for (vertex, base) in [0xb0, 0xbc, 0xc8].into_iter().enumerate() {
                if (base..=base + 8).contains(&target) && (target - base).is_multiple_of(4) {
                    return Ok(&mut birth.vertices[vertex][usize::from((target - base) / 4)]);
                }
            }
        }
        let vectors: [(u16, &mut [Option<ValueRange>; 3]); 11] = [
            (0x34, &mut birth.offset),
            (0x40, &mut birth.velocity),
            (0x4c, &mut birth.acceleration),
            (0x58, &mut birth.angles),
            (0x64, &mut birth.angular_velocity),
            (0x70, &mut birth.segment_offset),
            (0x98, &mut birth.orbit),
            (0xa4, &mut birth.orbit_velocity),
            (0xb0, &mut birth.size),
            (0xbc, &mut birth.size_velocity),
            (0xc8, &mut birth.size_acceleration),
        ];
        for (base, vector) in vectors {
            if (base..=base + 8).contains(&target) && (target - base).is_multiple_of(4) {
                return Ok(&mut vector[usize::from((target - base) / 4)]);
            }
        }
        if target == 0x84 {
            return Ok(&mut birth.segment_angle_step);
        }
        anyhow::bail!("unsupported effect scalar destination {target:#x}")
    }

    fn model_animation(&self, at: usize) -> Result<ModelAnimation> {
        ensure!(
            dol::u16(self.bytes, at + 2)? == 0x5c,
            "unsupported effect animation destination"
        );
        let model = u8::try_from(dol::u16(self.bytes, at + 4)?)?;
        let clip = dol::u16(self.bytes, at + 6)?;
        let blend_ticks = u8::try_from(dol::u16(self.bytes, at + 8)?)?;
        let speed = dol::u16(self.bytes, at + 10)?;
        let repeat = dol::u16(self.bytes, at + 12)?;
        ensure!(
            matches!(repeat, 0 | 8) && dol::u16(self.bytes, at + 14)? == 0,
            "unsupported effect animation control"
        );
        let animation = ModelAnimation {
            model,
            clip,
            frame: 0.,
            blend_ticks,
            rate: if speed == 0 {
                0.5
            } else {
                f32::from(speed) * 0.01
            },
            repeat: repeat == 0,
        };
        animation.validate()?;
        Ok(animation)
    }

    fn scalar_edits(
        &self,
        actor: &[u8],
        pointer: u16,
        birth: &mut ParticleBirth,
        state: &mut EditState,
        inherited: bool,
    ) -> Result<()> {
        let row = Row(actor);
        let mut at = usize::from(pointer);
        let mut explicit_set = HashSet::new();
        loop {
            let opcode = dol::u16(self.bytes, at)?;
            if opcode == 65535 {
                if let Some(animation) = birth.animation {
                    let model = birth
                        .model
                        .get_or_insert(ValueRange::fixed(f32::from(actor[212])));
                    ensure!(
                        model.min == f32::from(animation.model) && model.max == model.min,
                        "effect animation selects a different model"
                    );
                }
                for base in [
                    0x34, 0x40, 0x4c, 0x58, 0x64, 0x70, 0x98, 0xa4, 0xb0, 0xbc, 0xc8,
                ] {
                    for axis in 0..3 {
                        let target = base + axis * 4;
                        let slot = Self::scalar_slot(birth, target, actor[0])?;
                        if !explicit_set.contains(&target)
                            && *slot == Some(ValueRange::fixed(row.float(usize::from(target))))
                        {
                            *slot = None;
                        }
                    }
                }
                let slot = Self::scalar_slot(birth, 0x84, actor[0])?;
                if !explicit_set.contains(&0x84)
                    && *slot == Some(ValueRange::fixed(row.float(0x84)))
                {
                    *slot = None;
                }
                return Ok(());
            }
            if opcode == 9 {
                let target = dol::u16(self.bytes, at + 2)?;
                let floats = (0..4)
                    .map(|i| dol::u32(self.bytes, at + 4 + i * 4).map(f32::from_bits))
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    floats.iter().all(|value| value.is_finite()),
                    "invalid effect polar modifier"
                );
                let radius_jitter = i16::from_be_bytes(self.bytes[at + 20..at + 22].try_into()?);
                let angle_jitter = i16::from_be_bytes(self.bytes[at + 22..at + 24].try_into()?);
                let spread = PolarSpread {
                    angles: floats[..3].try_into()?,
                    radius: floats[3],
                    radius_jitter: f32::from(radius_jitter).abs() * 0.1,
                    angle_jitter: f32::from(angle_jitter).abs() * 0.1,
                    radius_draw: (radius_jitter != 0).then(|| {
                        let draw = state.draws + i64::from(birth.random_draws);
                        birth.random_draws += 1;
                        draw
                    }),
                    angle_draw: (angle_jitter != 0).then(|| {
                        let draw = state.draws + i64::from(birth.random_draws);
                        birth.random_draws += 1;
                        draw
                    }),
                };
                match target {
                    0x34 => {
                        birth.offset_spread = Some(spread);
                        birth.offset = [None; 3];
                    }
                    0x40 => {
                        birth.velocity_spread = Some(spread);
                        birth.velocity = [None; 3];
                    }
                    _ => anyhow::bail!("unsupported effect polar destination {target:#x}"),
                }
                at += 24;
                continue;
            }
            if opcode == 22 {
                ensure!(
                    actor[0] == 3 && row.word(20) & 0x20 != 0,
                    "effect animation requires program-owned model playback"
                );
                let animation = self.model_animation(at)?;
                ensure!(
                    birth.animation.replace(animation).is_none(),
                    "duplicate effect animation start"
                );
                at += 16;
                continue;
            }
            if opcode == 1 {
                let target = dol::u16(self.bytes, at + 2)?;
                for axis in 0..3 {
                    let offset = target + axis * 4;
                    let value =
                        f32::from_bits(dol::u32(self.bytes, at + 4 + usize::from(axis) * 4)?);
                    ensure!(value.is_finite(), "non-finite effect vector modifier");
                    *Self::scalar_slot(birth, offset, actor[0])? = Some(ValueRange::fixed(value));
                    explicit_set.insert(offset);
                }
                at += 16;
                continue;
            }
            if opcode == 18 {
                let target = dol::u16(self.bytes, at + 2)?;
                let channels = match target {
                    0x18 => &mut birth.color,
                    0x20 => &mut birth.end_color,
                    _ => anyhow::bail!("unsupported packed effect color destination"),
                };
                for (channel, value) in channels.iter_mut().enumerate() {
                    let raw = dol::u16(self.bytes, at + 4 + channel * 2)?;
                    ensure!(raw < 0x7ff8, "dynamic packed effect color is unsupported");
                    *value = Some(ValueRange::fixed(f32::from(raw as i16)));
                }
                at += 12;
                continue;
            }
            if opcode == 26 {
                let words = (0..12)
                    .map(|i| dol::u16(self.bytes, at + i * 2))
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    words[0..4] == [26, 0x40, 0, 0]
                        && (0x7ff8..=0x7ffb).contains(&words[4])
                        && words[5..] == [0, 27, 0x40, 0, 0, 0, 0]
                        && actor[0] == 3
                        && row.word(20) & 0x4000_0000 != 0
                        && row.word(20) & 4 != 0
                        && actor[145] & 2 != 0,
                    "unsupported held model frame"
                );
                let frame = state.scratch[usize::from(words[4] - 0x7ff8)]
                    .context("held model frame scratch is empty")?;
                ensure!(frame.min == frame.max, "held model frame is variable");
                birth.animation = Some(ModelAnimation {
                    model: actor[212],
                    clip: u16::from(actor[213]),
                    frame: frame.min,
                    blend_ticks: 0,
                    rate: 0.,
                    repeat: true,
                });
                at += 24;
                continue;
            }
            if matches!(opcode, 0 | 2 | 3 | 4 | 5 | 10 | 12 | 23 | 25) {
                let target = dol::u16(self.bytes, at + 2)?;
                if opcode == 25 {
                    ensure!(
                        (0x2c..=0x2f).contains(&target),
                        "unsupported effect integer division destination"
                    );
                }
                let operand = dol::u16(self.bytes, at + 4)?;
                let value = if opcode == 2 {
                    ensure!(
                        (1..0x7ff8).contains(&operand),
                        "invalid effect integer random range"
                    );
                    let draw = state.draws + i64::from(birth.random_draws);
                    birth.random_draws += 1;
                    ValueRange {
                        min: 0.,
                        max: f32::from(operand - 1),
                        step: 1.,
                        draw: Some(draw),
                        reverse: false,
                    }
                } else if (0x7ffc..=0x7fff).contains(&operand) {
                    state.integers[usize::from(operand - 0x7ffc)]
                        .context("effect integer scratch is empty")?
                } else {
                    ValueRange::fixed(f32::from(operand as i16))
                };
                if (0x7ff8..=0x7ffb).contains(&target) {
                    ensure!(
                        matches!(opcode, 0 | 2 | 3 | 5),
                        "unsupported effect scalar high-half edit"
                    );
                    // A halfword write invalidates the float scratch value. The
                    // operand's random draw above still advances draw order.
                    state.scratch[usize::from(target - 0x7ff8)] = None;
                    at += 8;
                    continue;
                }
                if target == 0x60 && opcode == 3 {
                    let before = birth.angles[2].unwrap_or(ValueRange::fixed(row.float(0x60)));
                    ensure!(
                        before.min == before.max && value.min == value.max && value.draw.is_none(),
                        "variable effect roll halfword"
                    );
                    let bits = before.min.to_bits();
                    let high = i32::try_from(bits >> 16)? + value.min as i32;
                    let high = u16::try_from(high).context("effect roll halfword overflows")?;
                    birth.angles[2] = Some(ValueRange::fixed(f32::from_bits(
                        u32::from(high) << 16 | bits & 0xffff,
                    )));
                    explicit_set.insert(target);
                    at += 8;
                    continue;
                }
                if target == 0x8c && opcode == 10 {
                    ensure!(
                        value.min == value.max && (0. ..=255.).contains(&value.min),
                        "variable effect attachment index"
                    );
                    if row.word(20) & (0x4000_0000 | 0x2000_0000) != 0 {
                        birth.attachment_bone = Some(value.min as u8);
                    } else {
                        ensure!(
                            row.word(20) & 0x200 != 0 && value.min < 16.,
                            "effect group is absent"
                        );
                        birth.effect_group = Some(value.min as u8);
                    }
                    at += 8;
                    continue;
                }
                if inherited
                    && matches!(opcode, 3 | 4)
                    && (8..=14).contains(&target)
                    && target.is_multiple_of(2)
                {
                    let axis = usize::from((target - 8) / 2);
                    let field = BirthField::Uv(axis as u8);
                    let before = birth.uv[axis]
                        .unwrap_or(ValueRange::fixed(f32::from(row.short(usize::from(target)))));
                    let existing = birth.sums.iter().position(|sum| sum.field == field);
                    if existing.is_some()
                        || (before.min != before.max
                            && value.min != value.max
                            && before.draw != value.draw)
                    {
                        let term = if opcode == 4 {
                            ValueRange {
                                min: -value.max,
                                max: -value.min,
                                step: value.step,
                                draw: value.draw,
                                reverse: !value.reverse,
                            }
                        } else {
                            value
                        };
                        if let Some(index) = existing {
                            birth.sums[index].terms.push(term);
                        } else {
                            birth.sums.push(BirthSum {
                                field,
                                terms: vec![before, term],
                            });
                        }
                        birth.uv[axis] = None;
                        at += 8;
                        continue;
                    }
                }
                let slot = if (0x7ffc..=0x7fff).contains(&target) {
                    &mut state.integers[usize::from(target - 0x7ffc)]
                } else if (8..=14).contains(&target) && target.is_multiple_of(2) {
                    &mut birth.uv[usize::from((target - 8) / 2)]
                } else if (0x18..=0x26).contains(&target) && target.is_multiple_of(2) {
                    let channel = usize::from((target - 0x18) / 2);
                    if channel < 4 {
                        &mut birth.color[channel]
                    } else {
                        &mut birth.end_color[channel - 4]
                    }
                } else if target == 0x12 {
                    &mut birth.geometry_count
                } else if target == 0x13 {
                    &mut birth.phase
                } else if opcode == 3 && target == 3 {
                    &mut birth.alpha_palette
                } else if target == 3 {
                    &mut birth.palette
                } else if target == 0x10 {
                    &mut birth.lifetime
                } else if target == 0xd4 {
                    &mut birth.model
                } else if target == 0x8d {
                    &mut birth.uv[1]
                } else if target == 0x30 {
                    &mut birth.brighten_until
                } else if target == 0x31 {
                    &mut birth.fade_from
                } else if (0x28..=0x2b).contains(&target) {
                    &mut birth.brighten[usize::from(target - 0x28)]
                } else if (0x2c..=0x2f).contains(&target) {
                    &mut birth.fade[usize::from(target - 0x2c)]
                } else {
                    anyhow::bail!("unsupported effect integer destination {target:#x}")
                };
                let initial = slot.unwrap_or_else(|| {
                    if (8..=14).contains(&target) {
                        ValueRange::fixed(f32::from(row.short(usize::from(target))))
                    } else if (0x18..=0x26).contains(&target) {
                        ValueRange::fixed(f32::from(row.short(usize::from(target))))
                    } else if opcode == 3 && target == 3 {
                        ValueRange::fixed(f32::from(actor[4]))
                    } else if target == 0x12 || target == 0x13 || target == 3 {
                        ValueRange::fixed(f32::from(actor[usize::from(target)]))
                    } else if target == 0x10 {
                        ValueRange::fixed(f32::from(row.short(16)))
                    } else if target == 0xd4 {
                        ValueRange::fixed(f32::from(actor[212]))
                    } else if target == 0x31 || (0x28..=0x2f).contains(&target) {
                        ValueRange::fixed(f32::from(actor[usize::from(target)]))
                    } else {
                        ValueRange::fixed(0.)
                    }
                });
                let result = match opcode {
                    3 | 12 | 4 | 23 => {
                        let sign = if matches!(opcode, 4 | 23) { -1. } else { 1. };
                        ValueRange {
                            min: initial.min + sign * value.min,
                            max: initial.max + sign * value.max,
                            step: if value.step != 0. {
                                value.step
                            } else {
                                initial.step
                            },
                            draw: value.draw.or(initial.draw),
                            reverse: value.reverse ^ initial.reverse,
                        }
                    }
                    5 => {
                        ensure!(
                            value.min == value.max || initial.min == initial.max,
                            "nonlinear integer variation needs decoding"
                        );
                        let (a, b) = if value.min == value.max {
                            (initial.min * value.min, initial.max * value.min)
                        } else {
                            (initial.min * value.min, initial.min * value.max)
                        };
                        ValueRange {
                            min: a.min(b),
                            max: a.max(b),
                            step: if value.min == value.max {
                                initial.step * value.min.abs()
                            } else {
                                value.step * initial.min.abs()
                            },
                            draw: value.draw.or(initial.draw),
                            reverse: initial.reverse ^ value.reverse ^ (a > b),
                        }
                    }
                    25 => {
                        ensure!(
                            initial.min == initial.max && value.min == value.max && value.min != 0.,
                            "varying effect integer division is unsupported"
                        );
                        ValueRange::fixed((initial.min / value.min).trunc())
                    }
                    _ => value,
                };
                result.validate()?;
                if opcode == 3 && target == 3 {
                    ensure!(
                        result.min >= 0. && result.max <= 255.,
                        "effect alpha palette edit crosses native byte range"
                    );
                }
                *slot = Some(result);
                at += 8;
                continue;
            }
            if opcode == 21 {
                let flags = dol::u32(self.bytes, at + 4)?;
                ensure!(
                    flags & !0x10400020 == 0,
                    "unsupported effect modifier flags"
                );
                if flags & 0x20 != 0 {
                    ensure!(actor[0] == 18, "effect depth modifier requires a ribbon");
                    birth.ribbon_depth = true;
                }
                birth.cull_back |= flags & 0x10000000 != 0;
                birth.element_tint |= flags & 0x400000 != 0;
                at += 8;
                continue;
            }
            ensure!(
                matches!(opcode, 7 | 8 | 11 | 13 | 14 | 15),
                "unsupported effect modifier opcode {opcode}"
            );
            let target = dol::u16(self.bytes, at + 2)?;
            if opcode == 7 && target == 0x80 {
                ensure!(
                    actor[0] == 4
                        && row.word(20) & 0x4000_0000 != 0
                        && dol::u32(self.bytes, at + 8)? == 0
                        && f32::from_bits(dol::u32(self.bytes, at + 4)?).is_finite(),
                    "unsupported owner-bone auxiliary scalar"
                );
                at += 12;
                continue;
            }
            if opcode == 7 || opcode == 11 {
                explicit_set.insert(target);
            }
            let value = if opcode == 11 {
                let range = i16::from_be_bytes(self.bytes[at + 4..at + 6].try_into()?);
                let extent = (f32::from(range).abs() - 1.) * 0.1;
                let draw = state.draws + i64::from(birth.random_draws);
                birth.random_draws += 1;
                ValueRange {
                    min: if range == 0 {
                        f32::from(i16::MIN) * 0.1
                    } else {
                        -extent
                    },
                    max: if range == 0 {
                        f32::from(i16::MAX) * 0.1
                    } else {
                        extent
                    },
                    step: if range == 1 || range == -1 { 0. } else { 0.1 },
                    draw: Some(draw),
                    reverse: false,
                }
            } else {
                let source = dol::u16(self.bytes, at + 8)?;
                if (0x7ff8..=0x7ffb).contains(&source) {
                    explicit_set.insert(target);
                    state.scratch[usize::from(source - 0x7ff8)]
                        .context("effect modifier reads empty scalar scratch")?
                } else {
                    ensure!(
                        dol::u32(self.bytes, at + 8)? == 0,
                        "dynamic effect modifier needs decoding"
                    );
                    let value = f32::from_bits(dol::u32(self.bytes, at + 4)?);
                    ensure!(value.is_finite(), "non-finite effect modifier");
                    ValueRange::fixed(value)
                }
            };
            let initial = if (0x7ff8..=0x7ffb).contains(&target) {
                state.scratch[usize::from(target - 0x7ff8)]
            } else {
                *Self::scalar_slot(birth, target, actor[0])?
            };
            let initial = initial.or_else(|| {
                (opcode != 7 && opcode != 11 && target < 0x7ff8)
                    .then(|| ValueRange::fixed(row.float(usize::from(target))))
            });
            let yaw_heading = if actor[0] == 3 && target == 0x5c {
                Some(if birth.angles[1].is_some() {
                    birth.yaw_heading
                } else {
                    1.
                })
            } else {
                None
            };
            let sum_field = if (0x34..=0x3c).contains(&target) {
                Some(BirthField::Offset(((target - 0x34) / 4) as u8))
            } else if (0x40..=0x48).contains(&target) {
                Some(BirthField::Velocity(((target - 0x40) / 4) as u8))
            } else if (0xb0..=0xb8).contains(&target) {
                Some(BirthField::Size(((target - 0xb0) / 4) as u8))
            } else {
                None
            }
            .filter(|_| target.is_multiple_of(4));
            if inherited
                && matches!(opcode, 8 | 13)
                && let Some(field) = sum_field
            {
                let before = initial.context("grouped effect has no initial scalar")?;
                let existing = birth.sums.iter().position(|sum| sum.field == field);
                let independent =
                    before.min != before.max && value.min != value.max && before.draw != value.draw;
                if independent || existing.is_some() {
                    let term = if opcode == 13 {
                        ValueRange {
                            min: -value.max,
                            max: -value.min,
                            step: value.step,
                            draw: value.draw,
                            reverse: !value.reverse,
                        }
                    } else {
                        value
                    };
                    if let Some(index) = existing {
                        birth.sums[index].terms.push(term);
                    } else {
                        birth.sums.push(BirthSum {
                            field,
                            terms: vec![before, term],
                        });
                    }
                    *Self::scalar_slot(birth, target, actor[0])? = None;
                    explicit_set.insert(target);
                    at += 12;
                    continue;
                }
            }
            let mut result = if opcode == 7 || opcode == 11 {
                value
            } else if opcode == 8 || opcode == 13 {
                let initial = initial.context("effect scratch arithmetic before initialization")?;
                ensure!(
                    initial.min == initial.max || value.min == value.max,
                    "independent effect variations need decoding"
                );
                let sign = if opcode == 13 { -1. } else { 1. };
                let (value_start, value_end) = if value.reverse {
                    (value.max, value.min)
                } else {
                    (value.min, value.max)
                };
                let (start, end) = if initial.reverse {
                    (initial.max, initial.min)
                } else {
                    (initial.min, initial.max)
                };
                let a = start + sign * value_start;
                let b = end + sign * value_end;
                ValueRange {
                    min: a.min(b),
                    max: a.max(b),
                    step: if initial.min == initial.max {
                        value.step
                    } else {
                        initial.step
                    },
                    draw: value.draw.or(initial.draw),
                    reverse: a > b,
                }
            } else {
                let initial = initial.context("effect scratch multiply before initialization")?;
                ensure!(
                    value.min == value.max && (opcode != 15 || value.min != 0.),
                    "nonlinear effect variation needs decoding"
                );
                let factor = if opcode == 15 {
                    1. / value.min
                } else {
                    value.min
                };
                let a = initial.min * factor;
                let b = initial.max * factor;
                ValueRange {
                    min: a.min(b),
                    max: a.max(b),
                    step: initial.step * factor.abs(),
                    draw: initial.draw,
                    reverse: initial.reverse ^ factor.is_sign_negative(),
                }
            };
            // The native draw still advances `random_draws`, but a collapsed
            // range carries no randomness into the authored birth value.
            if result.min == result.max && result.draw.is_some() && !result.reverse {
                result = ValueRange::fixed(result.min);
            }
            result.validate()?;
            if let Some(heading) = yaw_heading {
                birth.yaw_heading = match opcode {
                    7 | 11 => 0.,
                    14 => heading * value.min,
                    15 => heading / value.min,
                    _ => heading,
                };
                ensure!(
                    birth.yaw_heading.is_finite(),
                    "non-finite effect model yaw heading"
                );
            }
            if (0x7ff8..=0x7ffb).contains(&target) {
                state.scratch[usize::from(target - 0x7ff8)] = Some(result);
            } else {
                explicit_set.insert(target);
                *Self::scalar_slot(birth, target, actor[0])? = Some(result);
            }
            at += if opcode == 11 { 8 } else { 12 };
        }
    }

    fn delayed_motion(&self, pointer: u16) -> Result<MotionChange> {
        let mut at = usize::from(pointer);
        let mut motion = MotionChange::default();
        loop {
            let opcode = dol::u16(self.bytes, at)?;
            if opcode == 65535 {
                return Ok(motion);
            }
            if opcode == 22 {
                let animation = self.model_animation(at)?;
                ensure!(
                    motion.animation.replace(animation).is_none(),
                    "duplicate delayed effect animation"
                );
                at += 16;
                continue;
            }
            ensure!(
                matches!(opcode, 7 | 8 | 14 | 30) && dol::u32(self.bytes, at + 8)? == 0,
                "unsupported delayed effect motion edit"
            );
            let target = dol::u16(self.bytes, at + 2)?;
            let value = f32::from_bits(dol::u32(self.bytes, at + 4)?);
            ensure!(value.is_finite(), "non-finite delayed effect motion");
            if opcode == 30 {
                let forward = motion.forward_motion.get_or_insert_default();
                match target {
                    0x40 => forward.velocity = Some(value),
                    0x4c => forward.acceleration = Some(value),
                    _ => anyhow::bail!("unsupported delayed forward target {target:#x}"),
                }
                at += 12;
                continue;
            }
            if matches!(opcode, 8 | 14) {
                ensure!(
                    (0x40..=0x48).contains(&target) && (target - 0x40).is_multiple_of(4),
                    "unsupported delayed effect velocity arithmetic target {target:#x}"
                );
                let axis = usize::from((target - 0x40) / 4);
                if opcode == 8 {
                    motion.velocity_add.get_or_insert([0.; 3])[axis] += value;
                } else {
                    motion.velocity_scale.get_or_insert([1.; 3])[axis] *= value;
                    if let Some(add) = &mut motion.velocity_add {
                        add[axis] *= value;
                    }
                }
                at += 12;
                continue;
            }
            let vectors: [(u16, &mut [Option<f32>; 3]); 7] = [
                (0x34, &mut motion.offset),
                (0x40, &mut motion.velocity),
                (0x4c, &mut motion.acceleration),
                (0x64, &mut motion.angular_velocity),
                (0xb0, &mut motion.size),
                (0xbc, &mut motion.size_velocity),
                (0xc8, &mut motion.size_acceleration),
            ];
            let mut found = false;
            for (base, vector) in vectors {
                if (base..=base + 8).contains(&target) && (target - base).is_multiple_of(4) {
                    vector[usize::from((target - base) / 4)] = Some(value);
                    found = true;
                    break;
                }
            }
            ensure!(
                found,
                "unsupported delayed effect motion target {target:#x}"
            );
            if (0x40..=0x48).contains(&target) {
                let axis = usize::from((target - 0x40) / 4);
                if let Some(add) = &mut motion.velocity_add {
                    add[axis] = 0.;
                }
                if let Some(scale) = &mut motion.velocity_scale {
                    scale[axis] = 1.;
                }
            }
            at += 12;
        }
    }

    pub fn simple_program(&self, index: usize) -> Result<Vec<ScheduledEvent>> {
        self.program(index, None)
    }

    pub fn grouped_program(&self, index: usize, bones: &[String]) -> Result<Vec<ScheduledEvent>> {
        self.program(index, Some(bones))
    }

    fn program(&self, index: usize, bones: Option<&[String]>) -> Result<Vec<ScheduledEvent>> {
        ensure!(index < self.count, "effect program is absent");
        let start = usize::from(dol::u16(self.roots, index * 2)?);
        let commands = self
            .commands
            .get(start..)
            .context("effect command root exceeds bank")?;
        let mut at = 0;
        let mut pending: Vec<(u32, Option<(u16, u32)>, u8, u8, u16)> = Vec::new();
        let mut rows = commands.chunks_exact(6);
        while let Some(row) = rows.next() {
            let age = i16::from_be_bytes(row[..2].try_into()?);
            at = at.max(age.max(0) as u32);
            let command = row[2];
            if command == 254 {
                let end = at;
                let mut events = Vec::new();
                for (order, (at, repeat, command, argument, operand)) in
                    pending.into_iter().enumerate()
                {
                    if let Some((count, interval)) = repeat {
                        for emission in 0..count {
                            let tick = at + u32::from(emission) * interval;
                            if tick < end {
                                events.push((tick, true, order, command, argument, operand));
                            }
                        }
                    } else {
                        events.push((at, false, order, command, argument, operand));
                    }
                }
                events.sort_by_key(|(tick, repeated, order, _, _, _)| (*tick, *repeated, *order));
                let mut expanded = Vec::new();
                for (at, repeated, order, command, argument, operand) in events {
                    if command < 252 && (250..=253).contains(&argument) {
                        ensure!(!repeated, "repeated effect origin groups are unsupported");
                        let bones = bones.context("effect origin group has no owner bones")?;
                        let prefix = format!("ef{:02}", argument - 250);
                        let mut first = true;
                        for (bone, name) in bones.iter().enumerate() {
                            if name.starts_with(&prefix) {
                                ensure!(
                                    (1..128).contains(&bone),
                                    "effect origin group bone is out of range"
                                );
                                expanded.push((
                                    at, repeated, order, command, bone as u8, operand, !first,
                                ));
                                first = false;
                            }
                        }
                        ensure!(!first, "effect origin group {prefix} is empty");
                    } else {
                        expanded.push((at, repeated, order, command, argument, operand, false));
                    }
                }
                let mut state = EditState::default();
                let mut retained_spawns = Vec::new();
                let mut spawns = 0;
                let mut previous_birth: Option<ParticleBirth> = None;
                return expanded.into_iter().filter(|(_, _, _, command, _, _, _)| {
                    *command >= 252 || self.actor(*command).map(|actor| {
                        actor[0] != 0 || !actor.iter().enumerate().all(|(index, &byte)| byte == if index == 50 { 255 } else { 0 })
                    }).unwrap_or(true)
                }).map(|(at, _, _, command, argument, operand, inherited)| {
                    let operation = if command == 252 {
                        EffectOperation::Sound { id: u16::from(argument), priority: operand as u8 }
                    } else if command == 253 {
                        EffectOperation::SetMotion {
                            spawn: *retained_spawns.get(usize::from(argument))
                                .context("delayed effect refers to missing retained spawn")?,
                            motion: Box::new(self.delayed_motion(operand)?),
                        }
                    } else {
                        ensure!(command < 252, "effect program requires retained edit");
                        let actor = self.actor(command)?;
                        if actor[0] == 22 {
                            EffectOperation::Shake {
                                duration: u32::try_from(Row(actor).short(16))?,
                                amplitude: Row(actor).word(20),
                            }
                        } else if actor[0] == 24 {
                            let Declaration::StageTint { color, duration } = self.simple_particle(command)? else { unreachable!() };
                            EffectOperation::StageTint { color, duration }
                        } else if actor[0] == 25 {
                            let Declaration::Notice { text } = self.simple_particle(command)? else { unreachable!() };
                            EffectOperation::Notice { text }
                        } else {
                        ensure!(argument < 128, "unsupported effect origin selector {argument} for actor {command}");
                        ensure!(actor[0] != 0, "effect spawn refers to an empty actor");
                        let mut birth = if inherited {
                            let mut previous = previous_birth.clone().context("grouped effect has no predecessor")?;
                            if let Some(lifetime) = &mut previous.lifetime {
                                if lifetime.min > 0. { lifetime.min -= 1.; lifetime.max -= 1.; }
                            }
                            previous.random_draws = 0;
                            previous
                        } else {
                            ParticleBirth::default()
                        };
                        birth.origin_bone = (argument != 0).then_some(argument);
                        let (mut blend, mut palette) = (None, None);
                        let mut edits = usize::from(operand);
                        if edits != 0 {
                            while dol::u16(self.bytes, edits)? == 10
                                && matches!(dol::u16(self.bytes, edits + 2)?, 1 | 3)
                            {
                                let target = dol::u16(self.bytes, edits + 2)?;
                                let value = u8::try_from(dol::u16(self.bytes, edits + 4)?)?;
                                if target == 1 { ensure!(value <= 1, "unsupported effect blend"); blend = Some(value); }
                                else { palette = Some(value); }
                                edits += 8;
                            }
                            self.scalar_edits(actor, u16::try_from(edits)?, &mut birth, &mut state, inherited)?;
                        }
                        if let Some(lifetime) = &mut birth.lifetime {
                            if lifetime.min > 0. { lifetime.min += 1.; lifetime.max += 1.; }
                        }
                        if actor[0] == 3 && matches!(blend.unwrap_or(actor[1]), 1 | 2)
                            && let Some(animation) = &mut birth.animation
                        {
                            animation.frame += animation.rate;
                        }
                        if Row(actor).word(20) & 0x20000 != 0 {
                            birth.model_parent = Some(ModelParent {
                                spawn: *retained_spawns.get(usize::from(actor[141]))
                                    .context("effect model attachment has no earlier retained birth")?,
                                joint: actor[140],
                            });
                        }
                        state.draws += i64::from(birth.random_draws);
                        if Row(actor).word(20) & 0x40000 != 0 { retained_spawns.push(spawns); }
                        spawns += 1;
                        previous_birth = Some(birth.clone());
                        EffectOperation::Spawn { particle: command, blend, palette, birth: Box::new(birth) }
                        }
                    };
                    Ok(ScheduledEvent { at, operation })
                }).collect();
            }
            let repeat = if command == 255 {
                Some((
                    u16::from(row[3].max(1)),
                    (dol::u16(row, 4)? as i16).max(0) as u32,
                ))
            } else {
                None
            };
            let row = if repeat.is_some() {
                rows.next().context("effect repeat has no payload")?
            } else {
                row
            };
            let command = row[2];
            let argument = row[3];
            let operand = dol::u16(row, 4)?;
            pending.push((at, repeat, command, argument, operand));
        }
        anyhow::bail!("unterminated effect program")
    }
}

pub(crate) fn label(declaration: &Declaration) -> String {
    if let Declaration::ModelParticle { visual, .. } = declaration {
        let blend = match visual.blend {
            0 => "alpha",
            1 => "additive",
            2 => "subtractive",
            _ => "effect",
        };
        return format!("{blend}-model");
    }
    let Declaration::Particle { visual, .. } = declaration else {
        return "effect".into();
    };
    let blend = match visual.blend {
        0 => "alpha",
        1 => "additive",
        2 => "subtractive",
        _ => "effect",
    };
    let facing = match visual.orientation {
        Orientation::Billboard => "billboard-",
        Orientation::Camera => "camera-",
        Orientation::FollowMotion => "motion-",
        _ => "",
    };
    let shape = match visual.geometry {
        ParticleShape::Ring { .. } => "ring",
        ParticleShape::Quad => "quad",
        ParticleShape::Hemisphere { .. } => "hemisphere",
        ParticleShape::Orbit => "orbit",
        ParticleShape::BillboardTrail => "billboard-trail",
        ParticleShape::RadialQuads { .. } => "radial-quads",
        ParticleShape::Spiral { .. } => "spiral",
        ParticleShape::Disc { .. } => "disc",
        ParticleShape::Shell { .. } => "shell",
        ParticleShape::Sphere { .. } => "sphere",
        ParticleShape::Ribbon { .. } => "ribbon",
        ParticleShape::VertexQuad { .. } => "vertex-quad",
        ParticleShape::TriangleBand => "triangle-band",
        ParticleShape::Unsupported { .. } => "unsupported-geometry",
        _ => "effect",
    };
    format!("{blend}-{facing}{shape}")
}

pub(crate) fn name_programs(
    bank: &NativeBank<'_>,
    programs: &mut [Vec<ScheduledEvent>],
    include: &[u8],
    mut bind: impl FnMut(u8, &mut Declaration) -> Result<()>,
) -> Result<(Vec<String>, Vec<Declaration>)> {
    let mut used = BTreeSet::from_iter(include.iter().copied());
    for event in programs.iter().flatten() {
        if let EffectOperation::Spawn { particle, .. } = event.operation {
            used.insert(particle);
        }
    }
    let mut templates = Vec::new();
    let mut names = Vec::new();
    let mut remap = BTreeMap::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for actor in used {
        let mut declaration = bank.simple_particle(actor)?;
        bind(actor, &mut declaration)?;
        let special = include.contains(&actor) && actor == 19;
        if !special && let Some(index) = templates.iter().position(|old| old == &declaration) {
            remap.insert(actor, u8::try_from(index)?);
            continue;
        }
        let base = if special {
            STUN_BIRD.to_string()
        } else {
            label(&declaration)
        };
        let number = counts.entry(base.clone()).or_default();
        *number += 1;
        names.push(if *number == 1 {
            base
        } else {
            format!("{base}-{number}")
        });
        remap.insert(actor, u8::try_from(templates.len())?);
        templates.push(declaration);
    }
    for event in programs.iter_mut().flatten() {
        if let EffectOperation::Spawn { particle, .. } = &mut event.operation {
            *particle = remap[particle];
        }
    }
    Ok((names, templates))
}

pub fn cook_common_effects(root: &Path, library: &Path) -> Result<()> {
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let bank = NativeBank::read(battle_effect_media::member(&usual, 2)?)?;
    let technique = NativeBank::read(battle_effect_media::member(&usual, 3)?)?;
    let (textures, sets) = resident_atlas(&usual, &bank, &technique)?;
    let mut programs = (0..bank.count)
        .map(|index| bank.simple_program(index))
        .collect::<Result<Vec<_>>>()?;
    let (template_names, templates) =
        name_programs(&bank, &mut programs, &[19], |actor, declaration| {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas {
                    slot, palette_set, ..
                } = &mut visual.texture
            {
                *palette_set = *sets
                    .get(&(*slot, bank.actor(actor)?[6]))
                    .context("effect atlas palette binding is missing")?;
            }
            Ok(())
        })?;
    let definition = Definition {
        programs,
        template_names,
        templates,
        artwork: Some(Artwork {
            textures,
            models: battle_effect_media::model_artwork(&usual)?,
        }),
    };
    let target = library.join("battle/effects/common/effects.json");
    crate::output::json(&target, &definition)?;
    Ok(())
}

fn resident_atlas(
    usual: &[u8],
    common: &NativeBank<'_>,
    technique: &NativeBank<'_>,
) -> Result<(
    BTreeMap<u8, Vec<resonance_content::battle_effect::EffectTexture>>,
    BTreeMap<(u8, u8), u8>,
)> {
    let mut bindings = Vec::new();
    for source in [common, technique] {
        for (index, actor) in source.actors.chunks_exact(352).enumerate() {
            if let Ok(Declaration::Particle { visual, .. }) =
                source.simple_particle(u8::try_from(index)?)
                && let ParticleTexture::Atlas { slot, .. } = visual.texture
            {
                bindings.push((slot, actor[6]));
            }
        }
    }
    // Stored scene effects also select this resident window (for example,
    // Dreaded Wave). It must exist in the common artwork before scene lookup.
    bindings.push((0, 32));
    battle_effect_media::atlas_artwork(usual, &bindings)
}

/// Resolve local effect actor strides against the resident atlas definition.
/// This uses the same palette deduplication as the common bank but writes no media.
pub(crate) fn resident_palette_sets(root: &Path) -> Result<BTreeMap<(u8, u8), u8>> {
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let common = NativeBank::read(battle_effect_media::member(&usual, 2)?)?;
    let technique = NativeBank::read(battle_effect_media::member(&usual, 3)?)?;
    Ok(resident_atlas(&usual, &common, &technique)?.1)
}

pub fn cook_technique_effects(root: &Path, library: &Path) -> Result<()> {
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let common = NativeBank::read(battle_effect_media::member(&usual, 2)?)?;
    let bank = NativeBank::read(battle_effect_media::member(&usual, 3)?)?;
    let (textures, sets) = resident_atlas(&usual, &common, &bank)?;
    let mut programs = (0..bank.count)
        .map(|index| bank.simple_program(index))
        .collect::<Result<Vec<_>>>()?;
    let (template_names, templates) =
        name_programs(&bank, &mut programs, &[], |actor, declaration| {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas {
                    slot, palette_set, ..
                } = &mut visual.texture
            {
                *palette_set = *sets
                    .get(&(*slot, bank.actor(actor)?[6]))
                    .context("effect atlas palette binding is missing")?;
            }
            Ok(())
        })?;
    let definition = Definition {
        programs,
        template_names,
        templates,
        artwork: Some(Artwork {
            textures,
            models: battle_effect_media::model_artwork(&usual)?,
        }),
    };
    let target = library.join("battle/effects/techniques/effects.json");
    crate::output::json(&target, &definition)?;
    Ok(())
}

/// Convert the shared resident effect library once, with one owner for each file.
pub fn cook_battle_effects(root: &Path, library: &Path) -> Result<()> {
    battle_effect_tints::cook_battle_effect_tints(root, library)?;
    battle_effect_media::cook_battle_effect_atlases(root, library)?;
    battle_effect_media::cook_battle_effect_models(root, library)?;
    cook_common_effects(root, library)?;
    cook_technique_effects(root, library)
}
