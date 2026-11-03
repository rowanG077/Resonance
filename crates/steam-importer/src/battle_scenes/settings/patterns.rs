//! Fixed strike schedules carried by the Steam spell callbacks.
use super::*;
use battle_scene::{PatternContact, PatternOrigin, PatternSpell, SpellRecipe};

pub(super) fn read(image: &pe::Image<'_>, id: u16) -> Result<Option<SpellRecipe>> {
    // These are the x87 scalar references used to initialize the callback's
    // local point arrays. Zero denotes fldz. Native vectors are Z/Y/X.
    const RAY: [[u32; 3]; 9] = [
        [0, 0, 0x88396c],
        [0x882efc, 0, 0x882efc],
        [0x88396c, 0, 0],
        [0x882efc, 0, 0x882ac8],
        [0, 0, 0x7a36d4],
        [0x882ac8, 0, 0x882ac8],
        [0x7a36d4, 0, 0],
        [0x882ac8, 0, 0x882efc],
        [0, 0, 0],
    ];
    const METEOR: [[u32; 3]; 14] = [
        [0, 0, 0x883f0c],
        [0x7a36d4, 0, 0x8834a0],
        [0x88396c, 0, 0x8838a4],
        [0x7a36d4, 0, 0x883f0c],
        [0x8834a0, 0, 0x88396c],
        [0x7a36d4, 0, 0x8838a4],
        [0x88396c, 0, 0x883f0c],
        [0x8838a4, 0, 0x88396c],
        [0x8834a0, 0, 0x7a36d4],
        [0x8838a4, 0, 0x7a36d4],
        [0, 0, 0x88396c],
        [0, 0, 0x7a36d4],
        [0x88396c, 0, 0x8834a0],
        [0, 0, 0x883f0c],
    ];
    let (points, duration, first, limit, mask, heading, member, origin) = match id {
        // Ray initializer 0x415bf0, callback 0x415ca0.
        252 => (
            RAY.as_slice(),
            0x415c1c,
            0x415d7e,
            0x415d86,
            0x415d9b,
            0x8829a8,
            2,
            PatternOrigin::SceneGround { toward_caster: 1. },
        ),
        // Meteor Storm initializer 0x4136d0, callback 0x413790.
        // Steam creates fourteen meteors at sixteen-tick intervals.
        233 => (
            METEOR.as_slice(),
            0x4136fc,
            0x413911,
            0x413919,
            0x41392e,
            0x883008,
            3,
            PatternOrigin::SceneFixed { position: [0.; 3] },
        ),
        _ => return Ok(None),
    };
    let word = |address| pe::word(image.slice(address, 4)?, 0);
    let lifetime = word(duration)?.try_into()?;
    let first = u16::from(image.slice(first, 1)?[0]);
    let limit: u16 = word(limit)?.try_into()?;
    let period: u16 = ((word(mask)? & 0x7fff_ffff) + 1).try_into()?;
    ensure!(
        period > 0
            && limit > first
            && usize::from((limit - first).div_ceil(period)) == points.len(),
        "Steam spell {id} schedule exceeds its point array"
    );
    let scalar = |address| -> Result<f32> {
        if address == 0 {
            Ok(0.)
        } else {
            Ok(f32::from_bits(pe::word(image.slice(address, 4)?, 0)?))
        }
    };
    let pattern = PatternSpell {
        lifetime,
        origin,
        heading_offset: f64::from_le_bytes(image.slice(heading, 8)?.try_into()?) as f32,
        startup_member: 1,
        startup_deferred: true,
        contacts: points
            .iter()
            .enumerate()
            .map(|(index, &[z, y, x])| {
                Ok(PatternContact {
                    tick: first + period * index as u16,
                    projectile: 1,
                    hit_variant: 0,
                    effect_member: member,
                    offset: [scalar(x)?, scalar(y)?, scalar(z)?],
                })
            })
            .collect::<Result<_>>()?,
    };
    pattern.validate()?;
    Ok(Some(SpellRecipe::FixedPattern(pattern)))
}
