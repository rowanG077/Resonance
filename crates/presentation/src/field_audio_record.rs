//! Bounded evidence recording through the actual field audio adapter.
use super::{Assets, AudioCommand, RATE};
use anyhow::{Context, Result, ensure};
use resonance_playback::Decodable;
use std::path::Path;

/// Schedule ordinary audio requests at PCM frame boundaries, without an audio
/// device, window, script clock, or a second implementation of the mixer.
/// This diagnostic cannot establish that the game's scripts issue the requests
/// at the right time; the complete New Game recording covers that separately.
pub fn record_field_audio(
    root: &Path,
    output: &Path,
    frames: u64,
    events: &[(u64, AudioCommand)],
    stereo: bool,
    levels: [u8; 3],
) -> Result<()> {
    ensure!(
        (1..=u64::from(RATE) * 300).contains(&frames),
        "audio recording exceeds 300 seconds"
    );
    ensure!(
        events.windows(2).all(|pair| pair[0].0 <= pair[1].0),
        "audio requests are out of order"
    );
    ensure!(
        events.iter().all(|(frame, _)| *frame < frames),
        "audio request is outside the recording"
    );
    ensure!(!output.exists(), "audio recording already exists");
    let (source, mut control) = Assets::load(root, 340)?.session();
    control.stereo(stereo)?;
    control.levels(levels)?;
    let mut stream = source.decoder();
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut wave = hound::WavWriter::create(output, super::PCM_SPEC)?;
    let mut events = events.iter().peekable();
    for frame in 0..frames {
        while let Some((_, command)) = events.next_if(|(at, _)| *at == frame) {
            control.send(command.clone())?;
        }
        for sample in stream
            .frame()?
            .context("field audio recording stopped early")?
        {
            wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
        }
    }
    control.check()?;
    wave.finalize()?;
    Ok(())
}
