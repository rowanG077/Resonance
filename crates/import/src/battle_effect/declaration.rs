//! Decode the fixed-size effect records into runtime recipes.
use super::source::{UvRecord, uv_animation};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    ModelAttachment, ModelBinding, ParticleGeometry, ParticleState, ParticleTemplate,
    declaration::Declaration,
    visual::{ModelVisual, Orientation, ParticleShape, ParticleTexture, ParticleVisual, UvLayout},
};

const ACTOR_BYTES: usize = 352;
const MODEL_JOINT: u32 = 0x0002_0000;
const OWNER_BONE: u32 = 0x4000_0000;
const MODEL_ANIMATION: u32 = 0x4;
const MODEL_ELEVATION: u32 = 0x2;
const PROGRAM_ANIMATION: u32 = 0x20;
const FLOOR_CLAMP: u32 = 0x0800_0000;
const FOLLOW_BONE_POSITION: u32 = 0x2000_0000;
const FOLLOW_ORIGIN: u32 = 0x400;
const DUAL_TEXTURE: u32 = 0x0400_0000;
const GRADIENT: u32 = 0x8;
const ANCHORED: u32 = 0x2;
const BILLBOARD: u32 = 0x40;
const RETAINED_PARTICLE: u32 = 0x0004_0000;
const CYCLING_UV: u32 = 0x10;
const EFFECT_GROUP: u32 = 0x200;
const TARGET_DRAW_ORDER: u32 = 0x0020_2000;
const PROJECTILE_MODEL_CONTROL: u8 = 1;
const PROJECTILE_MODEL_END: u8 = 45;

pub(super) fn read(bytes: &[u8], uv: &[UvRecord]) -> Result<Declaration> {
    ensure!(bytes.len() == ACTOR_BYTES, "truncated effect record");
    Ok(
        recipe(Row(bytes), uv).unwrap_or_else(|error| Declaration::Unsupported {
            reason: error.to_string(),
        }),
    )
}

pub(super) fn empty(bytes: &[u8]) -> bool {
    bytes.len() == ACTOR_BYTES
        && bytes
            .iter()
            .enumerate()
            .all(|(index, &value)| value == if index == 50 { 255 } else { 0 })
}

fn recipe(row: Row<'_>, uv: &[UvRecord]) -> Result<Declaration> {
    let bytes = row.0;
    let kind = bytes[0];
    if kind == 0 {
        ensure!(empty(bytes), "unsupported empty particle payload");
    }
    if kind == 22 {
        return Ok(Declaration::CameraShake {
            duration: u32::try_from(row.short(16)).context("negative camera shake duration")?,
            amplitude: row.word(20),
        });
    }
    if kind == 24 {
        let duration = row.short(16);
        let color = row.color(24);
        ensure!(duration > 0, "invalid stage tint duration");
        ensure!(
            row.word(20) == 0 && color[3] == 64,
            "unsupported stage tint controls"
        );
        let channel = |i| u8::try_from(color[i]).context("invalid stage tint color");
        return Ok(Declaration::StageTint {
            color: [channel(0)?, channel(1)?, channel(2)?],
            duration: duration as u16,
        });
    }
    if kind == 25 {
        let bytes = crate::read::c_string(bytes, 176)?;
        let (text, invalid) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
        ensure!(!invalid, "invalid effect notice text");
        ensure!(
            !text.is_empty() && !text.chars().any(char::is_control),
            "empty or invalid effect notice"
        );
        return Ok(Declaration::Notice {
            text: text.into_owned(),
        });
    }
    let flags = row.word(20);
    let slot = bytes[2];
    let scene_sample = slot == 10;
    if scene_sample {
        ensure!(
            kind == 4 && bytes[1] == 0 && bytes[142] == 0 && flags & (0x800000 | 0x10000000) == 0,
            "unsupported scene-sampling particle"
        );
    }
    // These fixed models are born by, follow, and retire with their projectile.
    // The model's control endpoint is not a standalone particle duration.
    let projectile_model = kind == 3
        && flags == FOLLOW_ORIGIN | MODEL_ANIMATION
        && slot == 2
        && row.short(16) == 0
        && bytes[140..145] == [0; 5]
        && bytes[145..150] == [PROJECTILE_MODEL_CONTROL, 0, 0, PROJECTILE_MODEL_END, 0]
        && matches!(bytes[212], 4 | 5)
        && bytes[213] == 0;
    let periodic = kind == 3
        && flags == FOLLOW_ORIGIN | MODEL_ANIMATION | MODEL_ELEVATION
        && slot == 2
        && bytes[142] == 0
        && bytes[145] == 0x0a
        && bytes[147] > 0
        && bytes[148..150] == [0; 2];
    let owner_bone_model = kind == 3
        && slot == 2
        && bytes[142] == 0
        && ((flags == OWNER_BONE | FOLLOW_ORIGIN | PROGRAM_ANIMATION && bytes[145] == 0)
            || (flags == OWNER_BONE | MODEL_ANIMATION
                && bytes[140] == 10
                && bytes[141] == 0
                && bytes[145..150] == [2, 0, 0, 0, 0]
                && bytes[212..214] == [0, 0]));
    let follow_bone = bytes[142] == 0
        && match kind {
            3 => flags & !FOLLOW_ORIGIN == FOLLOW_BONE_POSITION && bytes[145] == 0,
            4 | 5 => {
                flags & FOLLOW_BONE_POSITION != 0
                    && flags
                        & !(FOLLOW_BONE_POSITION
                            | ANCHORED
                            | GRADIENT
                            | DUAL_TEXTURE
                            | BILLBOARD
                            | RETAINED_PARTICLE
                            | if scene_sample { 0x80000 } else { 0 }
                            | if kind == 4 {
                                FOLLOW_ORIGIN | CYCLING_UV
                            } else {
                                0
                            })
                        == 0
            }
            _ => false,
        };
    ensure!(
        matches!(
            kind,
            0 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 18 | 19
        ),
        "effect controller {kind} is not supported"
    );
    if kind == 3 {
        ensure!(
            (flags & !(0x100c35a7 | MODEL_JOINT | FLOOR_CLAMP) == 0
                || follow_bone
                || owner_bone_model)
                && flags & 0x24 != 0x24
                && matches!(slot, 0 | 2 | 6),
            "model particle requires an unsupported binding or attachment"
        );
    }
    ensure!(
        (flags & FOLLOW_BONE_POSITION == 0 || follow_bone || bone_ring(row))
            && flags & 0x8000 == 0
            && (flags & FLOOR_CLAMP == 0 || matches!(kind, 3 | 5))
            && (flags & 0x80 == 0
                || (kind == 3 && flags & 0x400 != 0)
                || (kind == 15
                    && flags & 0x400 != 0
                    && flags & (0x40 | 0x2000000 | 0x60000000) == 0)
                || (kind == 4 && flags & (0x400 | 0x40 | 0x2000000 | 0x60000000) == 0))
            && bytes[145]
                & !(if kind == 3 {
                    2 | if periodic { 8 } else { 0 }
                        | if projectile_model {
                            PROJECTILE_MODEL_CONTROL
                        } else {
                            0
                        }
                } else if (kind == 5 && flags & 1 != 0)
                    || (matches!(kind, 14 | 15) && flags & (1 | FLOOR_CLAMP | 0x400) == 0)
                {
                    4
                } else {
                    0
                })
                == 0,
        "particle requires an unsupported attachment or child controller"
    );
    let mut rectangle = row.color(8);
    let uv_animation = match bytes[50] {
        255 => None,
        index @ 0..=127 if kind == 3 => {
            let track = uv
                .get(usize::from(index))
                .context("missing model UV track")?;
            ensure!(
                track.timing == 128 && track.control == 0,
                "model UV controller is not prepared"
            );
            ensure!(
                track.values[2] > 0 && track.values[3] > 0,
                "invalid model UV scroll extent"
            );
            rectangle = [0, 0, track.values[2], track.values[3]];
            Some(resonance_content::battle_effect::UvAnimation::Scroll {
                origin: [0; 2],
                step: [track.values[0], track.values[1]],
                interval: 1,
            })
        }
        index @ 0..=127 => Some(uv_animation(
            uv.get(usize::from(index)..)
                .context("particle UV root outside row pool")?,
        )?),
        _ => anyhow::bail!("invalid particle UV selector"),
    };
    if kind == 3 && flags & MODEL_JOINT == 0 && !follow_bone && !owner_bone_model {
        let (rows, frame) = (bytes[140], bytes[141]);
        ensure!(
            (rows == 0 && frame == 0) || (frame < rows && uv_animation.is_none()),
            "model texture rows require a valid frame without scrolling"
        );
        if uv_animation.is_none() {
            rectangle = [0, i16::from(frame), 0, i16::from(rows)];
        }
    }
    let value = row.vector(176);
    let velocity = row.vector(188);
    let acceleration = row.vector(200);
    let geometry = match kind {
        8 => ParticleGeometry::BillboardTrail {
            size: [value[0], value[1]],
            radius: value[2],
            radius_velocity: velocity[2],
            segment_size_step: acceleration,
            segment_offset: row.vector(112),
            segment_angle_step: row.float(132),
            steps_per_segment: bytes[19],
        },
        18 => ParticleGeometry::Ribbon {
            length: value[0],
            width: value[1],
            jitter: velocity[1],
            phase: bytes[19],
            phase_period: bytes[51] as i8,
        },
        10 => ParticleGeometry::Spiral {
            value,
            velocity,
            acceleration,
            segment_angle_step: row.float(132),
        },
        15 => ParticleGeometry::Quad {
            vertices: std::array::from_fn(|i| row.vector(176 + i * 12)),
            velocity: std::array::from_fn(|i| row.vector(224 + i * 12)),
        },
        _ => ParticleGeometry::Size {
            value,
            velocity,
            acceleration,
        },
    };
    let geometry_acceleration_until = if matches!(
        geometry,
        ParticleGeometry::Size { .. }
            | ParticleGeometry::Spiral { .. }
            | ParticleGeometry::Quad { .. }
    ) {
        duration(row.short(136))?
    } else {
        None
    };
    let template = ParticleTemplate {
        during_pause: flags & 0x100 != 0,
        // The authored endpoint is inclusive; the runtime stores a tick count.
        lifetime: duration(row.short(16))?.map(|last_age| last_age + 1),
        owner_bone: (bone_ring(row) || owner_bone_model).then_some(bytes[140]),
        follow_bone: follow_bone.then_some(bytes[140]),
        follow_origin: flags & FOLLOW_ORIGIN != 0 && !follow_bone && !owner_bone_model,
        follow_orientation: matches!(kind, 3 | 15) && flags & 0x80 != 0,
        fixed_orientation: kind == 4 && flags & 0x80 != 0,
        origin_motion: kind == 14,
        model_elevation: (kind == 3 && flags & MODEL_ELEVATION != 0).then(|| row.float(324)),
        model_ground_relative: kind == 3 && flags & 0x1000 != 0,
        draw_after_target: flags & 0x202000 != 0,
        ground_restitution: (flags & 1 != 0).then_some(0.65),
        ground_clamp: flags & FLOOR_CLAMP != 0,
        bounce_effect: (bytes[145] & 4 != 0 && flags & 1 != 0).then_some(u16::from(bytes[149])),
        floor_effect: (bytes[145] & 4 != 0 && flags & 1 == 0).then_some(u16::from(bytes[149])),
        periodic_effect: periodic.then_some(resonance_content::battle_effect::PeriodicEffect {
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
            geometry,
            colors: [row.color(24), row.color(32)],
            brighten: bytes[40..44].try_into().unwrap(),
            brighten_until: (bytes[48] != 0).then_some(u32::from(bytes[48])),
            uv: rectangle,
            palettes: [bytes[3], bytes[4]],
            geometry_count: bytes[18],
            effect_group: group_quad(row).then_some(bytes[140]),
        },
        jerk: if matches!(kind, 8 | 10) {
            [0.; 3]
        } else {
            row.vector(112)
        },
        orbit_velocity: row.vector(164),
        linear_orbit: kind == 5,
        geometry_acceleration_until,
        gradient: flags & 8 != 0,
        fade: bytes[44..48].try_into().unwrap(),
        fade_from: u32::from(bytes[49]),
        uv_animation,
    };
    template.validate()?;
    if kind == 3 {
        return Ok(Declaration::ModelParticle {
            template,
            binding: ModelBinding {
                slot: bytes[212],
                animation: (flags & MODEL_ANIMATION != 0).then_some(u16::from(bytes[213])),
                repeat: bytes[145] & 2 != 0,
                program_animation: flags & PROGRAM_ANIMATION != 0,
                attachment: (flags & MODEL_JOINT != 0).then_some(ModelAttachment {
                    retained: bytes[141],
                    joint: bytes[140],
                }),
            },
            visual: ModelVisual {
                blend: bytes[1],
                lit: flags & 0x4000 != 0,
                depth_test: flags & 0x82000 == 0,
                cull_back: bytes[1] == 0 || flags & 0x1000_0000 != 0,
                before_actor: flags & 0x200000 != 0,
            },
        });
    }
    Ok(Declaration::Particle {
        template,
        visual: visual(row),
    })
}

fn duration(value: i16) -> Result<Option<u32>> {
    ensure!(value >= 0, "negative particle duration");
    Ok((value != 0).then_some(value as u32))
}

fn bone_ring(row: Row<'_>) -> bool {
    let flags = row.word(20);
    row.0[0] == 4
        && flags & OWNER_BONE != 0
        && flags
            & !(OWNER_BONE
                | FOLLOW_BONE_POSITION
                | DUAL_TEXTURE
                | RETAINED_PARTICLE
                | CYCLING_UV
                | GRADIENT
                | ANCHORED)
            == 0
        && row.0[2] != 10
        && row.0[142] == 0
}

fn group_quad(row: Row<'_>) -> bool {
    const DISABLE_DEPTH: u32 = 0x80000;
    let flags = row.word(20);
    row.0[0] == 5
        && flags & EFFECT_GROUP != 0
        && flags
            & !(EFFECT_GROUP
                | BILLBOARD
                | DUAL_TEXTURE
                | FOLLOW_ORIGIN
                | TARGET_DRAW_ORDER
                | DISABLE_DEPTH)
            == 0
}

fn visual(row: Row<'_>) -> ParticleVisual {
    let b = row.0;
    let flags = row.word(20);
    let unsupported = |reason: &str| ParticleShape::Unsupported {
        reason: reason.into(),
    };
    let geometry = if flags & (0x80000000 | 0x10000) != 0
        || (flags & 0x200 != 0 && !group_quad(row))
        || (flags & 0x40000000 != 0 && !bone_ring(row))
    {
        unsupported("particle requires an unsupported bone or point-history drawing binding")
    } else if matches!(b[0], 4 | 6 | 10 | 11 | 12 | 13 | 18 | 19) && b[142] != 0 {
        unsupported("procedural particle requests unsupported copies")
    } else if b[2] == 10 && (flags & 0x202000 != 0 || b[0] == 15) {
        unsupported("screen particle requires an unsupported owner or texture-offset binding")
    } else {
        match b[0] {
            0 | 14 => ParticleShape::NoDraw,
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
                } else if flags & CYCLING_UV != 0 {
                    UvLayout::Cycle
                } else {
                    UvLayout::Repeat
                },
            },
            5 => ParticleShape::Quad,
            6 => ParticleShape::Hemisphere { columns: 16 },
            7 => ParticleShape::Orbit,
            8 => ParticleShape::BillboardTrail,
            9 => ParticleShape::RadialQuads {
                flat: flags & 0x20 != 0,
                advance_u: flags & 0x10 != 0,
            },
            10 if row.float(212).is_finite() && row.vector(112).iter().all(|v| v.is_finite()) => {
                ParticleShape::Spiral {
                    radius_step: row.float(212),
                    segment_offset: row.vector(112),
                    steps_per_segment: b[19],
                    uv_layout: if flags & 0x10 != 0 {
                        UvLayout::Advance
                    } else {
                        UvLayout::Repeat
                    },
                }
            }
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
                copy_axis: b[51],
                local_copies: flags & 0x20 != 0,
            },
            18 => ParticleShape::Ribbon {
                depth: flags & 0x20 != 0,
            },
            19 => ParticleShape::TriangleBand,
            _ => unsupported("particle geometry is not supported for drawing"),
        }
    };
    ParticleVisual {
        geometry,
        orientation: if b[0] == 4 && flags & 0x80 != 0 {
            Orientation::FixedWorld
        } else if b[0] == 15 && flags & 0x80 != 0 {
            Orientation::FollowMotion
        } else if flags & 0x40 != 0 {
            Orientation::Billboard
        } else if flags & 0x2000000 != 0 {
            Orientation::Camera
        } else {
            Orientation::World
        },
        texture: if matches!(b[0], 0 | 14) {
            ParticleTexture::Untextured
        } else {
            match b[2] {
                255 => ParticleTexture::Untextured,
                10 => ParticleTexture::Atlas {
                    slot: 0,
                    dual: true,
                    palette_set: 0,
                },
                slot => ParticleTexture::Atlas {
                    slot,
                    dual: flags & 0x4000000 != 0,
                    palette_set: 0,
                },
            }
        },
        scene_sample: (b[2] == 10).then(|| [row.word(216) as i8, row.word(220) as i8]),
        blend: b[1],
        depth_test: flags & 0x80000 == 0,
        depth_write: flags & 0x800000 != 0,
        ground_relative: flags & 0x1000 != 0,
        anchored: flags & 2 != 0,
        copies: b[142],
        copy_rotation: b[143],
        after_actor: flags & 0x2000 != 0,
    }
}

// The caller verifies the exact record size before using these fixed offsets.
#[derive(Clone, Copy)]
struct Row<'a>(&'a [u8]);
impl Row<'_> {
    fn short(self, at: usize) -> i16 {
        i16::from_be_bytes(self.0[at..at + 2].try_into().unwrap())
    }
    fn word(self, at: usize) -> u32 {
        u32::from_be_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    fn float(self, at: usize) -> f32 {
        f32::from_bits(self.word(at))
    }
    fn vector(self, at: usize) -> [f32; 3] {
        std::array::from_fn(|i| self.float(at + i * 4))
    }
    fn color(self, at: usize) -> [i16; 4] {
        std::array::from_fn(|i| self.short(at + i * 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_notice_decodes_authored_text_without_creating_a_particle() -> Result<()> {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 25;
        bytes[50] = 255;
        for text in ["Poisonous Pollen", "たすけてぇーん　ルパーン"] {
            bytes[176..].fill(0);
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(text);
            assert!(!invalid);
            bytes[176..176 + encoded.len()].copy_from_slice(&encoded);
            let declaration = read(&bytes, &[])?;
            assert_eq!(declaration, Declaration::Notice { text: text.into() });
            assert!(declaration.particle().is_err());
            let events = super::super::timeline::decode(
                &[
                    super::super::timeline::Record {
                        age: 3,
                        command: 0,
                        argument: 0,
                        operand: 0,
                    },
                    super::super::timeline::Record {
                        age: 4,
                        command: 254,
                        argument: 0,
                        operand: 0,
                    },
                ],
                &[declaration],
                &[false],
                &Default::default(),
            );
            assert_eq!(
                events,
                vec![resonance_content::battle_effect::ScheduledEvent {
                    at: 3,
                    operation: resonance_content::battle_effect::EffectOperation::Notice {
                        text: text.into()
                    },
                }]
            );
        }
        Ok(())
    }

    #[test]
    fn camera_shake_duration_is_checked_at_import() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 22;
        bytes[20..24].copy_from_slice(&12_u32.to_be_bytes());
        for duration in [0_i16, 512, i16::MAX] {
            bytes[16..18].copy_from_slice(&duration.to_be_bytes());
            assert_eq!(
                read(&bytes, &[]).unwrap(),
                Declaration::CameraShake {
                    duration: duration as u32,
                    amplitude: 12,
                }
            );
        }
        for duration in [-1_i16, i16::MIN] {
            bytes[16..18].copy_from_slice(&duration.to_be_bytes());
            assert!(matches!(read(&bytes, &[]).unwrap(),
                Declaration::Unsupported { reason } if reason.contains("negative camera shake duration")));
        }
        assert!(read(&bytes[..ACTOR_BYTES - 1], &[]).is_err());
    }

    #[test]
    fn particle_durations_are_optional_and_negative_active_values_are_unsupported() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 4;
        bytes[50] = 255;
        let template = read(&bytes, &[]).unwrap().particle().unwrap();
        assert!(template.lifetime.is_none());
        assert!(template.geometry_acceleration_until.is_none());
        bytes[16..18].copy_from_slice(&12_i16.to_be_bytes());
        assert_eq!(
            read(&bytes, &[]).unwrap().particle().unwrap().lifetime,
            Some(13)
        );
        bytes[16..18].fill(0);
        for offset in [16, 136] {
            bytes[offset..offset + 2].copy_from_slice(&(-1_i16).to_be_bytes());
            assert!(matches!(
                read(&bytes, &[]).unwrap(),
                Declaration::Unsupported { .. }
            ));
            bytes[offset..offset + 2].fill(0);
        }
    }

    #[test]
    fn particle_uv_layout_is_cooked_from_flags_without_narrowing_columns() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[50] = 255;
        for (kind, flags, expected) in [
            (4, 0, UvLayout::Repeat),
            (4, 0x10, UvLayout::Cycle),
            (4, 0x100000, UvLayout::HalfWidthCycle),
            (4, 0x100010, UvLayout::HalfWidthCycle),
            (10, 0x10, UvLayout::Advance),
            (12, 0x10, UvLayout::Cycle),
            (
                12,
                0x100000,
                UvLayout::AlternatingHalves { panels_per_half: 4 },
            ),
        ] {
            bytes[0] = kind;
            bytes[20..24].copy_from_slice(&(flags as u32).to_be_bytes());
            for columns in [0, 127, 128, 255] {
                bytes[18] = columns;
                let declaration = read(&bytes, &[]).unwrap();
                assert_eq!(
                    declaration.particle().unwrap().state.geometry_count,
                    columns
                );
                let (ParticleShape::Ring { uv_layout, .. }
                | ParticleShape::Shell { uv_layout, .. }
                | ParticleShape::Spiral { uv_layout, .. }) =
                    declaration.particle_visual().unwrap().geometry
                else {
                    panic!("procedural particle");
                };
                assert_eq!(uv_layout, expected);
            }
        }
    }

    #[test]
    fn unused_state_does_not_leak_into_cooked_recipes() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 3;
        bytes[50] = 255;
        bytes[212] = 6;
        bytes[20..24].copy_from_slice(&0x487_u32.to_be_bytes());
        bytes[213] = 7;
        bytes[145] = 2;
        bytes[324..328].copy_from_slice(&12.5_f32.to_be_bytes());
        let recipe = read(&bytes, &[]).unwrap();
        bytes[216..324].fill(255); // Animation state and pointers are created at runtime.
        bytes[328..].fill(255); // Emitter context comes from the caller.
        assert_eq!(read(&bytes, &[]).unwrap(), recipe);
        let template = recipe.particle().unwrap();
        assert!(template.follow_origin && template.follow_orientation);
        assert_eq!(template.model_elevation, Some(12.5));
        assert_eq!(template.ground_restitution, Some(0.65));
        let Declaration::ModelParticle { binding, .. } = recipe else {
            unreachable!()
        };
        assert_eq!(binding.slot, 6);
        assert_eq!(binding.animation, Some(7));
        assert!(binding.repeat);
        bytes[324..328].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(matches!(
            read(&bytes, &[]).unwrap(),
            Declaration::Unsupported { .. }
        ));
        assert!(read(&bytes[..ACTOR_BYTES - 1], &[]).is_err());
        bytes[324..328].copy_from_slice(&12.5_f32.to_be_bytes());
        bytes[20..24].copy_from_slice(&0x82400_u32.to_be_bytes());
        let Declaration::ModelParticle { visual, .. } = read(&bytes, &[]).unwrap() else {
            panic!("depth-disabled model should remain supported");
        };
        assert!(!visual.depth_test);

        // Enemy007's repeated model keeps its animation while emitting member10
        // every8 visits. Other child controls remain explicitly unavailable.
        bytes = [0; ACTOR_BYTES];
        bytes[0] = 3;
        bytes[2] = 2;
        bytes[50] = 255;
        bytes[20..24].copy_from_slice(&0x406_u32.to_be_bytes());
        bytes[145..148].copy_from_slice(&[0x0a, 10, 8]);
        let Declaration::ModelParticle {
            template, binding, ..
        } = read(&bytes, &[]).unwrap()
        else {
            panic!("periodic model expected");
        };
        assert_eq!(
            template.periodic_effect,
            Some(resonance_content::battle_effect::PeriodicEffect {
                member: 10,
                interval: 8,
            })
        );
        assert!(binding.repeat);
        assert_eq!(binding.animation, Some(0));
        bytes[147] = 0;
        assert!(matches!(
            read(&bytes, &[]).unwrap(),
            Declaration::Unsupported { .. }
        ));
        bytes[147] = 8;
        bytes[145] |= 1;
        assert!(matches!(
            read(&bytes, &[]).unwrap(),
            Declaration::Unsupported { .. }
        ));

        bytes = [0; ACTOR_BYTES];
        bytes[0] = 3;
        bytes[2] = 2;
        bytes[50] = 255;
        bytes[20..24].copy_from_slice(&(FOLLOW_ORIGIN | MODEL_ANIMATION).to_be_bytes());
        bytes[145] = PROJECTILE_MODEL_CONTROL;
        bytes[148] = PROJECTILE_MODEL_END;
        for slot in [4, 5] {
            bytes[212] = slot;
            let Declaration::ModelParticle {
                template, binding, ..
            } = read(&bytes, &[]).unwrap()
            else {
                panic!("projectile-owned model expected");
            };
            assert!(template.follow_origin);
            assert!(template.lifetime.is_none());
            assert!(template.periodic_effect.is_none());
            assert_eq!(binding.animation, Some(0));
            assert!(!binding.repeat);
        }
        for (offset, value) in [(16, 1), (22, 0), (145, 3), (148, 44), (212, 3)] {
            let mut unsupported = bytes;
            unsupported[offset] = value;
            assert!(matches!(
                read(&unsupported, &[]).unwrap(),
                Declaration::Unsupported { .. }
            ));
        }
    }

    #[test]
    fn unknown_attachments_are_explicitly_unavailable() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 4;
        bytes[50] = 255;
        assert!(!read(&bytes, &[]).unwrap().particle().unwrap().during_pause);
        bytes[20..24].copy_from_slice(&0x100_u32.to_be_bytes());
        assert!(read(&bytes, &[]).unwrap().particle().unwrap().during_pause);
        bytes[20..24].copy_from_slice(&0x80_u32.to_be_bytes());
        assert!(
            read(&bytes, &[])
                .unwrap()
                .particle()
                .unwrap()
                .fixed_orientation
        );
        for flags in [
            0x8000_u32,
            FLOOR_CLAMP,
            OWNER_BONE | FOLLOW_BONE_POSITION | MODEL_JOINT,
        ] {
            bytes[20..24].copy_from_slice(&flags.to_be_bytes());
            assert!(
                matches!(read(&bytes, &[]).unwrap(), Declaration::Unsupported { .. }),
                "flags {flags:#x}"
            );
        }
        bytes[0] = 5;
        bytes[140] = 3;
        for drawing in [
            BILLBOARD | DUAL_TEXTURE | FOLLOW_ORIGIN,
            BILLBOARD | TARGET_DRAW_ORDER,
            DUAL_TEXTURE,
        ] {
            bytes[20..24].copy_from_slice(&(EFFECT_GROUP | drawing).to_be_bytes());
            let declaration = read(&bytes, &[]).unwrap();
            let template = declaration.particle().unwrap();
            assert_eq!(template.state.effect_group, Some(3));
            assert_eq!(template.follow_origin, drawing & FOLLOW_ORIGIN != 0);
            assert_eq!(template.owner_bone, None);
            assert!(matches!(
                declaration.particle_visual().unwrap().geometry,
                ParticleShape::Quad
            ));
        }
        bytes[20..24].copy_from_slice(&(EFFECT_GROUP | BILLBOARD | OWNER_BONE).to_be_bytes());
        assert!(matches!(
            read(&bytes, &[])
                .unwrap()
                .particle_visual()
                .unwrap()
                .geometry,
            ParticleShape::Unsupported { .. }
        ));
    }

    #[test]
    fn bone_ring_preserves_anchor_and_gradient_without_accepting_other_bindings() -> Result<()> {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 4;
        bytes[2] = 2;
        bytes[50] = 255;
        bytes[140] = 63;
        bytes[20..24].copy_from_slice(&0x4000_000a_u32.to_be_bytes());
        let declaration = read(&bytes, &[])?;
        let template = declaration.template()?;
        assert_eq!(template.owner_bone, Some(63));
        assert!(template.gradient);
        let visual = declaration.particle_visual()?;
        assert!(visual.anchored);
        assert!(matches!(visual.geometry, ParticleShape::Ring { .. }));

        let mut combined = bytes;
        combined[20..24].copy_from_slice(
            &(OWNER_BONE | FOLLOW_BONE_POSITION | DUAL_TEXTURE | CYCLING_UV | ANCHORED)
                .to_be_bytes(),
        );
        let declaration = read(&combined, &[])?;
        let template = declaration.template()?;
        assert_eq!(template.owner_bone, Some(63));
        assert_eq!(template.follow_bone, None);
        assert!(!template.follow_origin);
        template.validate()?;
        let visual = declaration.particle_visual()?;
        assert!(visual.anchored && visual.texture.dual());
        assert!(matches!(
            visual.geometry,
            ParticleShape::Ring {
                uv_layout: UvLayout::Cycle,
                ..
            }
        ));

        for (field, value) in [(2, 10), (142, 1), (20, 0xc0)] {
            let mut unsupported = bytes;
            unsupported[field] = value;
            assert!(matches!(
                read(&unsupported, &[])?,
                Declaration::Unsupported { .. }
                    | Declaration::Particle {
                        visual: ParticleVisual {
                            geometry: ParticleShape::Unsupported { .. },
                            ..
                        },
                        ..
                    }
            ));
        }
        bytes[20..24].copy_from_slice(&0x2000_000a_u32.to_be_bytes());
        let declaration = read(&bytes, &[])?;
        let template = declaration.template()?;
        assert_eq!(template.follow_bone, Some(63));
        assert_eq!(template.owner_bone, None);
        assert!(!template.follow_origin && template.gradient);
        assert!(matches!(
            declaration.particle_visual()?.geometry,
            ParticleShape::Ring { .. }
        ));
        bytes[20..24].copy_from_slice(&(FOLLOW_BONE_POSITION | DUAL_TEXTURE).to_be_bytes());
        let declaration = read(&bytes, &[])?;
        assert_eq!(declaration.template()?.follow_bone, Some(63));
        assert_eq!(declaration.template()?.owner_bone, None);
        assert!(declaration.particle_visual()?.texture.dual());
        bytes[20..24].copy_from_slice(
            &(FOLLOW_BONE_POSITION | FOLLOW_ORIGIN | DUAL_TEXTURE | CYCLING_UV).to_be_bytes(),
        );
        let declaration = read(&bytes, &[])?;
        assert_eq!(declaration.template()?.follow_bone, Some(63));
        assert!(!declaration.template()?.follow_origin);
        assert!(declaration.particle_visual()?.texture.dual());
        assert!(matches!(
            declaration.particle_visual()?.geometry,
            ParticleShape::Ring {
                uv_layout: UvLayout::Cycle,
                ..
            }
        ));
        let mut quad = bytes;
        quad[0] = 5;
        quad[140] = 6;
        quad[20..24].copy_from_slice(
            &(FOLLOW_BONE_POSITION | DUAL_TEXTURE | BILLBOARD | RETAINED_PARTICLE).to_be_bytes(),
        );
        let declaration = read(&quad, &[])?;
        assert_eq!(declaration.template()?.follow_bone, Some(6));
        assert_eq!(
            declaration.particle_visual()?.orientation,
            Orientation::Billboard
        );
        assert!(declaration.particle_visual()?.texture.dual());
        for (field, value) in [(2, 10), (142, 1)] {
            let mut unsupported = quad;
            unsupported[field] = value;
            assert!(matches!(
                read(&unsupported, &[])?,
                Declaration::Unsupported { .. }
            ));
        }
        let mut sampled = bytes;
        sampled[2] = 10;
        sampled[20..24]
            .copy_from_slice(&(FOLLOW_BONE_POSITION | BILLBOARD | 0x80000).to_be_bytes());
        sampled[216..220].copy_from_slice(&5_i32.to_be_bytes());
        sampled[220..224].copy_from_slice(&(-3_i32).to_be_bytes());
        let declaration = read(&sampled, &[])?;
        assert_eq!(declaration.template()?.follow_bone, Some(63));
        let visual = declaration.particle_visual()?;
        assert_eq!(visual.scene_sample, Some([5, -3]));
        assert_eq!(visual.texture.slot(), Some(0));
        assert!(visual.texture.dual());
        assert!(!visual.depth_test && !visual.depth_write);
        for (flags, orientation, ground_relative) in [
            (BILLBOARD | DUAL_TEXTURE, Orientation::Billboard, false),
            (0x1000, Orientation::World, true),
        ] {
            sampled[20..24].copy_from_slice(&flags.to_be_bytes());
            let declaration = read(&sampled, &[])?;
            assert_eq!(declaration.template()?.follow_bone, None);
            let visual = declaration.particle_visual()?;
            assert_eq!(visual.scene_sample, Some([5, -3]));
            assert_eq!(visual.orientation, orientation);
            assert_eq!(visual.ground_relative, ground_relative);
            assert!(visual.depth_test && !visual.depth_write);
        }
        let mixed_binding = u32::from_be_bytes(quad[20..24].try_into().unwrap()) | MODEL_JOINT;
        quad[20..24].copy_from_slice(&mixed_binding.to_be_bytes());
        assert!(matches!(read(&quad, &[])?, Declaration::Unsupported { .. }));
        bytes[20..24].copy_from_slice(
            &(OWNER_BONE | FOLLOW_BONE_POSITION | GRADIENT | ANCHORED).to_be_bytes(),
        );
        let declaration = read(&bytes, &[])?;
        assert_eq!(declaration.template()?.owner_bone, Some(63));
        assert_eq!(declaration.template()?.follow_bone, None);
        bytes[0] = 3;
        for flags in [FOLLOW_BONE_POSITION, FOLLOW_BONE_POSITION | FOLLOW_ORIGIN] {
            bytes[20..24].copy_from_slice(&flags.to_be_bytes());
            let Declaration::ModelParticle {
                template, binding, ..
            } = read(&bytes, &[])?
            else {
                panic!("static bone-follow model expected");
            };
            assert_eq!(template.follow_bone, Some(63));
            assert!(!template.follow_origin && !template.follow_orientation);
            assert_eq!(template.state.uv, [0; 4]);
            assert!(binding.animation.is_none() && binding.attachment.is_none());
        }
        for flags in [
            FOLLOW_BONE_POSITION | 4,
            FOLLOW_BONE_POSITION | MODEL_JOINT,
            FOLLOW_BONE_POSITION | FOLLOW_ORIGIN | 0x20,
            FOLLOW_BONE_POSITION | FOLLOW_ORIGIN | 0x4000_0000,
        ] {
            bytes[20..24].copy_from_slice(&flags.to_be_bytes());
            assert!(matches!(
                read(&bytes, &[])?,
                Declaration::Unsupported { .. }
            ));
        }
        bytes = [0; ACTOR_BYTES];
        bytes[0] = 3;
        bytes[2] = 2;
        bytes[50] = 255;
        bytes[140] = 111;
        let flags = OWNER_BONE | FOLLOW_ORIGIN | PROGRAM_ANIMATION;
        bytes[20..24].copy_from_slice(&flags.to_be_bytes());
        let Declaration::ModelParticle {
            template, binding, ..
        } = read(&bytes, &[])?
        else {
            panic!("static owner-bone model expected");
        };
        assert_eq!(template.owner_bone, Some(111));
        assert_eq!(template.follow_bone, None);
        assert!(!template.follow_origin && !template.follow_orientation);
        assert_eq!(template.state.uv, [0; 4]);
        assert!(binding.program_animation);
        assert!(binding.animation.is_none() && binding.attachment.is_none());
        for flag in [MODEL_ANIMATION, MODEL_JOINT, FOLLOW_BONE_POSITION] {
            bytes[20..24].copy_from_slice(&(flags | flag).to_be_bytes());
            assert!(matches!(
                read(&bytes, &[])?,
                Declaration::Unsupported { .. }
            ));
        }
        bytes[20..24].copy_from_slice(&flags.to_be_bytes());
        for (field, value) in [(2, 0), (142, 1), (145, 2)] {
            let mut unsupported = bytes;
            unsupported[field] = value;
            assert!(matches!(
                read(&unsupported, &[])?,
                Declaration::Unsupported { .. }
            ));
        }
        bytes[20..24].copy_from_slice(&(OWNER_BONE | MODEL_ANIMATION).to_be_bytes());
        bytes[140] = 10;
        bytes[145] = 2;
        let Declaration::ModelParticle {
            template, binding, ..
        } = read(&bytes, &[])?
        else {
            panic!("animated owner-bone model expected");
        };
        assert_eq!(template.owner_bone, Some(10));
        assert!(template.follow_bone.is_none());
        assert!(!template.follow_origin && !template.follow_orientation);
        assert_eq!(binding.animation, Some(0));
        assert_eq!(binding.slot, 0);
        assert!(binding.repeat && !binding.program_animation);
        assert!(binding.attachment.is_none());
        for (field, value) in [(140, 11), (141, 1), (142, 1), (145, 0), (212, 1), (213, 1)] {
            let mut unsupported = bytes;
            unsupported[field] = value;
            assert!(matches!(
                read(&unsupported, &[])?,
                Declaration::Unsupported { .. }
            ));
        }
        Ok(())
    }

    #[test]
    fn model_uv_scroll_keeps_fractional_extents_and_rejects_other_controllers() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 3;
        bytes[2] = 6;
        let mut track = UvRecord {
            timing: 128,
            control: 0,
            values: [1, -15, 2500, 2500],
        };
        let declaration = read(&bytes, &[track]).unwrap();
        let template = declaration.particle().unwrap();
        assert_eq!(template.state.uv, [0, 0, 2500, 2500]);
        assert_eq!(
            template.uv_animation,
            Some(resonance_content::battle_effect::UvAnimation::Scroll {
                origin: [0; 2],
                step: [1, -15],
                interval: 1,
            })
        );
        bytes[1] = 1;
        for flags in [0x480_u32, 0x10000480] {
            bytes[20..24].copy_from_slice(&flags.to_be_bytes());
            let Declaration::ModelParticle {
                template, visual, ..
            } = read(&bytes, &[track]).unwrap()
            else {
                panic!("followed model with explicit culling must remain supported");
            };
            assert!(template.follow_origin && template.follow_orientation);
            assert_eq!(visual.cull_back, flags & 0x10000000 != 0);
        }
        for timing in [0, 1, 129, 255] {
            track.timing = timing;
            assert!(matches!(
                read(&bytes, &[track]).unwrap(),
                Declaration::Unsupported { .. }
            ));
        }
        track.timing = 128;
        track.values[2] = 0;
        assert!(matches!(
            read(&bytes, &[track]).unwrap(),
            Declaration::Unsupported { .. }
        ));
    }

    #[test]
    fn uv_rows_are_resolved_before_publication() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 7;
        bytes[50] = 1;
        let rows = [
            UvRecord {
                timing: 255,
                control: 0,
                values: [0; 4],
            },
            UvRecord {
                timing: 129,
                control: 0,
                values: [1, 2, 3, 4],
            },
        ];
        bytes[12..14].copy_from_slice(&16_i16.to_be_bytes());
        bytes[14..16].copy_from_slice(&16_i16.to_be_bytes());
        assert_eq!(
            read(&bytes, &rows)
                .unwrap()
                .particle()
                .unwrap()
                .uv_animation,
            Some(resonance_content::battle_effect::UvAnimation::Scroll {
                origin: [1, 2],
                step: [3, 4],
                interval: 1,
            })
        );
        bytes[50] = 2;
        assert!(matches!(
            read(&bytes, &rows).unwrap(),
            Declaration::Unsupported { .. }
        ));
    }
}
