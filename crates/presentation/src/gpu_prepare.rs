//! Prepare the native renderer's pipelines before their draws are visible.
mod observer;
use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems},
};
pub(crate) use observer::{
    Capture, Observer, Report, Shared, capture_submitted, rendered, shader_pending,
};
use std::sync::{Arc, Mutex, Weak, mpsc};

struct Job {
    prepare: Box<dyn Fn(&mut World) -> bool + Send + Sync>,
    observe: Box<dyn Fn(&World, &Observer) + Send + Sync>,
}
#[derive(Resource, Clone, Default)]
pub(crate) struct Registry(Option<mpsc::Sender<Job>>);
#[derive(Resource)]
struct Jobs {
    incoming: Mutex<mpsc::Receiver<Job>>,
    live: Vec<Job>,
    observer: Option<bevy::ecs::system::SystemState<Observer<'static>>>,
}

/// The view a prepared draw will appear in, which decides its fog: field and
/// world cameras always carry linear fog, the battle camera none.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum View {
    Field,
    Battle,
}

/// A draw a scene will show: its view, mesh layout (none for UI) and material, whether its
/// geometry is streamed, and an explicit sample count where the scene pass's
/// may change.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Request {
    pub view: View,
    pub mesh: Option<AssetId<Mesh>>,
    pub material: crate::native::Prepare,
    pub streamed: bool,
    pub samples: Option<u32>,
}
impl Report {
    pub fn prepare(
        &mut self,
        view: View,
        mesh: impl Into<Option<AssetId<Mesh>>>,
        material: impl Into<crate::native::Prepare>,
        streamed: bool,
        samples: Option<u32>,
    ) {
        self.direct = true;
        let mesh = mesh.into();
        self.meshes.extend(mesh);
        let request = Request {
            view,
            mesh,
            material: material.into(),
            streamed,
            samples,
        };
        // Early uploads and final scene admission share this report. Keep the
        // identity after preparation too, so later stages cannot requeue it.
        if self.requested.insert(request) {
            self.requests.push(request);
        }
    }
}
impl Registry {
    pub fn watch<T: Send + Sync + 'static>(
        &self,
        source: &Arc<Mutex<T>>,
        report: fn(&mut T) -> &mut Report,
        observe: impl Fn(&mut T, &World, &Observer) + Send + Sync + 'static,
    ) {
        let source: Weak<Mutex<T>> = Arc::downgrade(source);
        if let Some(sender) = &self.0 {
            let observed = source.clone();
            let job = Job {
                observe: Box::new(move |world, observer| {
                    if let Some(source) = observed.upgrade() {
                        observe(&mut source.lock().unwrap(), world, observer);
                    }
                }),
                prepare: Box::new(move |world| {
                    let Some(source) = source.upgrade() else {
                        return false;
                    };
                    let mut source = source.lock().unwrap();
                    let report = report(&mut source);
                    if !report.submitted {
                        let requests = std::mem::take(&mut report.requests);
                        for request in requests {
                            // Errors surface later through the pipeline state.
                            match crate::native::render::prepare_pipeline(world, &request) {
                                Some(pipeline) => {
                                    report.pipelines.insert(pipeline);
                                }
                                None => report.requests.push(request),
                            }
                        }
                    }
                    true
                }),
            };
            let _ = sender.send(job);
        }
    }
}

pub(crate) fn install(app: &mut App) {
    if app.get_sub_app(RenderApp).is_none() || app.world().contains_resource::<Registry>() {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    app.insert_resource(Registry(Some(sender)));
    app.sub_app_mut(RenderApp)
        .insert_resource(Jobs {
            incoming: Mutex::new(receiver),
            live: Vec::new(),
            observer: None,
        })
        .add_systems(
            Render,
            update
                .after(RenderSystems::Render)
                .before(RenderSystems::Cleanup),
        );
}
fn update(world: &mut World) {
    profile_span!("pipeline registry");
    let mut jobs = world.remove_resource::<Jobs>().unwrap();
    let incoming: Vec<_> = jobs.incoming.get_mut().unwrap().try_iter().collect();
    jobs.live.extend(incoming);
    jobs.live.retain(|job| (job.prepare)(world));
    let observer = jobs
        .observer
        .get_or_insert_with(|| bevy::ecs::system::SystemState::new(world));
    let observer = observer.get(world).expect("GPU observer resources");
    for job in &jobs.live {
        (job.observe)(world, &observer);
    }
    world.insert_resource(jobs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_unique_across_stages_and_keep_distinct_draw_inputs() {
        let mesh = AssetId::<Mesh>::default();
        let material = crate::native::Prepare::from(&crate::materials::TitleSurface::default());
        let mut report = Report::default();
        report.prepare(View::Field, mesh, material, false, Some(1));
        report.prepare(View::Field, mesh, material, false, Some(1));
        assert_eq!(report.requests.len(), 1);
        // Preparation drains the pending queue. A final admission fence still
        // needs the existing pipeline, without repeating its preparation.
        report.requests.clear();
        report.restart_submission(false);
        report.prepare(View::Field, mesh, material, false, Some(1));
        assert!(report.requests.is_empty());
        let mut meshes = Assets::<Mesh>::default();
        let other_mesh = meshes.add(Mesh::from(Rectangle::default())).id();
        let other_material = crate::native::Prepare::from(&crate::materials::TitleSurface {
            depth_write: false,
            ..default()
        });
        let base = Request {
            view: View::Field,
            mesh: Some(mesh),
            material,
            streamed: false,
            samples: Some(1),
        };
        for request in [
            Request {
                streamed: true,
                ..base
            },
            Request {
                view: View::Battle,
                ..base
            },
            Request {
                samples: Some(4),
                ..base
            },
            Request {
                mesh: Some(other_mesh),
                ..base
            },
            Request {
                material: other_material,
                ..base
            },
            Request {
                material: crate::native::Prepare::SceneSample {
                    masked: false,
                    depth_test: true,
                },
                ..base
            },
        ] {
            report.prepare(
                request.view,
                request.mesh,
                request.material,
                request.streamed,
                request.samples,
            );
        }
        assert_eq!(report.requests.len(), 6);
        assert_eq!(report.requested.len(), 7);
    }
}
