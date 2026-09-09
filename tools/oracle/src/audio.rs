use anyhow::{Context, Result, ensure};
use hound::{SampleFormat, WavReader};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

pub(crate) mod gaps;

#[derive(Serialize)]
pub(super) struct Window {
    pub reference_start_frame: u32,
    pub actual_start_frame: u32,
    pub frames: u32,
    pub tolerance: f64,
    pub max_changed_fraction: f64,
}

#[derive(Serialize)]
pub(super) struct Report {
    reference_sha256: String,
    actual_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_baseline_sha256: Option<String>,
    window: Window,
    sample_rate: u32,
    channels: u16,
    changed_samples: u64,
    changed_fraction: f64,
    mean_absolute_error: f64,
    root_mean_squared_error: f64,
    max_sample_error: f64,
    pub passed: bool,
}

fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn samples(wave: &mut WavReader<BufReader<File>>) -> Box<dyn Iterator<Item = Result<f64>> + '_> {
    let spec = wave.spec();
    if spec.sample_format == SampleFormat::Float {
        Box::new(
            wave.samples::<f32>()
                .map(|sample| Ok(f64::from(sample?) * 32768.)),
        )
    } else {
        let scale = 2f64.powi(16 - i32::from(spec.bits_per_sample));
        Box::new(
            wave.samples::<i32>()
                .map(move |sample| Ok(f64::from(sample?) * scale)),
        )
    }
}

pub(super) fn compare(
    reference: &Path,
    actual: &Path,
    baseline: Option<&Path>,
    window: &Window,
) -> Result<Report> {
    ensure!(
        window.frames > 0
            && window.tolerance.is_finite()
            && window.tolerance >= 0.
            && (0.0..=1.0).contains(&window.max_changed_fraction),
        "invalid audio comparison window or tolerance"
    );
    let mut a = WavReader::open(reference)?;
    let mut b = WavReader::open(actual)?;
    let mut baseline_wave = baseline.map(WavReader::open).transpose()?;
    let spec = a.spec();
    ensure!(
        spec.sample_rate == b.spec().sample_rate && spec.channels == b.spec().channels,
        "audio formats disagree; comparison never silently resamples or remixes"
    );
    ensure!(
        u64::from(window.reference_start_frame) + u64::from(window.frames)
            <= u64::from(a.duration())
            && u64::from(window.actual_start_frame) + u64::from(window.frames)
                <= u64::from(b.duration()),
        "audio comparison window extends beyond recording"
    );
    a.seek(window.reference_start_frame)?;
    b.seek(window.actual_start_frame)?;
    if let Some(wave) = &mut baseline_wave {
        ensure!(
            wave.spec().sample_rate == spec.sample_rate && wave.spec().channels == spec.channels,
            "baseline audio format disagrees with reference"
        );
        ensure!(
            u64::from(window.reference_start_frame) + u64::from(window.frames)
                <= u64::from(wave.duration()),
            "baseline comparison window extends beyond recording"
        );
        wave.seek(window.reference_start_frame)?;
    }
    let mut a = samples(&mut a);
    let mut b = samples(&mut b);
    let mut baseline_samples = baseline_wave.as_mut().map(samples);
    let count = u64::from(window.frames) * u64::from(spec.channels);
    let (mut changed, mut total, mut squared, mut maximum) = (0u64, 0f64, 0f64, 0f64);
    for _ in 0..count {
        let mut a = a.next().context("truncated reference audio")??;
        if let Some(samples) = &mut baseline_samples {
            a -= samples.next().context("truncated baseline audio")??;
        }
        let b = b.next().context("truncated actual audio")??;
        ensure!(a.is_finite() && b.is_finite(), "nonfinite PCM sample");
        let error = (a - b).abs();
        changed += u64::from(error > window.tolerance);
        total += error;
        squared += error * error;
        maximum = maximum.max(error);
    }
    let fraction = changed as f64 / count as f64;
    Ok(Report {
        reference_sha256: hash(reference)?,
        actual_sha256: hash(actual)?,
        reference_baseline_sha256: baseline.map(hash).transpose()?,
        window: Window { ..*window },
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        changed_samples: changed,
        changed_fraction: fraction,
        mean_absolute_error: total / count as f64,
        root_mean_squared_error: (squared / count as f64).sqrt(),
        max_sample_error: maximum,
        passed: fraction <= window.max_changed_fraction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_windows_preserve_channel_order_and_reject_overruns() {
        let path =
            std::env::temp_dir().join(format!("resonance-pcm-window-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 32000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for sample in [0i16, 0, -100, 400, -100, 400, 20, 30] {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        let mut window = Window {
            reference_start_frame: 1,
            actual_start_frame: 2,
            frames: 1,
            tolerance: 0.,
            max_changed_fraction: 0.,
        };
        assert!(compare(&path, &path, None, &window).unwrap().passed);
        window.frames = 2;
        let report = compare(&path, &path, None, &window).unwrap();
        assert!(!report.passed);
        assert_eq!(report.changed_samples, 2);
        window.frames = 3;
        assert!(compare(&path, &path, None, &window).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn baseline_subtraction_preserves_channels_and_checks_its_window() {
        let root =
            std::env::temp_dir().join(format!("resonance-pcm-baseline-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 32028,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let reference = root.join("reference.wav");
        let actual = root.join("actual.wav");
        let baseline = root.join("baseline.wav");
        for (path, pcm) in [
            (&reference, [0i16, 0, 130, -250]),
            (&actual, [30, -50, 0, 0]),
            (&baseline, [0, 0, 100, -200]),
        ] {
            let mut writer = hound::WavWriter::create(path, spec).unwrap();
            for sample in pcm {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        let window = Window {
            reference_start_frame: 1,
            actual_start_frame: 0,
            frames: 1,
            tolerance: 0.,
            max_changed_fraction: 0.,
        };
        assert!(!compare(&reference, &actual, None, &window).unwrap().passed);
        let report = compare(&reference, &actual, Some(&baseline), &window).unwrap();
        assert!(report.passed);
        assert_eq!(
            report.reference_baseline_sha256,
            Some(hash(&baseline).unwrap())
        );
        hound::WavWriter::create(&baseline, spec)
            .unwrap()
            .finalize()
            .unwrap();
        assert!(compare(&reference, &actual, Some(&baseline), &window).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
