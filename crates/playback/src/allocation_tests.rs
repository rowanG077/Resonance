//! Allocation instrumentation is test-only and local to the callback's thread.
use crate::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::Arc,
    time::{Duration, Instant},
};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<(usize,usize)> = const { Cell::new((0,0)) };
}
struct Counting;
#[global_allocator]
static ALLOCATOR: Counting = Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNTS.try_with(|counter| {
                let (a, d) = counter.get();
                counter.set((a + 1, d));
            });
        }
        // SAFETY: this allocator forwards the unchanged layout to System.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNTS.try_with(|counter| {
                let (a, d) = counter.get();
                counter.set((a, d + 1));
            });
        }
        // SAFETY: pointer and layout came from the same forwarding allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}
struct Copy;
impl Converter for Copy {
    fn convert(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) -> anyhow::Result<()> {
        output.extend_from_slice(input);
        Ok(())
    }
}
#[test]
fn callback_allocates_and_deallocates_nothing_when_playing_muted_or_starved() {
    let clock = Arc::new(Clock::new(SOURCE_RATE));
    let ring = Output::new(clock.clone(), 256);
    let (_, mixer) = Mixer::new(clock);
    let worker = Worker::start(mixer, Box::new(Copy), ring.clone()).unwrap();
    let began = Instant::now();
    while !ring.ready() {
        assert!(began.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(worker);
    let mut live = Callback::new(ring.clone(), false);
    let mut muted = Callback::new(ring.clone(), true);
    let mut samples = [0.; 512];
    COUNTS.set((0, 0));
    TRACK.set(true);
    for _ in 0..10 {
        live.render(&mut samples, 2, Duration::ZERO, |s| s);
        muted.render(&mut samples, 2, Duration::ZERO, |s| s);
    }
    TRACK.set(false);
    assert_eq!(COUNTS.get(), (0, 0));
    assert!(ring.diagnostics().underrun_frames > 0);
}
