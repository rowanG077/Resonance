//! Tetraspell retains its own scene and the installed handler's five contacts.
use super::*;
use battle_scene::{PatternAim, PatternCutin};

pub(super) fn read(image: &pe::Image<'_>) -> Result<SpellRecipe> {
    let descriptor = pe::word(image.slice(0x8e9b08 + 298 * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == 0x41a310
            && pe::word(image.slice(descriptor + 4, 4)?, 0)? == 0x4280b0
            && pe::word(image.slice(descriptor + 8, 4)?, 0)? == 0x41a880,
        "Tetraspell native handler changed"
    );
    for (at, expected) in [
        (0x41a355, &[0xb8, 0x3c, 1, 0, 0][..]),
        (0x41a3a8, &[0x6a, 1][..]),
        (0x41a4d0, &[0xc7, 0x43, 0x14, 0x50, 0xa5, 0x41, 0][..]),
        (0x41a5f7, &[0x66, 0x83, 0x7b, 0x36, 16][..]),
        (0x41a63f, &[0x8d, 0x81, 0x90, 1, 0, 0][..]),
        (0x41a669, &[0x66, 0x83, 0x7b, 0x36, 80][..]),
        (0x41a6b1, &[0x8d, 0x81, 0x20, 3, 0, 0][..]),
        (0x41a6c6, &[0x83, 0xc1, 0x1c][..]),
        (0x41a6de, &[0xba, 150, 0, 0, 0][..]),
        (0x41a72a, &[0x8d, 0x81, 0xb0, 4, 0, 0][..]),
        (0x41a73f, &[0x83, 0xc1, 0x38][..]),
        (0x41a757, &[0xba, 160, 0, 0, 0][..]),
        (0x41a7a3, &[0x8d, 0x81, 0x90, 1, 0, 0][..]),
        (0x41a7cd, &[0xba, 220, 0, 0, 0][..]),
        (0x41a819, &[0x8d, 0x81, 0x40, 6, 0, 0][..]),
        (0x41a82e, &[0x83, 0xc1, 0x54][..]),
        (0x41a58d, &[0xb9, 186, 0, 0, 0][..]),
        (0x41a598, &[0xf6, 0x86, 0x20, 0x13, 0, 0, 1][..]),
        (0x41a462, &[0xc6, 1, 1][..]),
        (0x41a465, &[0xc6, 0x83, 0xd6, 0, 0, 0, 1][..]),
        // The common initializer captures the target floor plus its unit nudge.
        (0x42805a, &[0xd9, 0x5f, 0x24][..]),
        (0x428067, &[0xe8, 0x84, 0, 4, 0][..]),
    ] {
        ensure!(
            image.slice(at, expected.len())? == expected,
            "Tetraspell callback changed at {at:#x}"
        );
    }
    let lifetime = pe::word(image.slice(0x41a356, 4)?, 0)?.try_into()?;
    let ticks = [
        u16::from(image.slice(0x41a5fb, 1)?[0]),
        u16::from(image.slice(0x41a66d, 1)?[0]),
        pe::word(image.slice(0x41a6df, 4)?, 0)?.try_into()?,
        pe::word(image.slice(0x41a758, 4)?, 0)?.try_into()?,
        pe::word(image.slice(0x41a7ce, 4)?, 0)?.try_into()?,
    ];
    let recipe = PatternSpell {
        lifetime,
        origin: PatternOrigin::SceneGround { toward_caster: 1. },
        heading_offset: 0.,
        startup_member: 1,
        startup_deferred: false,
        contacts: ticks
            .into_iter()
            .zip([(1, 0), (2, 1), (3, 2), (1, 0), (4, 3)])
            .map(|(tick, (projectile, hit_variant))| PatternContact {
                tick,
                projectile,
                hit_variant,
                aim: PatternAim::Captured,
                effect_member: 0,
                offset: [0.; 3],
            })
            .collect(),
        cutin: Some(PatternCutin {
            catalogue: 97,
            tick: pe::word(image.slice(0x41a58e, 4)?, 0)?.try_into()?,
            duration: 64,
            hold_ticks: 61,
        }),
        opponent_stop_ticks: lifetime,
    };
    recipe.validate()?;
    Ok(SpellRecipe::FixedPattern(recipe))
}
