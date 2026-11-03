//! Executable finisher schedules use the shared action lifetime and skill bank.
use super::*;

pub(super) fn attach(
    source: &Source,
    image: &pe::Image<'_>,
    member: u16,
    bytes: &[u8],
    attack: &mut Attack,
) -> Result<()> {
    let (
        native,
        initializer,
        callback_at,
        callback,
        protection,
        party_protection,
        releases,
        voice_at,
        voice,
    ): (u32, u32, u32, u32, u16, u16, &[(u16, u8, u8)], u16, u32) = match member {
        146 => (
            253,
            0x40ce40,
            0x40cf0e,
            0x40d150,
            345,
            285,
            &[(70, 1, 1), (168, 2, 2)],
            98,
            0x5ca,
        ),
        147 => (
            254,
            0x40d410,
            0x40d4de,
            0x40d720,
            660,
            600,
            &[
                (70, 1, 1),
                (168, 2, 2),
                (300, 1, 3),
                (350, 3, 4),
                (436, 4, 5),
            ],
            244,
            0x5cd,
        ),
        152 => (
            200,
            0x40f170,
            0x40f2f7,
            0x40f550,
            600,
            480,
            &[(22, 1, 0), (352, 2, 2)],
            308,
            0x5d7,
        ),
        _ => anyhow::bail!("unexpected stored finisher {member}"),
    };
    let descriptor = pe::word(image.slice(0x8e98d0 + u32::from(member) * 4, 4)?, 0)?;
    let handlers = image.slice(descriptor, 8)?;
    ensure!(
        pe::word(handlers, 0)? == initializer
            && pe::word(handlers, 4)? == 0x427f20
            && pe::word(image.slice(callback_at, 4)?, 0)? == callback,
        "finisher {member} handlers changed"
    );
    let (flags_at, timer_at, timer_write, entry_at, bank_at, program_at, launch_at) = match member {
        146 => (
            0x40cf04, 0x40cf23, 0x40cf33, 0x40d12f, 0x40cfb8, 0x40cfbe, 0x40cfd6,
        ),
        147 => (
            0x40d4d4, 0x40d4f3, 0x40d503, 0x40d6ff, 0x40d588, 0x40d58e, 0x40d5a6,
        ),
        152 => (
            0x40f2ed, 0x40f30c, 0x40f31c, 0x40f52e, 0x40f3a7, 0x40f3ad, 0x40f3c5,
        ),
        _ => unreachable!(),
    };
    ensure!(
        image.slice(flags_at, 4)? == [0x24, 0xfc, 0x0c, 0x0c]
            && image.slice(timer_at, 1)? == [0xb8]
            && pe::word(image.slice(timer_at + 1, 4)?, 0)? == u32::from(protection)
            && image.slice(timer_write, 7)? == [0x66, 0x89, 0x8e, 0x9c, 2, 0, 0],
        "finisher transition protection changed"
    );
    ensure!(
        image.slice(entry_at, 1)? == [0x68]
            && pe::word(image.slice(entry_at + 1, 4)?, 0)? == u32::from(party_protection)
            && image.slice(entry_at + 7, 1)? == [0xe8],
        "finisher scene admission changed"
    );
    ensure!(
        image.slice(bank_at, 2)? == [4, 6]
            && image.slice(program_at, 2)? == [0x6a, 1]
            && image.slice(launch_at, 1)? == [0xe8],
        "finisher initial skill artwork changed"
    );
    let slot = pe::word(image.slice(0x8d84b0 + native * 96 + 0x58, 4)?, 0)? + 1;
    let name = image.text(pe::word(image.slice(0x8e98a8 + slot * 4, 4)?, 0)?)?;
    let package = source.require(name)?;
    let projectiles =
        battle_scenes::member(&package, 0x18c)?.context("missing finisher projectiles")?;
    let rules = bytes
        .get(pe::word(bytes, 0)? as usize..pe::word(bytes, 4)? as usize)
        .context("missing finisher hits")?;
    if member == 152 {
        ensure!(
            rules.get(50..53) == Some(&[2, 0, 6]),
            "Final Fury skill200 contact member2 changed"
        );
        ensure!(attack.events.iter().any(|(_,event)| matches!(event, Event::Contact(contact) if contact.hit.effect == Some(2))), "Final Fury contact artwork was lost");
    }
    let rules = canonical(rules, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
    let poses = bytes
        .get(pe::word(bytes, 8)? as usize..pe::word(bytes, 12)? as usize)
        .context("missing finisher poses")?;
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
    let reset_scale = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    for &(at, projectile, rule) in releases {
        let row = projectiles
            .get(usize::from(projectile) * 400..(usize::from(projectile) + 1) * 400)
            .context("missing finisher projectile")?;
        let kind = match row[17] {
            0 => DamageKind::Slash,
            1 => DamageKind::Thrust,
            2 => DamageKind::Magic,
            n => anyhow::bail!("invalid finisher damage kind {n}"),
        };
        let hit = resonance_importer::decode_martial_hit(&rules, rule, kind, row[19])?;
        let mut definition = resonance_importer::decode_projectile(
            &battle_projectiles::canonical_record(row)?,
            reset_scale,
        )?;
        ensure!(
            definition.unsupported_reason.is_none(),
            "unsupported finisher projectile: {:?}",
            definition.unsupported_reason
        );
        definition.birth_effect.bank = 2;
        definition.trail_effect.bank = 2;
        if definition.ground_effect.member != 0 {
            ensure!(
                definition.ground_effect.bank == 2,
                "finisher ground effect bank changed"
            );
        }
        attack.events.push((
            gameplay_tick(at, 0, &holds)?,
            Event::EmitProjectile {
                definition: Box::new(definition),
                hit,
                feedback: None,
            },
        ));
    }
    attack.events.extend([
        (0, Event::TransitionProtection(protection)),
        (0, Event::ActionScene { party_protection }),
    ]);
    if member == 147 {
        ensure!(
            image.slice(0x40d937, 7)? == [0xc6, 0x86, 0x60, 0x35, 1, 0, 0]
                && image.slice(0x40d99b, 7)? == [0xc6, 0x86, 0x60, 0x35, 1, 0, 1],
            "Crimson Devastation axe visibility changed"
        );
        for (at, visible) in [(170, false), (244, true)] {
            attack.events.push((
                gameplay_tick(at, 0, &holds)?,
                Event::ActionWeaponVisibility {
                    slot: resonance_content::battle_action::WeaponSlot(0),
                    visible,
                },
            ));
        }
    }
    attack.events.push((
        gameplay_tick(voice_at, 0, &holds)?,
        Event::Voice(crate::battle_voices::sound(voice)?),
    ));
    attack.chain_at = None;
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}
