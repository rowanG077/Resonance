//! Native Time Stop becomes an ordinary timed opponent benefit and clock artwork.
use super::*;

pub(super) fn read(image: &pe::Image<'_>) -> Result<SpellRecipe> {
    let descriptor = pe::word(image.slice(0x8e9b08 + 266 * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == 0x417be0
            && pe::word(image.slice(descriptor + 8, 4)?, 0)? == 0x417d40,
        "Time Stop native callbacks changed"
    );
    for (at, bytes) in [
        (0x417c1a, &[0xb8, 0x36, 1, 0, 0][..]),
        (0x417c9a, &[0xf6, 0xd2][..]),
        (0x417c9f, &[0x83, 0xe2, 1][..]),
        (0x417cb3, &[0xc6, 0, 1][..]),
        (0x417cb6, &[0xc6, 0x87, 0xd6, 0, 0, 0, 1][..]),
        (0x417d77, &[0x38, 0x90, 0xd6, 0, 0, 0][..]),
        (0x417d8c, &[0x88, 0x94, 0x0b, 0xee, 0x93, 0, 0][..]),
    ] {
        ensure!(
            image.slice(at, bytes.len())? == bytes,
            "Time Stop side-freeze callback changed at {at:#x}"
        );
    }
    let lifetime: u16 = pe::word(image.slice(0x417c1b, 4)?, 0)?.try_into()?;
    let pattern = PatternSpell {
        lifetime,
        origin: PatternOrigin::SceneFixed { position: [0.; 3] },
        heading_offset: 0.,
        startup_member: 1,
        startup_deferred: true,
        contacts: Vec::new(),
        cutin: None,
        opponent_stop_ticks: lifetime,
    };
    pattern.validate()?;
    Ok(SpellRecipe::FixedPattern(pattern))
}
