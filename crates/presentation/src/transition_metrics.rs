//! Transition readiness ends at the GPU fence for a ready frame, not CPU load.
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        renderer::{RenderDevice, RenderQueue},
    },
};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Place {
    #[cfg(test)]
    Title,
    Field(u32),
    World,
    Battle,
}
impl Place {
    pub(crate) fn current(world: &World) -> Option<Self> {
        let session = world.get_resource::<crate::new_game::Session>()?;
        Some(if session.is_field() {
            Self::Field(session.map_id())
        } else {
            Self::World
        })
    }
    fn label(self) -> String {
        match self {
            #[cfg(test)]
            Self::Title => "title".into(),
            Self::Field(map) => format!("field:{map}"),
            Self::World => "overworld".into(),
            Self::Battle => "battle".into(),
        }
    }
    fn ready(self, world: &mut World) -> bool {
        if self == Self::Battle {
            return world
                .get_resource::<crate::battle::Owner>()
                .is_some_and(|owner| owner.presenting());
        }
        Self::current(world) == Some(self)
            && !world.contains_resource::<crate::battle::Owner>()
            && world
                .resource::<crate::loading::Resident>()
                .active
                .load(Ordering::Acquire)
            && crate::new_game::scene_ready(world)
    }
}

#[derive(serde::Serialize)]
pub(crate) struct Record {
    pub sequence: u64,
    pub source: String,
    pub destination: String,
    pub outcome: &'static str,
    pub total_ms: f64,
}
#[derive(Resource, Default)]
pub(crate) struct Trace {
    sequence: u64,
    pending: Option<Pending>,
    pub last: Option<Record>,
}
struct Pending {
    from: Place,
    to: Place,
    started: Instant,
    frame: Option<Arc<Ticket>>,
}
struct Ticket {
    started: Instant,
    submitted: AtomicBool,
    completed: OnceLock<f64>,
}
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
struct Frame(Option<Arc<Ticket>>);

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Trace>()
        .init_resource::<Frame>()
        .add_plugins(ExtractResourcePlugin::<Frame>::default())
        .add_systems(Last, observe);
    app.sub_app_mut(RenderApp)
        .add_systems(Render, submitted.in_set(RenderSystems::Cleanup));
}

pub(crate) fn begin(world: &mut World, from: Place, to: Place) {
    let Some(mut trace) = world.get_resource_mut::<Trace>() else {
        return;
    };
    if trace
        .pending
        .as_ref()
        .is_some_and(|p| p.from == from && p.to == to)
    {
        return;
    }
    if trace.pending.is_some() {
        trace.finish("superseded");
    }
    trace.pending = Some(Pending {
        from,
        to,
        started: Instant::now(),
        frame: None,
    });
    world.resource_mut::<Frame>().0 = None;
}

pub(crate) fn failed(world: &mut World) {
    if let Some(mut trace) = world.get_resource_mut::<Trace>() {
        trace.finish("failed");
    }
    if let Some(mut frame) = world.get_resource_mut::<Frame>() {
        frame.0 = None;
    }
}

impl Trace {
    #[cfg(test)]
    pub(crate) fn completed_since(&self, sequence: u64, destination: Place) -> Option<&Record> {
        self.last.as_ref().filter(|record| {
            record.sequence > sequence
                && record.destination == destination.label()
                && record.outcome == "ready"
        })
    }

    fn finish(&mut self, outcome: &'static str) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let completed = pending
            .frame
            .and_then(|frame| frame.completed.get().copied());
        let record = Record {
            sequence: self.sequence + 1,
            source: pending.from.label(),
            destination: pending.to.label(),
            outcome: if completed.is_some() {
                "ready"
            } else {
                outcome
            },
            total_ms: completed.unwrap_or_else(|| pending.started.elapsed().as_secs_f64() * 1000.),
        };
        info!(source=%record.source, destination=%record.destination, outcome=record.outcome, total_ms=record.total_ms, "Scene transition");
        self.sequence = record.sequence;
        self.last = Some(record);
    }
}

pub(crate) fn observe(world: &mut World) {
    let mut trace = world.remove_resource::<Trace>().unwrap();
    if let Some(pending) = &mut trace.pending {
        if let Some(frame) = &pending.frame {
            if frame.completed.get().is_some() {
                trace.finish("ready");
                world.resource_mut::<Frame>().0 = None;
            }
        } else if pending.to.ready(world) {
            let frame = Arc::new(Ticket {
                started: pending.started,
                submitted: AtomicBool::new(false),
                completed: OnceLock::new(),
            });
            pending.frame = Some(frame.clone());
            world.resource_mut::<Frame>().0 = Some(frame);
        }
    }
    world.insert_resource(trace);
}

fn submitted(frame: Res<Frame>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let Some(frame) = &frame.0 else { return };
    let _ = device.poll(bevy::render::render_resource::PollType::Poll);
    if !frame.submitted.swap(true, Ordering::AcqRel) {
        let frame = frame.clone();
        queue.on_submitted_work_done(move || {
            let _ = frame
                .completed
                .set(frame.started.elapsed().as_secs_f64() * 1000.);
        });
        // Ordinary transitions poll without blocking on GPU work. Opt-in
        // profiling also polls in the background for a prompt callback timestamp.
        #[cfg(all(test, feature = "preparation-trace", not(target_arch = "wasm32")))]
        if crate::preparation_profile::enabled() {
            let device = device.clone();
            bevy::tasks::IoTaskPool::get()
                .spawn(async move {
                    let _ = device.poll(bevy::render::render_resource::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(1)),
                    });
                })
                .detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_observation_keeps_the_request_start_and_uses_the_gpu_timestamp() {
        let mut world = World::new();
        world.init_resource::<Trace>();
        world.init_resource::<Frame>();
        begin(&mut world, Place::Field(332), Place::Field(340));
        let started = world.resource::<Trace>().pending.as_ref().unwrap().started;
        assert!(
            world
                .resource::<Trace>()
                .completed_since(0, Place::Field(340))
                .is_none()
        );
        begin(&mut world, Place::Field(332), Place::Field(340));
        assert_eq!(
            world.resource::<Trace>().pending.as_ref().unwrap().started,
            started
        );
        let ticket = Arc::new(Ticket {
            started,
            submitted: AtomicBool::new(true),
            completed: OnceLock::from(37.5),
        });
        world
            .resource_mut::<Trace>()
            .pending
            .as_mut()
            .unwrap()
            .frame = Some(ticket);
        observe(&mut world);
        let record = world.resource::<Trace>().last.as_ref().unwrap();
        assert!(
            world
                .resource::<Trace>()
                .completed_since(0, Place::Field(340))
                .is_some()
        );
        assert!(
            world
                .resource::<Trace>()
                .completed_since(1, Place::Field(340))
                .is_none()
        );
        assert!(
            world
                .resource::<Trace>()
                .completed_since(0, Place::Battle)
                .is_none()
        );
        assert_eq!(
            (record.sequence, record.outcome, record.total_ms),
            (1, "ready", 37.5)
        );
        assert_eq!(
            (&*record.source, &*record.destination),
            ("field:332", "field:340")
        );
        observe(&mut world);
        assert_eq!(world.resource::<Trace>().sequence, 1);
        begin(&mut world, Place::Field(340), Place::Battle);
        failed(&mut world);
        let trace = world.resource::<Trace>();
        assert_eq!(trace.last.as_ref().unwrap().outcome, "failed");
        assert_eq!(trace.sequence, 2);
        assert!(trace.pending.is_none());
        assert!(world.resource::<Frame>().0.is_none());
    }
}
