use anyhow::Result;
use std::path::Path;

/// Write only the supplied path; callers validate temporary files before publishing them.
pub fn write_pcm16(
    path: &Path,
    channels: u16,
    sample_rate: u32,
    samples: impl IntoIterator<Item = i16>,
) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}
