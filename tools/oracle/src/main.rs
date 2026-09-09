//! Repeatable Dolphin input fixtures and explicit image comparisons.
mod audio;
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use image::{Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(about = "Resonance Dolphin oracle tools")]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Diagnose reference-only zero gaps. Reports timing mismatch even when content matches.
    AnalyzeAudioGaps {
        reference: PathBuf,
        actual: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        reference_start_frame: u32,
        #[arg(long, default_value_t = 0)]
        actual_start_frame: u32,
        #[arg(long)]
        frames: u32,
    },
    /// Compare explicit PCM windows offline; never opens an audio output device.
    CompareAudio {
        reference: PathBuf,
        actual: PathBuf,
        /// Subtract a baseline recording at the same reference-frame offset.
        #[arg(long)]
        reference_baseline: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        reference_start_frame: u32,
        #[arg(long)]
        actual_start_frame: u32,
        #[arg(long)]
        frames: u32,
        /// Absolute error in signed 16-bit PCM units.
        #[arg(long, default_value_t = 0.)]
        tolerance: f64,
        #[arg(long, default_value_t = 0.)]
        max_changed_fraction: f64,
    },
    /// Generate a GameCube DTM from an explicit sequence of controller polls.
    Dtm {
        case: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Compare equal-size images; writes metrics and an amplified difference.
    Compare {
        reference: PathBuf,
        actual: PathBuf,
        /// Also usable for small UI elements whose errors a full-frame gate can miss.
        #[arg(long, num_args = 4, value_names = ["X", "Y", "WIDTH", "HEIGHT"])]
        region: Vec<u32>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 0)]
        tolerance: u8,
        #[arg(long, default_value_t = 0.)]
        max_changed_fraction: f64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayCase {
    game_id: String,
    polls: u32,
    #[serde(default = "default_rtc")]
    rtc: u64,
    #[serde(default)]
    from_save_state: bool,
    inputs: Vec<PollInput>,
}
fn default_rtc() -> u64 {
    1_700_000_000
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PollInput {
    poll: u32,
    duration: u32,
    buttons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stick: Option<[u8; 2]>,
}

fn encode_dtm(case: &ReplayCase) -> Result<Vec<u8>> {
    ensure!(
        case.game_id == "GQSEAF",
        "only GQSEAF is supported by this oracle profile"
    );
    ensure!(
        (1..=1_000_000).contains(&case.polls),
        "poll count must be 1..=1000000"
    );
    let mut buttons = vec![0x4000u16; case.polls as usize]; // Controller connected.
    let mut sticks = vec![[128u8; 2]; case.polls as usize];
    for input in &case.inputs {
        let end = input
            .poll
            .checked_add(input.duration)
            .context("input range overflow")?;
        ensure!(
            input.duration > 0 && end <= case.polls,
            "input exceeds replay"
        );
        let mut bits = 0;
        for button in &input.buttons {
            bits |= match button.as_str() {
                "start" => 1,
                "a" => 2,
                "b" => 4,
                "x" => 8,
                "y" => 16,
                "z" => 32,
                "up" => 64,
                "down" => 128,
                "left" => 256,
                "right" => 512,
                "l" => 1024,
                "r" => 2048,
                _ => anyhow::bail!("unknown button {button}"),
            };
        }
        for state in &mut buttons[input.poll as usize..end as usize] {
            *state |= bits;
        }
        if let Some(stick) = input.stick {
            sticks[input.poll as usize..end as usize].fill(stick);
        }
    }
    let mut bytes = vec![0u8; 256];
    bytes[..4].copy_from_slice(b"DTM\x1a");
    bytes[4..10].copy_from_slice(case.game_id.as_bytes());
    bytes[11] = 1; // Port 1, boot replay, configuration supplied separately.
    bytes[12] = u8::from(case.from_save_state);
    for offset in [13, 21] {
        bytes[offset..offset + 8].copy_from_slice(&(case.polls as u64).to_le_bytes());
    }
    bytes[49..58].copy_from_slice(b"Resonance");
    bytes[129..137].copy_from_slice(&case.rtc.to_le_bytes());
    bytes[151] = 1; // Memory card in slot A.
    bytes[152] = 1; // Fresh card for boot fixtures.
    // No measured CPU tick endpoint for authored fixtures. End at the final
    // input record; zero here would terminate playback after its first poll.
    bytes[237..245].copy_from_slice(&u64::MAX.to_le_bytes());
    for (state, stick) in buttons.into_iter().zip(sticks) {
        bytes.extend(state.to_le_bytes());
        bytes.extend([0, 0, stick[0], stick[1], 128, 128]);
    }
    Ok(bytes)
}

#[derive(Debug, Serialize)]
struct Comparison {
    reference_sha256: String,
    actual_sha256: String,
    width: u32,
    height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<[u32; 4]>,
    tolerance: u8,
    changed_pixels: u64,
    changed_fraction: f64,
    mean_absolute_error: f64,
    root_mean_squared_error: f64,
    max_channel_error: u8,
    allowed_changed_fraction: f64,
    passed: bool,
}

fn compare(
    reference: &Path,
    actual: &Path,
    output: &Path,
    tolerance: u8,
    limit: f64,
    region: Option<[u32; 4]>,
) -> Result<bool> {
    ensure!(
        limit.is_finite() && (0.0..=1.0).contains(&limit),
        "changed fraction must be 0..=1"
    );
    let reference_bytes = fs::read(reference)?;
    let actual_bytes = fs::read(actual)?;
    let reference = image::load_from_memory(&reference_bytes)?.to_rgb8();
    let actual = image::load_from_memory(&actual_bytes)?.to_rgb8();
    ensure!(
        reference.dimensions() == actual.dimensions(),
        "image dimensions differ: reference {:?}, actual {:?}; use the same capture profile",
        reference.dimensions(),
        actual.dimensions()
    );
    let (width, height) = reference.dimensions();
    let (reference, actual) = if let Some([x, y, w, h]) = region {
        ensure!(
            w > 0
                && h > 0
                && x.checked_add(w).is_some_and(|right| right <= width)
                && y.checked_add(h).is_some_and(|bottom| bottom <= height),
            "comparison region must be nonempty and inside both images"
        );
        (
            image::imageops::crop_imm(&reference, x, y, w, h).to_image(),
            image::imageops::crop_imm(&actual, x, y, w, h).to_image(),
        )
    } else {
        (reference, actual)
    };
    let mut difference = RgbImage::new(reference.width(), reference.height());
    let mut changed = 0u64;
    let mut total = 0u64;
    let mut squared = 0u64;
    let mut maximum = 0u8;
    for ((r, a), d) in reference
        .pixels()
        .zip(actual.pixels())
        .zip(difference.pixels_mut())
    {
        let errors: [u8; 3] = std::array::from_fn(|i| r[i].abs_diff(a[i]));
        changed += u64::from(errors.iter().any(|e| *e > tolerance));
        for error in errors {
            total += u64::from(error);
            squared += u64::from(error).pow(2);
            maximum = maximum.max(error);
        }
        *d = Rgb(errors.map(|v| v.saturating_mul(4)));
    }
    let pixels = u64::from(reference.width()) * u64::from(reference.height());
    let fraction = changed as f64 / pixels as f64;
    let report = Comparison {
        reference_sha256: format!("{:x}", Sha256::digest(reference_bytes)),
        actual_sha256: format!("{:x}", Sha256::digest(actual_bytes)),
        width,
        height,
        region,
        tolerance,
        changed_pixels: changed,
        changed_fraction: fraction,
        mean_absolute_error: total as f64 / (pixels * 3) as f64,
        root_mean_squared_error: (squared as f64 / (pixels * 3) as f64).sqrt(),
        max_channel_error: maximum,
        allowed_changed_fraction: limit,
        passed: fraction <= limit,
    };
    fs::create_dir_all(output)?;
    difference.save(output.join("difference.png"))?;
    let json = serde_json::to_string_pretty(&report)?;
    fs::write(output.join("comparison.json"), &json)?;
    println!("{json}");
    Ok(report.passed)
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::AnalyzeAudioGaps {
            reference,
            actual,
            output,
            reference_start_frame,
            actual_start_frame,
            frames,
        } => {
            let report = audio::gaps::analyze(
                &reference,
                &actual,
                audio::gaps::Window {
                    reference_start_frame,
                    actual_start_frame,
                    frames,
                },
            )?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
            println!(
                "Matched {} of {frames} source frames; see {} for silent insertions and continuous-match status",
                report.matched_source_frames,
                output.display()
            );
            ensure!(
                report.matched_source_frames == frames,
                "non-silent content difference remains"
            );
        }
        Command::CompareAudio {
            reference,
            actual,
            reference_baseline,
            output,
            reference_start_frame,
            actual_start_frame,
            frames,
            tolerance,
            max_changed_fraction,
        } => {
            let report = audio::compare(
                &reference,
                &actual,
                reference_baseline.as_deref(),
                &audio::Window {
                    reference_start_frame,
                    actual_start_frame,
                    frames,
                    tolerance,
                    max_changed_fraction,
                },
            )?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let text = serde_json::to_string_pretty(&report)?;
            fs::write(&output, &text)?;
            println!("{text}");
            ensure!(
                report.passed,
                "audio comparison failed; see {}",
                output.display()
            );
        }
        Command::Dtm { case, output } => {
            let case: ReplayCase = serde_json::from_slice(&fs::read(case)?)?;
            let bytes = encode_dtm(&case)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, bytes)?;
            println!(
                "Wrote {} controller polls to {}",
                case.polls,
                output.display()
            );
        }
        Command::Compare {
            reference,
            actual,
            output,
            tolerance,
            max_changed_fraction,
            region,
        } => {
            let region = if region.is_empty() {
                None
            } else {
                Some(<[u32; 4]>::try_from(region).expect("Clap requires four region values"))
            };
            ensure!(
                compare(
                    &reference,
                    &actual,
                    &output,
                    tolerance,
                    max_changed_fraction,
                    region
                )?,
                "oracle comparison failed; see {}",
                output.join("comparison.json").display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn region_comparison_detects_small_details_and_rejects_invalid_bounds() {
        let root =
            std::env::temp_dir().join(format!("resonance-image-region-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let reference = root.join("reference.png");
        let actual = root.join("actual.png");
        let output = root.join("difference");
        let mut image = RgbImage::new(20, 20);
        image.save(&reference).unwrap();
        image.put_pixel(10, 10, Rgb([20, 0, 0]));
        image.save(&actual).unwrap();
        // One wrong UI pixel is hidden by a whole-frame 1% gate.
        assert!(compare(&reference, &actual, &output, 8, 0.01, None).unwrap());
        assert!(!compare(&reference, &actual, &output, 8, 0.01, Some([10, 10, 1, 1])).unwrap());
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("comparison.json")).unwrap()).unwrap();
        assert_eq!(report["region"], serde_json::json!([10, 10, 1, 1]));
        assert_eq!(report["changed_fraction"], 1.0);
        assert_eq!(
            report["reference_sha256"],
            format!("{:x}", Sha256::digest(fs::read(&reference).unwrap()))
        );
        for region in [[20, 0, 1, 1], [0, 0, 0, 1], [u32::MAX, 0, 2, 1]] {
            assert!(compare(&reference, &actual, &output, 8, 0.01, Some(region)).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn dtm_has_correct_header_and_poll_boundaries() {
        let case = ReplayCase {
            game_id: "GQSEAF".into(),
            polls: 4,
            rtc: default_rtc(),
            from_save_state: true,
            inputs: vec![PollInput {
                poll: 1,
                duration: 2,
                buttons: vec!["start".into(), "down".into()],
                stick: Some([0, 255]),
            }],
        };
        let bytes = encode_dtm(&case).unwrap();
        assert_eq!(bytes[12], 1);
        assert_eq!(bytes.len(), 288);
        assert_eq!(&bytes[113..129], &[0; 16]); // MD5 not confused with RTC.
        assert_eq!(
            u64::from_le_bytes(bytes[129..137].try_into().unwrap()),
            case.rtc
        );
        assert_eq!(&bytes[256..258], &0x4000u16.to_le_bytes());
        assert_eq!(&bytes[264..266], &0x4081u16.to_le_bytes());
        assert_eq!(&bytes[280..282], &0x4000u16.to_le_bytes());
        assert_eq!(&bytes[260..264], &[128, 128, 128, 128]);
        assert_eq!(&bytes[268..272], &[0, 255, 128, 128]);
        assert_eq!(&bytes[276..280], &[0, 255, 128, 128]);
        assert_eq!(&bytes[284..288], &[128, 128, 128, 128]);
    }
    #[test]
    fn rejects_unknown_buttons_and_out_of_range_inputs() {
        let mut case = ReplayCase {
            game_id: "GQSEAF".into(),
            polls: 4,
            rtc: default_rtc(),
            from_save_state: false,
            inputs: vec![PollInput {
                poll: 0,
                duration: 1,
                buttons: vec!["strat".into()],
                stick: None,
            }],
        };
        assert!(encode_dtm(&case).is_err());
        case.inputs[0].buttons = vec!["a".into()];
        case.inputs[0].poll = 4;
        assert!(encode_dtm(&case).is_err());
    }
}
