//! Prepare the renderer's own pipelines before a field is visible.
use super::{field_view::Art, gpu_prepare as gpu, loading::Resident, materials::TitleSurface};
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
    early: bool,
    samples: u32,
}
#[derive(SystemParam)]
struct Gpu<'w, 's> {
    images: ResMut<'w, Assets<Image>>,
    surfaces: ResMut<'w, Assets<TitleSurface>>,
    variants: ResMut<'w, Pipelines>,
    cameras: Query<'w, 's, &'static Msaa, With<super::FieldCamera>>,
}
impl Preparation {
    fn retire(&self, commands: &mut Commands) {
        commands.remove_resource::<Preparation>();
    }
}

// Structural pipeline representatives contain no scene textures. The finite
// material-state cache is shared by all fields; actual sampler views live in Part.
#[derive(Resource, Default)]
struct Pipelines(HashMap<super::materials::SurfaceKey, Handle<TitleSurface>>);
pub(super) fn install(app: &mut App) {
    let shared = Shared::default();
    let resident = app.world().resource::<Resident>().clone();
    app.world()
        .resource::<crate::gpu_prepare::Registry>()
        .watch(
            &shared.0,
            |report| &mut report.draws,
            Some((resident.diagnostics.clone(), "field draw pipeline")),
            {
                let resident = resident.clone();
                move |report, _, observer| rendered(report, &resident, observer)
            },
        );
    app.init_resource::<Pipelines>()
        .insert_resource(shared.clone())
        .add_systems(
            Update,
            (retire, resolve_prefetch, begin, complete, guard)
                .chain()
                .after(super::field_view::FieldPreparation)
                .run_if(super::battle::field_running),
        )
        .add_systems(
            PostUpdate,
            capture_frame
                .after(bevy::camera::visibility::VisibilitySystems::CheckVisibility)
                .run_if(super::battle::field_running),
        );
    app.sub_app_mut(bevy::render::RenderApp)
        .insert_resource(resident);
}

fn retire(
    mut commands: Commands,
    session: Option<Res<super::new_game::Session>>,
    pending: Option<Res<super::loading::Pending>>,
    resident: Res<Resident>,
    preparation: Option<Res<Preparation>>,
    shared: Res<Shared>,
    field: Option<Res<super::loading::FieldPending>>,
) {
    if preparation.as_ref().is_some_and(|preparation| {
        preparation.early
            && field
                .as_ref()
                .is_some_and(|pending| pending.map == preparation.map)
    }) {
        return; // The source can be an overworld scene while its field destination warms.
    }
    let stale = preparation.as_ref().is_some_and(|preparation| {
        preparation.early
            && !field
                .as_ref()
                .is_some_and(|pending| pending.map == preparation.map)
            && !session
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
        gpu: &mut Gpu,
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
                            .map(|surface| {
                                gpu.variants
                                    .0
                                    .entry((&surface).into())
                                    .or_insert_with(|| gpu.surfaces.add(surface))
                                    .clone()
                            })
                            .collect::<Vec<_>>()
                    });
                for material in materials.iter() {
                    report.draws.prepare(
                        crate::gpu_prepare::View::Field,
                        draw.mesh.id(),
                        material.id().untyped(),
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
    mut gpu: Gpu,
    ui: Option<Res<super::field_ui::Artwork>>,
    ui_materials: Res<Assets<super::field_ui::Surface>>,
    server: Res<AssetServer>,
    mut failures: super::field_view::Failures,
    session: Option<Res<super::new_game::Session>>,
    backdrop: Query<
        (&Mesh2d, &MeshMaterial2d<super::menu_backdrop::Material>),
        With<super::menu_backdrop::Quad>,
    >,
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
                .is_some_and(|p| p.map == art.map && !p.early))
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
                        .is_none_or(|p| p.early && p.map == graphics.map)
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
        let Ok((mesh, material)) = backdrop.single() else {
            return;
        };
        Some((mesh.clone(), material.clone()))
    };
    let staged = preparation
        .as_deref_mut()
        .filter(|p| p.map == map && p.early);
    let (mut preparation, mut report) = match staged {
        Some(staged) => (
            staged.clone(),
            std::mem::take(&mut *shared.0.lock().unwrap()),
        ),
        None => (
            Preparation {
                map,
                early,
                samples,
                started: Instant::now(),
            },
            Report {
                map: Some(map),
                ..Default::default()
            },
        ),
    };
    preparation.early = early;
    preparation.started = Instant::now();
    // Early uploads can finish independently. Final admission also needs the
    // newly instantiated scene's materials and actual draws, captured below.
    report.draws.restart_submission(early);
    if let Some((backdrop_mesh, backdrop_material)) = backdrop {
        let art = art.as_ref().unwrap();
        let ui = ui.as_ref().unwrap();
        // Loading may freeze the live cameras until preparation finishes. Warm the
        // menu backdrop offscreen too, so readiness never depends on that frozen view.
        report.draws.prepare(
            crate::gpu_prepare::View::Field,
            backdrop_mesh.0.id(),
            backdrop_material.0.id().untyped(),
            true,
            Some(samples),
        );
        if let Some(panorama) = &art.panorama
            && let Some(draw) = &panorama.draw
        {
            report.draws.prepare(
                crate::gpu_prepare::View::Field,
                draw.mesh.id(),
                draw.material.id().untyped(),
                false,
                Some(samples),
            );
            report.draws.images.insert(panorama.image.id());
        }
        for (mesh, material, essential) in ui.prepared_layers() {
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
                mesh.id(),
                material.id().untyped(),
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
                material.id().untyped(),
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
            &mut gpu,
            &mut report,
        );
    }
    if early {
        graphics.as_mut().unwrap().changed.clear();
    }
    let count = report.draws.material_bindings.len();
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
    if preparation.early {
        return;
    }
    let shared = world.resource::<Shared>().clone();
    if shared.0.lock().unwrap().draws.armed || !super::field_view::ready(world) {
        return;
    }
    let mut report = shared.0.lock().unwrap();
    // Visibility has been resolved for the held opening pose. Waiting on the
    // current bindings matters even when geometry and pipelines were retained.
    capture_draws(world, &mut report.draws);
    report.draws.armed = true;
}

fn capture_draws(world: &mut World, report: &mut gpu::Report) {
    use bevy::camera::visibility::{RenderLayers, VisibleEntities};
    use std::any::TypeId;

    let layers = RenderLayers::from_layers(&[0, 3]);
    for (camera, visible, render_layers, model) in world
        .query_filtered::<(
            &Camera,
            &VisibleEntities,
            Option<&RenderLayers>,
            Has<Camera3d>,
        ), Or<(With<Camera3d>, With<Camera2d>)>>()
        .iter(world)
    {
        if !camera.is_active
            || !render_layers
                .unwrap_or(&Default::default())
                .intersects(&layers)
        {
            continue;
        }
        // ViewVisibility includes every camera: a 2D view can mark a 3D mesh
        // visible without ever queuing it. Only wait for this view's draw class.
        let class = if model {
            TypeId::of::<Mesh3d>()
        } else {
            TypeId::of::<Mesh2d>()
        };
        for &entity in visible.iter(class) {
            let binding = if model {
                world
                    .get::<Mesh3d>(entity)
                    .zip(world.get::<MeshMaterial3d<TitleSurface>>(entity))
                    .map(|(mesh, material)| (mesh.0.id(), material.0.id().untyped()))
            } else {
                world
                    .get::<Mesh2d>(entity)
                    .zip(world.get::<MeshMaterial2d<super::field_ui::Surface>>(entity))
                    .map(|(mesh, material)| (mesh.0.id(), material.0.id().untyped()))
            };
            if let Some((mesh, material)) = binding {
                report.meshes.insert(mesh);
                report.material_bindings.insert((!model, material));
                report.expected.insert(entity.into());
            }
        }
    }
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
    if preparation.early {
        return;
    }
    if report.draws.completed.load(Ordering::Acquire) && refraction.get() {
        info!(
            "Field {} GPU preparation complete in {:.3}s: {} bindings, {} pipelines",
            preparation.map,
            preparation.started.elapsed().as_secs_f64(),
            report.draws.material_bindings.len(),
            report.draws.pipelines.len()
        );
        resident.active.store(true, Ordering::Release);
        if let Some(mut art) = art.filter(|art| art.map == preparation.map) {
            art.gpu = Some(preparation.samples);
        }
        preparation.retire(&mut commands);
    } else if preparation.started.elapsed().as_secs() > 120 {
        failures.fatal(
            "field preparation",
            anyhow::anyhow!(
                "Field {} GPU preparation timed out ({} pending pipelines)",
                preparation.map,
                report.draws.requests.len()
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
    fn opening_frame_only_waits_for_draws_seen_by_the_matching_camera() {
        use bevy::camera::visibility::VisibleEntities;
        use std::any::TypeId;

        let mut world = World::new();
        let model = world
            .spawn((Mesh3d::default(), MeshMaterial3d::<TitleSurface>::default()))
            .id();
        let offscreen = world
            .spawn((Mesh3d::default(), MeshMaterial3d::<TitleSurface>::default()))
            .id();
        let quad = world
            .spawn((
                Mesh2d::default(),
                MeshMaterial2d::<crate::field_ui::Surface>::default(),
            ))
            .id();
        let mut scene_visible = VisibleEntities::default();
        scene_visible.push(model, TypeId::of::<Mesh3d>());
        scene_visible.push(quad, TypeId::of::<Mesh2d>());
        world.spawn((Camera3d::default(), scene_visible));
        let mut overlay_visible = VisibleEntities::default();
        overlay_visible.push(model, TypeId::of::<Mesh3d>());
        overlay_visible.push(offscreen, TypeId::of::<Mesh3d>());
        overlay_visible.push(quad, TypeId::of::<Mesh2d>());
        world.spawn((Camera2d, overlay_visible));

        let mut report = gpu::Report::default();
        capture_draws(&mut world, &mut report);
        assert_eq!(report.expected, [model.into(), quad.into()].into());
        assert_eq!(report.material_bindings.len(), 2);
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
            early: false,
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
