//! Fixed strike schedules carried by the Steam spell callbacks.
use super::*;
use battle_scene::{PatternContact, PatternOrigin, PatternSpell, SpellRecipe};
mod angel;
mod tetraspell;
mod time_stop;
use angel::{divine_judgment, grand_cross};

pub(super) fn read(
    image: &pe::Image<'_>,
    id: u16,
    points: &[[f32; 3]],
) -> Result<Option<SpellRecipe>> {
    if id == 298 {
        return tetraspell::read(image).map(Some);
    }
    if id == 271 {
        return grand_cross(image).map(Some);
    }
    if id == 281 {
        return divine_judgment(image, points).map(Some);
    }
    if id == 266 {
        return time_stop::read(image).map(Some);
    }
    if id == 263 {
        return sacred_light(image).map(Some);
    }
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
        cutin: None,
        opponent_stop_ticks: 0,
        contacts: points
            .iter()
            .enumerate()
            .map(|(index, &[z, y, x])| {
                Ok(PatternContact {
                    tick: first + period * index as u16,
                    projectile: 1,
                    hit_variant: 0,
                    aim: Default::default(),
                    effect_member: member,
                    offset: [scalar(x)?, scalar(y)?, scalar(z)?],
                })
            })
            .collect::<Result<_>>()?,
    };
    pattern.validate()?;
    Ok(Some(SpellRecipe::FixedPattern(pattern)))
}

fn sacred_light(image: &pe::Image<'_>) -> Result<SpellRecipe> {
    let descriptor = pe::word(image.slice(0x8e9b08 + 263 * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == 0x4175e0,
        "Steam Sacred Light initializer changed"
    );
    for (address, expected) in [
        (0x417625, &[0xb8][..]),
        (0x41766a, &[0x6a, 1][..]),
        (0x4177a0, &[0xc7, 0x43, 0x14, 0x20, 0x78, 0x41, 0][..]),
        (0x417829, &[0x8b, 0x41, 0x18, 0x8b, 0x50, 0x0c][..]),
        (0x4178c5, &[0xba][..]),
        (0x4178ca, &[0x66, 0x39, 0x51, 0x36][..]),
        (0x4178d3, &[0xd9, 0x86, 0xac, 0x38, 1, 0][..]),
        (0x41791a, &[0x05, 0x90, 1, 0, 0][..]),
    ] {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "Steam Sacred Light callback changed at {address:#x}"
        );
    }
    let pattern = PatternSpell {
        lifetime: pe::word(image.slice(0x417626, 4)?, 0)?.try_into()?,
        origin: PatternOrigin::SceneFixed { position: [0.; 3] },
        heading_offset: 0.,
        startup_member: 1,
        startup_deferred: true,
        cutin: None,
        opponent_stop_ticks: 0,
        contacts: vec![PatternContact {
            tick: pe::word(image.slice(0x4178c6, 4)?, 0)?.try_into()?,
            projectile: 1,
            hit_variant: 0,
            aim: battle_scene::PatternAim::Live,
            effect_member: 0,
            offset: [0.; 3],
        }],
    };
    pattern.validate()?;
    Ok(SpellRecipe::FixedPattern(pattern))
}
