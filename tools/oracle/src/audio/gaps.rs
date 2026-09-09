//! Diagnose inserted silence without changing PCM or relaxing comparison gates.
use super::hash;
use anyhow::{Context, Result, ensure};
use hound::{SampleFormat, WavReader};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Copy, Serialize)]
pub(crate) struct Window {
    pub reference_start_frame: u32,
    pub actual_start_frame: u32,
    pub frames: u32,
}

#[derive(Debug, Serialize)]
struct Gap {
    actual_frame: u32,
    reference_frame: u32,
    frames: u32,
}

#[derive(Serialize)]
pub(crate) struct Report {
    reference_sha256: String,
    actual_sha256: String,
    sample_rate: u32,
    window: Window,
    pub matched_source_frames: u32,
    reference_end_frame: u32,
    inserted_silent_frames: u32,
    continuous_match: bool,
    gaps: Vec<Gap>,
}

fn scan(
    mut reference: impl Iterator<Item = Result<[i16; 2]>>,
    mut actual: impl Iterator<Item = Result<[i16; 2]>>,
    window: Window,
) -> Result<Report> {
    let mut report = Report {
        reference_sha256: String::new(),
        actual_sha256: String::new(),
        sample_rate: 0,
        window,
        matched_source_frames: 0,
        reference_end_frame: window.reference_start_frame,
        inserted_silent_frames: 0,
        continuous_match: false,
        gaps: vec![],
    };
    for frame in 0..window.frames {
        let expected = actual.next().context("truncated source PCM")??;
        let mut observed = reference.next().context("truncated reference PCM")??;
        let gap_start = report.reference_end_frame;
        while observed != expected && observed == [0; 2] {
            report.reference_end_frame += 1;
            observed = reference
                .next()
                .context("reference ended inside a silence gap")??;
        }
        let gap = report.reference_end_frame - gap_start;
        if gap > 0 {
            ensure!(report.gaps.len() < 100_000, "excessive silence gaps");
            report.gaps.push(Gap {
                actual_frame: window.actual_start_frame + frame,
                reference_frame: gap_start,
                frames: gap,
            });
            report.inserted_silent_frames += gap;
        }
        if observed != expected {
            break;
        }
        report.matched_source_frames += 1;
        report.reference_end_frame += 1;
    }
    report.continuous_match =
        report.matched_source_frames == window.frames && report.gaps.is_empty();
    Ok(report)
}

pub(crate) fn analyze(reference: &Path, actual: &Path, window: Window) -> Result<Report> {
    let mut a = WavReader::open(reference)?;
    let mut b = WavReader::open(actual)?;
    let spec = a.spec();
    ensure!(
        spec == b.spec()
            && spec.channels == 2
            && spec.bits_per_sample == 16
            && spec.sample_format == SampleFormat::Int,
        "silence analysis requires matching stereo PCM16 WAVs"
    );
    ensure!(
        window.frames > 0
            && u64::from(window.actual_start_frame) + u64::from(window.frames)
                <= u64::from(b.duration())
            && window.reference_start_frame < a.duration(),
        "invalid silence-analysis window"
    );
    a.seek(window.reference_start_frame)?;
    b.seek(window.actual_start_frame)?;
    fn frames(
        mut samples: impl Iterator<Item = std::result::Result<i16, hound::Error>>,
    ) -> impl Iterator<Item = Result<[i16; 2]>> {
        std::iter::from_fn(move || {
            samples
                .next()
                .map(|left| Ok([left?, samples.next().context("truncated stereo PCM")??]))
        })
    }
    let mut report = scan(
        frames(a.samples::<i16>()),
        frames(b.samples::<i16>()),
        window,
    )?;
    report.reference_sha256 = hash(reference)?;
    report.actual_sha256 = hash(actual)?;
    report.sample_rate = spec.sample_rate;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_inserted_silence_from_changed_or_deleted_audio() {
        let source = [[1, 2], [0, 0], [3, 4], [5, 6]];
        let window = Window {
            reference_start_frame: 10,
            actual_start_frame: 20,
            frames: 4,
        };
        let reference = [[1, 2], [0, 0], [0, 0], [0, 0], [3, 4], [5, 6]];
        let report = scan(
            reference.into_iter().map(Ok),
            source.into_iter().map(Ok),
            window,
        )
        .unwrap();
        assert_eq!(report.matched_source_frames, 4);
        assert_eq!(report.inserted_silent_frames, 2);
        assert_eq!(report.reference_end_frame, 16);
        assert!(!report.continuous_match);
        for incorrect in [
            [[1, 2], [0, 0], [3, 5], [5, 6]],
            [[1, 2], [0, 0], [5, 6], [0, 0]],
        ] {
            let report = scan(
                incorrect.into_iter().map(Ok),
                source.into_iter().map(Ok),
                window,
            )
            .unwrap();
            assert_eq!(report.matched_source_frames, 2);
            assert!(!report.continuous_match);
        }
        assert!(
            scan(
                source.into_iter().map(Ok),
                source.into_iter().map(Ok),
                window
            )
            .unwrap()
            .continuous_match
        );
    }
}
