//! Render uncomplicated one-shot cues once during import. Programmed timing,
//! release envelopes, loops and auxiliary routing retain their authored recipe.
use super::*;
use crate::data::Command;

pub fn simple_wave(loaded: Arc<Loaded>) -> Result<Option<Vec<u8>>> {
    if loaded.score().first_events.is_empty()
        || loaded
            .resources()
            .samples
            .values()
            .any(|sample| sample.loop_length != 0)
        || loaded
            .resources()
            .programs
            .values()
            .flatten()
            .any(|command| {
                !matches!(
                    command,
                    Command::AgePeriod { .. }
                        | Command::Interpolation { .. }
                        | Command::SelectControl { .. }
                        | Command::StartSample { .. }
                        | Command::Wait { .. }
                        | Command::StopSample
                        | Command::SetAge { .. }
                        | Command::End
                )
            })
    {
        return Ok(None);
    }
    let mut stream = crate::sequence::stream::Stream::new(loaded, false)?;
    let mut pcm = Vec::new();
    while let Some(block) = stream.block(LiveControls::default())? {
        if pcm.len() + block.len() * 2 > SOURCE_RATE as usize * 2 * 30 {
            return Ok(None);
        }
        for frame in block {
            if frame[1..].iter().flatten().any(|&v| v != 0)
                || frame[0].iter().any(|&v| i16::try_from(v).is_err())
            {
                return Ok(None);
            }
            pcm.extend(frame[0].map(|v| v as i16));
        }
    }
    if pcm.is_empty() {
        return Ok(None);
    }
    let mut bytes = Cursor::new(Vec::new());
    let mut wave = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 2,
            sample_rate: SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in pcm {
        wave.write_sample(sample)?;
    }
    wave.finalize()?;
    Ok(Some(bytes.into_inner()))
}
