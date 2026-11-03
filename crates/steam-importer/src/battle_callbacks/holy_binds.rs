//! The two executable-owned Holy Binds schedules use ordinary projectile events.
use super::*;

pub(super) fn attach(
    source: &Source,
    image: &pe::Image<'_>,
    member: u16,
    bytes: &[u8],
    attack: &mut Attack,
) -> Result<()> {
    let (catalogue, initializer, callback_operand, callback, releases, voice_at, voice) =
        match member {
            148 => (
                249,
                0x40de20,
                0x40deff,
                0x40e1a0,
                [32, 86, 220, 284],
                200,
                0x318,
            ),
            151 => (
                153,
                0x40e8a0,
                0x40e97f,
                0x40ec70,
                [12, 46, 190, 254],
                168,
                0x5db,
            ),
            _ => anyhow::bail!("unexpected Holy Binds action"),
        };
    let descriptor = pe::word(image.slice(0x8e98d0 + u32::from(member) * 4, 4)?, 0)?;
    let handlers = image.slice(descriptor, 8)?;
    ensure!(
        pe::word(handlers, 0)? == initializer
            && pe::word(handlers, 4)? == 0x427f20
            && pe::word(image.slice(callback_operand, 4)?, 0)? == callback,
        "Holy Binds action handlers changed"
    );
    let slot = pe::word(image.slice(0x8d84b0 + catalogue * 96 + 0x58, 4)?, 0)? + 1;
    let name = image.text(pe::word(image.slice(0x8e98a8 + slot * 4, 4)?, 0)?)?;
    let package = source.require(name)?;
    let rows = battle_scenes::member(&package, 0x18c)?.context("missing Holy Binds projectiles")?;
    ensure!(rows.len() >= 5 * 400, "truncated Holy Binds projectiles");
    let reset_scale = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    let rules = bytes
        .get(pe::word(bytes, 0)? as usize..pe::word(bytes, 4)? as usize)
        .context("missing Holy Binds hit rules")?;
    let rules = canonical(rules, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
    let poses = bytes
        .get(pe::word(bytes, 8)? as usize..pe::word(bytes, 12)? as usize)
        .context("missing Holy Binds poses")?;
    let mut holds = Vec::new();
    for row in poses.chunks_exact(12) {
        let at = pe::half(row, 0)? as i16;
        if at == -2 {
            break;
        }
        if at >= 0 && row[2] < 254 {
            holds.push((at as u16, row[3]));
        }
    }
    for (index, raw_at) in releases.into_iter().enumerate() {
        let row = &rows[(index + 1) * 400..(index + 2) * 400];
        let kind = match row[17] {
            0 => DamageKind::Slash,
            1 => DamageKind::Thrust,
            2 => DamageKind::Magic,
            value => anyhow::bail!("invalid Holy Binds projectile damage kind {value}"),
        };
        let hit = resonance_importer::decode_martial_hit(&rules, index as u8, kind, row[19])?;
        let mut definition = resonance_importer::decode_projectile(
            &battle_projectiles::canonical_record(row)?,
            reset_scale,
        )?;
        ensure!(
            definition.unsupported_reason.is_none(),
            "unsupported Holy Binds projectile {}: {:?}",
            index + 1,
            definition.unsupported_reason
        );
        // Native callbacks zero world Y for the final releases while airborne.
        if index >= 2 {
            definition.motion.origin = ProjectileOrigin::ActorFloor;
        }
        definition.birth_effect.bank = 2;
        definition.trail_effect.bank = 2;
        attack.events.push((
            gameplay_tick(raw_at, 0, &holds)?,
            Event::EmitProjectile {
                definition: Box::new(definition),
                hit,
                feedback: None,
            },
        ));
    }
    attack.events.push((
        gameplay_tick(voice_at, 0, &holds)?,
        Event::Voice(crate::battle_voices::sound(voice)?),
    ));
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}
