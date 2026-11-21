//! Independent GPU timestamps around the render graph, without waiting for it.
//! Includes all views/passes in the graph; excludes CPU preparation, queued
//! buffer writes before the graph, readbacks after it, and surface presentation.
use bevy::{
    prelude::*,
    render::{
        RenderApp,
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
    },
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

// At high throughput, mapping callbacks can arrive in bursts several frames
// after the graph completes. Keep enough asynchronous slots to absorb that
// latency without waiting for the GPU or silently losing percentile samples.
pub(super) const SLOTS: usize = 64;
const MAX_PENDING: usize = 256;

#[derive(Clone, serde::Serialize)]
pub(super) struct Frame {
    pub frame: u64,
    pub elapsed_seconds: f64,
    pub render_graph_ms: f64,
}

#[derive(Default, serde::Serialize)]
pub(super) struct Sample {
    pub supported: Option<bool>,
    pub completed: u64,
    pub invalid: u64,
    pub skipped: u64,
    pub dropped: u64,
    pub frames: Vec<Frame>,
}

struct Pending {
    origin: Instant,
    sample: Sample,
}

#[derive(Resource, Clone)]
pub(super) struct Shared(Arc<Mutex<Pending>>);
impl Default for Shared {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Pending {
            origin: Instant::now(),
            sample: Sample::default(),
        })))
    }
}
impl Shared {
    pub fn set_origin(&self, origin: Instant) {
        self.0.lock().unwrap().origin = origin;
    }

    /// Drain every completed frame, rather than repeatedly sampling only the
    /// latest one and losing results when several callbacks run in one poll.
    pub fn take(&self) -> Sample {
        let mut pending = self.0.lock().unwrap();
        let frames = std::mem::take(&mut pending.sample.frames);
        Sample {
            frames,
            supported: pending.sample.supported,
            completed: pending.sample.completed,
            invalid: pending.sample.invalid,
            skipped: pending.sample.skipped,
            dropped: pending.sample.dropped,
        }
    }
}

struct Slot {
    readback: wgpu::Buffer,
    free: Arc<AtomicBool>,
}
struct Gpu {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: [Slot; SLOTS],
    period_ns: f32,
}
struct Active {
    slot: usize,
    frame: u64,
    queued: Instant,
    elapsed_seconds: f64,
}
#[derive(Resource, Default)]
struct Profiler {
    initialized: bool,
    gpu: Option<Gpu>,
    active: Option<Active>,
    frame: u64,
}

pub(super) fn install(app: &mut App) {
    let shared = Shared::default();
    app.insert_resource(shared.clone());
    app.sub_app_mut(RenderApp)
        .insert_resource(shared)
        .init_resource::<Profiler>()
        .add_systems(
            RenderGraph,
            (
                begin.before(RenderGraphSystems::Begin),
                end.after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            ),
        );
}

fn begin(
    mut profiler: ResMut<Profiler>,
    shared: Res<Shared>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut context: RenderContext,
) {
    if !profiler.initialized {
        profiler.initialized = true;
        let required =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let period_ns = queue.get_timestamp_period();
        let supported =
            device.features().contains(required) && period_ns.is_finite() && period_ns > 0.;
        shared.0.lock().unwrap().sample.supported = Some(supported);
        if supported {
            let device = device.wgpu_device();
            profiler.gpu = Some(Gpu {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("resonance/frame-timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: (SLOTS * 2) as u32,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resonance/frame-query-resolve"),
                    size: wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT * SLOTS as u64,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                slots: std::array::from_fn(|_| Slot {
                    readback: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("resonance/frame-query-readback"),
                        size: 16,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    }),
                    free: Arc::new(AtomicBool::new(true)),
                }),
                period_ns,
            });
        }
    }
    profiler.frame += 1;
    let Some(gpu) = &profiler.gpu else { return };
    // Poll never waits for GPU completion. A saturated ring skips a measurement
    // and reports it, rather than adding a synchronization stall to the workload.
    let _ = device.poll(wgpu::PollType::Poll);
    let Some(slot) = gpu
        .slots
        .iter()
        .position(|s| s.free.swap(false, Ordering::AcqRel))
    else {
        shared.0.lock().unwrap().sample.skipped += 1;
        return;
    };
    context
        .command_encoder()
        .write_timestamp(&gpu.queries, (slot * 2) as u32);
    profiler.active = Some(Active {
        slot,
        frame: profiler.frame,
        queued: Instant::now(),
        elapsed_seconds: shared.0.lock().unwrap().origin.elapsed().as_secs_f64(),
    });
}

fn elapsed_ms(begin: u64, end: u64, period_ns: f32) -> Option<f64> {
    let ticks = end.checked_sub(begin)?;
    let ms = ticks as f64 * f64::from(period_ns) / 1e6;
    (begin != 0 && period_ns > 0. && ms.is_finite() && ms > 0.).then_some(ms)
}

fn end(mut profiler: ResMut<Profiler>, shared: Res<Shared>, mut context: RenderContext) {
    let Some(active) = profiler.active.take() else {
        return;
    };
    let gpu = profiler.gpu.as_ref().unwrap();
    let slot = &gpu.slots[active.slot];
    let offset = active.slot as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
    let encoder = context.command_encoder();
    encoder.write_timestamp(&gpu.queries, (active.slot * 2 + 1) as u32);
    encoder.resolve_query_set(
        &gpu.queries,
        (active.slot * 2) as u32..(active.slot * 2 + 2) as u32,
        &gpu.resolve,
        offset,
    );
    encoder.copy_buffer_to_buffer(&gpu.resolve, offset, &slot.readback, 0, 16);
    let buffer = slot.readback.clone();
    let free = slot.free.clone();
    let period_ns = gpu.period_ns;
    let shared = shared.clone();
    encoder.map_buffer_on_submit(&slot.readback, wgpu::MapMode::Read, .., move |result| {
        let duration = if result.is_ok() {
            let duration = buffer.slice(..).get_mapped_range().ok().and_then(|bytes| {
                let begin = u64::from_ne_bytes(bytes[0..8].try_into().unwrap());
                let end = u64::from_ne_bytes(bytes[8..16].try_into().unwrap());
                elapsed_ms(begin, end, period_ns)
                    // GPU execution cannot exceed wall time since its commands
                    // were queued. Reject corruption with an explicit count.
                    .filter(|ms| *ms <= active.queued.elapsed().as_secs_f64() * 1000. + 1.)
            });
            buffer.unmap();
            duration
        } else {
            None
        };
        let mut pending = shared.0.lock().unwrap();
        match duration {
            Some(render_graph_ms) => {
                pending.sample.completed += 1;
                if pending.sample.frames.len() < MAX_PENDING {
                    pending.sample.frames.push(Frame {
                        frame: active.frame,
                        elapsed_seconds: active.elapsed_seconds,
                        render_graph_ms,
                    });
                } else {
                    pending.sample.dropped += 1;
                }
            }
            None => pending.sample.invalid += 1,
        }
        free.store(true, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_require_a_real_monotonic_interval_and_valid_period() {
        assert_eq!(elapsed_ms(10, 20, 1000.), Some(0.01));
        for (begin, end, period) in [
            (0, 20, 1.),
            (20, 10, 1.),
            (10, 10, 1.),
            (10, 20, 0.),
            (10, 20, -1.),
            (10, 20, f32::NAN),
            (10, 20, f32::INFINITY),
        ] {
            assert!(elapsed_ms(begin, end, period).is_none());
        }
    }

    #[test]
    #[ignore = "requires a graphics adapter with encoder timestamps; no window or audio"]
    fn headless_timestamp_queries_complete() {
        let mut app = App::new();
        app.add_plugins(crate::test_support::render_plugins_with_features(
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
        ));
        // Exercise query submission and asynchronous readback without a scene.
        install(&mut app);
        crate::test_support::finish_app(&mut app).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(60);
        let mut frames = Vec::new();
        loop {
            app.update();
            let sample = app.world().resource::<Shared>().take();
            assert_ne!(
                sample.supported,
                Some(false),
                "adapter lacks encoder timestamp support"
            );
            assert_eq!(sample.invalid, 0, "GPU returned invalid query values");
            assert_eq!(sample.dropped, 0);
            frames.extend(sample.frames);
            if frames.len() >= 10 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "GPU timestamp readbacks did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            frames
                .iter()
                .all(|f| f.render_graph_ms.is_finite() && f.render_graph_ms > 0.)
        );
        println!(
            "{} completed GPU frame intervals, mean {:.4} ms",
            frames.len(),
            frames.iter().map(|f| f.render_graph_ms).sum::<f64>() / frames.len() as f64
        );
    }
}
