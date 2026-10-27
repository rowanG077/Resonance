//! Opt-in CPU timings for the speculation and pose stages (nanoseconds).
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

static ENABLED: AtomicBool = AtomicBool::new(false);
static NANOS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

pub(crate) fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}
pub(crate) fn take() -> Option<[u64; 4]> {
    ENABLED
        .load(Ordering::Relaxed)
        .then(|| NANOS.each_ref().map(|v| v.swap(0, Ordering::Relaxed)))
}
pub(crate) struct Timer(usize, Option<Instant>);
impl Timer {
    pub fn start(stage: usize) -> Self {
        Self(stage, ENABLED.load(Ordering::Relaxed).then(Instant::now))
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.1 {
            NANOS[self.0].fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }
}
