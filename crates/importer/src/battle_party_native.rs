//! Native event records shared by party attack converters.
use crate::{battle_enemy_rules, dol, native_archive};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::*,
    battle_projectile::{DamageKind, HitShape},
    battle_voice::Sound,
};

pub(crate) fn half(bytes: &[u8], at: usize) -> Result<u16> {
    dol::u16(bytes, at)
}
pub(crate) fn signed(bytes: &[u8], at: usize) -> Result<i16> {
    Ok(half(bytes, at)? as i16)
}
pub(crate) fn word(bytes: &[u8], at: usize) -> Result<u32> {
    dol::u32(bytes, at)
}
pub(crate) fn float(bytes: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(word(bytes, at)?))
}
pub(crate) fn vector(bytes: &[u8], at: usize) -> Result<[f32; 3]> {
    Ok([
        float(bytes, at)?,
        float(bytes, at + 4)?,
        float(bytes, at + 8)?,
    ])
}
pub(crate) fn member(bytes: &[u8], index: usize) -> Result<&[u8]> {
    native_archive::member(bytes, index, 14)?.context("missing resident battle member")
}
pub(crate) fn sound(encoded: u16) -> Option<Sound> {
    match encoded {
        0 => None,
        n if n & 0x8000 != 0 => Some(Sound::Stream(n & 0x7fff)),
        n => Some(Sound::Cue(n + 501)),
    }
}
pub(crate) fn duration(usual: &[u8], sound: Sound) -> Result<u16> {
    let index = match sound {
        Sound::Stream(n) => n,
        Sound::Cue(n) => n.checked_sub(501).context("not an actor voice")?,
    };
    half(member(usual, 12)?, usize::from(index) * 2)
}
pub(crate) fn action_pose(row: &[u8]) -> Result<ActionPose> {
    let rate = float(row, 8)?;
    Ok(ActionPose {
        clip: u16::from(row[2]),
        start: f32::from(row[4]) * rate.abs(),
        end: (row[5] != 0).then_some(f32::from(row[5]) * rate.abs()),
        rate,
        blend: row[3],
        repeat: row[6] & 0x40 != 0,
    })
}
pub(crate) fn damage_kind(value: u8) -> Result<DamageKind> {
    Ok(match value {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        n => anyhow::bail!("unknown damage kind {n}"),
    })
}
pub(crate) fn hit_rule(rules: &[u8], index: u8, kind: DamageKind, recoil: u8) -> Result<ActionHit> {
    battle_enemy_rules::hit_rule(rules, index, recoil, kind)
}
pub(crate) fn hit(rules: &[u8], row: &[u8]) -> Result<ActionHit> {
    hit_rule(rules, row[18], damage_kind(row[17])?, row[20])
}
pub(crate) fn contact(rules: &[u8], row: &[u8]) -> Result<MeleeContact> {
    let duration = u16::try_from(row[2] as i8).context("negative contact duration")?;
    let count = usize::from(row[3]);
    ensure!(
        duration > 0 && (1..=4).contains(&count) && half(row, 22)? == 0 && row[19] <= 1,
        "invalid party contact"
    );
    let attachments = match row[4] {
        255 => {
            ensure!(count == 1, "invalid center contact");
            ContactAttachments::BodyCenter {}
        }
        254 => {
            ensure!(count == 1, "invalid joint contact");
            ContactAttachments::BodyBone(u16::from(row[5]))
        }
        _ => ContactAttachments::Groups(row[4..4 + count].to_vec()),
    };
    let shape = match row[16] {
        0 => HitShape::Box,
        1 => HitShape::Cylinder,
        2 => HitShape::GroundCircle,
        3 => HitShape::Ring {
            width: float(row, 28)?,
        },
        4 => HitShape::Sphere,
        n => anyhow::bail!("unknown contact shape {n}"),
    };
    Ok(MeleeContact {
        duration: duration + 1,
        attachments,
        shape,
        radius: float(row, 8)?,
        height: float(row, 12)?,
        hit: ActionHit {
            bypass_guard: row[19] == 0,
            ..hit(rules, row)?
        },
    })
}
