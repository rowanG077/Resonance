//! Opt-in CPU timings for the speculation and pose stages (nanoseconds).
//! Stages: 0 prediction, 1 field/world poses, 2 title/UI, 3 hierarchy
//! propagation (including affine poses), 4 committed battle update, 5 battle presentation,
//! 6 display-list compile, 7 surface record sync.
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

static ENABLED: AtomicBool = AtomicBool::new(false);
static NANOS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

pub(crate) fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}
pub(crate) fn take() -> Option<[u64; 8]> {
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
