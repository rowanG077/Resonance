//! The Unison prompt uses physical controller buttons from the battle HUD atlas.
use anyhow::Result;
use resonance_content::{battle::ui::UnisonArt, font::UiRegion};

pub(super) fn cook(
    palettes: [u8; 4],
    sprite: &mut impl FnMut(usize, [u32; 4]) -> Result<UiRegion>,
) -> Result<UnisonArt> {
    let mut button = |slot: usize| -> Result<[UiRegion; 2]> {
        let mut frame = |frame: u32| {
            sprite(
                usize::from(palettes[slot]),
                [384 + slot as u32 * 32, 64 + frame * 32, 32, 32],
            )
        };
        Ok([frame(0)?, frame(1)?])
    };
    Ok(UnisonArt {
        buttons: [button(0)?, button(1)?, button(2)?, button(3)?],
    })
}
