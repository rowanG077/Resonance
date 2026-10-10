//! Decode 400-byte projectile templates.
use crate::read::Field;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_projectile::*;

fn record(row: &[u8]) -> Result<Projectile> {
    ensure!(row.len() == 400, "invalid projectile record size");
    let effect = |at| Effect {
        bank: row[at],
        member: row[at + 1],
    };
    let flags = u32::read(row, 8)?;
    let mut unsupported_reason = None;
    if flags & 0x1020 != 0
        && [
            0..8,
            0x10..0x11,
            0x14..0x15,
            0x18..0x24,
            0x58..0x5c,
            0x8c..0x90,
            0x93..0x94,
            0x9e..400,
        ]
        .into_iter()
        .any(|range| row[range].iter().any(|&byte| byte != 0))
    {
        unsupported_reason = Some("projectile response requires unsupported instance state".into());
    }
    if row[0x9a] != 0 || row[0x9b] != 0 {
        unsupported_reason
            .get_or_insert_with(|| "projectile bone targeting is not prepared".into());
    }
    if !matches!(row[0x9c], 0 | 2) {
        unsupported_reason.get_or_insert_with(|| "projectile update mode is not prepared".into());
    }
    let mut scalar = |offset, active| {
        let value = f32::from_bits(u32::read(row, offset).expect("validated projectile size"));
        if value.is_finite() {
            value
        } else {
            if active {
                unsupported_reason
                    .get_or_insert_with(|| format!("non-finite projectile value at {offset:#x}"));
            }
            0.
        }
    };
    let velocity = std::array::from_fn(|index| scalar(0x24 + index * 4, true));
    let acceleration = std::array::from_fn(|index| scalar(0x30 + index * 4, true));
    let speed = (flags & 0x10000 != 0).then(|| scalar(0x3c, true));
    let response = (flags & 0x1020 != 0).then(|| ProjectileResponse {
        bounce: flags & 0x20 != 0,
        ricochet: flags & 0x1000 != 0,
        restitution: scalar(0x40, true),
    });
    let radius = scalar(0x44, true);
    let height = scalar(0x48, true);
    let width = scalar(0x4c, row[0x15] == 3);
    let growth = std::array::from_fn(|index| scalar(0x50 + index * 4, true));
    let spawn_offset = std::array::from_fn(|index| scalar(0x60 + index * 4, true));
    let velocity_jitter = std::array::from_fn(|index| scalar(0x6c + index * 4, true));
    let hit_offset = std::array::from_fn(|index| scalar(0x78 + index * 4, true));
    let steering = (flags & 0x400004 != 0).then(|| ProjectileSteering {
        blend: scalar(0x94, true),
        start: u32::from(row[0x99]),
        end: (row[0x98] != 0).then_some(u32::from(row[0x98])),
        planar: flags & 0x400000 != 0,
    });
    let mut unsupported = |reason: &str| {
        unsupported_reason.get_or_insert_with(|| reason.into());
    };
    if flags & !0xc1347f != 0 || row[0x9d] != 0 {
        unsupported("projectile motion/effect controller is not prepared");
    }
    let ground_effect = effect(0xe);
    let trail_effect = effect(0x5e);
    if response.is_some()
        && (flags & !0x3469 != 0 || ground_effect.member != 0 || trail_effect.member != 0)
    {
        unsupported("projectile response combination is not prepared");
    }
    let shape = match row[0x15] {
        0 => HitShape::Box,
        1 => HitShape::Cylinder,
        2 => HitShape::GroundCircle,
        3 => HitShape::Ring { width },
        4 => HitShape::Sphere,
        _ => {
            unsupported("projectile contact shape is not prepared");
            HitShape::Box
        }
    };
    let lifetime = i16::read(row, 0xc)?;
    if lifetime < 0 {
        unsupported("invalid projectile lifetime");
    }
    let lifetime = (lifetime > 0).then_some(lifetime as u32);
    let start = i16::read(row, 0x84)?;
    let duration = i16::read(row, 0x86)?;
    let active = if duration == 0 {
        None
    } else if start >= 0 && duration > 0 {
        Some([start as u32, start as u32 + duration as u32])
    } else {
        unsupported("invalid projectile active interval");
        None
    };
    let interval = i16::read(row, 0x90)?;
    let trail_interval = if trail_effect.member == 0 {
        None
    } else if row[0x92] == 4 && interval > 0 {
        Some(interval as u32)
    } else {
        unsupported("projectile periodic effect controller is not prepared");
        None
    };
    if speed.is_some_and(|speed| speed <= 0.) {
        unsupported("invalid projectile speed");
    }
    if steering.is_some_and(|steering| {
        !(0.0..=1.0).contains(&steering.blend)
            || steering.end.is_some_and(|end| steering.start >= end)
    }) {
        unsupported("invalid projectile steering");
    }
    Ok(Projectile {
        lifetime,
        velocity,
        acceleration,
        spawn_offset,
        clamp_ground: flags & 0x40 != 0,
        active,
        motion: ProjectileMotion {
            velocity_jitter,
            speed,
            steering,
            response,
        },
        contact: Contact {
            shape,
            repeat_limit: row[0x17],
            radius,
            height,
            growth,
            offset: hit_offset,
            survives_contact: flags & 8 != 0,
            clashes: flags & 0x2000 != 0,
        },
        birth_effect: effect(0x5c),
        ground_effect,
        trail_effect,
        trail_interval,
        shadow: (flags & 0x400 == 0).then_some(ProjectileShadow {
            color: <[u8; 4]>::read(row, 0x88)?,
            additive: flags & 0x800000 != 0,
            radius,
        }),
        unsupported_reason,
    })
}

pub(crate) fn read(bytes: &[u8]) -> Result<Table> {
    ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(400),
        "misaligned projectile table"
    );
    Ok(Table {
        source_sha256: crate::digest(bytes),
        records: bytes.chunks_exact(400).map(record).collect::<Result<_>>()?,
    })
}

/// Enemy package members end at the next declared pointer. They contain
/// 400-byte rows; the following member may be aligned to 32 bytes in the package.
/// Alignment slack may contain nonzero bytes.
pub(crate) fn read_package_member(bytes: &[u8], start: usize) -> Result<Table> {
    let length = bytes.len() / 400 * 400;
    let end = start
        .checked_add(bytes.len())
        .context("projectile member range overflow")?;
    let aligned_end = start
        .checked_add(length)
        .and_then(|end| end.checked_next_multiple_of(32))
        .context("projectile member alignment overflow")?;
    ensure!(
        start.is_multiple_of(4) && length != 0 && (length == bytes.len() || end == aligned_end),
        "projectile member at {start:#x} has {} bytes outside its 400-byte rows and package alignment",
        bytes.len() - length
    );
    let mut table = read(&bytes[..length])?;
    // Hash the complete member, including alignment bytes.
    table.source_sha256 = crate::digest(bytes);
    Ok(table)
}

pub fn publish(usual: &[u8], output: &std::path::Path, prefix: &str) -> Result<String> {
    let table = read(crate::source_assets::section(usual, 7)?)?;
    let path = format!("{prefix}/projectiles.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&table)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_semantics_and_marks_unsupported_templates_without_losing_other_rows() -> Result<()> {
        let mut bytes = vec![0; 800];
        bytes[8..12].copy_from_slice(&0x20_u32.to_be_bytes());
        bytes[0xc..0xe].copy_from_slice(&45_i16.to_be_bytes());
        bytes[0x24..0x28].copy_from_slice(&3_f32.to_be_bytes());
        bytes[0x40..0x44].copy_from_slice(&0.75_f32.to_be_bytes());
        let table = read(&bytes)?;
        assert_eq!(table.records[0].velocity, [3., 0., 0.]);
        assert_eq!(table.records[0].lifetime, Some(45));
        assert_eq!(table.records[0].motion.response.unwrap().restitution, 0.75);
        assert!(table.records[0].unsupported_reason.is_none());
        bytes[0xb2] = 1;
        let table = read(&bytes)?;
        assert!(table.records[0].unsupported_reason.is_some());
        assert!(table.records[1].unsupported_reason.is_none());
        bytes[0xb2] = 0;
        for offset in [0x9a, 0x9b, 0x9c] {
            bytes[offset] = 3;
            assert!(read(&bytes)?.records[0].unsupported_reason.is_some());
            bytes[offset] = 0;
        }
        bytes[0x24..0x28].copy_from_slice(&f32::NAN.to_be_bytes());
        let table = read(&bytes)?;
        assert!(table.records[0].unsupported_reason.is_some());
        assert_eq!(table.records[0].velocity[0], 0.);
        assert!(serde_json::to_vec(&table).is_ok());
        assert!(read(&[]).is_err());
        assert!(read(&bytes[..799]).is_err());
        Ok(())
    }

    #[test]
    fn disabled_float_fields_are_discarded() -> Result<()> {
        let mut bytes = [0; 400];
        for offset in [0x3c, 0x40, 0x4c, 0x94] {
            bytes[offset..offset + 4].copy_from_slice(&f32::NAN.to_be_bytes());
        }
        let projectile = record(&bytes)?;
        assert!(projectile.unsupported_reason.is_none());
        assert!(projectile.motion.speed.is_none());
        assert!(projectile.motion.response.is_none());
        assert!(matches!(projectile.contact.shape, HitShape::Box));
        assert!(projectile.motion.steering.is_none());
        Ok(())
    }

    #[test]
    fn projectile_clocks_reject_negative_durations_and_keep_wide_interval_ends() -> Result<()> {
        let mut bytes = [0; 400];
        assert!(record(&bytes)?.lifetime.is_none());
        bytes[0x84..0x86].copy_from_slice(&32767_i16.to_be_bytes());
        bytes[0x86..0x88].copy_from_slice(&2_i16.to_be_bytes());
        assert_eq!(record(&bytes)?.active, Some([32767, 32769]));
        for offset in [0xc, 0x86] {
            let mut invalid = bytes;
            invalid[offset..offset + 2].copy_from_slice(&(-1_i16).to_be_bytes());
            assert!(record(&invalid)?.unsupported_reason.is_some());
        }
        Ok(())
    }

    #[test]
    fn specialized_controllers_remain_unavailable_after_decoding() -> Result<()> {
        for flags in [
            0x8000_u32, 0x100000, 0x40000, 0x20000, 0x200000, 0x10020, 0x24,
        ] {
            let mut bytes = [0; 400];
            bytes[8..12].copy_from_slice(&flags.to_be_bytes());
            assert!(
                record(&bytes)?.unsupported_reason.is_some(),
                "flags {flags:#x}"
            );
        }
        for (at, value) in [(0x9d, 1), (0x15, 5), (0x5f, 1)] {
            let mut bytes = [0; 400];
            bytes[at] = value;
            assert!(
                record(&bytes)?.unsupported_reason.is_some(),
                "field {at:#x}"
            );
        }
        Ok(())
    }

    #[test]
    fn enabled_motion_and_effects_have_explicit_settings() -> Result<()> {
        let mut bytes = [0; 400];
        bytes[8..12].copy_from_slice(&0xc10048_u32.to_be_bytes());
        bytes[0x3c..0x40].copy_from_slice(&5_f32.to_be_bytes());
        bytes[0x94..0x98].copy_from_slice(&0.25_f32.to_be_bytes());
        bytes[0x98..0x9a].copy_from_slice(&[9, 2]);
        bytes[0x84..0x88].copy_from_slice(&[0, 3, 0, 4]);
        bytes[0x5e..0x60].copy_from_slice(&[2, 7]);
        bytes[0x90..0x93].copy_from_slice(&[0, 6, 4]);
        let projectile = record(&bytes)?;
        assert!(projectile.unsupported_reason.is_none());
        assert_eq!(projectile.motion.speed, Some(5.));
        assert_eq!(
            projectile.motion.steering,
            Some(ProjectileSteering {
                blend: 0.25,
                start: 2,
                end: Some(9),
                planar: true,
            })
        );
        assert_eq!(projectile.active, Some([3, 7]));
        assert_eq!(projectile.trail_effect, Effect { bank: 2, member: 7 });
        assert_eq!(projectile.trail_interval, Some(6));
        assert!(projectile.shadow.unwrap().additive);
        assert!(projectile.clamp_ground && projectile.contact.survives_contact);
        Ok(())
    }

    #[test]
    fn package_alignment_uses_absolute_offset_and_rejects_unexplained_tail() -> Result<()> {
        let mut bytes = vec![0; 400];
        bytes[8..12].copy_from_slice(&0x2009_u32.to_be_bytes());
        // The table starts at a word boundary; only the following member is
        // 32-byte aligned. Alignment bytes can contain unrelated data.
        bytes.extend(1_u8..=28);
        let table = read_package_member(&bytes, 0x7d4)?;
        assert_eq!(table.records.len(), 1);
        assert!(table.records[0].contact.survives_contact);
        assert!(table.records[0].contact.clashes);
        assert_eq!(table.source_sha256, crate::digest(&bytes));
        assert!(read(&bytes).is_err());
        assert!(read_package_member(&bytes, 0x7d0).is_err());
        assert!(read_package_member(&bytes[..427], 0x7d4).is_err());
        assert!(read_package_member(&bytes[..399], 0x7d4).is_err());
        assert!(read_package_member(&[], 0x7d4).is_err());
        assert!(read_package_member(&bytes[..400], 0x7d5).is_err());
        bytes.push(0);
        assert!(read_package_member(&bytes, 0x7d4).is_err());
        bytes.resize(460, 0);
        assert!(read_package_member(&bytes, 0x7d4).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires extracted game assets"]
    fn imports_lightning_projectile() -> Result<()> {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat");
        let bytes = std::fs::read(file)?;
        let table = read(crate::source_assets::section(&bytes, 7)?)?;
        assert_eq!(table.records.len(), 26);
        let lightning = &table.records[4];
        assert_eq!(lightning.lifetime, Some(20));
        assert_eq!(
            lightning.birth_effect,
            Effect {
                bank: 1,
                member: 28
            }
        );
        assert!(lightning.active.is_none());
        assert!(lightning.unsupported_reason.is_none());
        Ok(())
    }
}
