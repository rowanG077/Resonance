//! Prepared, animated catalogue models. Loading belongs to the paused menu.
use crate::gpu_prepare as gpu;
use crate::native::{MeshMaterial2d, MeshMaterial3d};
use crate::sparse_animation::affine::Pose;
pub(super) mod material;
mod motion;
mod prediction;
mod residency;
mod source;
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::{
    camera::{
        CameraProjection, RenderTarget, SubCameraView,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    ecs::system::SystemParam,
    gltf::Gltf,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::{
        extract_resource::ExtractResourcePlugin, render_resource::TextureFormat,
        sync_world::MainEntity,
    },
};
use material::{Composite, Surface};
use resonance_content::model_preview::{ModelPreview, PreviewPart};
use resonance_events::effect::Blend;
use resonance_game::menu::preview::PreviewId;
use resonance_model_behavior::{PoseOverrides, PreparedBehavior};
pub(super) use source::register;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
    time::Instant,
};
use symphonia_script_tools::PreparationCache;
type Pending = crate::loading::Task<source::Prepared>;
const LAYER: usize = 29;

pub(super) fn install(app: &mut App) {
    crate::native::init_material::<Composite>(app);
    let shared = gpu::Shared::default();
    app.insert_resource(shared.clone())
        .init_resource::<gpu::Capture>()
        .add_plugins(ExtractResourcePlugin::<gpu::Capture>::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (layout, prepare, residency::sync)
                .chain()
                .after(crate::field_view::FieldPreparation),
        )
        .add_systems(
            PostUpdate,
            (animate, motion::apply, prediction::present)
                .chain()
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
        let session = world.get_resource::<crate::new_game::Session>()?;
        let menu = session.menu()?;
        let preview = menu.preview()?;
        Some((
            session.events().tick(),
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

struct Part {
    spec: PreviewPart,
    gltf: Handle<Gltf>,
    textures: Vec<Handle<Image>>,
    toon: Option<Handle<Image>>,
    root: Option<Entity>,
    materials: Vec<Handle<Surface>>,
    clips: Vec<Handle<crate::sparse_animation::Clip>>,
    motions: Vec<std::sync::Arc<resonance_content::animation::Motion>>,
    clip: Option<usize>,
    binding: crate::sparse_animation::Binding,
    bones: BTreeMap<String, Entity>,
    ready: bool,
}
impl Part {
    /// Resolve the preview clock to authored 30-Hz frames for both pose evaluators.
    fn animation_sample<'a>(
        &'a self,
        preview: &resonance_game::menu::preview::Preview<'_>,
    ) -> Option<(&'a resonance_content::animation::Motion, f32)> {
        let index = self.clip?;
        let motion = &self.motions[index];
        let tick = preview.sample(self.spec.scene.clips[index].duration_ticks());
        let frame =
            tick as f32 * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ;
        Some((motion, frame.min(motion.duration_frames)))
    }

    /// The committed pose and its disposable endpoint use the prepared model's
    /// coordinates, animation and behavior. The caller retains sampling storage.
    fn sample_pose(
        &self,
        preview: &resonance_game::menu::preview::Preview<'_>,
        model: &ModelPreview,
        overrides: &PoseOverrides,
        index: usize,
        poses: &mut Vec<Pose>,
    ) -> Result<Transform> {
        ensure!(
            overrides
                .scales
                .keys()
                .all(|node| usize::from(node.part) < model.parts.len()),
            "behavior references a missing preview part"
        );
        self.binding
            .sample_poses(self.animation_sample(preview), poses)?;
        for (node, scale) in &overrides.scales {
            if usize::from(node.part) == index {
                let pose = poses
                    .get_mut(usize::from(node.bone))
                    .context("behavior references a missing bound node")?;
                *pose = pose.with_scale(Vec3::from_array(*scale));
            }
        }
        Ok(if self.spec.attached_to.is_some() {
            // Carried models share their parent bones' Z-up coordinates.
            Transform::IDENTITY
        } else {
            Transform::from_translation(preview_translation(overrides, model.elevation))
                .with_rotation(Quat::from_rotation_z(preview.yaw.to_radians()))
                .with_scale(Vec3::splat(model.scale))
        })
    }
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
    status: PreviewStatus,
    behaviors: PreparationCache,
    behavior: Option<PreparedBehavior>,
    behavior_tick: Option<u32>,
    overrides: PoseOverrides,
    prediction: prediction::Cache,
    dormant: Option<residency::Dormant>,
}
#[derive(Debug, PartialEq, Eq)]
enum PreviewStatus {
    Loading,
    Ready,
    Unavailable,
}
#[derive(Clone)]
struct Record {
    id: PreviewId,
    preview: Arc<ModelPreview>,
}

/// Capture the requested preview and its submitted-draw preparation fence.
pub(super) fn diagnostic(world: &World) -> Option<serde_json::Value> {
    let viewer = world.get_resource::<Viewer>()?;
    let record = viewer.record.as_ref()?;
    let preview = world
        .get_resource::<crate::new_game::Session>()?
        .menu()?
        .preview()?;
    let shared = world.get_resource::<gpu::Shared>()?.0.lock().unwrap();
    Some(serde_json::json!({
        "requested_id": preview.id,
        "retained_id": record.id,
        "document": record.preview.document,
        "status": format!("{:?}", viewer.status),
        "animation_tick": preview.animation_tick,
        "opacity": preview.opacity,
        "page_fade": preview.page_fade,
        "yaw": preview.yaw,
        "distance": preview.distance,
        "parts": viewer.parts.iter().map(|part| serde_json::json!({
            "mesh": part.spec.scene.mesh, "ready": part.ready,
            "materials": part.materials.len(), "animation_slot": part.spec.animation,
            "animation_duration_ticks": part.clip.map(|i| part.spec.scene.clips[i].duration_ticks()),
        })).collect::<Vec<_>>(),
        "draws": {
            "armed": shared.armed, "expected": shared.expected.len(),
            "pending": shared.pending.len(), "submitted": shared.submitted,
            "completed": shared.completed.load(Ordering::Acquire), "error": shared.error,
        },
    }))
}

#[derive(SystemParam)]
pub(super) struct State<'w> {
    live: Option<ResMut<'w, crate::new_game::Session>>,
}
impl State<'_> {
    fn menu(&self) -> Option<&resonance_game::menu::Menu> {
        self.live.as_ref()?.menu()
    }
    fn menu_mut(&mut self) -> Option<&mut resonance_game::menu::Menu> {
        self.live.as_mut()?.menu_mut()
    }
}
#[derive(SystemParam)]
struct AssetsForPreview<'w> {
    images: ResMut<'w, Assets<Image>>,
    surfaces: ResMut<'w, Assets<Surface>>,
    composites: ResMut<'w, Assets<Composite>>,
    clips: Res<'w, Assets<crate::sparse_animation::Clip>>,
    gltfs: Res<'w, Assets<Gltf>>,
    scenes: Res<'w, Assets<WorldAsset>>,
}
#[derive(SystemParam)]
struct PreviewContext<'w> {
    source: Res<'w, source::Source>,
    server: Res<'w, AssetServer>,
    resident: Option<Res<'w, crate::loading::Resident>>,
    shared: Res<'w, gpu::Shared>,
}
#[derive(SystemParam)]
struct PreviewEntities<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    nodes: Query<'w, 's, (&'static Name, &'static Transform, &'static ChildOf)>,
    instances: Query<'w, 's, &'static crate::model_asset::Instance>,
}

fn setup(
    mut commands: Commands,
    display: Option<Res<crate::display::Display>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composites: ResMut<Assets<Composite>>,
    sampled: Res<crate::scene::SampledImages>,
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
            Msaa::Off,
            Camera {
                order: -10,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderLayers::layer(LAYER),
            RenderTarget::Image(target.clone().into()),
            crate::native::Offscreen,
            preview_projection(size),
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
            Transform {
                translation: Vec3::new(0., 0., crate::field_ui::model_preview_depth()),
                scale: (size.ui_size() / Vec2::new(640., 480.)).extend(1.),
                ..default()
            },
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
        sampled: sampled.clone(),
        started: Instant::now(),
        status: PreviewStatus::Loading,
        behaviors: PreparationCache::default(),
        behavior: None,
        behavior_tick: None,
        overrides: PoseOverrides::default(),
        prediction: Default::default(),
        dormant: None,
    });
}

/// Expand clip coordinates into the display margins while retaining the authored
/// perspective, model placement and subpixel raster alignment.
#[derive(Debug, Clone)]
struct PreviewProjection {
    authored: crate::camera::TitleProjection,
    canvas: Vec2,
}
impl CameraProjection for PreviewProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        Mat4::from_scale(self.canvas.recip().extend(1.)) * self.authored.get_clip_from_view()
    }
    fn get_clip_from_view_for_sub(&self, view: &SubCameraView) -> Mat4 {
        let full = view.full_size.as_vec2();
        let size = view.size.as_vec2();
        let scale = full / size;
        let offset = (full - size - 2. * view.offset) / size * Vec2::new(1., -1.);
        Mat4::from_scale_rotation_translation(scale.extend(1.), Quat::IDENTITY, offset.extend(0.))
            * self.get_clip_from_view()
    }
    fn update(&mut self, _width: f32, _height: f32) {}
    fn far(&self) -> f32 {
        self.authored.far()
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [bevy::math::Vec3A; 8] {
        let scale = bevy::math::Vec3A::new(self.canvas.x, self.canvas.y, 1.);
        self.authored
            .get_frustum_corners(near, far)
            .map(|corner| corner * scale)
    }
}

fn preview_projection(size: crate::Resolution) -> Projection {
    Projection::custom(PreviewProjection {
        authored: crate::camera::TitleProjection(PerspectiveProjection {
            fov: 15.834_f32.to_radians(),
            aspect_ratio: 10. / 7.,
            near: 100.,
            far: 10000.,
            ..default()
        }),
        canvas: size.ui_size() / Vec2::new(640., 480.),
    })
}

fn layout(
    display: Res<crate::display::Display>,
    viewer: Res<Viewer>,
    mut images: ResMut<Assets<Image>>,
    composites: Res<Assets<Composite>>,
    mut projections: Query<&mut Projection>,
    mut transforms: Query<&mut Transform>,
) {
    if !display.is_changed() {
        return;
    }
    let size = display.0;
    let image = &composites.get(&viewer.composite).unwrap().image;
    let mut target = images.get_mut(image).unwrap();
    if target.size() != UVec2::new(size.width, size.scene_height()) {
        target.resize(bevy::render::render_resource::Extent3d {
            width: size.width,
            height: size.scene_height(),
            depth_or_array_layers: 1,
        });
    }
    *projections.get_mut(viewer.camera).unwrap() = preview_projection(size);
    transforms.get_mut(viewer.quad).unwrap().scale =
        (size.ui_size() / Vec2::new(640., 480.)).extend(1.);
}

fn prepare(
    mut commands: Commands,
    mut state: State,
    mut viewer: ResMut<Viewer>,
    mut assets: AssetsForPreview,
    context: PreviewContext,
    entities: PreviewEntities,
    mut failures: crate::field_view::Failures,
) {
    let server = &context.server;
    let source = &context.source;
    let shared = &context.shared;
    let children = &entities.children;
    let nodes = &entities.nodes;
    let result = (|| -> Result<()> {
        let Some(menu) = state.menu().filter(|m| m.preview().is_some()) else {
            viewer.behavior_tick = None;
            commands.entity(viewer.quad).insert(Visibility::Hidden);
            commands
                .entity(viewer.camera)
                .entry::<Camera>()
                .and_modify(|mut c| c.is_active = false);
            return Ok(());
        };
        let selected = menu.preview().unwrap();
        let same_selection = viewer.record.as_ref().is_some_and(|r| r.id == selected.id);
        if same_selection && viewer.status == PreviewStatus::Unavailable {
            if menu.busy {
                state.menu_mut().unwrap().busy = false;
            }
            return Ok(());
        }
        commands.entity(viewer.quad).insert(Visibility::Inherited);
        commands
            .entity(viewer.camera)
            .entry::<Camera>()
            .and_modify(|mut c| c.is_active = true);
        if same_selection && viewer.status == PreviewStatus::Ready {
            return Ok(());
        }
        let menu = state.menu_mut().unwrap();
        let selected = menu.preview().unwrap();
        if !same_selection {
            let record = Record {
                id: selected.id,
                preview: Arc::new(selected.model.clone()),
            };
            viewer.record = Some(record.clone());
            viewer.status = PreviewStatus::Loading;
            menu.busy = true;
            // Catalogue declarations have no GLB-derived bones yet. Preparation
            // validates hidden geometry and attachments against the loaded model.
            record.preview.validate_definition()?;
            viewer.behavior = None;
            viewer.behavior_tick = None;
            for part in &viewer.parts {
                if part.spec.attached_to.is_none()
                    && let Some(root) = part.root
                {
                    commands.entity(root).despawn();
                }
            }
            viewer.retired = std::mem::take(&mut viewer.parts);
            let library = context
                .resident
                .as_ref()
                .context("missing scene content")?
                .files
                .read()
                .unwrap()
                .as_ref()
                .context("missing scene content")?
                .library()
                .context("preview requires a named library")?
                .clone();
            viewer.job = Some(source.prepare(&record.preview, library)?);
            viewer.started = Instant::now();
            *shared.0.lock().unwrap() = Default::default();
            assets
                .composites
                .get_mut(&viewer.composite)
                .unwrap()
                .opacity = 0.;
        }
        if let Some(job) = &viewer.job
            && let Some(result) = job.poll()?
        {
            let prepared = result?;
            let record = Record {
                id: viewer.record.as_ref().unwrap().id,
                preview: prepared.model,
            };
            let behavior = record
                .preview
                .behavior
                .as_ref()
                .map(|binding| {
                    PreparedBehavior::prepare(
                        &mut viewer.behaviors,
                        &prepared.sources,
                        binding,
                        &record.preview,
                    )
                })
                .transpose()?;
            viewer.overrides = evaluate_behavior(behavior.as_ref(), menu)?;
            viewer.behavior = behavior;
            viewer.behavior_tick = Some(menu.tick);
            viewer.record = Some(record.clone());
            *source.files.write().unwrap() = Some(prepared.files);
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
                    toon: (spec.scene.outline_color.is_none()
                        && spec.scene.materials.iter().any(|m| m.color.is_some()))
                    .then(|| {
                        server
                            .load_builder()
                            .with_settings(|s: &mut ImageLoaderSettings| {
                                s.is_srgb = false;
                                s.sampler = ImageSampler::linear();
                            })
                            .load(format!(
                                "preview://{}",
                                resonance_content::model_preview::document::TOON_RAMP
                            ))
                    }),
                    spec: spec.clone(),
                    root: None,
                    materials: Vec::new(),
                    clips: spec
                        .scene
                        .clips
                        .iter()
                        .map(|clip| {
                            server
                                .load_builder()
                                .load(format!("preview://{}", clip.motion))
                        })
                        .collect(),
                    clip: None,
                    motions: Vec::new(),
                    binding: Default::default(),
                    bones: BTreeMap::new(),
                    ready: false,
                })
                .collect();
        }
        let record = viewer.record.clone().unwrap();
        let mut sampled = std::mem::take(&mut viewer.sampled);
        for part in &mut viewer.parts {
            for id in std::iter::once(part.gltf.id().untyped())
                .chain(
                    part.textures
                        .iter()
                        .chain(part.toon.iter())
                        .map(|h| h.id().untyped()),
                )
                .chain(part.clips.iter().map(|h| h.id().untyped()))
            {
                if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(id) {
                    anyhow::bail!("preview asset failed: {error}");
                }
                if let Some(bevy::asset::RecursiveDependencyLoadState::Failed(error)) =
                    server.get_recursive_dependency_load_state(id)
                {
                    anyhow::bail!("preview dependency failed: {error}");
                }
            }
            if !assets.gltfs.contains(&part.gltf) {
                continue;
            }
            if !server.is_loaded_with_dependencies(part.gltf.id()) {
                continue;
            }
            if !part.clips.iter().all(|h| assets.clips.contains(h))
                || !part
                    .textures
                    .iter()
                    .chain(part.toon.iter())
                    .all(|h| assets.images.contains(h.id()))
            {
                continue;
            }
            if part.root.is_none() {
                part.instantiate(&mut commands, &mut assets, &record, &mut sampled)?;
            }
            let root = part.root.unwrap();
            if part.ready || !entities.instances.contains(root) {
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
                viewer.status = PreviewStatus::Ready;
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
            viewer.status == PreviewStatus::Ready || viewer.started.elapsed().as_secs() < 60,
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
        fail(
            error,
            state.menu_mut(),
            &mut viewer,
            &mut commands,
            shared,
            &mut failures,
        );
    }
}

impl Part {
    fn instantiate(
        &mut self,
        commands: &mut Commands,
        assets: &mut AssetsForPreview,
        record: &Record,
        sampled: &mut crate::scene::SampledImages,
    ) -> Result<()> {
        let gltf = assets.gltfs.get(&self.gltf).unwrap();
        let scene = &self.spec.scene;
        let handle = gltf.scenes.first().context("preview has no scene")?;
        crate::model_asset::prepared(
            assets
                .scenes
                .get(handle)
                .context("loaded preview scene missing")?,
        )?
        .check_bones(scene.bone_names.len())?;

        ensure!(
            gltf.meshes.len() == scene.materials.len(),
            "preview material count differs from its recipe"
        );
        self.motions = self
            .clips
            .iter()
            .zip(&scene.clips)
            .map(|(handle, spec)| {
                assets
                    .clips
                    .get(handle)
                    .unwrap()
                    .bind(&scene.bone_names, spec)
            })
            .collect::<Result<_>>()?;
        for (index, spec) in scene.materials.iter().enumerate() {
            let binding = |binding: &resonance_content::TextureBinding| {
                (self.textures[binding.texture].clone(), binding.clone())
            };
            let base = crate::materials::TitleSurface {
                vertex_color: spec.vertex_color,
                uv_offsets: Vec4::from_array(
                    self.spec.uv_offsets.get(index).copied().unwrap_or([0.; 4]),
                ),
                multiply: crate::scene::sampled_image(
                    spec.multiply.as_ref().map(binding),
                    &mut assets.images,
                    sampled,
                ),
                constant_color: scene.outline_color_for(spec).is_some(),
                tint: scene.outline_color_for(spec).map_or(Vec4::ONE, |c| {
                    Vec4::from_array(c.map(|c| f32::from(c) / 255.))
                }),
                toon_ramp: spec.color.is_some().then(|| self.toon.clone()).flatten(),
                field_light: preview_light(Vec3::new(0., 0., record.preview.elevation)),
                shade_colors: [49., 66.].map(|v| Vec3::splat(v / 255.).extend(1.)),
                // Fade each surface so overlapping triangles remain visible.
                // The same prepared pipeline also handles full opacity.
                blend: Some(if self.spec.additive {
                    Blend::Additive
                } else {
                    Blend::Alpha
                }),
                depth_write: spec.depth_write,
                cull: spec.cull,
                ..crate::materials::TitleSurface::textured(crate::scene::sampled_image(
                    spec.color.as_ref().map(binding),
                    &mut assets.images,
                    sampled,
                ))
            };
            self.materials.push(assets.surfaces.add(base));
        }
        if let Some((index, _)) = self.spec.selected_clip()? {
            self.clip = Some(index);
        }
        let root = commands
            .spawn((
                Visibility::Hidden,
                RenderLayers::layer(LAYER),
                Transform::default(),
            ))
            .id();
        crate::model_asset::instantiate(commands, root, handle.clone());
        self.root = Some(root);
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
        let instance = entities.instances.get(root)?;
        let names = instance.named_bones();
        if !scene.secondary_motion.is_empty() {
            let rig = crate::secondary_motion::Rig::new(root, scene, &names)
                .context("preview secondary-motion rig is incomplete")?;
            commands.entity(root).insert(rig);
        }
        self.binding = instance.binding(scene.bone_names.len())?;
        self.bones = names
            .into_iter()
            .map(|(name, (entity, _, _))| (name, entity))
            .collect();
        for (node, &entity) in instance.data.nodes.iter().zip(&instance.nodes) {
            commands
                .entity(entity)
                .insert((RenderLayers::layer(LAYER), NoFrustumCulling));
            if let Some(draw) = &node.draw {
                let i = draw.slot.index(self.materials.len())?;
                let hidden = self.spec.attached_to.is_none()
                    && scene.material_nodes.get(i).is_some_and(|indices| {
                        indices.iter().any(|&n| {
                            record
                                .preview
                                .hidden_geometry
                                .contains(&scene.bone_names[usize::from(n)])
                        })
                    });
                commands.entity(entity).insert((
                    MeshMaterial3d(self.materials[i].clone()),
                    crate::draw_order::DrawOrder(
                        crate::draw_order::Layer::Scene(scene.materials[i].draw_order),
                        0,
                        0,
                    ),
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

        self.ready = true;
        commands.entity(root).insert(Visibility::Inherited);
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)] // Animation stores and the optional-preview failure boundary.
fn animate(
    mut commands: Commands,
    mut state: State,
    mut viewer: ResMut<Viewer>,
    mut transforms: Query<&mut Transform>,
    mut affine: ResMut<crate::sparse_animation::affine::Locals>,
    mut surfaces: ResMut<Assets<Surface>>,
    shared: Res<gpu::Shared>,
    mut failures: crate::field_view::Failures,
    mut poses: Local<Vec<Pose>>,
) {
    let Some(menu) = state.menu() else {
        return;
    };
    let Some(preview) = menu.preview() else {
        return;
    };
    if viewer.parts.is_empty() {
        return;
    }
    let result = (|| -> Result<()> {
        if viewer.behavior_tick != Some(menu.tick) {
            viewer.overrides = evaluate_behavior(viewer.behavior.as_ref(), menu)?;
            viewer.behavior_tick = Some(menu.tick);
        }
        let translation = preview_translation(
            &viewer.overrides,
            viewer.record.as_ref().unwrap().preview.elevation,
        );
        *transforms
            .get_mut(viewer.camera)
            .context("preview camera is missing")? = preview_camera(&preview);
        // Each ready part must be sampled before dynamics initialize, even while
        // another part is still loading.
        for (index, part) in viewer
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.ready)
        {
            for (index, handle) in part.materials.iter().enumerate() {
                let surface = &mut surfaces
                    .get_mut(handle)
                    .context("prepared preview material is missing")?;
                surface.field_light = preview_light(translation);
                surface.tint.w = f32::from(preview.opacity) / 255.
                    * part
                        .spec
                        .scene
                        .outline_color_for(&part.spec.scene.materials[index])
                        .map_or(1., |c| f32::from(c[3]) / 255.);
            }
            let root_pose = part.sample_pose(
                &preview,
                &viewer.record.as_ref().unwrap().preview,
                &viewer.overrides,
                index,
                &mut poses,
            )?;
            *transforms.get_mut(part.root.context("prepared preview root missing")?)? = root_pose;
            for (&(entity, rest), &pose) in part.binding.0.iter().zip(poses.iter()) {
                let mut local = rest;
                affine.set(entity, &mut local, pose);
                transforms.get_mut(entity)?.set_if_neq(local);
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        fail(
            error,
            state.menu_mut(),
            &mut viewer,
            &mut commands,
            &shared,
            &mut failures,
        );
    }
}

fn evaluate_behavior(
    behavior: Option<&PreparedBehavior>,
    menu: &resonance_game::menu::Menu,
) -> Result<PoseOverrides> {
    let Some(behavior) = behavior else {
        return Ok(PoseOverrides::default());
    };
    let checkpoint = menu
        .checkpoint
        .as_ref()
        .context("preview behavior has no checkpoint flags")?;
    behavior.evaluate(|flag| checkpoint.progress().event_flags.contains(&flag))
}

fn preview_camera(preview: &resonance_game::menu::preview::Preview<'_>) -> Transform {
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
    Transform::from_translation(eye).looking_at(target, Vec3::Z)
}

fn preview_translation(pose: &PoseOverrides, elevation: f32) -> Vec3 {
    pose.translation
        .map_or(Vec3::new(0., 0., elevation), Vec3::from_array)
}

fn preview_light(translation: Vec3) -> Vec4 {
    (translation + Vec3::new(-600., -600., 1400.)).extend(192.)
}

fn fail(
    error: anyhow::Error,
    menu: Option<&mut resonance_game::menu::Menu>,
    viewer: &mut Viewer,
    commands: &mut Commands,
    shared: &gpu::Shared,
    failures: &mut crate::field_view::Failures,
) {
    if !failures.skip("model preview", error) {
        return;
    }
    if let Some(menu) = menu {
        menu.busy = false;
    }
    viewer.status = PreviewStatus::Unavailable;
    viewer.job = None;
    viewer.behavior = None;
    viewer.behavior_tick = None;
    viewer.overrides = PoseOverrides::default();
    for part in viewer.parts.drain(..) {
        if let Some(root) = part.root {
            commands.entity(root).try_despawn();
        }
    }
    viewer.retired.clear();
    viewer.dormant = None;
    commands.entity(viewer.quad).insert(Visibility::Hidden);
    commands
        .entity(viewer.camera)
        .entry::<Camera>()
        .and_modify(|mut camera| camera.is_active = false);
    *shared.0.lock().unwrap() = Default::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_animation::Binding;
    use bevy::ecs::system::RunSystemOnce;

    fn part(spec: PreviewPart, binding: Binding) -> Part {
        Part {
            spec,
            gltf: Default::default(),
            textures: vec![],
            toon: None,
            root: None,
            materials: vec![],
            clips: vec![],
            motions: vec![],
            clip: None,
            binding,
            bones: Default::default(),
            ready: true,
        }
    }

    #[test]
    fn viewport_margins_reveal_models_without_moving_the_authored_view() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Composite>>();
        world.init_resource::<crate::scene::SampledImages>();
        world.insert_resource(crate::display::Display(crate::Resolution::default()));
        world.run_system_once(setup).unwrap();
        let (camera, quad, composite) = {
            let viewer = world.resource::<Viewer>();
            (viewer.camera, viewer.quad, viewer.composite.clone())
        };
        let image = world
            .resource::<Assets<Composite>>()
            .get(&composite)
            .unwrap()
            .image
            .clone();
        let baseline = world
            .get::<Projection>(camera)
            .unwrap()
            .get_clip_from_view();
        // This point is past the authored right edge but inside a wider display.
        let point = Vec3::new(225., 20., -1000.);
        let original = baseline.project_point3(point).truncate() * Vec2::new(320., 240.);
        assert!(original.x > 320. && original.x < 400.);
        for resolution in [
            "1280x720",
            "1920x1080",
            "2560x1080",
            "480x640",
            "640x640",
            "1080x1920",
            "800x600",
            "640x480",
        ] {
            let size: crate::Resolution = resolution.parse().unwrap();
            world.resource_mut::<crate::display::Display>().0 = size;
            world.run_system_once(layout).unwrap();
            assert_eq!(
                world
                    .resource::<Assets<Composite>>()
                    .get(&composite)
                    .unwrap()
                    .image,
                image
            );
            assert_eq!(
                world
                    .resource::<Assets<Image>>()
                    .get(&image)
                    .unwrap()
                    .size(),
                UVec2::new(size.width, size.scene_height())
            );
            let canvas = size.ui_size();
            let scale = world.get::<Transform>(quad).unwrap().scale;
            assert_eq!(scale, (canvas / Vec2::new(640., 480.)).extend(1.));
            let projection = world
                .get::<Projection>(camera)
                .unwrap()
                .get_clip_from_view();
            let projected = projection.project_point3(point).truncate() * canvas * 0.5;
            assert!(
                projected.abs_diff_eq(original, 0.0001),
                "model moved at {resolution}"
            );
            if size.aspect() > 4. / 3. {
                assert!(projected.x < canvas.x * 0.5, "display margin is clipped");
            } else if size.aspect() == 4. / 3. {
                assert_eq!(projection, baseline, "authored framing changed");
            }
        }
    }

    #[test]
    fn preview_poses_use_prepared_coordinates_and_thirty_hz_frames() -> Result<()> {
        use resonance_content::animation::{
            Channel, Interpolation, Motion, NodeTrack, Track, Transform as Trs,
        };
        let motion = Arc::new(Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![Arc::new(NodeTrack {
                    translation: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![[0.; 3], [30., 0., 0.]],
                    }),
                    ..NodeTrack::new(Trs::default())
                })],
            }],
        });
        let spec: PreviewPart = serde_json::from_value(serde_json::json!({
            "resource":1,"mesh":"test.glb","textures":[],"materials":[],
                "translation":[0.,0.,0.],"autoplay":false,"texture_animations":[],
                "clips":[{"motion":"test.motion","slots":[0],"duration_seconds":1.}],
            "animation":0,"attached_to":null,"additive":false
        }))?;
        let mut part = part(
            spec.clone(),
            Binding(vec![(Entity::PLACEHOLDER, Transform::IDENTITY)]),
        );
        part.motions.push(motion);
        part.clip = Some(0);
        let catalogue = ModelPreview {
            document: Default::default(),
            scale: 1.,
            elevation: 0.,
            parts: vec![spec],
            hidden_geometry: vec![],
            behavior: None,
        };
        // Named model documents can replace the catalogue's initial placement.
        let mut prepared = catalogue.clone();
        prepared.scale = 2.;
        prepared.elevation = 80.;
        let mut poses = Vec::new();
        for (tick, expected) in [(0, 0.), (1, 0.5), (21, 10.5), (60, 30.), (61, 0.5)] {
            let preview = resonance_game::menu::preview::Preview {
                id: PreviewId::Monster(0),
                model: &catalogue,
                yaw: 90.,
                distance: 1.,
                animation_tick: tick,
                opacity: 255,
                page_fade: 0,
            };
            for attached in [false, true] {
                part.spec.attached_to = attached.then(|| "hand".into());
                let root =
                    part.sample_pose(&preview, &prepared, &Default::default(), 0, &mut poses)?;
                assert_eq!(poses[0].global().translation(), Vec3::X * expected);
                if attached {
                    assert_eq!(root, Transform::IDENTITY);
                } else {
                    assert_eq!(root.translation, Vec3::Z * 80.);
                    assert_eq!(root.scale, Vec3::splat(2.));
                    assert!((root.rotation * Vec3::X).abs_diff_eq(Vec3::Y, 0.00001));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn failed_preview_releases_the_menu_and_hides_its_view_unless_paranoid() {
        #[derive(Resource)]
        struct Menu(resonance_game::menu::Menu);

        for paranoid in [false, true] {
            let mut world = World::new();
            world.init_resource::<Assets<Image>>();
            world.init_resource::<Assets<Mesh>>();
            world.init_resource::<Assets<Composite>>();
            world.init_resource::<crate::scene::SampledImages>();
            world.init_resource::<Messages<AppExit>>();
            world.init_resource::<gpu::Shared>();
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
            world.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
            world.run_system_once(setup).unwrap();
            world.resource_mut::<Viewer>().record = Some(Record {
                id: PreviewId::Monster(1),
                preview: Arc::new(ModelPreview {
                    document: None,
                    scale: 1.,
                    elevation: 0.,
                    parts: Vec::new(),
                    hidden_geometry: Vec::new(),
                    behavior: None,
                }),
            });
            let mut menu =
                resonance_game::menu::Menu::new(resonance_game::menu::Page::Monsters, None, false);
            menu.busy = true;
            world.insert_resource(Menu(menu));
            let (camera, quad) = {
                let viewer = world.resource::<Viewer>();
                (viewer.camera, viewer.quad)
            };
            world.get_mut::<Camera>(camera).unwrap().is_active = true;
            *world.get_mut::<Visibility>(quad).unwrap() = Visibility::Inherited;
            world.resource::<gpu::Shared>().0.lock().unwrap().armed = true;
            world
                .run_system_once(
                    |mut commands: Commands,
                     mut viewer: ResMut<Viewer>,
                     mut menu: ResMut<Menu>,
                     shared: Res<gpu::Shared>,
                     mut failures: crate::field_view::Failures| {
                        fail(
                            anyhow::anyhow!("preview texture failed to load"),
                            Some(&mut menu.0),
                            &mut viewer,
                            &mut commands,
                            &shared,
                            &mut failures,
                        );
                    },
                )
                .unwrap();
            assert_eq!(!world.resource::<Messages<AppExit>>().is_empty(), paranoid);
            assert_eq!(world.resource::<Menu>().0.busy, paranoid);
            assert_eq!(world.get::<Camera>(camera).unwrap().is_active, paranoid);
            assert_eq!(
                *world.get::<Visibility>(quad).unwrap(),
                if paranoid {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                }
            );
            assert_eq!(
                world.resource::<gpu::Shared>().0.lock().unwrap().armed,
                paranoid
            );
            assert_eq!(
                world.resource::<Viewer>().status == PreviewStatus::Unavailable,
                !paranoid
            );
            assert_eq!(
                world.resource::<Viewer>().record.as_ref().unwrap().id,
                PreviewId::Monster(1)
            );
            assert!(
                diagnostics.entries()[0]
                    .message
                    .contains("preview texture failed")
            );
        }
    }

    #[test]
    fn indexed_behavior_scales_reset_body_and_outline_without_an_animation() -> Result<()> {
        use resonance_content::{
            animation::{Motion, NodeTrack, Track, Transform as Trs},
            model_behavior::Node,
        };
        let mut world = World::new();
        let rest = Transform::from_xyz(1., 2., 3.);
        let entities: [[Entity; 2]; 2] = std::array::from_fn(|_| {
            std::array::from_fn(|_| world.spawn((Name::new("repeated"), rest)).id())
        });
        let spec: PreviewPart = serde_json::from_value(serde_json::json!({
            "mesh":"test.glb", "clips":[{"motion":"test.motion","slots":[0],"duration_seconds":1.}]
        }))?;
        let model = ModelPreview {
            document: None,
            scale: 1.,
            elevation: 0.,
            parts: vec![spec.clone(); 2],
            hidden_geometry: vec![],
            behavior: None,
        };
        let preview = resonance_game::menu::preview::Preview {
            id: PreviewId::Monster(0),
            model: &model,
            yaw: 0.,
            distance: 1.,
            animation_tick: 0,
            opacity: 255,
            page_fade: 0,
        };
        let affine = Arc::new(Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![
                    Arc::new(NodeTrack::new(Trs {
                        translation: [20.; 3],
                        ..Default::default()
                    })),
                    Arc::new(NodeTrack::new(Trs::default())),
                ],
            }],
        });
        let mut parts = entities.map(|nodes| {
            let mut part = part(
                spec.clone(),
                Binding(nodes.map(|entity| (entity, rest)).into()),
            );
            part.motions.push(affine.clone());
            part
        });
        let mut poses = Vec::new();
        for hidden in [true, false, true] {
            for (index, part) in parts.iter_mut().enumerate() {
                part.clip = hidden.then_some(0);
                let mut overrides = PoseOverrides::default();
                if hidden {
                    overrides.scales.insert(
                        Node {
                            part: index as u16,
                            bone: 0,
                        },
                        [0.; 3],
                    );
                }
                part.sample_pose(&preview, &model, &overrides, index, &mut poses)?;
                assert_eq!(
                    poses[0].global(),
                    if hidden {
                        GlobalTransform::from(Transform {
                            translation: Vec3::splat(20.),
                            scale: Vec3::ZERO,
                            ..Default::default()
                        })
                    } else {
                        GlobalTransform::from(rest)
                    }
                );
                assert_eq!(poses[1], rest.into());
                overrides.scales.insert(
                    Node {
                        part: index as u16,
                        bone: 2,
                    },
                    [1.; 3],
                );
                assert!(
                    part.sample_pose(&preview, &model, &overrides, index, &mut poses)
                        .is_err()
                );
            }
        }
        let translated = PoseOverrides {
            translation: Some([10., 20., -80.]),
            ..Default::default()
        };
        let translation = preview_translation(&translated, 0.);
        assert_eq!(translation, Vec3::new(10., 20., -80.));
        assert_eq!(
            preview_light(translation).truncate() - translation,
            Vec3::new(-600., -600., 1400.)
        );
        assert_eq!(
            preview_translation(&PoseOverrides::default(), 0.),
            Vec3::ZERO
        );
        Ok(())
    }
}
