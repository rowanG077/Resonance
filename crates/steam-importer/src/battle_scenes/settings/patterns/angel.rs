//! Added angel callbacks compile to ordinary scheduled contacts and a portrait.
use super::*;
use battle_scene::{PatternAim, PatternCutin};

pub(super) fn grand_cross(image: &pe::Image<'_>) -> Result<SpellRecipe> {
    // The initializer captures the selected target root; the callback retains
    // it for three different contact rows and launches no duplicate effect.
    let tick = |at| -> Result<u16> { Ok(u16::from(image.slice(at, 1)?[0])) };
    let recipe = PatternSpell {
        lifetime: pe::word(image.slice(0x418f8c, 4)?, 0)?.try_into()?,
        origin: PatternOrigin::SceneTargetRoot,
        heading_offset: 0.,
        startup_member: 1,
        startup_deferred: false,
        cutin: None,
        contacts: [
            tick(0x419043)?,
            tick(0x4190c4)?,
            pe::word(image.slice(0x419135, 4)?, 0)?.try_into()?,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, tick)| PatternContact {
            tick,
            projectile: 1 + index as u8,
            hit_variant: index as u8,
            aim: PatternAim::Live,
            effect_member: 0,
            offset: [0.; 3],
        })
        .collect(),
    };
    recipe.validate()?;
    Ok(SpellRecipe::FixedPattern(recipe))
}

pub(super) fn divine_judgment(image: &pe::Image<'_>, points: &[[f32; 3]]) -> Result<SpellRecipe> {
    for (at, expected) in [
        (0x40fda7, &[0x66, 0x83, 0x7b, 0x36, 2][..]),
        (0x40fd30, &[0x66, 0x83, 0x7b, 0x36, 0x3e][..]),
        (0x40ffc2, &[0xba, 0xce, 0, 0, 0][..]),
        (0x41004a, &[0xb8, 0x0e, 1, 0, 0][..]),
        (0x40fe5f, &[0x81, 0xe1, 7, 0, 0, 0x80][..]),
    ] {
        ensure!(
            image.slice(at, expected.len())? == expected,
            "Divine Judgment callback changed at {at:#x}"
        );
    }
    ensure!(
        points.len() == 16,
        "Divine Judgment beam point count changed"
    );
    let contact = |tick, projectile, hit_variant, effect_member, offset, aim| PatternContact {
        tick,
        projectile,
        hit_variant,
        effect_member,
        offset,
        aim,
    };
    let mut contacts = vec![contact(2, 1, 0, 0, [0.; 3], PatternAim::Live)];
    contacts.extend(points.iter().enumerate().map(|(index, &point)| {
        contact(70 + index as u16 * 8, 2, 1, 2, point, PatternAim::Captured)
    }));
    contacts.extend([
        contact(206, 3, 2, 0, [0.; 3], PatternAim::Live),
        contact(270, 4, 3, 0, [0.; 3], PatternAim::Live),
    ]);
    let recipe = PatternSpell {
        lifetime: pe::word(image.slice(0x40f806, 4)?, 0)?.try_into()?,
        origin: PatternOrigin::SceneGround { toward_caster: 0. },
        heading_offset: 0.,
        startup_member: 1,
        startup_deferred: true,
        contacts,
        cutin: Some(PatternCutin {
            catalogue: 144,
            tick: 62,
            duration: 52,
            hold_ticks: 49,
        }),
    };
    recipe.validate()?;
    Ok(SpellRecipe::FixedPattern(recipe))
}
