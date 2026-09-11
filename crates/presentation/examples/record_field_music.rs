//! Record background music through the actual field source without a device.
use anyhow::{Context, Result, ensure};
use resonance_events::AudioCommand;
use std::path::Path;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .context("ASSETS OUTPUT MUSIC SECONDS [VOLUME FADE_TICKS MODE] required")?;
    let output = args.next().context("OUTPUT required")?;
    let music = args.next().context("MUSIC required")?.parse()?;
    let seconds: u64 = args.next().context("SECONDS required")?.parse()?;
    let volume: u8 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(127);
    let fade_ticks = args.next().map(|s| s.parse()).transpose()?.unwrap_or(6);
    let stereo = match args.next().as_deref() {
        None | Some("stereo") => true,
        Some("mono") => false,
        _ => anyhow::bail!("mode must be stereo or mono"),
    };
    ensure!(args.next().is_none(), "unexpected arguments");
    ensure!(
        (1..=300).contains(&seconds),
        "duration must be 1..=300 seconds"
    );
    ensure!(volume <= 127, "volume must be 0..=127");
    let frames = seconds * 32028;
    resonance_presentation::record_field_audio(
        Path::new(&root),
        Path::new(&output),
        frames,
        &[
            (0, AudioCommand::Music(music)),
            (
                0,
                AudioCommand::MusicVolume {
                    volume,
                    duration_ticks: fade_ticks,
                },
            ),
        ],
        stereo,
        [127; 3],
    )?;
    println!("Recorded {seconds} seconds to {output}; no audio device");
    Ok(())
}
