//! Guardian Field's executable callback uses ordinary contacts and recovery.
use super::*;
use resonance_content::battle_projectile::{
    Contact, Effect, HitShape, Projectile, RecoilDirection,
};

pub(super) fn attach(image: &pe::Image<'_>, bytes: &[u8], attack: &mut Attack) -> Result<()> {
    let descriptor = pe::word(image.slice(0x8e98d0 + 150 * 4, 4)?, 0)?;
    let handlers = image.slice(descriptor, 8)?;
    ensure!(
        pe::word(handlers, 0)? == 0x40e5d0
            && pe::word(handlers, 4)? == 0x427f20
            && pe::word(image.slice(0x40e5ea, 4)?, 0)? == 0x40e600,
        "Guardian Field action handlers changed"
    );
    let single =
        |address| -> Result<f32> { Ok(f32::from_le_bytes(image.slice(address, 4)?.try_into()?)) };
    let rules = bytes
        .get(pe::word(bytes, 0)? as usize..pe::word(bytes, 4)? as usize)
        .context("missing Guardian Field hit rules")?;
    let rules = canonical(rules, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
    let hit = resonance_importer::decode_martial_hit(&rules, 0, DamageKind::Slash, 1)?;
    let blend = attack
        .events
        .iter()
        .find_map(|(at, event)| match event {
            Event::Pose(pose) if *at == 0 => Some(pose.blend),
            _ => None,
        })
        .context("missing Guardian Field opening pose")?;
    // Native helper 0x465290 creates an eight-update stationary cylinder;
    // callback 0x40e600 emits it at action ages 14, 24 and 34.
    let pulse = Projectile {
        lifetime: Some(8),
        velocity: [0.; 3],
        acceleration: [0.; 3],
        spawn_offset: [0., single(0x7d9220)?, 0.],
        clamp_ground: false,
        active: None,
        motion: Default::default(),
        contact: Contact {
            repeat_limit: 0,
            shape: HitShape::Cylinder,
            recoil_direction: RecoilDirection::AwayFromOwner,
            radius: single(0x8834e8)?,
            height: single(0x8830b0)?,
            growth: [0.; 2],
            offset: [0.; 3],
            survives_contact: true,
            clashes: false,
        },
        birth_effect: Effect { bank: 0, member: 0 },
        ground_effect: Effect { bank: 0, member: 0 },
        trail_effect: Effect { bank: 0, member: 0 },
        trail_interval: None,
        shadow: None,
        unsupported_reason: None,
    };
    for raw_at in [14, 24, 34] {
        attack.events.push((
            gameplay_tick(raw_at, 0, &[(0, blend)])?,
            Event::EmitProjectile {
                definition: Box::new(pulse.clone()),
                hit,
                feedback: None,
            },
        ));
    }
    attack.events.push((
        gameplay_tick(20, 0, &[(0, blend)])?,
        Event::HealOtherAllies {
            radius: f64::from_le_bytes(image.slice(0x8834f0, 8)?.try_into()?) as f32,
            percent: 5,
        },
    ));
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}
