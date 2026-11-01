//! Exercise the live material/mesh specializations before a field is visible.
use super::{field_view::Art, loading::Resident, materials::TitleSurface};
use bevy::{
    camera::{
        RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d, tonemapping::Tonemapping},
    mesh::{BaseMeshPipelineKey, MeshVertexBufferLayoutRef, MeshVertexBufferLayouts},
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, CachedRenderPipelineId, TextureFormat},
        sync_world::MainEntity,
        texture::GpuImage,
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

enum MaterialVariant {
    Actor {
        depth_write: bool,
        lighting: bool,
        blend: Option<resonance_events::effect::Blend>,
        two_sided: bool,
    },
    Override {
        blend: resonance_events::effect::Blend,
        depth_write: bool,
    },
}

#[derive(Resource, Clone, Default)]
pub(super) struct Shared(Arc<Mutex<Report>>);
#[derive(Default)]
struct Report {
    map: Option<u32>,
    expected: HashSet<MainEntity>,
    images: HashSet<AssetId<Image>>,
    pipelines: HashSet<CachedRenderPipelineId>,
    armed: bool,
    submitted: bool,
    completed: Arc<AtomicBool>,
    failure: Option<String>,
    seen: usize,
    missing: Vec<MainEntity>,
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
    retained: Bindings,
}
#[derive(Resource)]
struct PreparedMaterials {
    fields: HashMap<u32, Bindings>,
}
#[derive(Clone, Default)]
struct Bindings {
    surfaces: Vec<Handle<TitleSurface>>,
    // The sampler cache holds weak handles. Hidden actors still need their
    // sampled images retained even when another model warms their pipeline.
    images: HashSet<Handle<Image>>,
}
#[derive(Component)]
struct Scene {
    materials: Vec<Option<Handle<TitleSurface>>>,
    loaded: bool,
    converted: bool,
    draws: Vec<MainEntity>,
}

/// Pipeline specialization depends on mesh layout and material features, not
/// the model, texture identity or number of instances using them.
#[derive(Clone, PartialEq, Eq, Hash)]
struct MeshKey {
    bits: u64,
    layout: MeshVertexBufferLayoutRef,
}
impl MeshKey {
    fn new(mesh: &Mesh, layouts: &mut MeshVertexBufferLayouts) -> Self {
        let bits = BaseMeshPipelineKey::from_primitive_topology_and_strip_index(
            mesh.primitive_topology(),
            mesh.indices().map(|indices| match indices {
                bevy::mesh::Indices::U16(_) => bevy::render::render_resource::IndexFormat::Uint16,
                bevy::mesh::Indices::U32(_) => bevy::render::render_resource::IndexFormat::Uint32,
            }),
        );
        // Morph targets are disabled in the renderer's Bevy feature set.
        Self {
            bits: bits.bits(),
            layout: mesh.get_mesh_vertex_buffer_layout(layouts),
        }
    }
}

pub(super) fn install(app: &mut App) {
    let shared = Shared::default();
    let resident = app.world().resource::<Resident>().clone();
    app.insert_resource(shared.clone()).add_systems(
        Update,
        (retire, begin, convert, effects, complete, guard)
            .chain()
            .after(super::field_view::FieldPreparation)
            .run_if(super::battle::field_running),
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
    if session.as_ref().is_some_and(|s| s.is_field()) || pending.is_some() {
        return;
    }
    if session.is_none() {
        resident.active.store(false, Ordering::Release);
        // The title also owns prepared files without a gameplay session. Scene
        // transitions own that snapshot; retiring field GPU work must keep it.
    }
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
    ui_materials: Res<Assets<super::field_ui::Surface>>,
    server: Res<AssetServer>,
    mut failures: super::field_view::Failures,
    session: Option<Res<super::new_game::Session>>,
    backdrop: Query<
        (&Mesh2d, &MeshMaterial2d<super::menu_backdrop::Material>),
        With<super::menu_backdrop::Quad>,
    >,
    scenes: Res<Assets<WorldAsset>>,
    meshes: Res<Assets<Mesh>>,
) {
    let Some(art) = art.filter(|a| a.ready) else {
        return;
    };
    if session.is_none_or(|s| {
        s.overworld().is_some()
            || s.map_id() != art.map
            || s.events().world.field_transition.is_some()
            || s.events().world.world_transition.is_some()
    }) {
        return;
    }
    if resident.active.load(Ordering::Acquire)
        || preparation.as_ref().is_some_and(|p| p.map == art.map)
    {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    match ui.essential_ready(&images, &server) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            failures.fatal("field essential artwork", error);
            return;
        }
    }
    let Ok((backdrop_mesh, backdrop_material)) = backdrop.single() else {
        return;
    };
    let mut entities = Vec::new();
    let mut report = Report {
        map: Some(art.map),
        ..Default::default()
    };
    let target = images.add(Image::new_target_texture(
        64,
        64,
        TextureFormat::Bgra8Unorm,
        None,
    ));
    entities.push(
        commands
            .spawn((
                Camera3d::default(),
                // Match FieldCamera even before a script enables fog.
                DistanceFog::default(),
                Camera {
                    order: -20,
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                RenderLayers::layer(LAYER),
                Msaa::Off,
                Tonemapping::Linear,
                Projection::custom(super::camera::TitleProjection(
                    PerspectiveProjection::default(),
                )),
                Transform::from_xyz(0., -1000., 500.).looking_at(Vec3::ZERO, Vec3::Z),
            ))
            .id(),
    );
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
                Tonemapping::Linear,
            ))
            .id(),
    );
    // Loading may freeze the live cameras until preparation finishes. Warm the
    // menu backdrop offscreen too, so readiness never depends on that frozen view.
    let backdrop = commands
        .spawn((
            backdrop_mesh.clone(),
            backdrop_material.clone(),
            Transform::default(),
            RenderLayers::layer(LAYER),
            NoFrustumCulling,
        ))
        .id();
    report.expected.insert(backdrop.into());
    entities.push(backdrop);
    if let Some(panorama) = &art.panorama
        && let Some(draw) = &panorama.draw
    {
        let entity = commands
            .spawn((
                Mesh3d(draw.mesh.clone()),
                MeshMaterial3d(draw.material.clone()),
                Transform::default(),
                RenderLayers::layer(LAYER),
                NoFrustumCulling,
            ))
            .id();
        report.expected.insert(entity.into());
        report.images.insert(panorama.image.id());
        entities.push(entity);
    }
    for (mesh, material, essential) in ui.prepared_layers() {
        // Optional pages may still be loading, or may never be selected. Their
        // missing textures must not enter the mandatory submission fence.
        if !essential
            && !ui_materials
                .get(material)
                .is_some_and(|surface| surface.images_ready(&images))
        {
            continue;
        }
        let entity = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(320., -240., 0.),
                RenderLayers::layer(LAYER),
                NoFrustumCulling,
            ))
            .id();
        report.expected.insert(entity.into());
        entities.push(entity);
    }
    let mut roots = Vec::new();
    let mut retained = Bindings::default();
    let mut layouts = MeshVertexBufferLayouts::default();
    let mut prepared = HashSet::new();
    for (&resource, parts) in &art.models {
        for (index, part) in parts.iter().enumerate() {
            let scene = scenes.get(&part.scene).expect("prepared field scene");
            let mut geometry = vec![Vec::new(); part.spec.materials.len()];
            if let Some(mut query) = scene
                .world
                .try_query::<(&Mesh3d, &super::materials::MaterialSlot)>()
            {
                for (mesh, slot) in query.iter(&scene.world) {
                    let slot = slot.index(geometry.len()).expect("validated material slot");
                    geometry[slot].push(MeshKey::new(
                        meshes.get(&mesh.0).expect("prepared field mesh"),
                        &mut layouts,
                    ));
                }
            }
            // Resolve every sampler once, including those needed by hidden
            // actors. GPU warmup only needs representatives of distinct keys.
            let actor_surfaces: Vec<_> = art
                .surfaces(resource, index, &mut images, &mut sampled)
                .collect();
            let particle_surfaces = part.surfaces(&mut images, &mut sampled);
            retained
                .images
                .extend(
                    actor_surfaces
                        .iter()
                        .chain(&particle_surfaces)
                        .flat_map(|surface| {
                            surface
                                .color
                                .iter()
                                .chain(&surface.sampling)
                                .chain(&surface.multiply)
                                .chain(&surface.toon_ramp)
                                .cloned()
                        }),
                );
            // Scripts may draw any loaded model as an actor or a model particle.
            // Warm every blend/depth key, including currently hidden geometry.
            use resonance_events::effect::Blend;
            for variant in [false, true]
                .into_iter()
                .flat_map(|depth_write| {
                    [false, true].into_iter().flat_map(move |lighting| {
                        std::iter::once((None, false))
                            .chain(Blend::ACTOR_MODES.into_iter().flat_map(|blend| {
                                [false, true].map(|two_sided| (Some(blend), two_sided))
                            }))
                            .map(move |(blend, two_sided)| MaterialVariant::Actor {
                                depth_write,
                                lighting,
                                blend,
                                two_sided,
                            })
                    })
                })
                .chain(
                    [Blend::Alpha, Blend::Additive, Blend::Subtractive]
                        .into_iter()
                        .flat_map(|blend| {
                            [false, true]
                                .map(|depth_write| MaterialVariant::Override { blend, depth_write })
                        }),
                )
            {
                let source = match variant {
                    MaterialVariant::Actor { .. } => &actor_surfaces,
                    MaterialVariant::Override { .. } => &particle_surfaces,
                };
                let materials: Vec<_> = source
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(material, mut surface)| {
                        match variant {
                            MaterialVariant::Actor {
                                depth_write,
                                lighting,
                                blend,
                                two_sided,
                            } => {
                                surface.blend = blend.or_else(|| {
                                    part.spec.materials[material].blend.then_some(Blend::Alpha)
                                });
                                if two_sided {
                                    surface.cull = resonance_content::CullFace::None;
                                }
                                surface.depth_write = depth_write;
                                surface.toon_ramp = art.toon_ramp_for(
                                    resource,
                                    index,
                                    &part.spec.materials[material],
                                    Some(i32::from(lighting)),
                                );
                            }
                            MaterialVariant::Override { blend, depth_write } => {
                                super::field_model_particles::material(&mut surface, blend);
                                surface.depth_write = depth_write;
                            }
                        }
                        let key = super::materials::SurfaceKey::from(&surface);
                        let mut needed = false;
                        for mesh in &geometry[material] {
                            needed |= prepared.insert((mesh.clone(), key));
                        }
                        if !needed {
                            return None;
                        }
                        let handle = surfaces.add(surface);
                        retained.surfaces.push(handle.clone());
                        Some(handle)
                    })
                    .collect();
                if materials.iter().all(Option::is_none) {
                    continue;
                }
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
                                        report.expected.remove(&entity);
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
    report.images = retained.images.iter().map(Handle::id).collect();
    *shared.0.lock().unwrap() = report;
    let scene_count = roots.len();
    info!(
        "Field {} warming {scene_count} scene variants and {} UI layers",
        art.map,
        entities.len() - 2
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
    resident: Res<Resident>,
    mut scenes: Query<(Entity, &mut Scene)>,
    children: Query<&Children>,
    meshes: Query<&super::materials::MaterialSlot>,
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
            if let Ok(slot) = meshes.get(child) {
                let index = match slot.index(scene.materials.len()) {
                    Ok(index) => index,
                    Err(error) => {
                        let error = format!("field {}: {error}", preparation.map);
                        if resident
                            .diagnostics
                            .report("field warm draw", anyhow::anyhow!(error.clone()))
                            .is_err()
                        {
                            report.failure = Some(error);
                        }
                        commands.entity(child).insert(Visibility::Hidden);
                        continue;
                    }
                };
                let Some(material) = &scene.materials[index] else {
                    commands.entity(child).insert(Visibility::Hidden);
                    continue;
                };
                commands
                    .entity(child)
                    .insert(MeshMaterial3d(material.clone()));
                report.expected.insert(child.into());
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

fn effects(
    mut commands: Commands,
    preparation: Option<ResMut<Preparation>>,
    shared: Res<Shared>,
    art: Option<Res<Art>>,
    effects: Option<Res<super::field_effects::Artwork>>,
) {
    let Some(mut preparation) = preparation else {
        return;
    };
    if preparation.effects_copied || art.as_ref().is_none_or(|a| !a.ready) {
        return;
    }
    let Some(effects) = effects else { return };
    let mut report = shared.0.lock().unwrap();
    // Effect and shadow assets are created by field preparation before this
    // pass. Copy their bindings into the offscreen view, not their state.
    let bindings = effects
        .prepared_bindings()
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
        report.expected.insert(entity.into());
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
    mut failures: super::field_view::Failures,
    mut logged: Local<u64>,
    names: Query<(
        Option<&Name>,
        Option<&Mesh2d>,
        Option<&Mesh3d>,
        Option<&ViewVisibility>,
        Option<&InheritedVisibility>,
    )>,
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
            info!("Missing warm draw {id:?}: {:?}", names.get(id.id()));
        }
    }
    if let Some(error) = &report.failure {
        failures.fatal("field preparation", anyhow::anyhow!(error.clone()));
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
                .fields
                .insert(preparation.map, preparation.retained.clone());
        } else {
            commands.insert_resource(PreparedMaterials {
                fields: [(preparation.map, preparation.retained.clone())].into(),
            });
        }
        commands.remove_resource::<Preparation>();
    } else if preparation.started.elapsed().as_secs() > 120 {
        failures.fatal(
            "field preparation",
            anyhow::anyhow!(
                "Field {} GPU preparation timed out ({} expected draws)",
                preparation.map,
                report.expected.len()
            ),
        );
    }
}

fn guard(shared: Res<Shared>, resident: Res<Resident>, mut exit: MessageWriter<AppExit>) {
    let report = shared.0.lock().unwrap();
    let failure = report.failure.clone().or_else(|| {
        (resident.unprepared_reads.load(Ordering::Relaxed) > 0)
            .then(|| format!("field {:?} attempted an undeclared asset read", report.map))
    });
    if let Some(error) = failure
        && resident
            .diagnostics
            .report("field GPU", anyhow::anyhow!(error))
            .is_err()
    {
        exit.write(AppExit::error());
    }
}

#[allow(clippy::too_many_arguments)] // Warm draws, pipelines, image uploads and GPU completion.
fn rendered(
    shared: Res<Shared>,
    resident: Res<Resident>,
    cache: Res<bevy::render::render_resource::PipelineCache>,
    phases3: Res<ViewSortedRenderPhases<Transparent3d>>,
    phases2: Res<ViewSortedRenderPhases<Transparent2d>>,
    queue: Res<bevy::render::renderer::RenderQueue>,
    device: Res<bevy::render::renderer::RenderDevice>,
    images: Res<RenderAssets<GpuImage>>,
) {
    let mut report = shared.0.lock().unwrap();
    // The overworld also uses Resident, but never starts this field-only GPU
    // preparation pass. A retired field report cannot govern its pipelines.
    if report.map.is_none() {
        return;
    }
    if resident.battle.load(Ordering::Acquire) {
        return;
    }
    if resident.active.load(Ordering::Acquire) {
        let error = report.pipelines.iter().find_map(|&pipeline| {
            if let CachedPipelineState::Err(error) = cache.get_render_pipeline_state(pipeline)
                && !crate::model_preview::gpu::shader_pending(error)
            {
                Some(error.to_string())
            } else {
                None
            }
        });
        if let Some(error) = error
            && resident
                .diagnostics
                .report("field draw pipeline", anyhow::anyhow!(error.clone()))
                .is_err()
        {
            report.failure = Some(error);
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
    for (entity, pipeline) in draws(&phases3, &phases2) {
        if report.expected.contains(&entity) {
            report.pipelines.insert(pipeline);
            match cache.get_render_pipeline_state(pipeline) {
                CachedPipelineState::Ok(_) => {
                    seen.insert(entity);
                }
                CachedPipelineState::Err(error)
                    if !crate::model_preview::gpu::shader_pending(error) =>
                {
                    if resident
                        .diagnostics
                        .report("field draw pipeline", anyhow::anyhow!("{error}"))
                        .is_err()
                    {
                        report.failure = Some(error.to_string());
                        return;
                    }
                    // Bevy omits this failed draw. Other draws still have to
                    // complete their own preparation and submission.
                    report.expected.remove(&entity);
                    report.pipelines.remove(&pipeline);
                }
                _ => {}
            }
        }
    }
    seen.retain(|entity| report.expected.contains(entity));
    report.seen = seen.len();
    report.missing = report.expected.difference(&seen).copied().collect();
    if seen == report.expected
        && report
            .images
            .iter()
            .all(|image| images.get(*image).is_some())
    {
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
    assert!(world.resource::<Preparation>().roots.is_empty());
    world.run_system_once(effects).unwrap();
    world
        .resource::<Shared>()
        .0
        .lock()
        .unwrap()
        .completed
        .store(true, Ordering::Release);
    world.insert_resource(super::field_refraction::Ready::completed());
    world.run_system_once(complete).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::RenderAssetUsages, ecs::system::RunSystemOnce, mesh::PrimitiveTopology};

    #[test]
    fn field_retirement_keeps_title_files_while_releasing_field_gpu_work() {
        let mut world = World::new();
        let files = Arc::new(resonance_content::prepared::Files::default());
        let resident = Resident::default();
        *resident.files.write().unwrap() = Some(files.clone());
        resident.active.store(true, Ordering::Release);
        world.insert_resource(resident.clone());
        world.init_resource::<Shared>();
        let root = world.spawn_empty().id();
        world.insert_resource(Preparation {
            map: 340,
            roots: vec![root],
            entities: vec![],
            scene_count: 0,
            converted: 0,
            started: Instant::now(),
            effects_copied: false,
            retained: Bindings::default(),
        });
        world.insert_resource(PreparedMaterials {
            fields: HashMap::new(),
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
            assert!(!world.contains_resource::<PreparedMaterials>());
            assert!(world.resource::<Shared>().0.lock().unwrap().map.is_none());
        }
        assert!(world.get_entity(root).is_err());
    }

    #[test]
    fn warm_keys_share_geometry_but_preserve_pipeline_features() {
        let mut layouts = MeshVertexBufferLayouts::default();
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.; 3]; 3]);
        let moved = mesh
            .clone()
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[10.; 3]; 3]);
        let skin = mesh
            .clone()
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                bevy::mesh::VertexAttributeValues::Uint16x4(vec![[0; 4]; 3]),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1., 0., 0., 0.]; 3]);
        let strip = Mesh::new(
            PrimitiveTopology::TriangleStrip,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.; 3]; 3]);
        let strip16 = strip
            .clone()
            .with_inserted_indices(bevy::mesh::Indices::U16(vec![0, 1, 2]));
        let strip32 = strip.with_inserted_indices(bevy::mesh::Indices::U32(vec![0, 1, 2]));
        let key = MeshKey::new(&mesh, &mut layouts);
        assert!(key == MeshKey::new(&moved, &mut layouts));
        assert!(key != MeshKey::new(&skin, &mut layouts));
        assert!(key != MeshKey::new(&strip16, &mut layouts));
        assert!(MeshKey::new(&strip16, &mut layouts) != MeshKey::new(&strip32, &mut layouts));

        let surface = TitleSurface::default();
        let mut keys = HashSet::new();
        keys.insert((
            key.clone(),
            super::super::materials::SurfaceKey::from(&surface),
        ));
        let mut tinted = surface.clone();
        tinted.tint = Vec4::ZERO;
        assert!(!keys.insert((
            key.clone(),
            super::super::materials::SurfaceKey::from(&tinted)
        )));
        for variant in [
            TitleSurface {
                depth_write: false,
                ..surface.clone()
            },
            TitleSurface {
                blend: Some(resonance_events::effect::Blend::Alpha),
                ..surface.clone()
            },
            TitleSurface {
                cull: resonance_content::CullFace::None,
                ..surface.clone()
            },
            TitleSurface {
                toon_ramp: Some(Handle::default()),
                ..surface
            },
        ] {
            assert!(keys.insert((
                key.clone(),
                super::super::materials::SurfaceKey::from(&variant)
            )));
        }
    }

    #[test]
    fn warm_scene_only_exposes_selected_materials() {
        let mut world = World::new();
        let mut surfaces = Assets::<TitleSurface>::default();
        let surface = surfaces.add(TitleSurface::default());
        let root = world
            .spawn(Scene {
                materials: vec![Some(surface), None],
                loaded: true,
                converted: false,
                draws: Vec::new(),
            })
            .id();
        let selected = world
            .spawn((super::super::materials::MaterialSlot(0), ChildOf(root)))
            .id();
        let duplicate = world
            .spawn((super::super::materials::MaterialSlot(1), ChildOf(root)))
            .id();
        world.insert_resource(Shared::default());
        world.init_resource::<Resident>();
        world.insert_resource(Preparation {
            map: 340,
            roots: vec![root],
            entities: Vec::new(),
            scene_count: 1,
            converted: 0,
            started: Instant::now(),
            effects_copied: false,
            retained: Bindings::default(),
        });
        world.run_system_once(convert).unwrap();
        assert_eq!(
            world.get::<Visibility>(selected),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            world.get::<Visibility>(duplicate),
            Some(&Visibility::Hidden)
        );
        assert!(
            world
                .get::<MeshMaterial3d<TitleSurface>>(selected)
                .is_some()
        );
        assert!(
            world
                .get::<MeshMaterial3d<TitleSurface>>(duplicate)
                .is_none()
        );
        let report = world.resource::<Shared>().0.lock().unwrap();
        assert_eq!(report.expected, HashSet::from([selected.into()]));
        assert_eq!(world.resource::<Preparation>().converted, 1);
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
