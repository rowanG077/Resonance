use super::*;
#[path = "reference.rs"]
mod reference;

fn sequence() -> BootAssets {
    serde_json::from_value(serde_json::json!([
        {"image":"nintendo.ktx2","fade_out_at":180},
        {"image":"namco.ktx2","fade_out_at":240,"skip_after":120},
        {"image":"tales-studio.ktx2","fade_out_at":180},
        {"image":"adx.ktx2","fade_out_at":120}
    ]))
    .unwrap()
}

#[test]
fn every_presented_frame_matches_before_migration_including_skip_boundaries() -> anyhow::Result<()>
{
    // No input, continuously held input, and a single press on every possible
    // update cover both transitions and the complete skippable interval.
    for press in 0..=reference::LOGO_TICKS + 1 {
        let mut current = Logos::new(sequence())?;
        let mut original = reference::Reference::default();
        for tick in 1..=reference::LOGO_TICKS {
            let accept = press == reference::LOGO_TICKS + 1 || press == tick;
            current.step(accept);
            original.step(accept);
            assert_eq!(
                (
                    current.tick,
                    [3, 0, 1, 2][current.texture],
                    current.alpha,
                    current.active()
                ),
                (
                    original.tick,
                    original.texture,
                    original.alpha,
                    original.active()
                ),
                "press {press}, tick {tick}"
            );
        }
    }
    let mut current = Logos::new(sequence())?;
    for _ in 0..972 {
        current.step(false);
    }
    // Independent Dolphin startup-1020-silent checkpoint: ADX, local tick 180.
    assert_eq!(
        (current.texture, current.phase_tick, current.alpha),
        (3, 180, 11)
    );
    assert_eq!(current.duration(), 976);
    Ok(())
}

#[test]
fn edited_sequence_controls_order_fades_skips_and_duration() -> anyhow::Result<()> {
    let sequence: BootAssets = serde_json::from_value(serde_json::json!([
        {"image":"custom-first.ktx2","fade_out_at":10,"fade_ticks":2,"skip_after":5},
        {"image":"custom-second.ktx2","fade_out_at":15,"fade_ticks":3}
    ]))?;
    let mut natural = Logos::new(sequence.clone())?;
    let mut skipped = Logos::new(sequence)?;
    assert_eq!(natural.duration(), 30);
    natural.step(false);
    assert_eq!((natural.texture, natural.alpha), (0, 128));
    while natural.active() {
        natural.step(false);
    }
    assert_eq!((natural.tick, natural.texture, natural.alpha), (30, 1, 0));
    while skipped.active() {
        skipped.step(true);
    }
    assert_eq!((skipped.tick, skipped.texture, skipped.alpha), (25, 1, 0));
    let mut empty = Logos::new(BootAssets::default())?;
    empty.step(true);
    assert_eq!(
        (empty.active(), empty.duration(), empty.tick),
        (false, 0, 0)
    );
    Ok(())
}
