//! Location lettering evaluated from the chosen image bank at runtime.
use anyhow::{Result, ensure};

// Caption banks have three shared layers and up to ten lettering tiles.
const LETTERING_TILES: usize = 10;
const FIRST_LETTER: usize = 3;
const BAR_SPEED: usize = 8;
const ORNAMENT_PHASE_TICKS: usize = 9;
const ORNAMENT_PHASES: usize = 5;
const ORNAMENT_ROW: u8 = 40;
const ORNAMENT_END: usize = ORNAMENT_PHASES * ORNAMENT_PHASE_TICKS;
const MAIN_FADE_TICKS: usize = 32;
const LETTER_DELAY: usize = 12;

pub struct Sprite {
    pub texture: usize,
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub alpha: u8,
}
/// Evaluate only the requested tick: no generated track or first-use preparation.
pub fn frame(
    textures: impl ExactSizeIterator<Item = [u32; 2]>,
    age: usize,
    mut draw: impl FnMut(Sprite),
) -> Result<()> {
    let count = textures.len();
    ensure!(
        (FIRST_LETTER + 1..=FIRST_LETTER + LETTERING_TILES).contains(&count),
        "invalid location caption image count"
    );
    let mut widths = [0_u32; FIRST_LETTER + LETTERING_TILES];
    for (width, size) in widths.iter_mut().zip(textures) {
        *width = size[0];
    }
    let main_width = widths[2];
    let multi = main_width > 8;
    let total_width: u64 = widths[FIRST_LETTER..count]
        .iter()
        .map(|&w| u64::from(w))
        .sum();
    let width = total_width.max(u64::from(main_width));
    if age == 0 {
        return Ok(()); // Initialization callback draws nothing.
    }
    let mut emit = |texture, rect, uv: [u8; 4], alpha| {
        draw(Sprite {
            texture,
            rect,
            uv: uv.map(|v| f32::from(v) / 256.),
            alpha,
        });
    };
    let half = (age.saturating_mul(BAR_SPEED).min(width as usize) / 2) as f32;
    emit(0, [-half, -4., half, 4.], [0, 0, 255, 255], 255);

    let ornament_age = age.saturating_sub(width as usize / BAR_SPEED);
    if ornament_age > 0 {
        let phase = (ornament_age / ORNAMENT_PHASE_TICKS).min(ORNAMENT_PHASES - 1) as u8;
        let alpha =
            (31 * (ornament_age % ORNAMENT_PHASE_TICKS + usize::from(phase > 0))).min(255) as u8;
        let offset = (width / 2) as f32 + 32.;
        for (x, u0, u1) in [(-offset, 0, 127), (offset, 128, 255)] {
            let rect = [x - 32., -32., x + 32., 32.];
            let uv = |row| [u0, row, u1, row + ORNAMENT_ROW];
            if usize::from(phase) < ORNAMENT_PHASES - 1 {
                emit(1, rect, uv((phase + 1) * ORNAMENT_ROW), alpha);
            }
            emit(1, rect, uv(phase * ORNAMENT_ROW), 255);
        }
    }
    if multi && ornament_age >= ORNAMENT_END {
        let shine = ((ornament_age - (ORNAMENT_END - 1)).min(MAIN_FADE_TICKS) * 8).min(255) as u8;
        let half = (main_width / 2) as f32;
        emit(2, [-half, -52., half, -4.], [0, 0, 255, 255], shine);
    }
    // A two-line caption fades in its main plate for 32 ticks first.
    let lettering_start = ORNAMENT_END + if multi { MAIN_FADE_TICKS - 1 } else { 0 };
    let mut x = -((total_width / 2) as f32);
    let y = if multi { 4. } else { -64. };
    for (i, &width) in widths[FIRST_LETTER..count].iter().enumerate() {
        let start = lettering_start + LETTER_DELAY * i;
        if ornament_age < start {
            break;
        }
        let alpha = (1 + 10 * (ornament_age - start + 1).min(26)).min(255) as u8;
        let right = x + width as f32;
        emit(
            i + FIRST_LETTER,
            [x, y, right, y + 64.],
            [0, 0, 255, 255],
            alpha,
        );
        x = right;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn caption_width_is_not_limited_to_the_native_screen() {
        let mut frame = Vec::new();
        super::frame(
            [[144, 8], [144, 512], [1024, 48], [800, 64]].into_iter(),
            128,
            |sprite| frame.push(sprite),
        )
        .unwrap();
        assert_eq!(frame[0].rect, [-512., -4., 512., 4.]);
    }

    #[test]
    fn iselia_reveal_matches_the_paused_dolphin_controller() {
        // GQSEAF school grounds: remaining hold 129/190, phase 1, frame 2,
        // bar alpha 93; neither the main plate nor lettering has started.
        let textures = [[144, 8], [144, 512], [400, 48], [192, 64]];
        let mut frame = Vec::new();
        super::frame(textures.into_iter(), 61, |sprite| frame.push(sprite)).unwrap();
        assert_eq!(frame.len(), 5);
        assert_eq!(frame[0].rect, [-200., -4., 200., 4.]);
        for (sprite, (rect, uv, alpha)) in frame[1..].iter().zip([
            ([-264., -32., -200., 32.], [0., 80., 127., 120.], 93),
            ([-264., -32., -200., 32.], [0., 40., 127., 80.], 255),
            ([200., -32., 264., 32.], [128., 80., 255., 120.], 93),
            ([200., -32., 264., 32.], [128., 40., 255., 80.], 255),
        ]) {
            assert_eq!(sprite.texture, 1);
            assert_eq!(sprite.rect, rect);
            assert_eq!(sprite.uv, uv.map(|v| v / 256.));
            assert_eq!(sprite.alpha, alpha);
        }
    }
}
