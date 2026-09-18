//! Exercise the production audio worker and device callback without a sound card.
use anyhow::{Result, ensure};
use resonance_media::output::Resampler;
use resonance_playback::{Callback, Clock, Mixer, Output, SOURCE_RATE, Worker};
use std::{
    f32::consts::TAU,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn stereo_tones_reach_the_output_callback_at_device_rates() -> Result<()> {
    for rate in [44_100, 48_000] {
        let clock = Arc::new(Clock::new(rate));
        let output = Output::new(clock.clone(), 256);
        let (control, mixer) = Mixer::new(clock);
        let _handle = control.play(false, || {
            Ok(Box::new((0..).flat_map(|frame| {
                let time = frame as f32 / SOURCE_RATE as f32;
                [
                    (TAU * 440. * time).sin() * 0.25,
                    (TAU * 880. * time).sin() * 0.125,
                ]
            })))
        })?;
        let worker = Worker::start(mixer, Box::new(Resampler::new(rate)?), output.clone())?;
        let mut callback = Callback::new(output.clone(), false);
        let mut samples = Vec::with_capacity(rate as usize);
        // Half a second through the real producer/ring/callback, without timing assumptions
        // about shared CI CPUs. This checks correctness, not realtime device deadlines.
        while samples.len() < rate as usize {
            wait_ready(&output)?;
            let frames = 256.min((rate as usize - samples.len()) / 2);
            let mut buffer = vec![f32::NAN; frames * 2];
            callback.render(&mut buffer, 2, Duration::ZERO, |sample| sample);
            samples.extend(buffer);
        }
        write_capture(rate, &samples)?;
        output.check()?;
        ensure!(
            output.diagnostics().submitted_frames == u64::from(rate / 2),
            "wrong duration"
        );
        ensure!(
            samples.iter().all(|v| v.is_finite() && v.abs() < 0.3),
            "nonfinite or clipped audio"
        );
        for (channel, frequency, amplitude) in [(0, 440., 0.25), (1, 880., 0.125)] {
            // Ignore filter startup and compare tone energy, including phase-independent
            // frequency correlation so rate errors and swapped channels cannot pass.
            let signal: Vec<_> = samples
                .chunks_exact(2)
                .skip(rate as usize / 20)
                .map(|frame| f64::from(frame[channel]))
                .collect();
            let rms = (signal.iter().map(|v| v * v).sum::<f64>() / signal.len() as f64).sqrt();
            ensure!(
                (rms - amplitude / 2_f64.sqrt()).abs() < 0.005,
                "{rate} Hz channel {channel}: RMS {rms}"
            );
            let mut sine = 0.;
            let mut cosine = 0.;
            for (index, &value) in signal.iter().enumerate() {
                let phase = std::f64::consts::TAU * frequency * index as f64 / f64::from(rate);
                sine += value * phase.sin();
                cosine += value * phase.cos();
            }
            let measured = 2. * sine.hypot(cosine) / signal.len() as f64;
            ensure!(
                (measured - amplitude).abs() < 0.005,
                "{rate} Hz channel {channel}: tone amplitude {measured}"
            );
        }
        // Silent mode must consume frames while writing silence to the same callback.
        wait_ready(&output)?;
        let mut muted = Callback::new(output.clone(), true);
        let mut buffer = [f32::NAN; 512];
        muted.render(&mut buffer, 2, Duration::ZERO, |sample| sample);
        ensure!(buffer == [0.; 512], "muted callback emitted audio");
        output.check()?;
        drop(worker);
    }
    Ok(())
}

fn write_capture(rate: u32, samples: &[f32]) -> Result<()> {
    if let Some(directory) = std::env::var_os("RESONANCE_SMOKE_OUTPUT") {
        std::fs::create_dir_all(&directory)?;
        let path = std::path::Path::new(&directory).join(format!("audio-{rate}.wav"));
        let mut wav = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for &sample in samples {
            wav.write_sample(sample)?;
        }
        wav.finalize()?;
    }
    Ok(())
}

fn wait_ready(output: &Output) -> Result<()> {
    let started = Instant::now();
    while !output.ready() {
        output.check()?;
        ensure!(
            started.elapsed() < Duration::from_secs(5),
            "audio worker timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
