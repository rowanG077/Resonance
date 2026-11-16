//! Prepare the renderer's own pipelines before a field is visible.
use super::{field_view::Art, gpu_prepare as gpu, loading::Resident, materials::TitleSurface};
use crate::native::{MeshMaterial2d, MeshMaterial3d};
use bevy::{ecs::system::SystemParam, prelude::*};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::Ordering},
    time::Instant,
};

#[derive(Resource, Clone, Default)]
pub(super) struct Shared(Arc<Mutex<Report>>);
#[derive(Default)]
struct Report {
    map: Option<u32>,
    draws: gpu::Report,
}
#[derive(Resource, Clone)]
struct Preparation {
    map: u32,
    started: Instant,
    // Prefetch has no live artwork yet. A final submission belongs to one model revision.
    model_revision: Option<u64>,
    samples: u32,
}
#[derive(SystemParam)]
struct Gpu<'w, 's> {
    images: Res<'w, Assets<Image>>,
    surfaces: Res<'w, Assets<TitleSurface>>,
    cameras: Query<'w, 's, &'static Msaa, With<super::FieldCamera>>,
}
impl Preparation {
    fn retire(&self, commands: &mut Commands) {
        commands.remove_resource::<Preparation>();
    }
}

pub(super) fn install(app: &mut App) {
    let shared = Shared::default();
    let resident = app.world().resource::<Resident>().clone();
    app.world()
        .resource::<crate::gpu_prepare::Registry>()
        .watch(&shared.0, |report| &mut report.draws, {
            let resident = resident.clone();
            move |report, _, observer| rendered(report, &resident, observer)
        });
    app.insert_resource(shared.clone())
        .add_systems(
            Update,
            (retire, resolve_prefetch, begin, complete, guard)
                .chain()
                .after(super::field_view::FieldPreparation)
                .run_if(super::battle::field_running),
        )
        .add_systems(
            Last,
            capture_frame
                .after(crate::native::Compile)
                .run_if(super::battle::field_running),
        );
    app.sub_app_mut(bevy::render::RenderApp)
        .insert_resource(resident);
}

/// Standalone captures have no live scene owner, but their scripts can create
/// any loaded model after settling. Prepare those layouts before advancing time.
pub(super) fn install_capture(app: &mut App) {
    let shared = Shared::default();
    app.world().resource::<gpu::Registry>().watch(
        &shared.0,
        |report| &mut report.draws,
        |report, _, observer| observer.render(&mut report.draws, None),
    );
    app.insert_resource(shared).add_systems(
        Update,
        prepare_capture.after(super::field_view::FieldPreparation),
    );
}

fn prepare_capture(
    mut commands: Commands,
    art: Option<ResMut<Art>>,
    preparation: Option<Res<Preparation>>,
    shared: Res<Shared>,
    gpu: Gpu,
    mut exit: crate::diagnostics::Fatal,
) {
    let Some(mut art) = art.filter(|art| art.ready) else {
        return;
    };
    let samples = gpu.cameras.single().map_or(1, Msaa::samples);
    let mut report = shared.0.lock().unwrap();
    if preparation.is_none_or(|p| {
        p.map != art.map || p.samples != samples || p.model_revision != Some(art.model_revision)
    }) {
        art.gpu = None;
        let preparation = Preparation {
            map: art.map,
            started: Instant::now(),
            model_revision: Some(art.model_revision),
            samples,
        };
        *report = Report {
            map: Some(art.map),
            ..default()
        };
        preparation.models(art.models.values().flatten(), &mut report);
        report.draws.armed = true;
        commands.insert_resource(preparation);
    }
    if let Some(error) = &report.draws.error {
        exit.fail("field capture preparation", anyhow::anyhow!(error.clone()));
    } else if report.draws.completed.load(Ordering::Acquire) {
        art.gpu = Some(samples);
    }
}

fn retire(
    mut commands: Commands,
    session: Option<Res<super::new_game::Session>>,
    pending: Option<Res<super::loading::Pending>>,
    resident: Res<Resident>,
    preparation: Option<Res<Preparation>>,
    shared: Res<Shared>,
    field: Option<Res<super::loading::FieldPending>>,
    art: Option<Res<Art>>,
) {
    if preparation.as_ref().is_some_and(|preparation| {
        preparation.model_revision.is_none()
            && field
                .as_ref()
                .is_some_and(|pending| pending.map == preparation.map)
    }) {
        return; // The source can be an overworld scene while its field destination warms.
    }
    let stale = preparation.as_ref().is_some_and(|preparation| {
        if let Some(revision) = preparation.model_revision {
            return art
                .as_ref()
                .is_none_or(|art| art.map != preparation.map || art.model_revision != revision);
        }
        !session
            .as_ref()
            .is_some_and(|session| session.is_field() && session.map_id() == preparation.map)
    });
    if !stale && (session.as_ref().is_some_and(|s| s.is_field()) || pending.is_some()) {
        return;
    }
    if session.is_none() {
        resident.active.store(false, Ordering::Release);
        // The title also owns prepared files without a gameplay session. Scene
        // transitions own that snapshot; retiring field GPU work must keep it.
    }
    if let Some(preparation) = preparation {
        preparation.retire(&mut commands);
    }
    if shared.0.lock().unwrap().map.is_some() {
        *shared.0.lock().unwrap() = Report::default();
    }
}

impl Preparation {
    fn models<'a>(
        &self,
        models: impl Iterator<Item = &'a super::field_view::Part>,
        report: &mut Report,
    ) {
        profile_span!("field pipeline requests");
        let mut groups = HashMap::new();
        for part in models {
            report.draws.images.extend(
                part.materials
                    .iter()
                    .flat_map(|material| material.sampled.iter().flatten().map(Handle::id)),
            );
            for draw in part
                .layout
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter_map(|node| node.draw.as_ref())
            {
                let spec = &part.spec.materials[draw.slot.0];
                if spec.image == resonance_content::SceneImage::Capture {
                    // The sampled depth texture has single- and multisampled
                    // layouts; prepare both before a particle's first frame.
                    for samples in [1, 2] {
                        report.draws.prepare(
                            gpu::View::Field,
                            draw.mesh.id(),
                            crate::native::Prepare::SceneSample {
                                masked: false,
                                depth_test: true,
                            },
                            false,
                            Some(samples),
                        );
                    }
                }
                let constant = part.spec.outline_color_for(spec).is_some();
                let materials = groups
                    .entry((constant, spec.cull, spec.blend))
                    .or_insert_with(|| {
                        let actor = TitleSurface {
                            constant_color: constant,
                            cull: spec.cull,
                            ..default()
                        };
                        model_variants(&actor, spec.blend)
                            .into_iter()
                            .map(|surface| crate::native::Prepare::from(&surface))
                            .collect::<Vec<_>>()
                    });
                for material in materials.iter() {
                    report.draws.prepare(
                        crate::gpu_prepare::View::Field,
                        draw.mesh.id(),
                        *material,
                        false,
                        Some(self.samples),
                    );
                }
            }
        }
    }
}

// Scripts can draw any loaded model as an actor or a particle. Enumerate the
// structural material keys once; textures and lighting share those pipelines.
fn model_variants(surface: &TitleSurface, blended: bool) -> Vec<TitleSurface> {
    use resonance_events::effect::Blend;
    let mut variants = Vec::new();
    for depth_write in [false, true] {
        let base = TitleSurface {
            blend: blended.then_some(Blend::Alpha),
            depth_write,
            ..surface.clone()
        };
        variants.push(base.clone());
        for blend in Blend::ACTOR_MODES {
            for cull in [base.cull, resonance_content::CullFace::None] {
                variants.push(TitleSurface {
                    blend: Some(blend),
                    cull,
                    ..base.clone()
                });
            }
        }
        for blend in [Blend::Alpha, Blend::Additive, Blend::Subtractive] {
            let mut surface = surface.clone();
            super::field_model_particles::material(&mut surface, blend);
            surface.depth_write = depth_write;
            variants.push(surface);
        }
    }
    variants
}

fn resolve_prefetch(
    graphics: Option<ResMut<super::loading::FieldPrefetch>>,
    pending: Option<Res<super::loading::FieldPending>>,
    session: Option<Res<super::new_game::Session>>,
    mut assets: super::model_asset::ModelAssets,
) {
    let Some(mut graphics) = graphics else { return };
    if graphics.disabled
        || !(pending.is_some_and(|pending| pending.map == graphics.map)
            || session
                .is_some_and(|session| session.is_field() && session.map_id() == graphics.map))
    {
        return;
    }
    let resolved = (|| -> anyhow::Result<_> {
        let mut changed = Vec::new();
        for (&resource, parts) in &mut graphics.models {
            for (index, part) in parts.iter_mut().enumerate() {
                anyhow::ensure!(
                    part.load_error(&assets.server).is_none(),
                    "early field model failed"
                );
                let unresolved = part.layout.is_none();
                if part.resolve(&mut assets)? && unresolved {
                    changed.push((resource, index));
                }
            }
        }
        Ok(changed)
    })();
    match resolved {
        Ok(changed) => graphics.changed.extend(changed),
        Err(_) => graphics.disabled = true, // Full capture keeps its normal error policy.
    }
}

#[allow(clippy::too_many_arguments)]
fn begin(
    mut commands: Commands,
    art: Option<Res<Art>>,
    effects: Option<Res<super::field_effects::Artwork>>,
    mut graphics: Option<ResMut<super::loading::FieldPrefetch>>,
    pending: Option<Res<super::loading::FieldPending>>,
    mut preparation: Option<ResMut<Preparation>>,
    resident: Res<Resident>,
    shared: Res<Shared>,
    gpu: Gpu,
    ui: Option<Res<super::field_ui::Artwork>>,
    ui_materials: Res<Assets<super::field_ui::Surface>>,
    server: Res<AssetServer>,
    mut failures: super::field_view::Failures,
    session: Option<Res<super::new_game::Session>>,
    backdrop: Query<&Mesh2d, With<super::menu_backdrop::Quad>>,
) {
    let art = art.filter(|art| {
        art.ready
            && session.as_ref().is_some_and(|session| {
                session.is_field()
                    && session.map_id() == art.map
                    && session.events().world.field_transition.is_none()
                    && session.events().world.world_transition.is_none()
            })
    });
    let samples = gpu.cameras.single().map_or(1, Msaa::samples);
    if let Some(art) = &art
        && (resident.active.load(Ordering::Acquire)
            || preparation
                .as_ref()
                .is_some_and(|p| p.map == art.map && p.model_revision.is_some()))
    {
        return;
    }
    let early = art.is_none();
    let (map, models, toon) = match &art {
        Some(art) => (art.map, &art.models, &art.toon_ramp),
        None => {
            let Some(graphics) = graphics.as_ref().filter(|graphics| {
                !graphics.changed.is_empty()
                    && !graphics.disabled
                    && gpu.images.contains(&graphics.toon_ramp)
                    && preparation
                        .as_ref()
                        .is_none_or(|p| p.model_revision.is_none() && p.map == graphics.map)
                    && (pending
                        .as_ref()
                        .is_some_and(|pending| pending.map == graphics.map)
                        || session.as_ref().is_some_and(|session| {
                            session.is_field() && session.map_id() == graphics.map
                        }))
            }) else {
                return;
            };
            (graphics.map, &graphics.models, &graphics.toon_ramp)
        }
    };
    let backdrop = if early {
        None
    } else {
        let Some(ui) = &ui else { return };
        if effects.is_none() {
            return;
        }
        match ui.essential_ready(&gpu.images, &server) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                failures.fatal("field essential artwork", error);
                return;
            }
        }
        let Ok(mesh) = backdrop.single() else {
            return;
        };
        Some(mesh.clone())
    };
    let staged = preparation
        .as_deref_mut()
        .filter(|p| p.map == map && p.model_revision.is_none());
    let (mut preparation, mut report) = match staged {
        Some(staged) => (
            staged.clone(),
            std::mem::take(&mut *shared.0.lock().unwrap()),
        ),
        None => (
            Preparation {
                map,
                model_revision: None,
                samples,
                started: Instant::now(),
            },
            Report {
                map: Some(map),
                ..Default::default()
            },
        ),
    };
    preparation.model_revision = art.as_ref().map(|art| art.model_revision);
    preparation.started = Instant::now();
    // Early uploads can finish independently. Final admission also needs the
    // newly instantiated scene's materials and actual draws, captured below.
    report.draws.restart_submission(early);
    if let Some(backdrop_mesh) = backdrop {
        let art = art.as_ref().unwrap();
        let ui = ui.as_ref().unwrap();
        // Loading may freeze the live cameras until preparation finishes. Warm the
        // menu backdrop offscreen too, so readiness never depends on that frozen view.
        report.draws.prepare(
            crate::gpu_prepare::View::Field,
            backdrop_mesh.0.id(),
            crate::native::Prepare::Backdrop,
            false,
            Some(samples),
        );
        if let Some(panorama) = &art.panorama
            && let Some(draw) = &panorama.draw
        {
            report.draws.prepare(
                crate::gpu_prepare::View::Field,
                draw.mesh.id(),
                gpu.surfaces
                    .get(&draw.material)
                    .expect("prepared panorama material"),
                false,
                Some(samples),
            );
            report.draws.images.insert(panorama.image.id());
        }
        for (material, essential) in ui.prepared_layers() {
            // Optional pages may still be loading, or may never be selected. Their
            // missing textures must not enter the mandatory submission fence.
            if !essential
                && !ui_materials
                    .get(material)
                    .is_some_and(|surface| surface.images_ready(&gpu.images))
            {
                continue;
            }
            report.draws.prepare(
                crate::gpu_prepare::View::Field,
                None,
                ui_materials
                    .get(material)
                    .expect("prepared field UI material"),
                true,
                Some(samples),
            );
        }
    }
    if !early {
        // The loader creates these immutable bindings before Art becomes ready.
        // They share the models/UI upload, specialization and submission gate.
        for (mesh, material) in effects
            .as_ref()
            .unwrap()
            .prepared_bindings()
            .chain(art.as_ref().and_then(|a| a.shadow_binding()))
        {
            report.draws.prepare(
                crate::gpu_prepare::View::Field,
                mesh.id(),
                gpu.surfaces
                    .get(&material)
                    .expect("prepared field effect material"),
                false,
                Some(samples),
            );
        }
    }
    report.draws.images.insert(toon.id());
    if early || art.as_ref().is_some_and(|art| art.gpu != Some(samples)) {
        preparation.models(
            models.iter().flat_map(|(&resource, parts)| {
                let graphics = &graphics;
                parts.iter().enumerate().filter_map(move |(index, part)| {
                    (part.layout.is_some()
                        && (!early
                            || graphics
                                .as_ref()
                                .unwrap()
                                .changed
                                .contains(&(resource, index))))
                    .then_some(part)
                })
            }),
            &mut report,
        );
    }
    if early {
        graphics.as_mut().unwrap().changed.clear();
    }
    let count = report.draws.requested.len();
    *shared.0.lock().unwrap() = report;
    if !early {
        commands.remove_resource::<super::loading::FieldPrefetch>();
        info!("Field {} preparing {} pipeline bindings", map, count);
    }
    commands.insert_resource(preparation);
}

fn capture_frame(world: &mut World) {
    let Some(preparation) = world.get_resource::<Preparation>() else {
        return;
    };
    if preparation.model_revision.is_none() {
        return;
    }
    let shared = world.resource::<Shared>().clone();
    let mut report = shared.0.lock().unwrap();
    if report.draws.armed {
        forget_hidden_draws(world, &mut report.draws);
        return;
    }
    if !super::field_view::ready(world) {
        return;
    }
    // Waiting on the current bindings matters even when geometry and
    // pipelines were retained.
    if capture_draws(world, &mut report.draws) {
        report.draws.armed = true;
        forget_hidden_draws(world, &mut report.draws);
    }
}

/// An entry script can hide the captured opening pose until the field is
/// admitted, and admission waits on this preparation: a captured draw that is
/// hidden or gone is not waited on. The requested pipelines still are.
fn forget_hidden_draws(world: &World, report: &mut gpu::Report) {
    if report.completed.load(Ordering::Acquire) {
        return;
    }
    report.expected.retain(|entity| {
        world
            .get::<InheritedVisibility>(entity.id())
            .is_some_and(|visibility| visibility.get())
    });
}

/// Wait on the draws of the held opening pose: what each active field camera
/// (layers 0 and 3) sees in the current compiled frame, including unresolved
/// materials. False until a 3D field
/// camera has been compiled; cameras the native frame never draws are skipped.
fn capture_draws(world: &mut World, report: &mut gpu::Report) -> bool {
    use bevy::camera::visibility::RenderLayers;

    let layers = RenderLayers::from_layers(&[0, 3]);
    let cameras: Vec<(Entity, bool)> = world
        .query_filtered::<(Entity, &Camera, Option<&RenderLayers>, Has<Camera3d>), Or<(With<Camera3d>, With<Camera2d>)>>()
        .iter(world)
        // The compiler already selected active cameras and removed views
        // covered by a later clearing camera.
        .filter(|(_, _, render_layers, _)| {
            render_layers
                .unwrap_or(&Default::default())
                .intersects(&layers)
        })
        .map(|(entity, _, _, model)| (entity, model))
        .collect();
    let compiled = world.resource::<crate::native::Compiled>();
    if !cameras
        .iter()
        .any(|(camera, model)| *model && compiled.views.contains_key(camera))
    {
        return false;
    }
    let draws: Vec<(Entity, bool)> = cameras
        .iter()
        .filter_map(|(camera, model)| Some((compiled.views.get(camera)?, *model)))
        .flat_map(|(draws, model)| draws.iter().map(move |entity| (*entity, model)))
        .collect();
    for (entity, model) in draws {
        if model {
            if let Some(mesh) = world
                .get::<Mesh3d>(entity)
                .filter(|_| world.get::<MeshMaterial3d<TitleSurface>>(entity).is_some())
            {
                report.meshes.insert(mesh.0.id());
                report.expected.insert(entity.into());
            }
        } else if world
            .get::<MeshMaterial2d<super::field_ui::Surface>>(entity)
            .is_some()
        {
            report.expected.insert(entity.into());
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn complete(
    mut commands: Commands,
    preparation: Option<Res<Preparation>>,
    shared: Res<Shared>,
    resident: Res<Resident>,
    refraction: Res<super::field_refraction::Ready>,
    mut failures: super::field_view::Failures,
    art: Option<ResMut<Art>>,
    cameras: Query<&Msaa, With<super::FieldCamera>>,
    names: Query<NameOrEntity>,
) {
    let Some(preparation) = preparation else {
        return;
    };
    let report = shared.0.lock().unwrap();
    if let Some(error) = &report.draws.error {
        failures.fatal("field preparation", anyhow::anyhow!(error.clone()));
        return;
    }
    let samples = cameras.single().map_or(1, Msaa::samples);
    if samples != preparation.samples {
        preparation.retire(&mut commands);
        return;
    }
    if preparation.model_revision.is_none() {
        return;
    }
    if report.draws.completed.load(Ordering::Acquire) && refraction.get() {
        info!(
            "Field {} GPU preparation complete in {:.3}s: {} draws, {} pipelines",
            preparation.map,
            preparation.started.elapsed().as_secs_f64(),
            report.draws.requested.len(),
            report.draws.pipelines.len()
        );
        resident.active.store(true, Ordering::Release);
        if let Some(mut art) = art.filter(|art| art.map == preparation.map) {
            art.gpu = Some(preparation.samples);
            art.retaining_output = false;
        }
        preparation.retire(&mut commands);
    } else if preparation.started.elapsed().as_secs() > 120 {
        let waiting: Vec<_> = report
            .draws
            .pending
            .iter()
            .take(4)
            .map(|entity| {
                names
                    .get(entity.id())
                    .map_or_else(|_| format!("{}", entity.id()), |name| format!("{name}"))
            })
            .collect();
        failures.fatal(
            "field preparation",
            anyhow::anyhow!(
                "Field {} GPU preparation timed out: {} requests left, {} of {} draws not drawn \
                 yet (such as {}), submitted {}, refraction ready {}",
                preparation.map,
                report.draws.requests.len(),
                report.draws.pending.len(),
                report.draws.expected.len(),
                waiting.join(", "),
                report.draws.submitted,
                refraction.get()
            ),
        );
    }
}

fn guard(shared: Res<Shared>, resident: Res<Resident>, mut exit: crate::diagnostics::Fatal) {
    let report = shared.0.lock().unwrap();
    let failure = report.draws.error.clone().or_else(|| {
        (resident.unprepared_reads.load(Ordering::Relaxed) > 0)
            .then(|| format!("field {:?} attempted an undeclared asset read", report.map))
    });
    if let Some(error) = failure
        && let Err(error) = resident
            .diagnostics
            .report("field GPU", anyhow::anyhow!(error))
    {
        exit.fail("field GPU", error);
    }
}

fn rendered(report: &mut Report, resident: &Resident, observer: &gpu::Observer) {
    if report.map.is_none() || resident.battle.load(Ordering::Acquire) {
        return;
    }
    if resident.active.load(Ordering::Acquire) {
        if let Some(error) = observer.failed(&report.draws)
            && resident
                .diagnostics
                .report("field draw pipeline", anyhow::anyhow!(error.clone()))
                .is_err()
        {
            report.draws.error = Some(error);
        }
    } else {
        observer.render(
            &mut report.draws,
            Some((&resident.diagnostics, "field draw pipeline")),
        );
    }
}

#[cfg(test)]
pub(super) fn begin_test_startup(world: &mut World) -> bool {
    use bevy::ecs::system::RunSystemOnce;
    world.run_system_once(begin).unwrap();
    world.contains_resource::<Preparation>()
}

/// CPU tests acknowledge only the GPU completion; activation still runs through
/// the production preparation owner and its completion system.
#[cfg(test)]
pub(super) fn complete_test_startup(world: &mut World) {
    use bevy::ecs::system::RunSystemOnce;
    world
        .resource::<Shared>()
        .0
        .lock()
        .unwrap()
        .draws
        .completed
        .store(true, Ordering::Release);
    world.insert_resource(super::field_refraction::Ready::completed());
    world.run_system_once(complete).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    #[ignore = "requires a GPU; verifies admission cannot omit unresolved visible materials"]
    fn opening_frame_waits_for_visible_materials_before_admission() -> anyhow::Result<()> {
        use bevy::{
            camera::{
                RenderTarget, ScalingMode,
                visibility::{NoFrustumCulling, RenderLayers},
            },
            render::RenderApp,
        };
        let mut app = App::new();
        app.add_plugins(crate::test_support::render_plugins());
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    32,
                    32,
                    crate::renderer::SCENE_COLOR_FORMAT,
                    None,
                ));
        crate::test_support::native_scene(&mut app, target.clone());
        gpu::install(&mut app);
        let shared = gpu::Shared::new(app.world());
        app.insert_resource(shared.clone()).add_systems(
            Last,
            (|world: &mut World| {
                let shared = world.resource::<gpu::Shared>().clone();
                let mut report = shared.0.lock().unwrap();
                if !report.submitted && capture_draws(world, &mut report) {
                    report.armed = true;
                    forget_hidden_draws(world, &mut report);
                }
            })
            .after(crate::native::Compile),
        );
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(1., 1.));
        let ready = app
            .world_mut()
            .resource_mut::<Assets<TitleSurface>>()
            .add(TitleSurface {
                constant_color: true,
                tint: Vec4::new(0., 0., 1., 1.),
                ..default()
            });
        let missing = app
            .world()
            .resource::<Assets<TitleSurface>>()
            .reserve_handle();
        let missing_ui = app
            .world()
            .resource::<Assets<crate::field_ui::Surface>>()
            .reserve_handle();
        let healthy = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(ready),
                Transform::from_xyz(-0.5, 0., 0.),
                NoFrustumCulling,
            ))
            .id();
        let model = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(missing.clone()),
                Transform::from_xyz(0.5, 0., 0.),
                NoFrustumCulling,
            ))
            .id();
        let other_layer = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(missing.clone()),
                Transform::IDENTITY,
                RenderLayers::layer(9),
                NoFrustumCulling,
            ))
            .id();
        let quad = app
            .world_mut()
            .spawn((
                crate::field_ui::UiGeometry::default(),
                MeshMaterial2d(missing_ui),
                NoFrustumCulling,
            ))
            .id();
        app.world_mut().spawn((
            Camera3d::default(),
            Camera {
                clear_color: Color::BLACK.into(),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: 2.,
                    height: 2.,
                },
                ..OrthographicProjection::default_3d()
            }),
            RenderTarget::Image(target.clone().into()),
            Transform::from_xyz(0., 0., 4.).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        app.world_mut().spawn((
            Camera2d,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
        ));
        crate::test_support::finish_app(&mut app)?;
        crate::test_support::render_until(&mut app, "healthy draw did not render", |app| {
            let world = app.sub_app(RenderApp).world();
            world
                .resource::<crate::native::Drawn>()
                .draws
                .iter()
                .any(|(entity, pipeline)| {
                    *entity == healthy.into()
                        && world
                            .resource::<bevy::render::render_resource::PipelineCache>()
                            .get_render_pipeline(*pipeline)
                            .is_some()
                })
        })?;
        for _ in 0..4 {
            app.update();
        }
        {
            let report = shared.0.lock().unwrap();
            assert_eq!(
                report.expected,
                [healthy.into(), model.into(), quad.into()].into()
            );
            assert_eq!(report.pending, [model.into(), quad.into()].into());
            assert!(!report.expected.contains(&other_layer.into()));
            assert!(!report.submitted && !report.completed.load(Ordering::Acquire));
        }
        // A now-hidden draw must stop blocking admission. The remaining visible
        // material must actually publish, upload and render before it completes.
        app.world_mut().entity_mut(quad).insert(Visibility::Hidden);
        app.world_mut()
            .resource_mut::<Assets<TitleSurface>>()
            .insert(
                missing.id(),
                TitleSurface {
                    constant_color: true,
                    tint: Vec4::new(1., 0., 0., 1.),
                    ..default()
                },
            )?;
        crate::test_support::render_until(
            &mut app,
            "ready opening frame did not complete",
            |_| shared.0.lock().unwrap().completed.load(Ordering::Acquire),
        )?;
        let pixels = crate::test_support::capture_image(&mut app, target)?
            .try_into_dynamic()?
            .to_rgba8();
        assert_eq!(pixels.get_pixel(8, 16).0, [0, 0, 255, 255]);
        assert_eq!(pixels.get_pixel(24, 16).0, [255, 0, 0, 255]);
        Ok(())
    }

    #[test]
    fn captured_draws_hidden_or_removed_before_admission_are_not_awaited() {
        let mut world = World::new();
        let shown = world.spawn(InheritedVisibility::VISIBLE).id();
        let hidden = world.spawn(InheritedVisibility::HIDDEN).id();
        let removed = world.spawn(InheritedVisibility::VISIBLE).id();
        world.despawn(removed);
        let mut report = gpu::Report {
            expected: [shown.into(), hidden.into(), removed.into()].into(),
            ..Default::default()
        };
        forget_hidden_draws(&world, &mut report);
        assert_eq!(report.expected, [shown.into()].into());

        // A completed report keeps what it waited on.
        report.expected.insert(hidden.into());
        report.completed.store(true, Ordering::Release);
        forget_hidden_draws(&world, &mut report);
        assert_eq!(report.expected.len(), 2);
    }

    #[test]
    #[ignore = "requires locally cooked field assets; CPU simulation of GPU acknowledgements"]
    fn background_only_field_holds_until_the_latest_model_submission() -> anyhow::Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            Into::into,
        );
        let package =
            crate::new_game::FieldPackage::prepare(&root, 332, &mut Default::default(), || false)?;
        let checkpoint = crate::test_support::field_checkpoint(&package.files)?;
        let mut session =
            crate::new_game::Session::load_prepared(Arc::new(package), Some(checkpoint), None)?;
        session.events_mut().world.actors.clear();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<Assets<crate::materials::TitleOutput>>()
            .init_resource::<Shared>();
        let resident = Resident::default();
        let art = crate::field_view::prepared_test_art(
            session.field_assets(),
            app.world().resource::<AssetServer>(),
        );
        app.insert_resource(art)
            .insert_resource(session)
            .insert_resource(resident.clone());
        let world = app.world_mut();
        let camera = world
            .spawn((Camera::default(), crate::FieldOverlayCamera))
            .id();
        let output = world
            .resource_mut::<Assets<crate::materials::TitleOutput>>()
            .add(crate::materials::TitleOutput {
                source: default(),
                overlay: None,
                brightness: Vec4::X,
            });
        let check_hold = |world: &mut World, retain, black| {
            world.run_system_once(crate::loading::black_hold).unwrap();
            assert_eq!(
                matches!(
                    world.get::<Camera>(camera).unwrap().output_mode,
                    bevy::camera::CameraOutputMode::Skip
                ),
                retain
            );
            let material = world
                .resource::<Assets<crate::materials::TitleOutput>>()
                .get(&output)
                .unwrap();
            assert_eq!(material.brightness.z, f32::from(black));
        };
        let submit = |world: &mut World| {
            let art = world.resource::<Art>();
            *world.resource::<Shared>().0.lock().unwrap() = Report {
                map: Some(art.map),
                ..default()
            };
            world.insert_resource(Preparation {
                map: art.map,
                model_revision: Some(art.model_revision),
                samples: 1,
                started: Instant::now(),
            });
        };
        // An initial load has no image to retain until the field is admitted.
        world.resource_mut::<Art>().begin_model_load(&resident);
        check_hold(world, false, true);
        world.resource_mut::<Art>().ready = true;
        submit(world);
        complete_test_startup(world);
        world.resource_mut::<Art>().begin_model_load(&resident);
        check_hold(world, true, false);
        world.insert_resource(crate::loading::Pending::spawn("transition cover", |_| {
            anyhow::bail!("unused transition result")
        })?);
        check_hold(world, false, true);
        world.remove_resource::<crate::loading::Pending>();
        world.resource_mut::<Art>().ready = true;
        submit(world);
        let obsolete = world
            .resource::<Shared>()
            .0
            .lock()
            .unwrap()
            .draws
            .completed
            .clone();
        world.resource_mut::<Art>().begin_model_load(&resident);
        world.run_system_once(retire).unwrap();
        assert!(!world.contains_resource::<Preparation>());
        world.run_system_once(complete).unwrap();
        check_hold(world, true, false);
        assert!(!resident.active.load(Ordering::Acquire));
        world.resource_mut::<Art>().ready = true;
        submit(world);
        obsolete.store(true, Ordering::Release);
        world.run_system_once(complete).unwrap();
        check_hold(world, true, false);
        complete_test_startup(world);
        check_hold(world, false, false);
        assert!(resident.active.load(Ordering::Acquire));
        Ok(())
    }

    #[test]
    fn field_retirement_keeps_title_files_while_releasing_field_gpu_work() {
        let mut world = World::new();
        let files = Arc::new(resonance_content::prepared::Files::default());
        let resident = Resident::default();
        *resident.files.write().unwrap() = Some(files.clone());
        resident.active.store(true, Ordering::Release);
        world.insert_resource(resident.clone());
        world.init_resource::<Shared>();
        world.insert_resource(Preparation {
            map: 340,
            started: Instant::now(),
            model_revision: Some(0),
            samples: 1,
        });
        world.resource::<Shared>().0.lock().unwrap().map = Some(340);

        // Covers both returning from a field and subsequent title-only updates.
        for _ in 0..2 {
            world.run_system_once(retire).unwrap();
            assert!(Arc::ptr_eq(
                resident.files.read().unwrap().as_ref().unwrap(),
                &files,
            ));
            assert!(!resident.active.load(Ordering::Acquire));
            assert!(!world.contains_resource::<Preparation>());
            assert!(world.resource::<Shared>().0.lock().unwrap().map.is_none());
        }
    }

    #[test]
    fn unprepared_field_reads_honor_the_session_policy_without_panicking() {
        for paranoid in [false, true] {
            let mut world = World::new();
            world.init_resource::<Resident>();
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
            let mut resident = world.resource_mut::<Resident>();
            resident.diagnostics = diagnostics.clone();
            resident.active.store(true, Ordering::Release);
            resident.unprepared_reads.store(1, Ordering::Release);
            world.insert_resource(Shared::default());
            world.init_resource::<Messages<AppExit>>();
            world.run_system_once(guard).unwrap();
            assert_eq!(!world.resource::<Messages<AppExit>>().is_empty(), paranoid);
            assert!(world.resource::<Resident>().active.load(Ordering::Acquire));
            assert!(
                diagnostics.entries()[0]
                    .message
                    .contains("undeclared asset read")
            );
        }
    }
}
