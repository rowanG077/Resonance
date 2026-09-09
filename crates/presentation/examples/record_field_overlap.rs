//! Record a cue over fading music through the actual field source, silently.
use anyhow::{Context, Result, ensure};
use resonance_events::AudioCommand;
use std::path::Path;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .context("ASSETS OUTPUT MUSIC CUE CUE_FRAME FRAMES required")?;
    let output = args.next().context("OUTPUT required")?;
    let music = args.next().context("MUSIC required")?.parse()?;
    let cue = args.next().context("CUE required")?.parse()?;
    let cue_frame = args.next().context("CUE_FRAME required")?.parse()?;
    let frames = args.next().context("FRAMES required")?.parse()?;
    ensure!(args.next().is_none(), "unexpected arguments");
    resonance_presentation::record_field_audio(
        Path::new(&root),
        Path::new(&output),
        frames,
        &[
            (0, AudioCommand::Music(music)),
            (
                cue_frame,
                AudioCommand::MusicVolume {
                    volume: 0,
                    duration_ticks: 1,
                },
            ),
            (
                cue_frame,
                AudioCommand::Sound {
                    id: cue,
                    pan: 64,
                    volume: 127,
                    slot: None,
                },
            ),
        ],
    )?;
    println!("Recorded {frames} frames to {output}; no audio device");
    Ok(())
}
