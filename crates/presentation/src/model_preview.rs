//! Prepared, animated catalogue models. Loading belongs to the paused menu.
mod gpu;
mod material;
mod motion;
mod source;
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{
        RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::Tonemapping,
    ecs::system::SystemParam,
    gltf::Gltf,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::{
        extract_resource::ExtractResourcePlugin, render_resource::TextureFormat,
        sync_world::MainEntity,
    },
    sprite_render::Material2dPlugin,
    world_serialization::WorldInstanceReady,
};
use material::{Composite, Surface};
use resonance_content::model_preview::{ModelPreview, PreviewPart};
use resonance_game::menu::preview::PreviewId;
pub(super) use source::register;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
    time::Instant,
};
type Pending = crate::loading::Task<source::Bytes>;
const LAYER: usize = 29;

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "model_preview.wgsl");
    let shared = gpu::Shared::default();
    app.insert_resource(shared.clone())
        .init_resource::<gpu::Capture>()
        .add_plugins((
            MaterialPlugin::<Surface>::default(),
            Material2dPlugin::<Composite>::default(),
            ExtractResourcePlugin::<gpu::Capture>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (prepare, animate)
                .chain()
                .after(crate::field_view::FieldPreparation),
        )
        .add_systems(
            PostUpdate,
            (pose, motion::apply)
                .chain()
                .after(bevy::app::AnimationSystems)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    app.get_sub_app_mut(bevy::render::RenderApp)
        .unwrap()
        .insert_resource(shared)
        .add_systems(
            bevy::render::Render,
            (gpu::rendered, gpu::capture_submitted).in_set(bevy::render::RenderSystems::Cleanup),
        );
}

/// Readback must composite a completed offscreen pose, even with pipelined rendering.
pub(super) fn synchronize_capture(app: &mut App) -> Result<()> {
    let snapshot = |world: &World| {
        let field = &world.get_resource::<crate::new_game::Session>()?.field;
        let menu = field.menu.as_ref()?;
        let preview = menu.preview()?;
        Some((
            field.events.tick(),
            menu.tick,
            preview.id,
            preview.animation_tick,
        ))
    };
    let Some(expected) = snapshot(app.world()) else {
        return Ok(());
    };
    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.world_mut().resource_mut::<gpu::Capture>().0 = Some(completed.clone());
    let started = Instant::now();
    while !completed.load(Ordering::Acquire) {
        ensure!(
            started.elapsed().as_secs() < 10,
            "preview render fence timed out"
        );
        app.update();
        crate::playthrough::check_exit(app)?;
        ensure!(
            snapshot(app.world()) == Some(expected),
            "preview advanced during readback"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.world_mut().resource_mut::<gpu::Capture>().0 = None;
    Ok(())
}

#[derive(Component)]
struct Instantiated;
struct Part {
    spec: PreviewPart,
    gltf: Handle<Gltf>,
    textures: Vec<Handle<Image>>,
    templates: Vec<Handle<StandardMaterial>>,
    root: Option<Entity>,
    materials: Vec<Handle<Surface>>,
    graph: Handle<AnimationGraph>,
    clip: Option<AnimationNodeIndex>,
    players: Vec<Entity>,
    bones: BTreeMap<String, Entity>,
    ready: bool,
}
#[derive(Resource)]
struct Viewer {
    camera: Entity,
    quad: Entity,
    composite: Handle<Composite>,
    record: Option<Record>,
    job: Option<Pending>,
    parts: Vec<Part>,
    /// Keep shared asset handles alive while the next preview acquires its own.
    retired: Vec<Part>,
    sampled: crate::scene::SampledImages,
    started: Instant,
    ready: bool,
}
#[derive(Clone)]
struct Record {
    id: PreviewId,
    preview: Arc<ModelPreview>,
}
#[derive(SystemParam)]
pub(super) struct State<'w> {
    checkpoint: Option<ResMut<'w, crate::field_view::Session>>,
    live: Option<ResMut<'w, crate::new_game::Session>>,
}
impl State<'_> {
    fn menu(&mut self) -> Option<&mut resonance_game::menu::Menu> {
        if let Some(session) = &mut self.checkpoint {
            session.0.menu.as_mut()
        } else {
            self.live.as_mut()?.field.menu.as_mut()
        }
    }
}
#[derive(SystemParam)]
struct AssetsForPreview<'w> {
    images: ResMut<'w, Assets<Image>>,
    surfaces: ResMut<'w, Assets<Surface>>,
    composites: ResMut<'w, Assets<Composite>>,
    graphs: ResMut<'w, Assets<AnimationGraph>>,
    gltfs: Res<'w, Assets<Gltf>>,
}
#[derive(SystemParam)]
struct PreviewContext<'w> {
    source: Res<'w, source::Source>,
    server: Res<'w, AssetServer>,
    art: Option<Res<'w, crate::field_view::Art>>,
    shared: Res<'w, gpu::Shared>,
}
#[derive(SystemParam)]
struct PreviewEntities<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    nodes: Query<'w, 's, (&'static Name, &'static Transform, &'static ChildOf)>,
    meshes: Query<'w, 's, (), With<Mesh3d>>,
    standard: Query<'w, 's, &'static MeshMaterial3d<StandardMaterial>>,
    players: Query<'w, 's, (), With<AnimationPlayer>>,
    instantiated: Query<'w, 's, (), With<Instantiated>>,
}

fn setup(
    mut commands: Commands,
    display: Option<Res<crate::display::Display>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composites: ResMut<Assets<Composite>>,
) {
    let size = display.map_or_else(crate::Resolution::default, |d| d.0);
    let mut target = Image::new_target_texture(
        size.width,
        size.scene_height(),
        TextureFormat::Bgra8Unorm,
        None,
    );
    target.sampler = ImageSampler::linear();
    let target = images.add(target);
    let camera = commands
        .spawn((
            Camera3d::default(),
            Tonemapping::None,
            Msaa::Off,
            Camera {
                order: -10,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderLayers::layer(LAYER),
            RenderTarget::Image(target.clone().into()),
            Projection::custom(crate::camera::TitleProjection(PerspectiveProjection {
                fov: 15.834_f32.to_radians(),
                aspect_ratio: 10. / 7.,
                near: 100.,
                far: 10000.,
                ..default()
            })),
            Transform::default(),
        ))
        .id();
    let composite = composites.add(Composite {
        image: target,
        opacity: 0.,
    });
    let quad = commands
        .spawn((
            Mesh2d(meshes.add(Rectangle::new(640., 480.))),
            MeshMaterial2d(composite.clone()),
            Transform::from_xyz(0., 0., crate::field_ui::model_preview_depth()),
            Visibility::Hidden,
        ))
        .id();
    commands.insert_resource(Viewer {
        camera,
        quad,
        composite,
        record: None,
        job: None,
        parts: Vec::new(),
        retired: Vec::new(),
        sampled: Default::default(),
        started: Instant::now(),
        ready: false,
    });
}

fn prepare(
    mut commands: Commands,
    mut state: State,
    mut viewer: ResMut<Viewer>,
    mut assets: AssetsForPreview,
    context: PreviewContext,
    entities: PreviewEntities,
    mut exit: MessageWriter<AppExit>,
) {
    let server = &context.server;
    let source = &context.source;
    let shared = &context.shared;
    let children = &entities.children;
    let nodes = &entities.nodes;
    let result = (|| -> Result<()> {
        let Some(menu) = state.menu().filter(|m| m.preview().is_some()) else {
            commands.entity(viewer.quad).insert(Visibility::Hidden);
            commands
                .entity(viewer.camera)
                .entry::<Camera>()
                .and_modify(|mut c| c.is_active = false);
            return Ok(());
        };
        commands.entity(viewer.quad).insert(Visibility::Inherited);
        commands
            .entity(viewer.camera)
            .entry::<Camera>()
            .and_modify(|mut c| c.is_active = true);
        let selected = menu.preview().unwrap();
        if viewer.ready && viewer.record.as_ref().is_some_and(|r| r.id == selected.id) {
            return Ok(());
        }
        if viewer.record.as_ref().is_none_or(|r| r.id != selected.id) {
            let record = Record {
                id: selected.id,
                preview: Arc::new(selected.model.clone()),
            };
            for part in &viewer.parts {
                if part.spec.attached_to.is_none()
                    && let Some(root) = part.root
                {
                    commands.entity(root).despawn();
                }
            }
            viewer.retired = std::mem::take(&mut viewer.parts);
            viewer.job = Some(source.prepare(&record.preview)?);
            viewer.record = Some(record);
            viewer.started = Instant::now();
            viewer.ready = false;
            *shared.0.lock().unwrap() = Default::default();
            assets
                .composites
                .get_mut(&viewer.composite)
                .unwrap()
                .opacity = 0.;
            menu.busy = true;
        }
        let record = viewer.record.clone().unwrap();
        if let Some(job) = &viewer.job
            && let Some(result) = job.poll()?
        {
            *source.bytes.write().unwrap() = result?;
            viewer.job = None;
            viewer.parts = record
                .preview
                .parts
                .iter()
                .map(|spec| Part {
                    gltf: server.load(format!("preview://{}", spec.scene.mesh)),
                    textures: spec
                        .scene
                        .textures
                        .iter()
                        .map(|path| {
                            server
                                .load_builder()
                                .with_settings(|s: &mut ImageLoaderSettings| {
                                    s.is_srgb = false;
                                    s.sampler = ImageSampler::linear();
                                })
                                .load(format!("preview://{path}"))
                        })
                        .collect(),
                    spec: spec.clone(),
                    templates: Vec::new(),
                    root: None,
                    materials: Vec::new(),
                    graph: Handle::default(),
                    clip: None,
                    players: Vec::new(),
                    bones: BTreeMap::new(),
                    ready: false,
                })
                .collect();
        }
        let mut sampled = std::mem::take(&mut viewer.sampled);
        for part in &mut viewer.parts {
            for id in std::iter::once(part.gltf.id().untyped())
                .chain(part.textures.iter().map(|h| h.id().untyped()))
            {
                if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(id) {
                    anyhow::bail!("preview asset failed: {error}");
                }
            }
            if !assets.gltfs.contains(&part.gltf) {
                continue;
            }
            if !server.is_loaded_with_dependencies(part.gltf.id()) {
                continue;
            }
            if !part.textures.iter().all(|h| assets.images.contains(h.id())) {
                continue;
            }
            if part.root.is_none() {
                part.instantiate(&mut commands, &mut assets, &context, &record, &mut sampled)?;
            }
            let root = part.root.unwrap();
            if part.ready || !entities.instantiated.contains(root) {
                continue;
            }
            part.bind(root, &mut commands, &entities, &record, shared)?;
        }
        viewer.sampled = sampled;
        if !viewer.parts.is_empty() && viewer.parts.iter().all(|p| p.ready) {
            for part in &viewer.parts {
                if let Some(bone) = &part.spec.attached_to {
                    let &parent = viewer.parts[0]
                        .bones
                        .get(bone)
                        .with_context(|| format!("preview bone {bone} was not instantiated"))?;
                    commands.entity(part.root.unwrap()).insert(ChildOf(parent));
                }
            }
            let mut report = shared.0.lock().unwrap();
            report.expected.insert(viewer.quad.into());
            report.armed = true;
            if let Some(error) = &report.error {
                anyhow::bail!("preview pipeline failed: {error}");
            }
            if report.completed.load(Ordering::Acquire) {
                viewer.ready = true;
                viewer.retired.clear();
                menu.busy = false;
                assets
                    .composites
                    .get_mut(&viewer.composite)
                    .unwrap()
                    .opacity = 1.;
            }
        }
        ensure!(
            viewer.ready || viewer.started.elapsed().as_secs() < 60,
            "model preview preparation timed out for {:?}: parts {:?}; pending draws {:?}",
            record.id,
            viewer
                .parts
                .iter()
                .map(|p| (
                    &p.spec.scene.mesh,
                    p.root,
                    p.ready,
                    server.get_load_state(p.gltf.id())
                ))
                .collect::<Vec<_>>(),
            viewer
                .parts
                .iter()
                .filter_map(|p| p.root)
                .flat_map(|root| {
                    children
                        .iter_descendants(root)
                        .filter(|&entity| shared.0.lock().unwrap().pending.contains(&entity.into()))
                        .map(|entity| {
                            nodes
                                .get(entity)
                                .map(|(name, _, _)| name.as_str())
                                .unwrap_or("unnamed")
                                .to_owned()
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        );
        Ok(())
    })();
    if let Err(error) = result {
        error!("Model preview failed: {error:#}");
        debug_assert!(false, "Model preview failed: {error:#}");
        exit.write(AppExit::error());
    }
}

impl Part {
    fn instantiate(
        &mut self,
        commands: &mut Commands,
        assets: &mut AssetsForPreview,
        context: &PreviewContext,
        record: &Record,
        sampled: &mut crate::scene::SampledImages,
    ) -> Result<()> {
        let gltf = assets.gltfs.get(&self.gltf).unwrap();
        let scene = &self.spec.scene;
        self.templates = (0..scene.materials.len())
            .map(|i| {
                context
                    .server
                    .load(format!("preview://{}#Material{i}/std", scene.mesh))
            })
            .collect();
        ensure!(
            gltf.materials.len() == scene.materials.len(),
            "preview material count differs from its recipe"
        );
        ensure!(
            gltf.animations.len() == scene.clips.len(),
            "preview animation count differs from its recipe"
        );
        for (index, spec) in scene.materials.iter().enumerate() {
            let binding = |binding: &resonance_content::TextureBinding| {
                (self.textures[binding.texture].clone(), binding.clone())
            };
            let base = crate::materials::TitleSurface {
                uv_offsets: Vec4::from_array(
                    self.spec.uv_offsets.get(index).copied().unwrap_or([0.; 4]),
                ),
                multiply: crate::scene::sampled_image(
                    spec.multiply.as_ref().map(binding),
                    &mut assets.images,
                    sampled,
                ),
                constant_color: scene.outline_color.is_some(),
                tint: scene.outline_color.map_or(Vec4::ONE, |c| {
                    Vec4::from_array(c.map(|c| f32::from(c) / 255.))
                }),
                toon_ramp: (scene.outline_color.is_none() && spec.color.is_some())
                    .then(|| context.art.as_ref().unwrap().toon_ramp.clone()),
                field_light: Vec4::new(-600., -600., record.preview.elevation + 1400., 192.),
                shade_colors: [49., 66.].map(|v| Vec3::splat(v / 255.).extend(1.)),
                // Fade each surface so overlapping triangles remain visible.
                // The same prepared pipeline also handles full opacity.
                blend: true,
                additive: self.spec.additive,
                depth_write: spec.depth_write,
                cull: spec.cull,
                ..crate::materials::TitleSurface::textured(crate::scene::sampled_image(
                    spec.color.as_ref().map(binding),
                    &mut assets.images,
                    sampled,
                ))
            };
            self.materials.push(assets.surfaces.add(Surface {
                base,
                extension: default(),
            }));
        }
        let (graph, clips) = AnimationGraph::from_clips(gltf.animations.clone());
        self.graph = assets.graphs.add(graph);
        self.clip = clips.first().copied();
        self.root = Some(
            commands
                .spawn((
                    WorldAssetRoot(gltf.scenes.first().context("preview has no scene")?.clone()),
                    Visibility::Hidden,
                    RenderLayers::layer(LAYER),
                    Transform::default(),
                ))
                .observe(|event: On<WorldInstanceReady>, mut commands: Commands| {
                    commands.entity(event.entity).insert(Instantiated);
                })
                .id(),
        );
        Ok(())
    }

    fn bind(
        &mut self,
        root: Entity,
        commands: &mut Commands,
        entities: &PreviewEntities,
        record: &Record,
        shared: &gpu::Shared,
    ) -> Result<()> {
        let scene = &self.spec.scene;
        let names = crate::field_pose::named_bones(
            root,
            &entities.children,
            &entities.nodes,
            &entities.meshes,
        );
        if !scene.secondary_motion.is_empty() {
            let rig = crate::secondary_motion::Rig::new(scene, &names)
                .context("preview secondary-motion rig is incomplete")?;
            commands.entity(root).insert(rig);
        }
        self.bones = names
            .into_iter()
            .map(|(name, (entity, _, _))| (name, entity))
            .collect();
        for entity in entities.children.iter_descendants(root) {
            commands
                .entity(entity)
                .insert((RenderLayers::layer(LAYER), NoFrustumCulling));
            if entities.players.contains(entity) {
                commands
                    .entity(entity)
                    .insert(AnimationGraphHandle(self.graph.clone()));
                self.players.push(entity);
            }
            if let Ok(material) = entities.standard.get(entity) {
                let i = self
                    .templates
                    .iter()
                    .position(|h| h.id() == material.id())
                    .context("undeclared preview material")?;
                let hidden = self.spec.attached_to.is_none()
                    && scene.material_nodes.get(i).is_some_and(|indices| {
                        indices.iter().any(|&n| {
                            record
                                .preview
                                .hidden_geometry
                                .contains(&scene.bone_names[usize::from(n)])
                        })
                    });
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert((
                        MeshMaterial3d(self.materials[i].clone()),
                        crate::draw_order::DrawOrder(scene.materials[i].draw_order, 0),
                        if hidden {
                            Visibility::Hidden
                        } else {
                            Visibility::Inherited
                        },
                    ));
                if !hidden {
                    shared
                        .0
                        .lock()
                        .unwrap()
                        .expected
                        .insert(MainEntity::from(entity));
                }
            }
        }
        ensure!(
            self.clip.is_none() || !self.players.is_empty(),
            "animated preview has no animation player"
        );
        self.ready = true;
        commands.entity(root).insert(Visibility::Inherited);
        Ok(())
    }
}

fn animate(
    mut state: State,
    viewer: Res<Viewer>,
    mut players: Query<&mut AnimationPlayer>,
    mut surfaces: ResMut<Assets<Surface>>,
) {
    let Some(preview) = state.menu().and_then(|m| m.preview()) else {
        return;
    };
    for part in &viewer.parts {
        if !part.ready {
            continue;
        }
        for handle in &part.materials {
            surfaces.get_mut(handle).unwrap().base.tint.w = f32::from(preview.opacity) / 255.
                * part
                    .spec
                    .scene
                    .outline_color
                    .map_or(1., |c| f32::from(c[3]) / 255.);
        }
        if let Some(clip) = part.clip {
            let duration = part.spec.scene.clips[0].duration_ticks();
            let tick = preview.sample(duration);
            for &entity in &part.players {
                if let Ok(mut player) = players.get_mut(entity) {
                    player
                        .play(clip)
                        .pause()
                        .set_seek_time(tick as f32 / resonance_content::ANIMATION_HZ);
                }
            }
        }
    }
}

fn pose(mut state: State, viewer: Res<Viewer>, mut transforms: Query<&mut Transform>) {
    let Some(menu) = state.menu().filter(|m| m.preview().is_some()) else {
        return;
    };
    let Some(record) = &viewer.record else { return };
    let preview = menu.preview().unwrap();
    let distance = preview.distance;
    let slide = 16 - i32::from(preview.page_fade) * 316 / 256;
    let target = Vec3::new(
        -distance * 4.8_f32.to_radians().sin() + (slide / 4 - 16) as f32,
        0.,
        distance * 6.4_f32.to_radians().sin(),
    );
    let eye = target
        + Vec3::new(
            0.,
            -distance * 15_f32.to_radians().cos(),
            distance * 15_f32.to_radians().sin(),
        );
    *transforms.get_mut(viewer.camera).unwrap() =
        Transform::from_translation(eye).looking_at(target, Vec3::Z);
    for part in &viewer.parts {
        if !part.ready {
            continue;
        }
        if let Ok(mut transform) = transforms.get_mut(part.root.unwrap()) {
            *transform = if part.spec.attached_to.is_some() {
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
            } else {
                Transform::from_xyz(0., 0., record.preview.elevation)
                    .with_rotation(Quat::from_rotation_z(preview.yaw.to_radians()))
                    .with_scale(Vec3::splat(record.preview.scale))
            };
        }
        if part.spec.attached_to.is_none() {
            for node in &record.preview.node_scales {
                if node.unless_flag.is_some_and(|flag| {
                    menu.checkpoint
                        .as_ref()
                        .unwrap()
                        .progress
                        .event_flags
                        .contains(&flag)
                }) {
                    continue;
                }
                if let Some(&entity) = part.bones.get(&node.bone)
                    && let Ok(mut transform) = transforms.get_mut(entity)
                {
                    transform.scale = Vec3::from_array(node.scale);
                }
            }
        }
    }
}
