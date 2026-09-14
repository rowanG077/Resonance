//! Exercise the live material/mesh specializations before a field is visible.
use super::{field_view::Art, loading::Resident, materials::TitleSurface};
use bevy::{
    camera::{
        RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d, tonemapping::Tonemapping},
    prelude::*,
    render::{
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, CachedRenderPipelineId, TextureFormat},
        sync_world::MainEntity,
    },
    world_serialization::WorldInstanceReady,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
const LAYER: usize = 30;

#[derive(Resource, Clone, Default)]
pub(super) struct Shared(Arc<Mutex<Report>>);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Draw {
    entity: MainEntity,
    /// Scene materials must finish in every supported camera format. UI and
    /// the live menu backdrop only need their ordinary view.
    view: Option<MainEntity>,
}
impl From<MainEntity> for Draw {
    fn from(entity: MainEntity) -> Self {
        Self { entity, view: None }
    }
}
#[derive(Default)]
struct Report {
    map: Option<u32>,
    expected: HashSet<Draw>,
    scene_views: Vec<MainEntity>,
    pipelines: HashSet<CachedRenderPipelineId>,
    armed: bool,
    submitted: bool,
    completed: Arc<AtomicBool>,
    failure: Option<String>,
    prepared_pipeline_count: usize,
    seen: usize,
    missing: Vec<Draw>,
}
impl Report {
    fn expect_scene_draw(&mut self, entity: MainEntity) {
        self.expected
            .extend(self.scene_views.iter().map(|&view| Draw {
                entity,
                view: Some(view),
            }));
    }

    fn expected_draw(&self, entity: MainEntity, view: MainEntity) -> Option<Draw> {
        [
            Draw {
                entity,
                view: Some(view),
            },
            entity.into(),
        ]
        .into_iter()
        .find(|draw| self.expected.contains(draw))
    }
}
#[derive(Resource)]
struct Preparation {
    map: u32,
    roots: Vec<Entity>,
    entities: Vec<Entity>,
    scene_count: usize,
    converted: usize,
    started: Instant,
    effects_copied: bool,
    retained: Vec<Handle<TitleSurface>>,
}
#[derive(Resource)]
struct PreparedMaterials {
    surfaces: HashMap<u32, Vec<Handle<TitleSurface>>>,
    sampler_misses: u64,
}
#[derive(Component)]
struct Scene {
    materials: HashMap<AssetId<StandardMaterial>, Handle<TitleSurface>>,
    loaded: bool,
    converted: bool,
    draws: Vec<MainEntity>,
}

pub(super) fn install(app: &mut App) {
    let shared = Shared::default();
    let resident = app.world().resource::<Resident>().clone();
    app.insert_resource(shared.clone()).add_systems(
        Update,
        (retire, begin, convert, effects, complete, guard)
            .chain()
            .after(super::field_view::FieldPreparation),
    );
    app.get_sub_app_mut(bevy::render::RenderApp)
        .unwrap()
        .insert_resource(shared)
        .insert_resource(resident)
        .add_systems(
            bevy::render::Render,
            rendered.in_set(bevy::render::RenderSystems::Cleanup),
        );
}

fn retire(
    mut commands: Commands,
    session: Option<Res<super::new_game::Session>>,
    pending: Option<Res<super::loading::Pending>>,
    resident: Res<Resident>,
    preparation: Option<Res<Preparation>>,
    retained: Option<Res<PreparedMaterials>>,
    shared: Res<Shared>,
) {
    if session.is_some() || pending.is_some() {
        return;
    }
    resident.active.store(false, Ordering::Release);
    resident.files.write().unwrap().take();
    if let Some(preparation) = preparation {
        for entity in preparation.roots.iter().chain(&preparation.entities) {
            commands.entity(*entity).despawn();
        }
        commands.remove_resource::<Preparation>();
    }
    if retained.is_some() {
        commands.remove_resource::<PreparedMaterials>();
    }
    if shared.0.lock().unwrap().map.is_some() {
        *shared.0.lock().unwrap() = Report::default();
    }
}

#[allow(clippy::too_many_arguments)]
fn begin(
    mut commands: Commands,
    art: Option<Res<Art>>,
    preparation: Option<Res<Preparation>>,
    resident: Res<Resident>,
    shared: Res<Shared>,
    mut images: ResMut<Assets<Image>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut sampled: ResMut<super::scene::SampledImages>,
    ui: Option<Res<super::field_ui::Artwork>>,
    session: Option<Res<super::new_game::Session>>,
    backdrop: Query<Entity, With<super::menu_backdrop::Quad>>,
    #[cfg(feature = "solari")] modern: Option<Res<super::ray_tracing::State>>,
) {
    let Some(art) = art.filter(|a| a.ready) else {
        return;
    };
    if session.is_none_or(|s| {
        s.assets.map_id != art.map || s.field.events.world.field_transition.is_some()
    }) {
        return;
    }
    if resident.active.load(Ordering::Acquire)
        || preparation.as_ref().is_some_and(|p| p.map == art.map)
    {
        return;
    }
    let Some(ui) = ui.filter(|ui| ui.ready(&images)) else {
        return;
    };
    let Ok(backdrop) = backdrop.single() else {
        return;
    };
    let mut entities = Vec::new();
    let mut report = Report {
        map: Some(art.map),
        ..Default::default()
    };
    report.expected.insert(MainEntity::from(backdrop).into());
    let target = images.add(Image::new_target_texture(
        64,
        64,
        TextureFormat::Bgra8Unorm,
        None,
    ));
    let scene_camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -20,
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            RenderLayers::layer(LAYER),
            Msaa::Off,
            Tonemapping::None,
            Projection::custom(super::camera::TitleProjection(
                PerspectiveProjection::default(),
            )),
            Transform::from_xyz(0., -1000., 500.).looking_at(Vec3::ZERO, Vec3::Z),
        ))
        .id();
    entities.push(scene_camera);
    report.scene_views.push(scene_camera.into());
    // Exercise hidden actor poses, effects and scenery in HDR as well as the
    // original format before gameplay can reveal them or F6 switches views.
    #[cfg(feature = "solari")]
    if let Some(modern) = modern.filter(|_| art.map == 340) {
        let target = images.add(Image::new_target_texture(
            64,
            64,
            TextureFormat::Bgra8Unorm,
            None,
        ));
        let mut camera = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -21,
                ..default()
            },
            RenderTarget::Image(target.into()),
            RenderLayers::layer(LAYER),
            Projection::custom(super::camera::TitleProjection(
                PerspectiveProjection::default(),
            )),
            Transform::from_xyz(0., -1000., 500.).looking_at(Vec3::ZERO, Vec3::Z),
        ));
        modern.prepare_view(&mut camera);
        report.scene_views.push(camera.id().into());
        entities.push(camera.id());
    }
    entities.push(
        commands
            .spawn((
                Camera2d,
                Camera {
                    order: -19,
                    clear_color: ClearColorConfig::None,
                    ..default()
                },
                RenderTarget::Image(target.into()),
                RenderLayers::layer(LAYER),
                Msaa::Off,
                Tonemapping::None,
            ))
            .id(),
    );
    for (mesh, material) in ui.prepared_layers() {
        let entity = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(320., -240., 0.),
                RenderLayers::layer(LAYER),
                NoFrustumCulling,
            ))
            .id();
        report.expected.insert(MainEntity::from(entity).into());
        entities.push(entity);
    }
    let mut roots = Vec::new();
    let mut retained = Vec::new();
    for (&resource, parts) in &art.models {
        for (index, part) in parts.iter().enumerate() {
            // Actor appearance can override depth writes independently of the
            // material recipe. Exercise both keys, including hidden geometry.
            for depth_write in [false, true] {
                let materials = art
                    .surfaces(resource, index, &mut images, &mut sampled)
                    .map(|(template, mut surface)| {
                        surface.depth_write = depth_write;
                        let handle = surfaces.add(surface);
                        retained.push(handle.clone());
                        (template, handle)
                    })
                    .collect();
                let root = commands
                    .spawn((
                        WorldAssetRoot(part.scene.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                        Scene {
                            materials,
                            loaded: false,
                            converted: false,
                            draws: Vec::new(),
                        },
                    ))
                    .observe(
                        |event: On<WorldInstanceReady>,
                         mut scenes: Query<&mut Scene>,
                         mut preparation: ResMut<Preparation>,
                         shared: Res<Shared>| {
                            if let Ok(mut scene) = scenes.get_mut(event.entity) {
                                // Loading another glTF label can replace an instance.
                                // Its old entity IDs no longer describe live draws.
                                if scene.converted {
                                    preparation.converted -= 1;
                                    let mut report = shared.0.lock().unwrap();
                                    for entity in scene.draws.drain(..) {
                                        report.expected.retain(|draw| draw.entity != entity);
                                    }
                                    report.armed = false;
                                    report.submitted = false;
                                    report.completed = Arc::new(AtomicBool::new(false));
                                }
                                scene.loaded = true;
                                scene.converted = false;
                            }
                        },
                    )
                    .id();
                roots.push(root);
            }
        }
    }
    *shared.0.lock().unwrap() = report;
    let scene_count = roots.len();
    info!(
        "Field {} warming {scene_count} scene variants and {} UI layers",
        art.map,
        ui.prepared_layers().count()
    );
    commands.insert_resource(Preparation {
        map: art.map,
        roots,
        entities,
        scene_count,
        converted: 0,
        started: Instant::now(),
        effects_copied: false,
        retained,
    });
}

fn convert(
    mut commands: Commands,
    preparation: Option<ResMut<Preparation>>,
    shared: Res<Shared>,
    mut scenes: Query<(Entity, &mut Scene)>,
    children: Query<&Children>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    let Some(mut preparation) = preparation else {
        return;
    };
    let mut report = shared.0.lock().unwrap();
    for (root, mut scene) in &mut scenes {
        if !scene.loaded || scene.converted {
            continue;
        }
        for child in children.iter_descendants(root) {
            commands.entity(child).insert((
                RenderLayers::layer(LAYER),
                Visibility::Inherited,
                NoFrustumCulling,
            ));
            if let Ok(material) = meshes.get(child) {
                let Some(prepared) = scene.materials.get(&material.id()) else {
                    report.failure = Some(format!(
                        "field {} has an undeclared warmup material {:?}",
                        preparation.map,
                        material.id()
                    ));
                    continue;
                };
                commands
                    .entity(child)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert(MeshMaterial3d(prepared.clone()));
                report.expect_scene_draw(child.into());
                scene.draws.push(child.into());
            }
        }
        commands
            .entity(root)
            .insert((Visibility::Inherited, RenderLayers::layer(LAYER)));
        scene.converted = true;
        preparation.converted += 1;
    }
}

#[allow(clippy::type_complexity)]
fn effects(
    mut commands: Commands,
    preparation: Option<ResMut<Preparation>>,
    shared: Res<Shared>,
    art: Option<Res<Art>>,
    meshes: Query<
        (
            &Mesh3d,
            Option<&MeshMaterial3d<TitleSurface>>,
            Option<&super::field_effects::HeadSymbol>,
        ),
        With<super::field_effects::EffectDraw>,
    >,
) {
    let Some(mut preparation) = preparation else {
        return;
    };
    if preparation.effects_copied || art.as_ref().is_none_or(|a| !a.ready) {
        return;
    }
    let mut report = shared.0.lock().unwrap();
    // Effect and shadow assets are created by field preparation before this
    // pass. Copy their bindings into the offscreen view, not their state.
    let bindings = meshes
        .iter()
        // Modern head symbols use the UI compositor. Still prepare their
        // authored 3D material so F6 never requests an unwarmed pipeline.
        .filter_map(|(m, s, symbol)| {
            s.map(|s| &s.0)
                .or_else(|| symbol.map(|symbol| &symbol.material))
                .map(|material| (m.0.clone(), material.clone()))
        })
        .chain(art.as_ref().and_then(|a| a.shadow_binding()));
    for (mesh, material) in bindings {
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::default(),
                RenderLayers::layer(LAYER),
                NoFrustumCulling,
            ))
            .id();
        preparation.entities.push(entity);
        report.expect_scene_draw(entity.into());
    }
    preparation.effects_copied = true;
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn complete(
    mut commands: Commands,
    preparation: Option<Res<Preparation>>,
    shared: Res<Shared>,
    resident: Res<Resident>,
    refraction: Res<super::field_refraction::Ready>,
    mut exit: MessageWriter<AppExit>,
    mut logged: Local<u64>,
    names: Query<(
        Option<&Name>,
        Option<&Mesh2d>,
        Option<&Mesh3d>,
        Option<&ViewVisibility>,
        Option<&InheritedVisibility>,
    )>,
    sampled: Res<super::scene::SampledImages>,
    retained: Option<ResMut<PreparedMaterials>>,
) {
    let Some(preparation) = preparation else {
        return;
    };
    let mut report = shared.0.lock().unwrap();
    report.armed = preparation.converted == preparation.scene_count && preparation.effects_copied;
    let seconds = preparation.started.elapsed().as_secs();
    if seconds / 5 > *logged {
        *logged = seconds / 5;
        info!(
            "Field {} preparation: {}/{} scenes, {}/{} draws ready, submitted={}",
            preparation.map,
            preparation.converted,
            preparation.scene_count,
            report.seen,
            report.expected.len(),
            report.submitted
        );
        for id in report.missing.iter().take(3) {
            info!("Missing warm draw {id:?}: {:?}", names.get(id.entity.id()));
        }
    }
    if let Some(error) = &report.failure {
        error!("Field preparation failed: {error}");
        exit.write(AppExit::error());
        return;
    }
    if report.completed.load(Ordering::Acquire) && refraction.get() {
        info!(
            "Field {} GPU preparation complete in {:.3}s: {} draws, {} pipelines",
            preparation.map,
            preparation.started.elapsed().as_secs_f64(),
            report.expected.len(),
            report.pipelines.len()
        );
        for entity in preparation.roots.iter().chain(&preparation.entities) {
            commands.entity(*entity).despawn();
        }
        resident.active.store(true, Ordering::Release);
        if let Some(mut retained) = retained {
            retained
                .surfaces
                .insert(preparation.map, preparation.retained.clone());
            retained.sampler_misses = sampled.misses;
        } else {
            commands.insert_resource(PreparedMaterials {
                surfaces: [(preparation.map, preparation.retained.clone())].into(),
                sampler_misses: sampled.misses,
            });
        }
        commands.remove_resource::<Preparation>();
    } else if preparation.started.elapsed().as_secs() > 120 {
        error!(
            "Field {} GPU preparation timed out ({} expected draws)",
            preparation.map,
            report.expected.len()
        );
        exit.write(AppExit::error());
    }
}

fn guard(
    shared: Res<Shared>,
    resident: Res<Resident>,
    prepared: Option<Res<PreparedMaterials>>,
    sampled: Res<super::scene::SampledImages>,
    mut exit: MessageWriter<AppExit>,
) {
    let mut report = shared.0.lock().unwrap();
    if resident.late_reads.load(Ordering::Relaxed) > 0 && report.failure.is_none() {
        report.failure = Some(format!(
            "field {:?} attempted a late or undeclared asset read",
            report.map
        ));
    }
    if resident.active.load(Ordering::Acquire)
        && prepared.is_some_and(|p| sampled.misses != p.sampler_misses)
        && report.failure.is_none()
    {
        report.failure = Some(format!(
            "field {:?} created an unprepared texture/sampler binding",
            report.map
        ));
    }
    if let Some(error) = &report.failure {
        debug_assert!(false, "{error}");
        error!("{error}");
        exit.write(AppExit::error());
    }
}

fn rendered(
    shared: Res<Shared>,
    resident: Res<Resident>,
    cache: Res<bevy::render::render_resource::PipelineCache>,
    phases3: Res<ViewSortedRenderPhases<Transparent3d>>,
    phases2: Res<ViewSortedRenderPhases<Transparent2d>>,
    queue: Res<bevy::render::renderer::RenderQueue>,
    device: Res<bevy::render::renderer::RenderDevice>,
) {
    let mut report = shared.0.lock().unwrap();
    use bevy::render::render_resource::PipelineDescriptor;
    let relevant = || {
        cache.pipelines().filter(|p| matches!(&p.descriptor,
        PipelineDescriptor::RenderPipelineDescriptor(d) if matches!(d.label.as_deref(),Some("resonance/surface" | "resonance/field-ui" | "resonance/refraction" | "resonance/menu-backdrop"))))
    };
    let count = relevant().count();
    if resident.active.load(Ordering::Acquire) {
        if count != report.prepared_pipeline_count
            || relevant().any(|p| !matches!(p.state, CachedPipelineState::Ok(_)))
        {
            report.failure = Some(format!(
                "field {:?} requested an unprepared rendering pipeline after activation (prepared {}, now {})",
                report.map, report.prepared_pipeline_count, count
            ));
        }
        return;
    }
    if !report.armed {
        return;
    }
    // Drive completion callbacks on headless backends as well as windowed
    // ones. Active gameplay needs no explicit polling or synchronization.
    let _ = device.poll(bevy::render::render_resource::PollType::Poll);
    if report.submitted {
        return;
    }
    let mut seen = HashSet::new();
    let models = phases3.0.iter().flat_map(|(view, phase)| {
        phase
            .items
            .values()
            .map(move |p| (view.main_entity, p.entity.1, p.pipeline))
    });
    let ui = phases2.0.iter().flat_map(|(view, phase)| {
        phase
            .items
            .values()
            .map(move |p| (view.main_entity, p.entity.1, p.pipeline))
    });
    for (view, entity, pipeline) in models.chain(ui) {
        if let Some(draw) = report.expected_draw(entity, view) {
            report.pipelines.insert(pipeline);
            if matches!(
                cache.get_render_pipeline_state(pipeline),
                CachedPipelineState::Ok(_)
            ) {
                seen.insert(draw);
            }
        }
    }
    report.seen = seen.len();
    report.missing = report.expected.difference(&seen).copied().collect();
    if let Some(pipeline) = cache
        .pipelines()
        .find(|p| matches!(p.state, CachedPipelineState::Err(_)))
    {
        report.failure = Some(format!("render pipeline failed: {:?}", pipeline.state));
        return;
    }
    if !report.expected.is_empty()
        && seen == report.expected
        && relevant().all(|p| matches!(p.state, CachedPipelineState::Ok(_)))
    {
        report.prepared_pipeline_count = count;
        report.submitted = true;
        let done = report.completed.clone();
        queue.on_submitted_work_done(move || done.store(true, Ordering::Release));
    }
}

/// Draws queued by both cameras; callers retain their own pipeline/error policy.
pub(super) fn draws<'a>(
    models: &'a ViewSortedRenderPhases<Transparent3d>,
    ui: &'a ViewSortedRenderPhases<Transparent2d>,
) -> impl Iterator<Item = (MainEntity, CachedRenderPipelineId)> + 'a {
    let models = models
        .0
        .values()
        .flat_map(|p| p.items.values())
        .map(|p| (p.entity.1, p.pipeline));
    let ui =
        ui.0.values()
            .flat_map(|p| p.items.values())
            .map(|p| (p.entity.1, p.pipeline));
    models.chain(ui)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_draw_must_finish_in_both_original_and_hdr_views() {
        let mut world = World::new();
        let original = MainEntity::from(world.spawn_empty().id());
        let hdr = MainEntity::from(world.spawn_empty().id());
        let live = MainEntity::from(world.spawn_empty().id());
        let mesh = MainEntity::from(world.spawn_empty().id());
        let ui = MainEntity::from(world.spawn_empty().id());
        let mut report = Report {
            scene_views: vec![original, hdr],
            ..Default::default()
        };
        report.expect_scene_draw(mesh);
        report.expected.insert(ui.into());
        let mut seen: HashSet<_> = [
            report.expected_draw(mesh, original).unwrap(),
            report.expected_draw(ui, live).unwrap(),
        ]
        .into();
        // Rendering the same mesh in the original view again cannot satisfy
        // its missing HDR specialization, nor can an unrelated live view.
        seen.insert(report.expected_draw(mesh, original).unwrap());
        assert!(report.expected_draw(mesh, live).is_none());
        assert_ne!(seen, report.expected);
        seen.insert(report.expected_draw(mesh, hdr).unwrap());
        assert_eq!(seen, report.expected);
    }
}
