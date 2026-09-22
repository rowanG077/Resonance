//! Real device stress test, unconditionally muted. Never writes to speakers.
use anyhow::{Context, Result, ensure};
use resonance_media::{MovieDecoder, MovieStream};
use resonance_playback::{Mixer, Worker};
use std::{
    collections::VecDeque,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().unwrap_or_else(|| "local/all-assets".into()));
    let seconds: f64 = args.next().unwrap_or_else(|| "640".into()).parse()?;
    let stall: u64 = args.next().unwrap_or_else(|| "200".into()).parse()?;
    let period: u32 = args.next().unwrap_or_else(|| "512".into()).parse()?;
    let device = resonance_audio_device::Device::with_period(period)?;
    println!("Device: {:?}; forced_silent=true", device.description);
    let output = device.output();
    let (control, mixer) = Mixer::new(output.clock.clone());
    let worker = Worker::start(
        mixer,
        resonance_audio_device::prioritize(Box::new(resonance_media::output::Resampler::new(
            device.description.rate,
        )?)),
        output.clone(),
    )?;
    let ready = Instant::now();
    while !output.ready() {
        output.check()?;
        ensure!(
            ready.elapsed() < Duration::from_secs(10),
            "device priming timeout"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let _device_stream = device.start(output.clone(), true)?;
    let began = Instant::now();
    let mut rounds = 0;
    while began.elapsed().as_secs_f64() < seconds {
        let id = rounds % 2;
        let manifest = root.join(resonance_content::movie::metadata_path(id));
        let asset: resonance_content::MovieAsset = serde_json::from_slice(
            &std::fs::read(&manifest).with_context(|| format!("reading {}", manifest.display()))?,
        )?;
        let stream = MovieStream::start(
            MovieDecoder::open(&root.join(&asset.path), asset.clone())?,
            VecDeque::new(),
            asset.sample_rate,
        )?;
        let pcm = stream.audio();
        let started = Instant::now();
        while pcm.buffered() < u64::from(asset.sample_rate) / 2 {
            stream.check()?;
            ensure!(
                started.elapsed() < Duration::from_secs(30),
                "movie priming timeout"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let source = pcm.clone();
        let handle = control.play(false, move || Ok(Box::new(source.source(false))))?;
        let mut frames = VecDeque::new();
        let mut selected = 0;
        let mut skipped = 0;
        let mut previous = None;
        let mut updates = 0u64;
        while !handle.empty() && began.elapsed().as_secs_f64() < seconds {
            output.check()?;
            stream.check()?;
            while let Some(frame) = stream.try_video() {
                frames.push_back(frame);
            }
            let position = handle.position();
            if let Some(previous) = previous {
                ensure!(position >= previous, "audible clock regressed");
            }
            previous = Some(position);
            let mut latest = None;
            while frames.front().is_some_and(|f| f.timestamp <= position) {
                if latest.is_some() {
                    skipped += 1;
                }
                latest = frames.pop_front();
            }
            if latest.is_some() {
                selected += 1;
            }
            updates += 1;
            // Simulate render-thread hitches while decoder, mixer and device run.
            let wait = if stall > 0 && updates.is_multiple_of(30) {
                Duration::from_millis(stall)
            } else {
                Duration::from_micros(33_366)
            };
            thread::sleep(wait);
        }
        println!(
            "Movie {id}: complete={} selected={selected} skipped_due_to_stalls={skipped} queue_drops={} decoded_underruns={} position={:?}",
            handle.empty(),
            stream.dropped_frames(),
            pcm.underruns(),
            handle.position()
        );
        handle.stop();
        drop(stream);
        rounds += 1;
        println!("Output: {:?}", output.diagnostics());
    }
    output.check()?;
    println!(
        "PASS: seconds={:.3}, rounds={rounds}, {:?}",
        began.elapsed().as_secs_f64(),
        output.diagnostics()
    );
    drop(worker);
    Ok(())
}
