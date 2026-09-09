use crate::{Clock, Mixer, NativeFrame, OutputFrame, SOURCE_BLOCK, SOURCE_RATE};
use anyhow::{Context, Result, ensure};
use crossbeam_queue::ArrayQueue;
use std::collections::VecDeque;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub const OUTPUT_BLOCK: usize = 256;
const CAPACITY: usize = 65_536;
#[derive(Clone, Copy)]
struct Frame {
    samples: [f32; 2],
    position: OutputFrame,
}
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    pub converted_frames: u64,
    pub submitted_frames: u64,
    pub callbacks: u64,
    pub underrun_frames: u64,
    pub underrun_callbacks: u64,
    pub device_errors: u64,
    pub backend_underruns: u64,
    pub device_lost: u64,
    pub maximum_callback_frames: usize,
    pub queued_frames: usize,
    pub maximum_render_ns: u64,
    pub maximum_callback_ns: u64,
    pub p999_callback_us: u64,
    pub callback_deadline_misses: u64,
    pub backend_delay_ns: u64,
    pub worker_realtime_priority: Option<bool>,
}
pub struct Output {
    queue: ArrayQueue<Frame>,
    pub clock: Arc<Clock>,
    target: AtomicUsize,
    converted: AtomicU64,
    submitted: AtomicU64,
    callbacks: AtomicU64,
    underrun_frames: AtomicU64,
    underrun_callbacks: AtomicU64,
    device_errors: AtomicU64,
    backend_underruns: AtomicU64,
    device_lost: AtomicU64,
    maximum_callback: AtomicUsize,
    maximum_render_ns: AtomicU64,
    callback_ns: AtomicU64,
    callback_histogram: [AtomicU64; 512],
    callback_deadline_misses: AtomicU64,
    backend_delay_ns: AtomicU64,
    worker_priority: AtomicUsize,
    stopped: AtomicBool,
    retain_state: AtomicBool,
    failure: Mutex<Option<String>>,
}
impl Output {
    pub fn new(clock: Arc<Clock>, callback_frames: usize) -> Arc<Self> {
        Arc::new(Self {
            queue: ArrayQueue::new(CAPACITY),
            clock,
            target: AtomicUsize::new((callback_frames * 3).clamp(OUTPUT_BLOCK * 3, CAPACITY / 2)),
            converted: AtomicU64::new(0),
            submitted: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
            underrun_callbacks: AtomicU64::new(0),
            device_errors: AtomicU64::new(0),
            backend_underruns: AtomicU64::new(0),
            device_lost: AtomicU64::new(0),
            maximum_callback: AtomicUsize::new(0),
            maximum_render_ns: AtomicU64::new(0),
            callback_ns: AtomicU64::new(0),
            callback_histogram: std::array::from_fn(|_| AtomicU64::new(0)),
            callback_deadline_misses: AtomicU64::new(0),
            backend_delay_ns: AtomicU64::new(0),
            worker_priority: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
            retain_state: AtomicBool::new(false),
            failure: Mutex::new(None),
        })
    }
    pub fn diagnostics(&self) -> Diagnostics {
        let count = self.callbacks.load(Ordering::Relaxed);
        let target = (count * 999).div_ceil(1000);
        let mut cumulative = 0;
        let mut p999 = 0;
        for (index, bucket) in self.callback_histogram.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= target {
                p999 = index as u64;
                break;
            }
        }
        Diagnostics {
            converted_frames: self.converted.load(Ordering::Relaxed),
            submitted_frames: self.submitted.load(Ordering::Relaxed),
            callbacks: self.callbacks.load(Ordering::Relaxed),
            underrun_frames: self.underrun_frames.load(Ordering::Relaxed),
            underrun_callbacks: self.underrun_callbacks.load(Ordering::Relaxed),
            device_errors: self.device_errors.load(Ordering::Relaxed),
            backend_underruns: self.backend_underruns.load(Ordering::Relaxed),
            device_lost: self.device_lost.load(Ordering::Relaxed),
            maximum_callback_frames: self.maximum_callback.load(Ordering::Relaxed),
            queued_frames: self.queue.len(),
            maximum_render_ns: self.maximum_render_ns.load(Ordering::Relaxed),
            maximum_callback_ns: self.callback_ns.load(Ordering::Relaxed),
            p999_callback_us: p999,
            callback_deadline_misses: self.callback_deadline_misses.load(Ordering::Relaxed),
            backend_delay_ns: self.backend_delay_ns.load(Ordering::Relaxed),
            worker_realtime_priority: match self.worker_priority.load(Ordering::Acquire) {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            },
        }
    }
    pub fn check(&self) -> Result<()> {
        if let Some(error) = self
            .failure
            .lock()
            .expect("audio fault lock poisoned")
            .as_ref()
        {
            anyhow::bail!("audio mixer failed: {error}");
        }
        ensure!(
            self.device_errors.load(Ordering::Acquire) == 0,
            "audio output device failed: {:?}",
            self.diagnostics()
        );
        ensure!(
            self.underrun_callbacks.load(Ordering::Acquire) == 0,
            "audio output underrun: {:?}",
            self.diagnostics()
        );
        Ok(())
    }
    pub fn device_error(&self, underrun: bool, lost: bool) {
        if underrun {
            self.backend_underruns.fetch_add(1, Ordering::Relaxed);
        }
        if lost {
            self.device_lost.fetch_add(1, Ordering::Relaxed);
        }
        self.device_errors.fetch_add(1, Ordering::Release);
    }
    pub fn ready(&self) -> bool {
        self.queue.len() >= self.target.load(Ordering::Relaxed)
    }
}

/// Only fixed-size values and atomics cross the callback boundary. This object
/// owns no sources, codecs, locks, logging, or producer thread handle.
pub struct Callback {
    output: Arc<Output>,
    silent: bool,
}
impl Callback {
    pub fn new(output: Arc<Output>, silent: bool) -> Self {
        Self { output, silent }
    }
    pub fn render<T: Copy>(
        &mut self,
        data: &mut [T],
        channels: usize,
        playback_delay: Duration,
        convert: impl Fn(f32) -> T,
    ) {
        let output = &self.output;
        let count = data.len() / channels;
        let now = output.clock.now_ns();
        output.maximum_callback.fetch_max(count, Ordering::Relaxed);
        // React to the actual backend period, rather than assuming its requested size.
        output.target.store(
            (count * 3).clamp(OUTPUT_BLOCK * 3, CAPACITY / 2),
            Ordering::Relaxed,
        );
        let mut missing = 0;
        let mut last = None;
        for (index, dest) in data.chunks_exact_mut(channels).enumerate() {
            let sample = if let Some(frame) = output.queue.pop() {
                last = Some((frame.position.0 + 1, index + 1));
                if self.silent {
                    [0.; 2]
                } else {
                    frame.samples.map(|s| s.clamp(-1., 1.))
                }
            } else {
                missing += 1;
                [0.; 2]
            };
            if channels == 1 {
                dest[0] = convert((sample[0] + sample[1]) * 0.5);
            } else {
                dest[0] = convert(sample[0]);
                dest[1] = convert(sample[1]);
                for channel in &mut dest[2..] {
                    *channel = convert(0.);
                }
            }
        }
        if let Some((end, length)) = last {
            let due = now
                + playback_delay.as_nanos() as u64
                + length as u64 * 1_000_000_000 / u64::from(output.clock.rate());
            output.clock.publish(OutputFrame(0), OutputFrame(end), due);
        }
        output.submitted.fetch_add(count as u64, Ordering::Relaxed);
        output.callbacks.fetch_add(1, Ordering::Relaxed);
        if missing > 0 {
            output.underrun_frames.fetch_add(missing, Ordering::Relaxed);
            output.underrun_callbacks.fetch_add(1, Ordering::Release);
        }
        let elapsed = output.clock.now_ns().saturating_sub(now);
        output.callback_ns.fetch_max(elapsed, Ordering::Relaxed);
        output.callback_histogram[(elapsed.div_ceil(1000) as usize).min(511)]
            .fetch_add(1, Ordering::Relaxed);
        output
            .backend_delay_ns
            .store(playback_delay.as_nanos() as u64, Ordering::Relaxed);
        if elapsed > count as u64 * 1_000_000_000 / u64::from(output.clock.rate()) {
            output
                .callback_deadline_misses
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}
/// Implemented by the codec adapter. Both conversion and allocations stay on
/// the mixer worker; the native DSP stream remains separately recordable.
pub trait Converter: Send {
    /// Optional worker-local platform setup. The returned guard is created and
    /// destroyed on that worker, so it does not need to be Send.
    fn prepare(&mut self) -> Result<Option<Box<dyn std::any::Any>>> {
        Ok(None)
    }
    fn realtime_priority(&self) -> Option<bool> {
        None
    }
    fn convert(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) -> Result<()>;
}
/// Native PCM history allows a replaced device to resume at the estimated
/// audible cursor, including audio already rendered but not yet heard.
pub struct Suspended {
    mixer: Mixer,
    history: VecDeque<[f32; 2]>,
    cursor: NativeFrame,
}
impl Suspended {
    pub fn resume(
        self,
        rate: u32,
        period: usize,
        converter: Box<dyn Converter>,
    ) -> Result<(Worker, Arc<Output>)> {
        let first = self.mixer.frame.saturating_sub(self.history.len() as u64);
        ensure!(
            (first..=self.mixer.frame).contains(&self.cursor.0),
            "device latency exceeded retained native PCM history"
        );
        let replay = self
            .history
            .iter()
            .skip((self.cursor.0 - first) as usize)
            .copied()
            .collect();
        let clock = self.mixer.clock();
        clock.rebase(rate, self.cursor);
        let output = Output::new(clock, period);
        let worker = Worker::spawn(self.mixer, converter, output.clone(), self.history, replay)?;
        Ok((worker, output))
    }
}
pub struct Worker {
    output: Arc<Output>,
    thread: Option<JoinHandle<Option<Suspended>>>,
}
impl Worker {
    pub fn start(mixer: Mixer, converter: Box<dyn Converter>, output: Arc<Output>) -> Result<Self> {
        Self::spawn(
            mixer,
            converter,
            output,
            VecDeque::with_capacity(SOURCE_RATE as usize / 2 + SOURCE_BLOCK),
            VecDeque::new(),
        )
    }
    fn spawn(
        mut mixer: Mixer,
        mut converter: Box<dyn Converter>,
        output: Arc<Output>,
        mut history: VecDeque<[f32; 2]>,
        mut replay: VecDeque<[f32; 2]>,
    ) -> Result<Self> {
        let shared = output.clone();
        let worker = thread::Builder::new()
            .name("resonance-audio-mixer".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                        let _worker_setup = converter.prepare()?;
                        shared.worker_priority.store(
                            match converter.realtime_priority() {
                                Some(true) => 1,
                                Some(false) => 2,
                                None => 0,
                            },
                            Ordering::Release,
                        );
                        let mut native = [[0.; 2]; SOURCE_BLOCK];
                        let mut converted = Vec::with_capacity(2048);
                        let mut position = 0u64;
                        while !shared.stopped.load(Ordering::Acquire) {
                            if shared.queue.len() >= shared.target.load(Ordering::Relaxed) {
                                thread::sleep(Duration::from_micros(500));
                                continue;
                            }
                            let started = std::time::Instant::now();
                            let length = if replay.is_empty() {
                                mixer.render(&mut native)?;
                                for &frame in &native {
                                    if history.len() == SOURCE_RATE as usize / 2 {
                                        history.pop_front();
                                    }
                                    history.push_back(frame);
                                }
                                SOURCE_BLOCK
                            } else {
                                let length = replay.len().min(SOURCE_BLOCK);
                                for frame in &mut native[..length] {
                                    *frame = replay.pop_front().unwrap();
                                }
                                length
                            };
                            converted.clear();
                            converter.convert(&native[..length], &mut converted)?;
                            ensure!(
                                converted.len() <= 8192,
                                "output converter exceeded block budget"
                            );
                            for &samples in &converted {
                                shared
                                    .queue
                                    .push(Frame {
                                        samples,
                                        position: OutputFrame(position),
                                    })
                                    .map_err(|_| anyhow::anyhow!("output ring budget exceeded"))?;
                                position += 1;
                            }
                            shared.converted.store(position, Ordering::Release);
                            shared
                                .maximum_render_ns
                                .fetch_max(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
                        }
                        Ok(())
                    }))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("audio mixer panicked")));
                if let Err(error) = result {
                    mixer.fail();
                    *shared.failure.lock().expect("audio fault lock poisoned") =
                        Some(format!("{error:#}"));
                    return None;
                }
                if shared.retain_state.load(Ordering::Acquire) {
                    Some(Suspended {
                        cursor: shared.clock.audible(),
                        mixer,
                        history,
                    })
                } else {
                    // Ordinary teardown destroys all sources on their owner worker.
                    None
                }
            })
            .context("start audio mixer worker")?;
        Ok(Self {
            output,
            thread: Some(worker),
        })
    }
    /// Stop the device before calling this. Reconfiguration never enters its callback.
    pub fn suspend(mut self) -> Result<Suspended> {
        self.output.clock.freeze();
        self.output.retain_state.store(true, Ordering::Release);
        self.output.stopped.store(true, Ordering::Release);
        self.thread
            .take()
            .context("audio worker already stopped")?
            .join()
            .map_err(|_| anyhow::anyhow!("audio worker panicked"))?
            .context("audio worker failed while suspending")
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.output.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}
