//! Keep frame-critical threads on the fastest cores of asymmetric CPUs.
//!
//! An energy-aware scheduler places a thread by its average utilization. A
//! render or simulation thread that finishes its frame in a fraction of a
//! millisecond looks small, is moved to an efficiency core, and then takes
//! several times longer exactly when a frame needs it. Only threads that
//! opt in are restricted, and only on systems reporting unequal capacities.
use std::cell::Cell;

/// Restrict the calling thread to the highest-capacity CPUs it may already
/// use. Returns the selected CPUs, or nothing when the system is symmetric,
/// the topology is incomplete, or `RESONANCE_ANY_CPU` is set.
#[cfg(target_os = "linux")]
pub(crate) fn prefer_fast_cpus() -> Vec<usize> {
    if std::env::var_os("RESONANCE_ANY_CPU").is_some() {
        return Vec::new();
    }
    // SAFETY: cpu_set_t is an initialized, correctly sized writable mask. PID
    // zero selects this calling thread; no other task's affinity is changed.
    let mut allowed = unsafe { std::mem::zeroed::<libc::cpu_set_t>() };
    if unsafe { libc::sched_getaffinity(0, size_of::<libc::cpu_set_t>(), &mut allowed) } != 0 {
        return Vec::new();
    }
    let mut capacities = Vec::new();
    for cpu in 0..libc::CPU_SETSIZE as usize {
        // SAFETY: cpu is within the cpu_set_t mask's supported range.
        if !unsafe { libc::CPU_ISSET(cpu, &allowed) } {
            continue;
        }
        let Some(capacity) =
            std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/cpu_capacity"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
        else {
            // Incomplete topology must not silently exclude allowed cores.
            return Vec::new();
        };
        capacities.push((cpu, capacity));
    }
    let Some(maximum) = capacities.iter().map(|(_, c)| *c).max() else {
        return Vec::new();
    };
    if capacities.iter().all(|(_, c)| *c == maximum) {
        return Vec::new();
    }
    let cpus: Vec<_> = capacities
        .into_iter()
        .filter_map(|(cpu, c)| (c == maximum).then_some(cpu))
        .collect();
    // SAFETY: All selected CPUs were present in the allowed mask above.
    let mut selected = unsafe { std::mem::zeroed::<libc::cpu_set_t>() };
    for &cpu in &cpus {
        unsafe { libc::CPU_SET(cpu, &mut selected) };
    }
    if unsafe { libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &selected) } != 0 {
        return Vec::new();
    }
    cpus
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn prefer_fast_cpus() -> Vec<usize> {
    Vec::new()
}

/// Apply [`prefer_fast_cpus`] to the calling thread the first time it runs
/// this. Cheap enough to call from a per-frame system on the thread that
/// executes it, which is how the main and render threads are reached.
pub(crate) fn pin_current_thread() {
    thread_local! {
        static PINNED: Cell<bool> = const { Cell::new(false) };
    }
    if !PINNED.replace(true) {
        let cpus = prefer_fast_cpus();
        if !cpus.is_empty() {
            bevy::log::debug!(
                "Thread {:?} restricted to performance CPUs {cpus:?}",
                std::thread::current().name()
            );
        }
    }
}
