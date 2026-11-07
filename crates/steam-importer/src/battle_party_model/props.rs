//! The executable's four costume-owned cosmetic weapon substitutions.
use crate::pe;
use anyhow::{Result, ensure};
use resonance_content::battle_model::document::WeaponProp;
use std::{collections::BTreeMap, num::NonZeroU16};

pub(super) fn read(image: &pe::Image<'_>) -> Result<BTreeMap<(u8, u8), WeaponProp>> {
    // The same clear-count byte is incremented and capped at100 by the
    // complete-game command; the battle selector tests it before drawing.
    ensure!(
        image.slice(0x56c37d, 7)? == [0x0f, 0xb6, 0xb8, 0x5a, 0x0f, 0, 0]
            && image.slice(0x56c384, 6)? == [0x8d, 0x4f, 1, 0x83, 0xf9, 0x64]
            && image.slice(0x47cc2b, 7)? == [0x80, 0xbf, 0x5a, 0x0f, 0, 0, 1]
            && image.slice(0x47cc61, 4)? == [0xa8, 0x0f, 0x75, 0x40],
        "costume prop clear-count or random gate changed"
    );
    let mut rules = BTreeMap::new();
    for address in [0x47cc65, 0x47cc75, 0x47cc85, 0x47cc95] {
        let row = image.slice(address, 16)?;
        ensure!(
            row[..2] == [0x80, 0xfa]
                && row[3] == 0x75
                && row[5..8] == [0x80, 0x7e, 0x11]
                && row[9..12] == [0x75, 5, 0xb9],
            "costume prop selector changed"
        );
        ensure!(
            rules
                .insert(
                    (row[2], row[8]),
                    WeaponProp {
                        equipment: pe::word(row, 12)?.try_into()?,
                        minimum_game_clears: 1,
                        one_in: NonZeroU16::new(16).unwrap(),
                    }
                )
                .is_none(),
            "duplicate costume prop"
        );
    }
    Ok(rules)
}
