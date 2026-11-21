//! Optional thread CPU clocks distinguish execution from wall-clock stalls.
//! Worker CPU and GPU work are deliberately outside this measurement.
#[cfg(target_os = "linux")]
pub(super) fn now() -> Option<u64> {
    let mut value = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: clock_gettime receives valid writable storage and initializes the
    // entire timespec on success. Failed calls never read that storage.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, value.as_mut_ptr()) } != 0 {
        return None;
    }
    let value = unsafe { value.assume_init() };
    u64::try_from(value.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(value.tv_nsec).ok()?)
}
#[cfg(not(target_os = "linux"))]
pub(super) fn now() -> Option<u64> {
    None
}
pub(super) fn elapsed_ms(start: Option<u64>, end: Option<u64>) -> f64 {
    start
        .zip(end)
        .and_then(|(a, b)| b.checked_sub(a))
        .map_or(0., |ns| ns as f64 / 1_000_000.)
}
