use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeFrame(pub u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutputFrame(pub u64);

/// A device timestamp is the predicted DAC time, not the time mixing finished.
/// One writer (the callback); readers take a coherent, allocation-free snapshot.
pub struct Clock {
    origin: Instant,
    rate: AtomicU64,
    base: AtomicU64,
    version: AtomicU64,
    first: AtomicU64,
    end: AtomicU64,
    due_ns: AtomicU64,
    offline: AtomicU64,
    last: AtomicU64,
}
impl Clock {
    pub fn new(rate: u32) -> Self {
        assert!(rate > 0);
        Self {
            origin: Instant::now(),
            rate: AtomicU64::new(u64::from(rate)),
            base: AtomicU64::new(0),
            version: AtomicU64::new(0),
            first: AtomicU64::new(0),
            end: AtomicU64::new(0),
            due_ns: AtomicU64::new(0),
            offline: AtomicU64::new(u64::MAX),
            last: AtomicU64::new(0),
        }
    }
    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Acquire) as u32
    }
    pub fn now_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos() as u64
    }
    pub(crate) fn publish(&self, first: OutputFrame, end: OutputFrame, due_ns: u64) {
        self.version.fetch_add(1, Ordering::AcqRel);
        self.first.store(first.0, Ordering::Relaxed);
        self.end.store(end.0, Ordering::Relaxed);
        self.due_ns.store(due_ns, Ordering::Relaxed);
        self.version.fetch_add(1, Ordering::Release);
    }
    pub(crate) fn manual(&self, frame: NativeFrame) {
        self.offline.store(frame.0, Ordering::Release);
    }
    pub fn freeze(&self) -> NativeFrame {
        let frame = self.audible();
        self.manual(frame);
        frame
    }
    /// Reconfigure only while the old callback is stopped.
    pub fn rebase(&self, rate: u32, base: NativeFrame) {
        assert!(rate > 0);
        self.version.fetch_add(1, Ordering::AcqRel);
        self.rate.store(u64::from(rate), Ordering::Relaxed);
        self.base.store(base.0, Ordering::Relaxed);
        self.first.store(0, Ordering::Relaxed);
        self.end.store(0, Ordering::Relaxed);
        self.due_ns.store(self.now_ns(), Ordering::Relaxed);
        self.last.store(base.0, Ordering::Relaxed);
        self.offline.store(u64::MAX, Ordering::Release);
        self.version.fetch_add(1, Ordering::Release);
    }
    pub fn audible(&self) -> NativeFrame {
        let manual = self.offline.load(Ordering::Acquire);
        if manual != u64::MAX {
            return NativeFrame(manual);
        }
        loop {
            let version = self.version.load(Ordering::Acquire);
            if !version.is_multiple_of(2) {
                std::hint::spin_loop();
                continue;
            }
            let first = self.first.load(Ordering::Relaxed);
            let end = self.end.load(Ordering::Relaxed);
            let due = self.due_ns.load(Ordering::Relaxed);
            let base = self.base.load(Ordering::Relaxed);
            let rate = self.rate.load(Ordering::Relaxed);
            if version != self.version.load(Ordering::Acquire) {
                continue;
            }
            let remaining = due.saturating_sub(self.now_ns());
            let ahead = (remaining as u128 * u128::from(rate) / 1_000_000_000) as u64;
            let frame = end.saturating_sub(ahead).clamp(first.min(end), end);
            let native = base + frame * u64::from(crate::SOURCE_RATE) / rate;
            // Backend timestamp estimates may jitter, but transport time must
            // never move backwards when a new callback anchor arrives.
            return NativeFrame(self.last.fetch_max(native, Ordering::AcqRel).max(native));
        }
    }
}
pub(crate) fn duration(frames: u64) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(crate::SOURCE_RATE))
}
