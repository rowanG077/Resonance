//! Record a spoken line through the field mixer, without an audio device.
use anyhow::{Context, Result, ensure};
use resonance_events::AudioCommand;
use std::path::Path;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .context("ASSETS OUTPUT VOICE FRAMES VOLUME required")?;
    let output = args.next().context("OUTPUT required")?;
    let voice = args.next().context("VOICE required")?.parse()?;
    let frames = args.next().context("FRAMES required")?.parse()?;
    let volume = args.next().context("VOLUME required")?.parse()?;
    ensure!(args.next().is_none(), "unexpected arguments");
    resonance_presentation::record_field_audio(
        Path::new(&root),
        Path::new(&output),
        frames,
        &[(0, AudioCommand::Voice(voice))],
        true,
        [127, 127, volume],
    )?;
    println!("Recorded {output}; no audio device");
    Ok(())
}
