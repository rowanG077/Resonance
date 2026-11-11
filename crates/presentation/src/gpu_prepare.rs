//! Prepare the renderer's own pipelines without disposable draw entities.
mod observer;
use bevy::{
    asset::UntypedAssetId,
    material::key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey},
    pbr::{MeshPipelineKey, PreparedMaterial, ViewKeyCache, alpha_mode_pipeline_key},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, erased_render_asset::ErasedRenderAssets,
        mesh::RenderMesh, render_asset::RenderAssets,
    },
    sprite_render::{Mesh2dPipelineKey, PreparedMaterial2d, alpha_mode_pipeline_key_2d},
};
#[cfg(test)]
pub(crate) use observer::draws;
pub(crate) use observer::{
    Capture, Observer, Report, Shared, capture_submitted, rendered, shader_pending,
};
use std::sync::{Arc, Mutex, Weak, mpsc};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum View {
    Field,
    Camera(Entity),
}
impl From<Entity> for View {
    fn from(entity: Entity) -> Self {
        Self::Camera(entity)
    }
}

// Reuse the game's permanent cameras. Their keys are retained while loading
// freezes the live views; explicit sample overrides follow the current AA mode.
#[derive(Resource, Clone, Default)]
struct Cameras(Arc<std::sync::OnceLock<[Entity; 2]>>);
#[derive(Resource)]
struct ViewKeys {
    cameras: Cameras,
    model: Option<MeshPipelineKey>,
    quad: Option<Mesh2dPipelineKey>,
}
fn remember_cameras(
    cameras: Res<Cameras>,
    model: Query<Entity, With<crate::FieldCamera>>,
    quad: Query<Entity, With<crate::FieldOverlayCamera>>,
) {
    if cameras.0.get().is_none()
        && let (Ok(model), Ok(quad)) = (model.single(), quad.single())
    {
        let _ = cameras.0.set([model, quad]);
    }
}

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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Request {
    view: View,
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    flat: bool,
    samples: Option<u32>,
}
impl Report {
    pub fn prepare(
        &mut self,
        view: impl Into<View>,
        mesh: AssetId<Mesh>,
        material: UntypedAssetId,
        flat: bool,
        samples: Option<u32>,
    ) {
        self.direct = true;
        self.meshes.insert(mesh);
        self.material_bindings.insert((flat, material));
        let request = Request {
            view: view.into(),
            mesh,
            material,
            flat,
            samples,
        };
        // Early uploads and final scene admission share this report. Keep the
        // identity after specialization too, so later stages cannot requeue it.
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
        policy: Option<(resonance_content::diagnostics::Diagnostics, &'static str)>,
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
                        let policy = policy
                            .as_ref()
                            .map(|(diagnostics, scope)| (diagnostics, *scope));
                        let requests = std::mem::take(&mut report.requests);
                        for request in requests {
                            match specialize(world, &request) {
                                Ok(Some(pipeline)) => {
                                    report.pipelines.insert(pipeline);
                                }
                                Ok(None) => report.requests.push(request),
                                Err(error) => {
                                    report.failure(
                                        format!("GPU pipeline preparation: {error:#}"),
                                        policy,
                                    );
                                }
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
    let cameras = Cameras::default();
    let keys = ViewKeys {
        cameras: cameras.clone(),
        model: None,
        quad: None,
    };
    app.insert_resource(Registry(Some(sender)))
        .insert_resource(cameras)
        .add_systems(Update, remember_cameras);
    app.sub_app_mut(RenderApp)
        .insert_resource(keys)
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
    world.resource_scope(|world, mut keys: Mut<ViewKeys>| {
        if keys.model.is_some() && keys.quad.is_some() {
            return;
        }
        let Some(&[model, quad]) = keys.cameras.0.get() else {
            return;
        };
        let format = crate::renderer::scene_color_format(
            world.get_resource::<bevy::render::renderer::RenderAdapterInfo>(),
        );
        // First extraction may still name Bevy's fallback image format. Wait
        // for the permanent scene target rather than caching that fallback.
        keys.model = world
            .resource::<ViewKeyCache>()
            .iter()
            .find_map(|(id, key)| {
                (id.main_entity.id() == model
                    && id.subview_index == 0
                    && key.target_format() == format)
                    .then_some(*key)
            });
        keys.quad = world
            .resource::<bevy::sprite_render::ViewKeyCache>()
            .get(&bevy::render::sync_world::MainEntity::from(quad))
            .copied()
            .filter(|key| key.target_format() == format);
    });
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

fn specialize(
    world: &mut World,
    request: &Request,
) -> anyhow::Result<Option<bevy::render::render_resource::CachedRenderPipelineId>> {
    let Some(mesh) = world
        .resource::<RenderAssets<RenderMesh>>()
        .get(request.mesh)
    else {
        return Ok(None);
    };
    let layout = mesh.layout.clone();
    let bits = mesh.key_bits.bits();
    let format = crate::renderer::scene_color_format(
        world.get_resource::<bevy::render::renderer::RenderAdapterInfo>(),
    );
    let (properties, mesh_key) = if request.flat {
        let Some(view) = (match request.view {
            View::Field => world.resource::<ViewKeys>().quad,
            View::Camera(entity) => world
                .resource::<bevy::sprite_render::ViewKeyCache>()
                .get(&bevy::render::sync_world::MainEntity::from(entity))
                .copied(),
        }) else {
            return Ok(None);
        };
        // A newly created preparation camera can still expose the fallback
        // target format during its first extraction. Keep its jobs pending.
        if view.target_format() != format {
            return Ok(None);
        }
        let Some(material) = world
            .resource::<ErasedRenderAssets<PreparedMaterial2d>>()
            .get(request.material)
        else {
            return Ok(None);
        };
        let mut view = view;
        if let Some(samples) = request.samples {
            view.remove(Mesh2dPipelineKey::from_msaa_samples(view.msaa_samples()));
            view |= Mesh2dPipelineKey::from_msaa_samples(samples);
        }
        let mut material_bits: Mesh2dPipelineKey =
            material.properties.mesh_pipeline_key_bits.downcast();
        material_bits.insert(alpha_mode_pipeline_key_2d(material.properties.alpha_mode));
        (
            material.properties.clone(),
            ErasedMeshPipelineKey::new(
                view | Mesh2dPipelineKey::from_bits_retain(bits) | material_bits,
            ),
        )
    } else {
        let Some(view) = (match request.view {
            View::Field => world.resource::<ViewKeys>().model,
            View::Camera(entity) => {
                world
                    .resource::<ViewKeyCache>()
                    .iter()
                    .find_map(|(id, key)| {
                        (id.main_entity.id() == entity && id.subview_index == 0).then_some(*key)
                    })
            }
        }) else {
            return Ok(None);
        };
        if view.target_format() != format {
            return Ok(None);
        }
        let Some(material) = world
            .resource::<ErasedRenderAssets<PreparedMaterial>>()
            .get(request.material)
        else {
            return Ok(None);
        };
        let mut view = view;
        if let Some(samples) = request.samples {
            view.remove(MeshPipelineKey::from_msaa_samples(view.msaa_samples()));
            view |= MeshPipelineKey::from_msaa_samples(samples);
        }
        let mut material_bits: MeshPipelineKey =
            material.properties.mesh_pipeline_key_bits.downcast();
        material_bits.insert(alpha_mode_pipeline_key(
            material.properties.alpha_mode,
            &Msaa::from_samples(view.msaa_samples()),
        ));
        (
            material.properties.clone(),
            ErasedMeshPipelineKey::new(
                view | MeshPipelineKey::from_bits_retain(bits) | material_bits,
            ),
        )
    };
    let key = ErasedMaterialPipelineKey {
        type_id: request.material.type_id(),
        mesh_key,
        material_key: properties.material_key.clone(),
    };
    // Bevy's specializer already caches by material key and vertex layout.
    // The report still checks the current pipeline state before admission.
    profile_span!("pipeline specialization");
    let specialize = properties
        .base_specialize
        .ok_or_else(|| anyhow::anyhow!("material has no pipeline specializer"))?;
    Ok(Some(specialize(world, key, &layout, &properties)?))
}

#[cfg(all(test, feature = "preparation-trace"))]
pub(crate) fn audit_field_pipeline(
    world: &mut World,
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    flat: bool,
    samples: u32,
) -> anyhow::Result<Option<bevy::render::render_resource::CachedRenderPipelineId>> {
    let keys = world.resource::<ViewKeys>();
    let expected = crate::renderer::scene_color_format(
        world.get_resource::<bevy::render::renderer::RenderAdapterInfo>(),
    );
    assert_eq!(
        keys.model.unwrap().target_format(),
        expected,
        "captured fallback 3D format"
    );
    assert_eq!(
        keys.quad.unwrap().target_format(),
        expected,
        "captured fallback UI format"
    );
    specialize(
        world,
        &Request {
            view: View::Field,
            mesh,
            material,
            flat,
            samples: Some(samples),
        },
    )
}

#[cfg(all(test, feature = "preparation-trace"))]
pub(crate) fn audit_field_view(world: &World, flat: bool, samples: u32) -> String {
    let keys = world.resource::<ViewKeys>();
    if flat {
        let mut key = keys.quad.unwrap();
        key.remove(Mesh2dPipelineKey::from_msaa_samples(key.msaa_samples()));
        key |= Mesh2dPipelineKey::from_msaa_samples(samples);
        format!("{:#x} ({key:?})", key.bits())
    } else {
        let mut key = keys.model.unwrap();
        key.remove(MeshPipelineKey::from_msaa_samples(key.msaa_samples()));
        key |= MeshPipelineKey::from_msaa_samples(samples);
        format!("{:#x} ({key:?})", key.bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_unique_across_stages_and_keep_distinct_draw_inputs() {
        let mut world = World::new();
        let camera = world.spawn_empty().id();
        let mesh = AssetId::<Mesh>::default();
        let material = AssetId::<crate::materials::TitleSurface>::default().untyped();
        let mut report = Report::default();
        report.prepare(View::Field, mesh, material, false, Some(1));
        report.prepare(View::Field, mesh, material, false, Some(1));
        assert_eq!(report.requests.len(), 1);
        // Specialization drains the pending queue. A final admission fence still
        // needs the existing pipeline, without repeating its preparation.
        report.requests.clear();
        report.restart_submission(false);
        report.prepare(View::Field, mesh, material, false, Some(1));
        assert!(report.requests.is_empty());
        let mut meshes = Assets::<Mesh>::default();
        let other_mesh = meshes.add(Mesh::from(Rectangle::default())).id();
        let mut surfaces = Assets::<crate::materials::TitleSurface>::default();
        let other_material = surfaces
            .add(crate::materials::TitleSurface::default())
            .id()
            .untyped();
        let base = Request {
            view: View::Field,
            mesh,
            material,
            flat: false,
            samples: Some(1),
        };
        for request in [
            Request {
                view: View::Camera(camera),
                ..base
            },
            Request { flat: true, ..base },
            Request {
                samples: Some(4),
                ..base
            },
            Request {
                mesh: other_mesh,
                ..base
            },
            Request {
                material: other_material,
                ..base
            },
        ] {
            report.prepare(
                request.view,
                request.mesh,
                request.material,
                request.flat,
                request.samples,
            );
        }
        assert_eq!(report.requests.len(), 5);
        assert_eq!(report.requested.len(), 6);
    }
}
