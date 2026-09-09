//! Compare device-rate conversion with an independent libsoxr reference.
use anyhow::{Result, ensure};
use resonance_playback::{Converter, SOURCE_BLOCK, SOURCE_RATE};
use std::{path::Path, process::Command};

fn write(path: &Path, rate: u32, samples: &[[f32; 2]]) -> Result<()> {
    let mut wave = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for frame in samples {
        for &sample in frame {
            wave.write_sample(sample)?;
        }
    }
    wave.finalize()?;
    Ok(())
}
fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("new output directory");
    let path = Path::new(&path);
    ensure!(!path.exists(), "resampler evidence directory exists");
    std::fs::create_dir_all(path)?;
    let input: Vec<_> = (0..SOURCE_RATE * 4)
        .map(|frame| {
            let t = f64::from(frame) / f64::from(SOURCE_RATE);
            if t < 0.25 {
                if frame == 1000 {
                    [0.5, 0.]
                } else if frame == 3000 {
                    [0., -0.5]
                } else {
                    [0.; 2]
                }
            } else if t < 1.25 {
                [
                    (0.2 * (std::f64::consts::TAU * 997. * t).sin()) as f32,
                    (0.1 * (std::f64::consts::TAU * 4001. * t).sin()) as f32,
                ]
            } else if t < 3.25 {
                let x = t - 1.25;
                let phase = std::f64::consts::TAU * (50. * x + 3200. * x * x);
                [(0.2 * phase.sin()) as f32, (0.1 * phase.cos()) as f32]
            } else {
                [0.; 2]
            }
        })
        .collect();
    write(&path.join("native.wav"), SOURCE_RATE, &input)?;
    let mut reports = Vec::new();
    for rate in [44100, 48000] {
        let mut converter = resonance_media::output::Resampler::new(rate)?;
        let mut actual = Vec::new();
        for block in input.chunks(SOURCE_BLOCK) {
            converter.convert(block, &mut actual)?;
        }
        let delay = converter.delay_frames();
        converter.finish(&mut actual)?;
        write(&path.join(format!("output-{rate}.wav")), rate, &actual)?;
        let reference = path.join(format!("soxr-{rate}.wav"));
        ensure!(
            Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
                .arg(path.join("native.wav"))
                .args([
                    "-af",
                    &format!("aresample={rate}:resampler=soxr:precision=28"),
                    "-c:a",
                    "pcm_f32le"
                ])
                .arg(&reference)
                .status()?
                .success(),
            "FFmpeg libsoxr reference failed"
        );
        let mut wave = hound::WavReader::open(&reference)?;
        ensure!(
            wave.spec().sample_rate == rate && wave.spec().channels == 2,
            "incorrect reference format"
        );
        let reference = wave
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            actual.len().abs_diff(reference.len() / 2) <= 1,
            "output/reference duration differs"
        );
        let (mut energy, mut error, mut maximum) = (0f64, 0f64, 0f64);
        // Compare the steady passband and sweep. Impulse and abrupt section
        // boundaries intentionally expose different filters in the saved files.
        for frame in (rate as usize * 3 / 10)..(rate as usize * 32 / 10) {
            if (rate as usize * 12 / 10..rate as usize * 13 / 10).contains(&frame) {
                continue;
            }
            for channel in 0..2 {
                let expected = f64::from(reference[frame * 2 + channel]);
                let delta = f64::from(actual[frame][channel]) - expected;
                energy += expected * expected;
                error += delta * delta;
                maximum = maximum.max(delta.abs());
            }
        }
        let snr = 10. * (energy / error).log10();
        ensure!(
            snr > 55.,
            "device resampling differs excessively from libsoxr: {snr:.2} dB"
        );
        let report = serde_json::json!({"rate":rate,"frames":actual.len(),"reference_frames":reference.len()/2,
            "filter_delay_before_flush":delay, "passband_and_sweep_snr_db":snr, "maximum_error":maximum});
        println!("{report}");
        reports.push(report);
    }
    std::fs::write(
        path.join("comparison.json"),
        serde_json::to_vec_pretty(&reports)?,
    )?;
    Ok(())
}
