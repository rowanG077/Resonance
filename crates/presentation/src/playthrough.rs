//! Render and record the real movie/title systems on a manually consumed mixer.
//! No output stream, window or audio-device plugin is created.
use super::*;
use resonance_game::clock::{
    UPDATE_HZ, UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR, UPDATE_STEP,
};
use resonance_playback::Decodable;
use std::{
    collections::BTreeSet,
    io::{BufWriter, Write},
    sync::atomic::AtomicU32,
    thread,
};

#[derive(Resource, Default)]
pub(super) struct Recording {
    pub started: bool,
    pub(super) output: PathBuf,
    pub(super) step: u64,
    pub(super) audio_frames: u64,
    requested: BTreeSet<String>,
    completed: Arc<AtomicU32>,
    failed: Arc<AtomicBool>,
}

const MOVIE_FRAMES: &[u32] = &[0, 867, 1867, 2867, 3629];
const BOOT_TICKS: &[u32] = &[0, 120, 244, 420, 548, 700, 792, 900, 948, 972, 975];
// Adjacent checkpoints exercise the DISC1 edge in the native opening path.
// Its absolute phase may differ after intentionally omitted startup waits.
const TITLE_FRAMES: &[u32] = &[0, 1, 8, 30, 117, 240, 843, 950, 964, 994, 995];
const RATE: u32 = 32028;

pub(super) fn record(mut app: App, output: PathBuf) -> Result<()> {
    use bevy::app::PluginsState;
    use bevy::time::TimeUpdateStrategy;
    use sha2::{Digest, Sha256};
    let ticks = app.world().resource::<RunOptions>().record_title_ticks;
    anyhow::ensure!(
        (1..=7200).contains(&ticks),
        "record-title-ticks must be 1..7200"
    );
    anyhow::ensure!(!output.exists(), "recording directory already exists");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&output)?;
    app.world_mut().resource_mut::<Recording>().output = output.clone();
    let began = Instant::now();
    while app.plugins_state() == PluginsState::Adding {
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(60),
            "recording plugin setup timed out"
        );
        bevy::tasks::tick_global_task_pools_on_main_thread();
        thread::sleep(Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    audio::validate_startup(&app, true, true)?;
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let mut ready_frames = 0;
    while ready_frames < 30 {
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(60),
            "recording renderer preparation timed out"
        );
        app.update();
        check_exit(&app)?;
        let ready = app.world().resource::<timing::Ready>().0;
        ready_frames = if ready { ready_frames + 1 } else { 0 };
        thread::sleep(UPDATE_STEP);
    }
    app.world_mut().resource_mut::<Recording>().started = true;
    // Present the initialized scene and start its audio before advancing its
    // first gameplay tick, as when assets become ready in the live Update.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let (mixer, mut samples) = resonance_playback::Offline::new();
    let partial = output.join("audio.partial.wav");
    let mut wave = hound::WavWriter::create(
        &partial,
        hound::WavSpec {
            channels: 2,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut timeline = BufWriter::new(fs::File::create(output.join("timeline.jsonl"))?);
    let started = Instant::now();
    let mut step = 0u64;
    let mut frames = 0u64;
    while app.world().resource::<Menu>().0.tick < ticks {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(300),
            "playthrough timed out"
        );
        step += 1;
        app.world_mut().resource_mut::<Recording>().step = step;
        app.update();
        if step == 1 {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(UPDATE_STEP));
        }
        check_exit(&app)?;
        attach::<movie::MovieAudio>(app.world_mut(), &mixer)?;
        attach::<GameAudio>(app.world_mut(), &mixer)?;
        serde_json::to_writer(&mut timeline, &observation(app.world()))?;
        timeline.write_all(b"\n")?;
        let end = step * u64::from(RATE) * UPDATE_RATE_DENOMINATOR / UPDATE_RATE_NUMERATOR;
        for _ in frames..end {
            for _ in 0..2 {
                let sample = samples.next().unwrap_or(0.);
                anyhow::ensure!(sample.is_finite(), "nonfinite player audio");
                wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
            }
        }
        frames = end;
        app.world_mut().resource_mut::<Recording>().audio_frames = frames;
        thread::sleep(
            Duration::from_secs_f64(step as f64 / UPDATE_HZ).saturating_sub(started.elapsed()),
        );
    }
    timeline.flush()?;
    // Keep the final scene and audio clock fixed while pending GPU readbacks finish.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let drain = Instant::now();
    loop {
        let record = app.world().resource::<Recording>();
        anyhow::ensure!(
            !record.failed.load(Ordering::Acquire),
            "playthrough image write failed"
        );
        if record.completed.load(Ordering::Acquire) as usize == record.requested.len() {
            break;
        }
        anyhow::ensure!(
            drain.elapsed() < Duration::from_secs(10),
            "playthrough readback timed out"
        );
        app.update();
        check_exit(&app)?;
        thread::sleep(Duration::from_millis(10));
    }
    wave.finalize()?;
    fs::rename(partial, output.join("audio.wav"))?;
    let record = app.world().resource::<Recording>();
    anyhow::ensure!(
        !record.failed.load(Ordering::Acquire),
        "playthrough image write failed"
    );
    let options = app.world().resource::<RunOptions>();
    let assets = &options.assets;
    if app.world().resource::<boot::Playback>().asset.is_some() {
        for tick in BOOT_TICKS {
            anyhow::ensure!(
                record.requested.contains(&format!("boot-{tick:06}")),
                "missed startup checkpoint {tick}"
            );
        }
    }
    for &tick in TITLE_FRAMES.iter().filter(|t| **t <= ticks) {
        anyhow::ensure!(
            record.requested.contains(&format!("title-{tick:06}")),
            "missed title checkpoint {tick}"
        );
    }
    if let Some(movie) = &app.world().resource::<movie::Playback>().asset {
        for &frame in MOVIE_FRAMES.iter().filter(|f| **f < movie.frames) {
            anyhow::ensure!(
                record.requested.contains(&format!("movie-{frame:06}")),
                "missed movie checkpoint {frame}"
            );
        }
    }
    let replay = options.replay.as_ref().map(|path| -> Result<serde_json::Value> {
        Ok(serde_json::json!({"path":path,"sha256":format!("{:x}",Sha256::digest(fs::read(path)?))}))
    }).transpose()?;
    let mut inputs = serde_json::Map::new();
    for name in [
        "title.json",
        "boot.json",
        "intro.json",
        "title-audio.json",
        "title-sounds.json",
    ] {
        let path = assets.join(name);
        if path.is_file() {
            inputs.insert(
                name.into(),
                serde_json::json!(format!("{:x}", Sha256::digest(fs::read(path)?))),
            );
        }
    }
    fs::write(
        output.join("recording.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version":1, "complete":true, "headless":true, "audio_device":false,
            "source":"resonance_offline_mixer", "fixed_update_hz":UPDATE_HZ,
            "update_rate_ratio":[UPDATE_RATE_NUMERATOR,UPDATE_RATE_DENOMINATOR], "sample_rate":RATE,
            "steps":step, "audio_frames":frames, "title_ticks":ticks,
            "movie_frames":app.world().resource::<movie::Playback>().asset.as_ref().map(|m| m.frames),
            "boot_ticks":app.world().resource::<boot::Playback>().logos.as_ref().map(|l| l.tick),
            "checkpoints":record.requested, "inputs":inputs,
            "presentation_start":options.presentation_start, "replay":replay,
            "audio_sha256":format!("{:x}", Sha256::digest(fs::read(output.join("audio.wav"))?)),
        }))?,
    )?;
    println!(
        "Recorded {step} rendered updates, {frames} stereo frames and {} checkpoints without a device",
        record.requested.len()
    );
    Ok(())
}

pub(super) fn check_exit(app: &App) -> Result<()> {
    anyhow::ensure!(
        app.should_exit().is_none(),
        "playthrough application exited before completion"
    );
    Ok(())
}

pub(super) fn attach<T: Asset + Decodable + Clone>(
    world: &mut World,
    mixer: &resonance_playback::Control,
) -> Result<()> {
    super::audio_output::attach::<T>(world, mixer)
}

fn observation(world: &World) -> serde_json::Value {
    let movie = world.resource::<movie::Playback>();
    let boot = world.resource::<boot::Playback>();
    let recording = world.resource::<Recording>();
    serde_json::json!({
        "step":recording.step, "audio_frame":recording.audio_frames,
        "presentation_counter":world.resource::<Clock>().0.tick(),
        "boot_active":boot.active(), "boot":boot.logos,
        "movie_active":movie.active && !boot.active(), "movie_frame":movie.presented_frame,
        "movie_seconds":movie.position(world).map(|p| p.as_secs_f64()),
        "title_tick":world.resource::<Menu>().0.tick,
        "selected":world.resource::<Menu>().0.selected,
        "title_audio_frames":world.resource::<audio::MenuSounds>().control.as_ref().map(|s| s.rendered_frames()),
    })
}

#[allow(clippy::too_many_arguments)] // Exact scene state accompanies asynchronous GPU readback.
pub(super) fn capture(
    mut commands: Commands,
    recording: Option<ResMut<Recording>>,
    movie: Res<movie::Playback>,
    boot: Res<boot::Playback>,
    menu: Res<Menu>,
    clock: Res<Clock>,
    framebuffer: Res<Framebuffer>,
) {
    let Some(mut record) = recording else {
        return;
    };
    if !record.started {
        return;
    }
    let key = if boot.active() {
        boot.logos
            .as_ref()
            .filter(|l| BOOT_TICKS.contains(&l.tick))
            .map(|l| format!("boot-{:06}", l.tick))
    } else if movie.active {
        movie
            .presented_frame
            .filter(|f| MOVIE_FRAMES.contains(f))
            .map(|f| format!("movie-{f:06}"))
    } else {
        TITLE_FRAMES
            .contains(&menu.0.tick)
            .then(|| format!("title-{:06}", menu.0.tick))
    };
    let Some(key) = key else {
        return;
    };
    if !record.requested.insert(key.clone()) {
        return;
    }
    let path = record.output.join(format!("{key}.png"));
    let metadata = serde_json::json!({
        "recording_step":record.step, "audio_frame":record.audio_frames,
        "presentation_counter":clock.0.tick(), "movie_frame":movie.presented_frame,
        "boot":boot.logos, "boot_active":boot.active(),
        "movie_active":movie.active && !boot.active(), "title":menu.0, "headless":true, "audio_device":false,
    });
    let completed = record.completed.clone();
    let failed = record.failed.clone();
    commands.spawn(Screenshot(framebuffer.0.clone())).observe(
        move |event: On<ScreenshotCaptured>| {
            let result = (|| -> Result<()> {
                event.image.clone().try_into_dynamic()?.save(&path)?;
                fs::write(
                    path.with_extension("json"),
                    serde_json::to_vec_pretty(&metadata)?,
                )?;
                Ok(())
            })();
            if let Err(error) = result {
                error!("playthrough checkpoint failed: {error:#}");
                failed.store(true, Ordering::Release);
            }
            completed.fetch_add(1, Ordering::Release);
        },
    );
}
