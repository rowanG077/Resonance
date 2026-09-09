//! Bevy lifecycle adapter. Source initialization, mixing and destruction run on
//! one worker. Bevy's audio-device plugin is never installed.
use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use resonance_playback::Decodable;
use resonance_playback::{Control, Handle as PlaybackHandle, Mixer, Output, Suspended, Worker};
use std::{
    sync::Arc,
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
    worker: Option<Worker>,
    suspended: Option<Suspended>,
    pending_device: Option<resonance_audio_device::Device>,
    retry: Instant,
    resume_game: bool,
    recovering: bool,
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
    let device = resonance_audio_device::Device::default_output()?;
    let output = device.output();
    let (control, mixer) = Mixer::new(output.clock.clone());
    let converter = resonance_media::output::Resampler::new(device.description.rate)?;
    let worker = Worker::start(
        mixer,
        resonance_audio_device::prioritize(Box::new(converter)),
        output.clone(),
    )?;
    let start = Instant::now();
    while !output.ready() {
        output.check()?;
        ensure!(
            start.elapsed() < Duration::from_secs(10),
            "audio priming timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let stream = device.start(output.clone(), silent)?;
    info!("Audio output: {:?}; silent={silent}", device.description);
    app.insert_non_send(Device {
        stream: Some(stream),
        worker: Some(worker),
        suspended: None,
        pending_device: None,
        retry: Instant::now(),
        resume_game: false,
        recovering: false,
        silent,
        control,
        output,
        report: Instant::now(),
    });
    app.add_systems(Last, (update, super::movie::pacing::update).chain());
    Ok(())
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
        let handle = control.play(paused, move || {
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
impl Device {
    fn poll(&mut self, world: &mut World) -> Result<()> {
        let lost = self.output.diagnostics().device_lost > 0;
        if lost && !self.recovering {
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
            self.resume_game = !world.resource::<Time<Virtual>>().is_paused();
            world.resource_mut::<Time<Virtual>>().pause();
            self.recovering = true;
            self.retry = Instant::now() - Duration::from_secs(1);
        }
        if self.recovering {
            if self.suspended.is_some() && self.retry.elapsed() >= Duration::from_secs(1) {
                self.retry = Instant::now();
                if let Ok(device) = resonance_audio_device::Device::default_output() {
                    let converter =
                        resonance_media::output::Resampler::new(device.description.rate)?;
                    let (worker, output) = self.suspended.take().unwrap().resume(
                        device.description.rate,
                        device.description.requested_frames.unwrap_or(2048) as usize,
                        resonance_audio_device::prioritize(Box::new(converter)),
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
                            self.recovering = false;
                            if self.resume_game {
                                world.resource_mut::<Time<Virtual>>().unpause();
                            }
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
        Ok(())
    })();
    if let Err(error) = result {
        error!("Audio playback failed: {error:#}");
        world.write_message(AppExit::error());
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
    #[test]
    #[ignore = "opens a real output device with mandatory mute; injects a device-loss notification"]
    fn muted_device_loss_reopens_output_without_restarting_sources() {
        let mut app = App::new();
        app.init_resource::<Time<Virtual>>();
        install(&mut app, true).unwrap();
        let mut device = app.world_mut().remove_non_send::<Device>().unwrap();
        let handle = device
            .control
            .play(false, || Ok(Box::new(std::iter::repeat(0.5))))
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
            while device.recovering {
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
