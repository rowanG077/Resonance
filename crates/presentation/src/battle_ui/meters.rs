//! Live battle meters share a compact row above the party panels.
use super::*;
use resonance_battle::{MAX_ESCAPE_GAUGE, MAX_UNISON_GAUGE, item::ITEM_COOLDOWN_TICKS};

pub(super) const COUNT: usize = 2;

pub(super) fn draw(frame: &BattleFrame, font: &BitmapFont, batches: &mut [Batch]) -> Result<()> {
    if frame.recognized_result.is_some() {
        return Ok(());
    }
    if frame.item_cooldown > 0 {
        let seconds = f64::from(frame.item_cooldown) / resonance_game::clock::UPDATE_HZ;
        meter(
            batches,
            font,
            16.,
            f32::from(frame.item_cooldown) / f32::from(ITEM_COOLDOWN_TICKS),
            &format!("ITEM {seconds:.1}"),
            [0.3, 0.8, 0.5, 1.],
        )?;
    }
    if let Some(escape) = frame.escape
        && (escape.requested || escape.gauge > 0)
    {
        meter(
            batches,
            font,
            232.,
            f32::from(escape.gauge) / f32::from(MAX_ESCAPE_GAUGE),
            "ESCAPE",
            [0.95, 0.65, 0.2, 1.],
        )?;
    }
    if frame.unison_available {
        meter(
            batches,
            font,
            448.,
            f32::from(frame.unison_gauge) / f32::from(MAX_UNISON_GAUGE),
            if frame.unison_gauge >= MAX_UNISON_GAUGE {
                "UNISON READY"
            } else {
                "UNISON"
            },
            [0.4, 0.65, 1., 1.],
        )?;
    }
    Ok(())
}

fn meter(
    batches: &mut [Batch],
    font: &BitmapFont,
    x: f32,
    progress: f32,
    label: &str,
    color: [f32; 4],
) -> Result<()> {
    const WIDTH: f32 = 176.;
    let bars = &mut batches[0];
    bars.quad(
        [x - 4., 362., x + WIDTH + 4., 386.],
        [0.5; 4],
        [0.04, 0.05, 0.08, 0.85],
    );
    bars.quad([x, 378., x + WIDTH, 384.], [0.5; 4], [0.2, 0.22, 0.25, 1.]);
    if progress > 0. {
        bars.quad(
            [x, 378., x + WIDTH * progress.clamp(0., 1.), 384.],
            [0.5; 4],
            color,
        );
    }
    text::glyphs(
        &mut batches[1],
        font,
        label,
        [x, 364.],
        [10., 12.],
        [128, 128, 128, 255],
    )
}
