use crate::{Clock, NativeFrame, clock::duration};
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

type Source = Box<dyn Iterator<Item = f32> + Send>;
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
    source: u64,
}
struct Shared {
    failure: Arc<AtomicBool>,
    clock: Arc<Clock>,
    paused: AtomicBool,
    stopped: AtomicBool,
    ended: AtomicBool,
    rendered: AtomicU64,
    timeline: Mutex<VecDeque<Span>>,
}
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
    pub epoch: u64,
}
impl Handle {
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
        self.shared.paused.load(Ordering::Acquire)
    }
    pub fn position(&self) -> Duration {
        duration(self.audible_frames())
    }
    pub fn rendered_frames(&self) -> u64 {
        self.shared.rendered.load(Ordering::Acquire)
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
        span.source + audible.min(span.end).saturating_sub(span.start)
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
                stopped: AtomicBool::new(false),
                ended: AtomicBool::new(false),
                rendered: AtomicU64::new(0),
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
            });
        }
        output.fill([0.; 2]);
        for playing in &mut self.playing {
            let state = &playing.handle.shared;
            if state.stopped.load(Ordering::Acquire)
                || state.paused.load(Ordering::Acquire)
                || state.ended.load(Ordering::Acquire)
            {
                continue;
            }
            let start = playing
                .at
                .0
                .saturating_sub(self.frame)
                .min(output.len() as u64) as usize;
            let mut count = 0;
            for frame in &mut output[start..] {
                let Some(left) = playing.source.next() else {
                    state.ended.store(true, Ordering::Release);
                    break;
                };
                let right = playing
                    .source
                    .next()
                    .context("incomplete stereo source frame")?;
                ensure!(
                    left.is_finite() && right.is_finite(),
                    "nonfinite source PCM"
                );
                frame[0] += left;
                frame[1] += right;
                count += 1;
            }
            if count > 0 {
                let source = state.rendered.load(Ordering::Relaxed);
                let span = Span {
                    start: self.frame + start as u64,
                    end: self.frame + start as u64 + count,
                    source,
                };
                let mut spans = state.timeline.lock().expect("audio timeline poisoned");
                if let Some(previous) = spans.back_mut()
                    && previous.end == span.start
                    && previous.source + previous.end - previous.start == source
                {
                    previous.end = span.end;
                } else {
                    if spans.len() == 512 {
                        spans.pop_front();
                    }
                    spans.push_back(span);
                }
                state.rendered.store(source + count, Ordering::Release);
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
