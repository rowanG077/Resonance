//! Executable-owned launch callbacks normalized to ordinary martial events.
use crate::{battle_normal::canonical, battle_projectiles, battle_scenes, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_martial::{Attack, gameplay_tick},
    battle_normal::Event,
    battle_projectile::{DamageKind, ProjectileOrigin},
};
mod finishers;

/// Both native owners share the verified callback's definitions; enemy clocks
/// retain raw ages so their own pose holds are applied by the shared compiler.
pub(crate) fn enemy_final_fury(
    source: &Source,
) -> Result<Vec<(u16, resonance_content::battle_action::EnemyEvent)>> {
    let catalogue = crate::battle_martial::read(source)?;
    let attack = catalogue.attack(200, 8)?;
    let mut launches = attack
        .events
        .iter()
        .filter_map(|(_, event)| matches!(event, Event::EmitProjectile { .. }).then_some(event));
    let mut events = Vec::new();
    for raw_at in [22, 352] {
        events.push((
            raw_at,
            resonance_content::battle_action::EnemyEvent::AuthoredCallback {
                skill: 200,
                event: Box::new(
                    launches
                        .next()
                        .context("missing Final Fury callback projectile")?
                        .clone(),
                ),
            },
        ));
    }
    ensure!(
        launches.next().is_none(),
        "extra Final Fury callback projectile"
    );
    for (at, event) in [
        (0, Event::TransitionProtection(600)),
        (
            0,
            Event::ActionScene {
                party_protection: 480,
            },
        ),
        (308, Event::Voice(crate::battle_voices::sound(0x5d7)?)),
    ] {
        events.push((
            at,
            resonance_content::battle_action::EnemyEvent::AuthoredCallback {
                skill: 200,
                event: Box::new(event),
            },
        ));
    }
    Ok(events)
}
mod guardian_field;
mod holy_binds;

/// A source bank-six contact belongs to this action's stored skill. Preserve
/// its member while making the shared decoder's local selector canonical.
pub(crate) fn canonical_hits(member: u16, rules: &mut [u8]) -> Result<()> {
    if member == 152 {
        let row = rules
            .get_mut(28..56)
            .context("missing Final Fury contact")?;
        ensure!(
            row[22] == 2 && row[24] == 6 && row[25..28] == [0; 3],
            "Final Fury local skill contact changed"
        );
        row[24] = 0;
    }
    Ok(())
}

pub(crate) fn attach(
    source: &Source,
    image: &pe::Image<'_>,
    member: u16,
    bytes: &[u8],
    attack: &mut Attack,
) -> Result<()> {
    match member {
        51 => grand_fall(source, image, bytes, attack),
        148 | 151 => holy_binds::attach(source, image, member, bytes, attack),
        150 => guardian_field::attach(image, bytes, attack),
        146 | 147 | 152 => finishers::attach(source, image, member, bytes, attack),
        _ => Ok(()),
    }
}

fn grand_fall(
    source: &Source,
    image: &pe::Image<'_>,
    bytes: &[u8],
    attack: &mut Attack,
) -> Result<()> {
    let descriptor = pe::word(image.slice(0x8e98d0 + 51 * 4, 4)?, 0)?;
    let handlers = image.slice(descriptor, 8)?;
    ensure!(
        pe::word(handlers, 0)? == 0x4092a0
            && pe::word(handlers, 4)? == 0x427f20
            && pe::word(image.slice(0x4092bb, 4)?, 0)? == 0x4092d0,
        "Grand Fall action handlers changed"
    );
    for (address, expected) in [
        (0x4092d9, &[0x66, 0x83, 0xba, 0xbe, 1, 0, 0, 0x1e][..]),
        (0x4092f5, &[0xd9, 5, 0xfc, 0x2e, 0x88, 0]),
        (0x40933e, &[0x8d, 0x83, 0x90, 1, 0, 0]),
        (0x409356, &[0xb1, 8]),
    ] {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "Grand Fall launch callback changed at {address:#x}"
        );
    }
    let skill = pe::word(image.slice(0x8d84b0 + 51 * 96 + 0x58, 4)?, 0)? + 1;
    let name = image.text(pe::word(image.slice(0x8e98a8 + skill * 4, 4)?, 0)?)?;
    let package = source.require(name)?;
    let projectiles =
        battle_scenes::member(&package, 0x18c)?.context("missing Grand Fall projectiles")?;
    let row = projectiles
        .get(400..800)
        .context("missing Grand Fall projectile one")?;
    let reset_scale = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    ensure!(reset_scale.is_finite(), "invalid projectile reset scale");
    let mut definition = resonance_importer::decode_projectile(
        &battle_projectiles::canonical_record(row)?,
        reset_scale,
    )?;
    ensure!(
        definition.unsupported_reason.is_none(),
        "unsupported Grand Fall projectile"
    );
    // Callback 0x4092d0 samples the target's X/Z, replaces world Y with 400,
    // and uses the stored skill bank for the falling object's effect programs.
    definition.motion.origin = ProjectileOrigin::TargetFloor;
    let height = f32::from_bits(pe::word(image.slice(0x882efc, 4)?, 0)?);
    ensure!(height.is_finite(), "invalid Grand Fall launch height");
    definition.spawn_offset[1] += height;
    definition.birth_effect.bank = 2;
    definition.trail_effect.bank = 2;
    let kind = match row[17] {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        value => anyhow::bail!("invalid Grand Fall projectile damage kind {value}"),
    };
    let rules = bytes
        .get(pe::word(bytes, 0)? as usize..pe::word(bytes, 4)? as usize)
        .context("missing Grand Fall hit rules")?;
    let rules = canonical(rules, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
    let hit = resonance_importer::decode_martial_hit(&rules, 0, kind, row[19])?;
    let poses = bytes
        .get(pe::word(bytes, 8)? as usize..pe::word(bytes, 12)? as usize)
        .context("missing Grand Fall poses")?;
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
    attack.events.push((
        gameplay_tick(30, 0, &holds)?,
        Event::EmitProjectile {
            definition: Box::new(definition),
            hit,
            feedback: None,
        },
    ));
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}
