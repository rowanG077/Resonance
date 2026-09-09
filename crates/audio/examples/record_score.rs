//! Record a cooked score through the shared synthesizer without an audio device.
use anyhow::{Context, Result, ensure};
use resonance_audio::{package::Package, sequence};
use std::{fs, path::Path};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .context("ASSETS PACKAGE OUTPUT FRAMES required")?;
    let package = args.next().context("PACKAGE required")?;
    let output = args.next().context("OUTPUT required")?;
    let frames: u32 = args.next().context("FRAMES required")?.parse()?;
    ensure!(!Path::new(&output).exists(), "recording already exists");
    let data = Package::load(Path::new(&root), &package)?;
    let preview = sequence::render_preview(
        &data.resources,
        &data.score,
        &data.tables,
        data.reverbs,
        frames,
    )?;
    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = hound::WavWriter::create(
        &output,
        hound::WavSpec {
            channels: 2,
            sample_rate: 32028,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in preview.pcm {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    println!("Recorded {frames} frames to {output}; no audio device");
    Ok(())
}
