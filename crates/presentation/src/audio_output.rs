//! Bevy lifecycle adapter. Source initialization, mixing and destruction run on
//! one worker. Bevy's audio-device plugin is never installed.
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::prelude::*;
use resonance_playback::Decodable;
use resonance_playback::{Control, Handle as PlaybackHandle, Mixer, Output, Suspended, Worker};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Component)]
#[require(PlaybackSettings)]
pub(super) struct Player<T: Asset>(pub bevy::asset::Handle<T>);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct PlaybackSettings {
    pub paused: bool,
}
impl PlaybackSettings {
    pub const ONCE: Self = Self { paused: false };
}

#[derive(Component)]
pub(super) struct Sink(pub PlaybackHandle);
impl Sink {
    pub fn pause(&self) {
        self.0.pause();
    }
    pub fn play(&self) {
        self.0.play();
    }
    pub fn empty(&self) -> bool {
        self.0.empty()
    }
    pub fn position(&self) -> Duration {
        self.0.position()
    }
}
impl Drop for Sink {
    fn drop(&mut self) {
        self.0.stop();
    }
}
// CPAL's platform stream need not be Sync. It belongs to Bevy's main thread.
pub(super) struct Device {
    stream: Option<resonance_audio_device::Stream>,
    clocked_sink: Option<ClockedSink>,
    worker: Option<Worker>,
    suspended: Option<Suspended>,
    pending_device: Option<resonance_audio_device::Device>,
    retry: Instant,
    pub silent: bool,
    control: Control,
    output: Arc<Output>,
    report: Instant,
}
impl Drop for Device {
    fn drop(&mut self) {
        info!("Audio output final: {:?}", self.output.diagnostics());
    }
}
pub(super) fn install(app: &mut App, silent: bool) -> Result<()> {
    info!("Opening default audio output; silent={silent}");
    let device = resonance_audio_device::Device::default_output()
        .context("selecting default audio device")?;
    info!("Selected audio device: {:?}", device.description);
    let output = device.output();
    let (control, mixer) = Mixer::new(output.clock.clone());
    let converter = resonance_media::output::Resampler::new(device.description.rate)?;
    let worker = Worker::start(mixer, Box::new(converter), output.clone())?;
    let start = Instant::now();
    while !output.ready() {
        output.check()?;
        ensure!(
            start.elapsed() < Duration::from_secs(10),
            "audio priming timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let stream = device
        .start(output.clone(), silent)
        .with_context(|| format!("starting audio stream {:?}", device.description))?;
    info!("Audio output: {:?}; silent={silent}", device.description);
    app.insert_non_send(Device {
        stream: Some(stream),
        clocked_sink: None,
        worker: Some(worker),
        suspended: None,
        pending_device: None,
        retry: Instant::now(),
        silent,
        control,
        output,
        report: Instant::now(),
    });
    app.add_systems(Last, (update, super::movie::pacing::update).chain());
    Ok(())
}

/// Consume the real mixer output at wall-clock cadence without opening a host
/// device. Voice/service completion still follows the playback clock.
pub(super) fn install_offscreen(app: &mut App) -> Result<()> {
    const RATE: u32 = 48_000;
    const PERIOD: usize = 512;
    let output = Output::new(Arc::new(resonance_playback::Clock::new(RATE)), PERIOD);
    let (control, mixer) = Mixer::new(output.clock.clone());
    let converter = resonance_media::output::Resampler::new(RATE)?;
    let worker = Worker::start(mixer, Box::new(converter), output.clone())?;
    let start = Instant::now();
    while !output.ready() {
        output.check()?;
        ensure!(
            start.elapsed() < Duration::from_secs(10),
            "offscreen audio priming timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let clocked_sink = ClockedSink::start(output.clone(), RATE, PERIOD)?;
    app.insert_non_send(Device {
        stream: None,
        clocked_sink: Some(clocked_sink),
        worker: Some(worker),
        suspended: None,
        pending_device: None,
        retry: Instant::now(),
        silent: true,
        control,
        output,
        report: Instant::now(),
    });
    app.add_systems(Last, (update, super::movie::pacing::update).chain());
    info!("Offscreen audio uses a clocked muted sink at {RATE} Hz; no host device");
    Ok(())
}

struct ClockedSink {
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl ClockedSink {
    fn start(output: Arc<Output>, rate: u32, period: usize) -> Result<Self> {
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = std::thread::Builder::new()
            .name("resonance-muted-clock".into())
            .spawn(move || {
                let mut callback = resonance_playback::Callback::new(output, true);
                let mut samples = vec![0_f32; period * 2];
                let start = Instant::now();
                let mut frames = 0_u64;
                while !stop.load(Ordering::Acquire) {
                    let deadline = start + Duration::from_secs_f64(frames as f64 / f64::from(rate));
                    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    callback.render(&mut samples, 2, Duration::ZERO, |v| v);
                    frames += period as u64;
                }
            })?;
        Ok(Self {
            stopped,
            thread: Some(thread),
        })
    }
}
impl Drop for ClockedSink {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub(super) fn attach<T: Asset + Decodable + Clone>(
    world: &mut World,
    control: &Control,
) -> Result<()> {
    let pending: Vec<_> = world
        .query_filtered::<(Entity, &Player<T>, &PlaybackSettings), Without<Sink>>()
        .iter(world)
        .map(|(entity, player, settings)| (entity, player.0.clone(), settings.paused))
        .collect();
    for (entity, asset, paused) in pending {
        let asset = world
            .resource::<Assets<T>>()
            .get(&asset)
            .context("audio source asset missing")?
            .clone();
        let transport = world
            .get_resource::<super::testing::Controls>()
            .map_or(Some(resonance_playback::Speed::Normal), |controls| {
                (!controls.paused).then_some(controls.speed)
            });
        let handle =
            control.schedule_with_transport(control.rendered(), paused, transport, move || {
                use resonance_playback::Source;
                let source = asset.decoder();
                ensure!(
                    source.sample_rate().get() == resonance_playback::SOURCE_RATE
                        && source.channels().get() == 2,
                    "source must be cooked to the native stereo mix rate"
                );
                Ok(Box::new(source))
            })?;
        world.entity_mut(entity).insert(Sink(handle));
    }
    Ok(())
}
/// Keeps simulation time stopped while a replacement device is being primed.
#[derive(Resource)]
pub(super) struct DevicePause {
    pub resume: bool,
}

impl DevicePause {
    pub(super) fn begin(world: &mut World) {
        let mut time = world.resource_mut::<Time<Virtual>>();
        let resume = !time.is_paused();
        time.pause();
        world.insert_resource(Self { resume });
    }

    pub(super) fn finish(world: &mut World) {
        if world
            .remove_resource::<Self>()
            .is_some_and(|pause| pause.resume)
        {
            world.resource_mut::<Time<Virtual>>().unpause();
        }
    }
}

impl Device {
    fn poll(&mut self, world: &mut World) -> Result<()> {
        if self.clocked_sink.is_some() {
            return self.output.check();
        }
        let lost = self.output.diagnostics().device_lost > 0;
        if lost && !world.contains_resource::<DevicePause>() {
            warn!(
                "Audio device lost; freezing playback and preparing to reconnect: {:?}",
                self.output.diagnostics()
            );
            self.output.clock.freeze();
            self.stream.take();
            self.suspended = Some(
                self.worker
                    .take()
                    .context("audio worker missing")?
                    .suspend()?,
            );
            DevicePause::begin(world);
            self.retry = Instant::now() - Duration::from_secs(1);
        }
        if world.contains_resource::<DevicePause>() {
            if self.suspended.is_some() && self.retry.elapsed() >= Duration::from_secs(1) {
                self.retry = Instant::now();
                if let Ok(device) = resonance_audio_device::Device::default_output() {
                    let converter =
                        resonance_media::output::Resampler::new(device.description.rate)?;
                    let (worker, output) = self.suspended.take().unwrap().resume(
                        device.description.rate,
                        device.description.requested_frames.unwrap_or(2048) as usize,
                        Box::new(converter),
                    )?;
                    self.worker = Some(worker);
                    self.output = output;
                    self.pending_device = Some(device);
                }
            }
            if self.pending_device.is_some() {
                self.output.check()?;
                ensure!(
                    self.retry.elapsed() < Duration::from_secs(10),
                    "replacement audio device priming timed out"
                );
                if self.output.ready() {
                    let device = self.pending_device.take().unwrap();
                    match device.start(self.output.clone(), self.silent) {
                        Ok(stream) => {
                            self.stream = Some(stream);
                            DevicePause::finish(world);
                            info!("Audio device restored: {:?}", device.description);
                        }
                        Err(error) => {
                            warn!("Audio replacement disappeared: {error:#}");
                            self.suspended = Some(self.worker.take().unwrap().suspend()?);
                        }
                    }
                }
            }
        } else {
            self.output.check()?;
        }
        Ok(())
    }
}
fn update(world: &mut World) {
    let mut device = world
        .remove_non_send::<Device>()
        .expect("audio output missing");
    let result = (|| -> Result<()> {
        device.poll(world)?;
        attach::<super::audio::GameAudio>(world, &device.control)?;
        attach::<super::field_audio::FieldSource>(world, &device.control)?;
        attach::<super::movie::MovieAudio>(world, &device.control)?;
        attach::<super::credits::Audio>(world, &device.control)?;
        super::testing::audio(world);
        Ok(())
    })();
    if let Err(error) = result {
        let error = error.context(format!(
            "recovering={}; pending device={:?}; output={:?}",
            world.contains_resource::<DevicePause>(),
            device
                .pending_device
                .as_ref()
                .map(|device| &device.description),
            device.output.diagnostics()
        ));
        super::diagnostics::fatal(world, "audio playback", error);
    }
    if device.report.elapsed() >= Duration::from_secs(30) {
        info!("Audio output: {:?}", device.output.diagnostics());
        device.report = Instant::now();
    }
    world.insert_non_send(device);
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Constant;
    impl Iterator for Constant {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            Some(0.5)
        }
    }
    impl resonance_playback::Source for Constant {
        fn channels(&self) -> resonance_playback::ChannelCount {
            resonance_playback::ChannelCount::new(2).unwrap()
        }
        fn sample_rate(&self) -> resonance_playback::SampleRate {
            resonance_playback::SampleRate::new(resonance_playback::SOURCE_RATE).unwrap()
        }
    }

    #[test]
    fn clocked_muted_output_advances_sources_without_a_host_device() {
        let mut app = App::new();
        install_offscreen(&mut app).unwrap();
        let device = app.world().non_send::<Device>();
        assert!(device.stream.is_none() && device.pending_device.is_none());
        assert!(device.clocked_sink.is_some());
        let handle = device
            .control
            .play(false, || Ok(Box::new(Constant)))
            .unwrap();
        let started = Instant::now();
        while handle.position() < Duration::from_millis(40) {
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "muted playback clock stalled"
            );
            device.output.check().unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(device.output.diagnostics().submitted_frames > 0);
        assert_eq!(device.output.diagnostics().device_errors, 0);
    }
    #[test]
    #[ignore = "opens a real output device with mandatory mute; injects a device-loss notification"]
    fn muted_device_loss_reopens_output_without_restarting_sources() {
        let mut app = App::new();
        app.init_resource::<Time<Virtual>>();
        install(&mut app, true).unwrap();
        let mut device = app.world_mut().remove_non_send::<Device>().unwrap();
        let handle = device
            .control
            .play(false, || Ok(Box::new(Constant)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        device.output.check().unwrap();
        let before = handle.position();
        assert!(before > Duration::ZERO);
        for already_paused in [false, true] {
            if already_paused {
                app.world_mut().resource_mut::<Time<Virtual>>().pause();
            }
            device.output.device_error(false, true);
            let began = Instant::now();
            device.poll(app.world_mut()).unwrap();
            while app.world().contains_resource::<DevicePause>() {
                assert!(
                    began.elapsed() < Duration::from_secs(10),
                    "device recovery timed out"
                );
                std::thread::sleep(Duration::from_millis(2));
                device.poll(app.world_mut()).unwrap();
            }
            assert!(device.stream.as_ref().unwrap().silent);
            assert_eq!(
                app.world().resource::<Time<Virtual>>().is_paused(),
                already_paused
            );
            std::thread::sleep(Duration::from_millis(300));
            device.output.check().unwrap();
            assert_eq!(handle.epoch, 1);
            assert!(handle.position() > before);
        }
        eprintln!(
            "Muted device recovery passed: {:?}",
            device.output.diagnostics()
        );
    }
}
