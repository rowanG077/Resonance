use crate::{Clock, NativeFrame, Speed, clock::duration};
use anyhow::{Context, Result, ensure};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

type Source = Box<dyn crate::Source>;
type Factory = Box<dyn FnOnce() -> Result<Source> + Send>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Priming,
    Playing,
    Paused,
    Draining,
    Stopped,
    Ended,
    Failed,
}
#[derive(Clone, Copy)]
struct Span {
    start: u64,
    end: u64,
    // Source positions and rate use tenths of a source frame. Device positions
    // remain whole mix frames, including across pauses and speed changes.
    source: u64,
    source_end: u64,
    rate: u64,
}
struct Shared {
    failure: Arc<AtomicBool>,
    clock: Arc<Clock>,
    paused: AtomicBool,
    rate: AtomicU64,
    suspended: AtomicBool,
    stopped: AtomicBool,
    ended: AtomicBool,
    rendered_units: AtomicU64,
    timeline: Mutex<VecDeque<Span>>,
}
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
    pub epoch: u64,
}
impl Handle {
    /// Change playback rate without changing the source's own pause state.
    pub fn set_transport(&self, paused: bool, speed: Speed) {
        self.shared.suspended.store(paused, Ordering::Release);
        self.shared.rate.store(speed.tenths(), Ordering::Release);
    }
    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Release);
    }
    pub fn play(&self) {
        self.shared.paused.store(false, Ordering::Release);
    }
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Release);
    }
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Acquire) || self.shared.suspended.load(Ordering::Acquire)
    }
    pub fn position(&self) -> Duration {
        duration(self.audible_frames())
    }
    pub fn rendered_frames(&self) -> u64 {
        self.shared.rendered_units.load(Ordering::Acquire) / 10
    }
    pub fn audible_frames(&self) -> u64 {
        let audible = self.shared.clock.audible().0;
        let spans = self
            .shared
            .timeline
            .lock()
            .expect("audio timeline poisoned");
        let Some(span) = spans.iter().rev().find(|s| s.start <= audible) else {
            return 0;
        };
        (span.source + audible.min(span.end).saturating_sub(span.start) * span.rate)
            .min(span.source_end)
            / 10
    }
    pub fn empty(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire)
            || (self.shared.ended.load(Ordering::Acquire)
                && self.audible_frames() >= self.rendered_frames())
    }
    pub fn state(&self) -> State {
        if self.shared.failure.load(Ordering::Acquire) {
            return State::Failed;
        }
        if self.shared.stopped.load(Ordering::Acquire) {
            State::Stopped
        } else if self.empty() {
            State::Ended
        } else if self.is_paused() {
            State::Paused
        } else if self.shared.ended.load(Ordering::Acquire) {
            State::Draining
        } else if self.rendered_frames() == 0 {
            State::Priming
        } else {
            State::Playing
        }
    }
}
struct Pending {
    factory: Factory,
    handle: Handle,
    at: NativeFrame,
}
struct Playing {
    source: Source,
    handle: Handle,
    at: NativeFrame,
    held: Option<[f32; 2]>,
    phase: u64,
}

impl Playing {
    fn read(&mut self) -> Result<Option<[f32; 2]>> {
        let Some(left) = self.source.next() else {
            if !self.source.is_pending() {
                self.handle.shared.ended.store(true, Ordering::Release);
            }
            return Ok(None);
        };
        let right = self
            .source
            .next()
            .context("incomplete stereo source frame")?;
        ensure!(
            left.is_finite() && right.is_finite(),
            "nonfinite source PCM"
        );
        Ok(Some([left, right]))
    }

    fn sample(&mut self, rate: u64) -> Result<([f32; 2], u64)> {
        let mut samples = [0.; 2];
        let mut read = 0;
        // Preserve the exact ordinary and double-speed mixing paths.
        if self.phase == 0 && rate >= 10 {
            for _ in 0..rate / 10 {
                let Some(next) = self.read()? else { break };
                samples[0] += next[0];
                samples[1] += next[1];
                read += 1;
            }
            if read > 0 {
                samples = samples.map(|v| v / read as f32);
            }
            return Ok((samples, read * 10));
        }
        // Hold a source sample across output frames at slow rates. Integrate
        // partial samples when changing speed, keeping both channels and the
        // source clock continuous across arbitrary output block boundaries.
        while read < rate {
            if self.held.is_none() {
                self.held = self.read()?;
            }
            let Some(next) = self.held else { break };
            let amount = (10 - self.phase).min(rate - read);
            samples[0] += next[0] * amount as f32;
            samples[1] += next[1] * amount as f32;
            read += amount;
            self.phase += amount;
            if self.phase == 10 {
                self.phase = 0;
                self.held = None;
            }
        }
        if read > 0 {
            samples = samples.map(|v| v / read as f32);
        }
        Ok((samples, read))
    }
}
#[derive(Clone)]
pub struct Control {
    send: SyncSender<Pending>,
    clock: Arc<Clock>,
    rendered: Arc<AtomicU64>,
    next_epoch: Arc<AtomicU64>,
    failure: Arc<AtomicBool>,
}
impl Control {
    /// Schedule at an absolute native mix frame; late requests use the earliest
    /// unwritten frame. An epoch owns all pause, stop and completion state.
    pub fn schedule(
        &self,
        at: NativeFrame,
        paused: bool,
        factory: impl FnOnce() -> Result<Source> + Send + 'static,
    ) -> Result<Handle> {
        ensure!(
            !self.failure.load(Ordering::Acquire),
            "audio mixer has failed"
        );
        let handle = Handle {
            epoch: self.next_epoch.fetch_add(1, Ordering::Relaxed),
            shared: Arc::new(Shared {
                failure: self.failure.clone(),
                clock: self.clock.clone(),
                paused: AtomicBool::new(paused),
                rate: AtomicU64::new(Speed::Normal.tenths()),
                suspended: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
                ended: AtomicBool::new(false),
                rendered_units: AtomicU64::new(0),
                timeline: Mutex::new(VecDeque::with_capacity(512)),
            }),
        };
        self.send
            .try_send(Pending {
                factory: Box::new(factory),
                handle: handle.clone(),
                at,
            })
            .map_err(|_| anyhow::anyhow!("audio source queue is full or stopped"))?;
        Ok(handle)
    }
    pub fn play(
        &self,
        paused: bool,
        factory: impl FnOnce() -> Result<Source> + Send + 'static,
    ) -> Result<Handle> {
        self.schedule(self.rendered(), paused, factory)
    }
    pub fn rendered(&self) -> NativeFrame {
        NativeFrame(self.rendered.load(Ordering::Acquire))
    }
}
/// The same mixer is driven by a live worker or explicitly by the recorder.
pub struct Mixer {
    receive: Receiver<Pending>,
    playing: Vec<Playing>,
    control: Control,
    pub(crate) frame: u64,
}
impl Mixer {
    pub(crate) fn fail(&self) {
        self.control.failure.store(true, Ordering::Release);
    }
    pub(crate) fn clock(&self) -> Arc<Clock> {
        self.control.clock.clone()
    }
    pub fn new(clock: Arc<Clock>) -> (Control, Self) {
        let (send, receive) = mpsc::sync_channel(128);
        let control = Control {
            send,
            clock,
            rendered: Arc::new(AtomicU64::new(0)),
            next_epoch: Arc::new(AtomicU64::new(1)),
            failure: Arc::new(AtomicBool::new(false)),
        };
        (
            control.clone(),
            Self {
                receive,
                playing: Vec::with_capacity(16),
                control,
                frame: 0,
            },
        )
    }
    pub fn render(&mut self, output: &mut [[f32; 2]]) -> Result<()> {
        for pending in self.receive.try_iter() {
            if pending.handle.shared.stopped.load(Ordering::Acquire) {
                continue;
            }
            ensure!(self.playing.len() < 128, "audio source limit exceeded");
            self.playing.push(Playing {
                source: (pending.factory)()?,
                handle: pending.handle,
                at: pending.at,
                held: None,
                phase: 0,
            });
        }
        output.fill([0.; 2]);
        for playing in &mut self.playing {
            let state = playing.handle.shared.clone();
            if state.stopped.load(Ordering::Acquire)
                || state.paused.load(Ordering::Acquire)
                || state.suspended.load(Ordering::Acquire)
                || state.ended.load(Ordering::Acquire)
            {
                continue;
            }
            let start = playing
                .at
                .0
                .saturating_sub(self.frame)
                .min(output.len() as u64) as usize;
            let rate = state.rate.load(Ordering::Acquire);
            let mut count = 0;
            let mut consumed = 0;
            for frame in &mut output[start..] {
                let (samples, read) = playing.sample(rate)?;
                if read == 0 {
                    break;
                }
                frame[0] += samples[0];
                frame[1] += samples[1];
                count += 1;
                consumed += read;
            }
            if count > 0 {
                let source = state.rendered_units.load(Ordering::Relaxed);
                let span = Span {
                    start: self.frame + start as u64,
                    end: self.frame + start as u64 + count,
                    source,
                    source_end: source + consumed,
                    rate,
                };
                let mut spans = state.timeline.lock().expect("audio timeline poisoned");
                if let Some(previous) = spans.back_mut()
                    && previous.end == span.start
                    && previous.rate == rate
                    && previous.source_end == source
                    && previous.source_end
                        == previous.source + (previous.end - previous.start) * rate
                {
                    previous.end = span.end;
                    previous.source_end = span.source_end;
                } else {
                    if spans.len() == 512 {
                        spans.pop_front();
                    }
                    spans.push_back(span);
                }
                state
                    .rendered_units
                    .store(source + consumed, Ordering::Release);
            }
        }
        // All source destruction happens here, never in the device callback.
        self.playing.retain(|p| {
            !p.handle.shared.stopped.load(Ordering::Acquire)
                && !p.handle.shared.ended.load(Ordering::Acquire)
        });
        self.frame += output.len() as u64;
        self.control.rendered.store(self.frame, Ordering::Release);
        Ok(())
    }
}
pub struct Offline {
    mixer: Mixer,
    clock: Arc<Clock>,
    channel: usize,
    frame: [[f32; 2]; 1],
}
impl Offline {
    pub fn new() -> (Control, Self) {
        let clock = Arc::new(Clock::new(crate::SOURCE_RATE));
        clock.manual(NativeFrame(0));
        let (control, mixer) = Mixer::new(clock.clone());
        (
            control,
            Self {
                mixer,
                clock,
                channel: 0,
                frame: [[0.; 2]],
            },
        )
    }
}
impl Iterator for Offline {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 {
            self.mixer
                .render(&mut self.frame)
                .expect("offline audio render failed");
        }
        let value = self.frame[0][self.channel];
        self.channel ^= 1;
        if self.channel == 0 {
            self.clock.manual(NativeFrame(self.mixer.frame));
        }
        Some(value)
    }
}
