//! Decode the fixed-size effect records into runtime recipes.
use super::source::{UvRecord, uv_animation};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    ModelBinding, ParticleGeometry, ParticleState, ParticleTemplate,
    declaration::Declaration,
    visual::{ModelVisual, Orientation, ParticleShape, ParticleTexture, ParticleVisual, UvLayout},
};

const ACTOR_BYTES: usize = 352;

pub(super) fn read(bytes: &[u8], uv: &[UvRecord]) -> Result<Declaration> {
    ensure!(bytes.len() == ACTOR_BYTES, "truncated effect record");
    Ok(
        recipe(Row(bytes), uv).unwrap_or_else(|error| Declaration::Unsupported {
            reason: error.to_string(),
        }),
    )
}

fn recipe(row: Row<'_>, uv: &[UvRecord]) -> Result<Declaration> {
    let bytes = row.0;
    let kind = bytes[0];
    if kind == 22 {
        return Ok(Declaration::CameraShake {
            duration: u32::try_from(row.short(16)).context("negative camera shake duration")?,
            amplitude: row.word(20),
        });
    }
    let flags = row.word(20);
    let slot = bytes[2];
    ensure!(
        matches!(kind, 3 | 4 | 5 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 15 | 18),
        "effect controller {kind} is not supported"
    );
    if kind == 3 {
        ensure!(
            flags & !0x486 == 0 && matches!(slot, 0 | 2) && bytes[50] == 255,
            "model particle requires an unsupported binding or attachment"
        );
    }
    ensure!(
        flags & (0x2000_0000 | 0x0800_0000 | 0x8000) == 0
            && (flags & 0x80 == 0 || (kind == 3 && flags & 0x400 != 0))
            && bytes[145] & !(if kind == 3 { 2 } else { 0 }) == 0,
        "particle requires an unsupported attachment or child controller"
    );
    let uv_animation = match bytes[50] {
        255 => None,
        index @ 0..=127 => Some(uv_animation(
            uv.get(usize::from(index)..)
                .context("particle UV root outside row pool")?,
        )?),
        _ => anyhow::bail!("invalid particle UV selector"),
    };
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
        lifetime: duration(row.short(16))?,
        follow_origin: flags & 0x400 != 0,
        follow_orientation: flags & 0x80 != 0,
        model_elevation: (kind == 3 && flags & 2 != 0).then(|| row.float(324)),
        draw_after_target: flags & 0x202000 != 0,
        ground_restitution: (flags & 1 != 0).then_some(0.65),
        element_tint: flags & 0x400000 != 0,
        state: ParticleState {
            blend: None,
            cull_back: flags & 0x1000_0000 != 0,
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
            uv: row.color(8),
            palettes: [bytes[3], bytes[4]],
            geometry_count: bytes[18],
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
                animation: (flags & 4 != 0).then_some(u16::from(bytes[213])),
                repeat: bytes[145] & 2 != 0,
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

fn visual(row: Row<'_>) -> ParticleVisual {
    let b = row.0;
    let flags = row.word(20);
    let unsupported = |reason: &str| ParticleShape::Unsupported {
        reason: reason.into(),
    };
    let geometry = if flags & (0xc0000000 | 0x10000 | 0x200) != 0 {
        unsupported("particle requires an unsupported bone or point-history drawing binding")
    } else if matches!(b[0], 4 | 10 | 11 | 12 | 13) && b[142] != 0 {
        unsupported("procedural particle requests unsupported copies")
    } else if b[2] == 10 && (flags & 0x202000 != 0 || b[0] == 15) {
        unsupported("screen particle requires an unsupported owner or texture-offset binding")
    } else {
        match b[0] {
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
            7 => ParticleShape::Orbit,
            8 => ParticleShape::BillboardTrail,
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
            _ => unsupported("particle geometry is not supported for drawing"),
        }
    };
    ParticleVisual {
        geometry,
        orientation: if flags & 0x40 != 0 {
            Orientation::Billboard
        } else if flags & 0x2000000 != 0 {
            Orientation::Camera
        } else {
            Orientation::World
        },
        texture: match b[2] {
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
        },
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
        bytes[20..24].copy_from_slice(&0x486_u32.to_be_bytes());
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
    }

    #[test]
    fn unknown_attachments_are_explicitly_unavailable() {
        let mut bytes = [0; ACTOR_BYTES];
        bytes[0] = 4;
        bytes[50] = 255;
        assert!(read(&bytes, &[]).unwrap().particle().is_ok());
        for flags in [0x80_u32, 0x8000, 0x0800_0000, 0x2000_0000] {
            bytes[20..24].copy_from_slice(&flags.to_be_bytes());
            assert!(matches!(
                read(&bytes, &[]).unwrap(),
                Declaration::Unsupported { .. }
            ));
        }
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
