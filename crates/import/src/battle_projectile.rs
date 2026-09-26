//! 400-byte templates consumed by 14CC8, 13B8C, 14064 and 147E0.
use crate::read::{Field, FloatOperand, unreferenced_storage};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_projectile::{Effect, Projectile, Table};

fn record(row: &[u8]) -> Result<Projectile> {
    ensure!(row.len() == 400, "invalid projectile record size");
    let effect = |at| Effect {
        bank: row[at],
        member: row[at + 1],
    };
    Ok(Projectile {
        flags: u32::read(row, 8)?,
        lifetime: i16::read(row, 0xc)?,
        ground_effect: effect(0xe),
        damage_kind: row[0x11],
        hit_class: row[0x12],
        reaction: row[0x13],
        shape: row[0x15],
        knockback: row[0x16],
        repeat_limit: row[0x17],
        velocity: <[FloatOperand; 3]>::read(row, 0x24)?,
        acceleration: <[FloatOperand; 3]>::read(row, 0x30)?,
        speed: FloatOperand::read(row, 0x3c)?,
        bounce_restitution: FloatOperand::read(row, 0x40)?,
        radius: FloatOperand::read(row, 0x44)?,
        height: FloatOperand::read(row, 0x48)?,
        inner_radius: FloatOperand::read(row, 0x4c)?,
        growth: <[FloatOperand; 2]>::read(row, 0x50)?,
        birth_effect: effect(0x5c),
        trail_effect: effect(0x5e),
        spawn_offset: <[FloatOperand; 3]>::read(row, 0x60)?,
        velocity_jitter: <[FloatOperand; 3]>::read(row, 0x6c)?,
        hit_offset: <[FloatOperand; 3]>::read(row, 0x78)?,
        active_start: i16::read(row, 0x84)?,
        active_duration: i16::read(row, 0x86)?,
        shadow_color: <[u8; 4]>::read(row, 0x88)?,
        trail_interval: i16::read(row, 0x90)?,
        pulse_state: row[0x92],
        steering_blend: FloatOperand::read(row, 0x94)?,
        steering_end: row[0x98],
        steering_start: row[0x99],
        toward_bone: row[0x9a],
        from_bone: row[0x9b],
        update_mode: row[0x9c],
        velocity_reset_age: row[0x9d],
        storage: unreferenced_storage(
            row,
            vec![
                8..0x10,
                0x11..0x14,
                0x15..0x18,
                0x24..0x58,
                0x5c..0x8c,
                0x90..0x93,
                0x94..0x9e,
            ],
        ),
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

/// Enemy package members end at the next declared pointer. Native 205AC reads
/// 400-byte rows; the following member may be aligned to 32 bytes in the package.
/// That alignment slack is not required to be zero (original enemies 155/163/164).
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
    // Bind the complete source member, including the alignment bytes.
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

    fn source_bytes(p: &Projectile) -> [u8; 400] {
        let mut bytes = [0; 400];
        for storage in &p.storage {
            bytes[storage.offset..storage.offset + storage.bytes.len()]
                .copy_from_slice(&storage.bytes);
        }
        bytes[8..12].copy_from_slice(&p.flags.to_be_bytes());
        for (at, value) in [
            (0xc, p.lifetime),
            (0x84, p.active_start),
            (0x86, p.active_duration),
            (0x90, p.trail_interval),
        ] {
            bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        for (at, effect) in [
            (0xe, p.ground_effect),
            (0x5c, p.birth_effect),
            (0x5e, p.trail_effect),
        ] {
            bytes[at..at + 2].copy_from_slice(&[effect.bank, effect.member]);
        }
        for (at, value) in [
            (0x11, p.damage_kind),
            (0x12, p.hit_class),
            (0x13, p.reaction),
            (0x15, p.shape),
            (0x16, p.knockback),
            (0x17, p.repeat_limit),
            (0x92, p.pulse_state),
            (0x98, p.steering_end),
            (0x99, p.steering_start),
            (0x9a, p.toward_bone),
            (0x9b, p.from_bone),
            (0x9c, p.update_mode),
            (0x9d, p.velocity_reset_age),
        ] {
            bytes[at] = value;
        }
        for (at, values) in [
            (0x24, p.velocity),
            (0x30, p.acceleration),
            (0x60, p.spawn_offset),
            (0x6c, p.velocity_jitter),
            (0x78, p.hit_offset),
        ] {
            for (i, value) in values.into_iter().enumerate() {
                bytes[at + 4 * i..at + 4 * (i + 1)].copy_from_slice(&value.bits().to_be_bytes());
            }
        }
        for (at, value) in [
            (0x3c, p.speed),
            (0x40, p.bounce_restitution),
            (0x44, p.radius),
            (0x48, p.height),
            (0x4c, p.inner_radius),
            (0x50, p.growth[0]),
            (0x54, p.growth[1]),
            (0x94, p.steering_blend),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.bits().to_be_bytes());
        }
        bytes[0x88..0x8c].copy_from_slice(&p.shadow_color);
        bytes
    }

    #[test]
    fn retains_unknown_selectors_nonfinite_operands_and_instance_storage() -> Result<()> {
        let mut bytes: Vec<_> = (0..400).map(|i| (i * 17 + 9) as u8).collect();
        for (at, bits) in [
            (0x24, 0x7fc12345_u32),
            (0x3c, 0xff800000),
            (0x94, 0x80000000),
        ] {
            bytes[at..at + 4].copy_from_slice(&bits.to_be_bytes());
        }
        let json = serde_json::to_vec(&read(&bytes)?)?;
        let restored: Table = serde_json::from_slice(&json)?;
        assert_eq!(source_bytes(&restored.records[0]), bytes.as_slice());
        assert_eq!(restored.records[0].speed.bits(), 0xff800000);
        assert_eq!(restored.records[0].shape, bytes[0x15]);
        for length in 0..400 {
            assert!(read(&bytes[..length]).is_err(), "length {length}");
        }
        bytes.push(0);
        assert!(read(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn package_alignment_uses_absolute_offset_and_rejects_unexplained_tail() -> Result<()> {
        let mut bytes = vec![0; 400];
        bytes[8..12].copy_from_slice(&0x2009_u32.to_be_bytes());
        // The table starts at a word boundary; only the following member is
        // 32-byte aligned. Alignment bytes can contain unrelated original data.
        bytes.extend(1_u8..=28);
        let table = read_package_member(&bytes, 0x7d4)?;
        assert_eq!(table.records.len(), 1);
        assert_eq!(table.records[0].flags, 0x2009);
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
    #[ignore = "requires both extracted original discs"]
    fn original_enemy_projectile_members_use_only_declared_package_alignment() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let sources = crate::source_assets::Sources::read(&extracted)?;
            let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
            let archive = extracted.join("files").join(sources.enemy);
            let mut padded = vec![];
            for id in 0..resonance_content::monster::MONSTER_COUNT {
                let package = crate::source_assets::enemy_package(&archive, &usual, id as u16)?;
                let members = crate::model_preview::PointerMembers::new(&package, 0x18..0x1e8)?;
                let Some(bytes) = members.model(0x1c8)? else {
                    continue;
                };
                let start = crate::read::u32(&package, 0x1c8)? as usize;
                let table = read_package_member(bytes, start)
                    .with_context(|| format!("disc{disc} enemy {id} projectile member"))?;
                for (row, original) in table.records.iter().zip(bytes.chunks_exact(400)) {
                    assert_eq!(source_bytes(row), original);
                }
                if !bytes.len().is_multiple_of(400) {
                    padded.push((id, bytes.len() % 400));
                    assert_eq!(table.records.len(), 1);
                }
            }
            assert_eq!(padded, [(155, 28), (163, 24), (164, 24)]);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted original discs"]
    fn original_projectiles_roundtrip_every_byte_and_match_across_discs() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let bytes = std::fs::read(local.join(format!("disc{disc}/files/BTL/BTLusual.dat")))?;
            let bytes = crate::source_assets::section(&bytes, 7)?;
            let table = read(bytes)?;
            assert_eq!(table.records.len(), 26);
            let json = serde_json::to_vec(&table)?;
            let restored: Table = serde_json::from_slice(&json)?;
            for (row, original) in restored.records.iter().zip(bytes.chunks_exact(400)) {
                assert_eq!(source_bytes(row), original);
            }
            let lightning = &restored.records[4];
            assert_eq!(lightning.flags, 0x44a);
            assert_eq!(lightning.lifetime, 20);
            assert_eq!(
                lightning.birth_effect,
                Effect {
                    bank: 1,
                    member: 28
                }
            );
            assert_eq!((lightning.active_start, lightning.active_duration), (4, 0));
            if let Some(previous) = &previous {
                assert_eq!(&json, previous);
            }
            previous = Some(json);
        }
        Ok(())
    }
}
