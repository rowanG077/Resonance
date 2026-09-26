//! Ordinary battle entry: C020 pieces over the last completed field image.
//! The owner clock follows 6184's dispatch, then BDA8, then BBA0 order.
use anyhow::{Context, Result};
use bevy::{
    asset::RenderAssetUsages,
    camera::{
        RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::{
        Core2dSystems, FullscreenShader, core_2d::Transparent2d, core_3d::Transparent3d,
        schedule::Core2d, tonemapping::Tonemapping,
    },
    image::ImageSampler,
    mesh::PrimitiveTopology,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_phase::ViewSortedRenderPhases,
        render_resource::{
            binding_types::{sampler, texture_2d},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        sync_world::MainEntity,
        texture::GpuImage,
        view::ViewTarget,
    },
};
use resonance_game::battle::entry_transition::EntryTransition;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const LAYER: usize = 26;

/// Original data+20 dispatch entries 0,2,3,4. The initial 5878 call is inside
/// 5C38, not another callback. GPU/worker waits never call `advance`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Dispatch {
    #[default]
    Initialize,
    Loading(u32),
    Actors,
    Camera,
}
impl Dispatch {
    pub fn admitted(self, gpu_ready: bool) -> bool {
        // Hold the readiness-dependent boundary without inventing loader visits
        // whose number would otherwise depend on host compilation speed.
        !matches!(self, Self::Loading(80) | Self::Actors) || gpu_ready
    }
    pub fn advance(&mut self, transition: &mut EntryTransition) {
        match *self {
            Self::Loading(80) => transition.begin_camera_fade(),
            Self::Camera => transition.advance_camera_fade(),
            _ => {}
        }
        *self = match *self {
            Self::Initialize => Self::Loading(1),
            Self::Loading(80) => Self::Actors,
            Self::Loading(counter) => Self::Loading(counter + 1),
            Self::Actors | Self::Camera => Self::Camera,
        };
    }
    pub fn source(self) -> u8 {
        match self {
            Self::Initialize => 0,
            Self::Loading(_) => 2,
            Self::Actors => 3,
            Self::Camera => 4,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Initialize => "initialize_5c38",
            Self::Loading(_) => "loading_56a8",
            Self::Actors => "actors_40c8",
            Self::Camera => "camera_3ea4",
        }
    }
}

#[derive(Default)]
struct CopyStatus {
    encoded: AtomicBool,
    submitted: AtomicBool,
    completed: AtomicBool,
    error: Mutex<Option<String>>,
}
#[derive(Component, Clone, ExtractComponent)]
struct Capture {
    image: Handle<Image>,
    status: Arc<CopyStatus>,
}
#[derive(Component, Clone, ExtractComponent)]
struct DrawReady(Arc<Mutex<crate::model_preview::gpu::Report>>);

pub(super) struct View {
    capture: Capture,
    field_camera: Entity,
    pub field_tick: u32,
    published: bool,
    camera: Option<Entity>,
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<crate::field_ui::Surface>>,
    warm_target: Option<Handle<Image>>,
    report: DrawReady,
    switched: bool,
    last_timer: Option<u16>,
}
impl View {
    pub fn capture(world: &mut World) -> Result<Self> {
        let field_camera = world
            .query_filtered::<Entity, With<crate::FieldOverlayCamera>>()
            .single(world)?;
        let field_tick = world
            .resource::<crate::new_game::Session>()
            .field
            .events
            .tick();
        // Field fn_2_0 copies 640x448 in RGB565 without halving. Keep the same
        // authored sampling at higher host resolutions instead of changing UVs.
        let mut image = Image::new_target_texture(640, 448, TextureFormat::Bgra8Unorm, None);
        image.sampler = ImageSampler::linear();
        let image = world.resource_mut::<Assets<Image>>().add(image);
        Ok(Self {
            capture: Capture {
                image,
                status: Arc::default(),
            },
            field_camera,
            field_tick,
            published: false,
            camera: None,
            entity: None,
            mesh: None,
            material: None,
            warm_target: None,
            report: DrawReady(Arc::default()),
            switched: false,
            last_timer: None,
        })
    }
    pub fn needs_field_publication(&self) -> bool {
        !self.published
    }
    /// Called after the frozen request tick's field pose, effects, and UI publish.
    pub fn publish(&mut self, commands: &mut Commands) {
        if !self.published {
            commands
                .entity(self.field_camera)
                .insert(self.capture.clone());
            self.published = true;
        }
    }
    pub fn ready(&self) -> Result<bool> {
        if let Some(error) = &*self.capture.status.error.lock().unwrap() {
            anyhow::bail!("{error}");
        }
        let report = self.report.0.lock().unwrap();
        if let Some(error) = &report.error {
            anyhow::bail!("{error}");
        }
        Ok(self.switched && report.completed.load(Ordering::Acquire))
    }
    pub fn copied(&self) -> bool {
        self.capture.status.completed.load(Ordering::Acquire)
    }
    pub fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"field_tick": self.field_tick,
            "published_tick": self.published.then_some(self.field_tick),
            "capture_tick": self.copied().then_some(self.field_tick),
            "copy_encoded": self.capture.status.encoded.load(Ordering::Acquire),
            "copy_submitted": self.capture.status.submitted.load(Ordering::Acquire),
            "copy_completed": self.copied(), "target_owned": self.switched,
            "draw_completed": self.report.0.lock().unwrap().completed.load(Ordering::Acquire)})
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        transition: &EntryTransition,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<crate::field_ui::Surface>,
        target: RenderTarget,
    ) -> Result<bool> {
        self.ready()?;
        if self.camera.is_none() {
            let warm = images.add(Image::new_target_texture(
                64,
                64,
                TextureFormat::Bgra8Unorm,
                None,
            ));
            let mesh = meshes.add(geometry(transition));
            let material = materials.add(crate::field_ui::Surface::captured(
                self.capture.image.clone(),
            ));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    RenderLayers::layer(LAYER),
                    Transform::default(),
                    NoFrustumCulling,
                ))
                .id();
            let camera = commands
                .spawn((
                    Camera2d,
                    Msaa::Off,
                    Tonemapping::None,
                    Camera {
                        order: -13,
                        clear_color: ClearColorConfig::Custom(Color::BLACK),
                        ..default()
                    },
                    RenderTarget::Image(warm.clone().into()),
                    RenderLayers::layer(LAYER),
                    crate::camera::overlay_alignment(),
                    crate::battle::overlay_projection(),
                    self.report.clone(),
                ))
                .id();
            *self.report.0.lock().unwrap() = crate::model_preview::gpu::Report {
                armed: true,
                expected: [MainEntity::from(entity)].into(),
                ..default()
            };
            self.camera = Some(camera);
            self.entity = Some(entity);
            self.mesh = Some(mesh);
            self.material = Some(material);
            self.warm_target = Some(warm);
            self.last_timer = Some(transition.timer());
            return Ok(false);
        }
        if !self.switched
            && self.copied()
            && self
                .report
                .0
                .lock()
                .unwrap()
                .completed
                .load(Ordering::Acquire)
        {
            let camera = self.camera.unwrap();
            commands
                .entity(camera)
                .insert((target, crate::battle::overlay_projection()))
                .entry::<Camera>()
                .and_modify(|mut camera| camera.order = -2);
            // The real target, view bindings, and projection must submit once too.
            let mut report = self.report.0.lock().unwrap();
            report.completed = Arc::default();
            report.submitted = false;
            self.switched = true;
        }
        self.ready()
    }
    pub fn owns_target(&self) -> bool {
        self.switched
    }
    pub fn render(
        &mut self,
        transition: &EntryTransition,
        battle_drawn: bool,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        if let Some(camera) = self.camera {
            commands
                .entity(camera)
                .entry::<Camera>()
                .and_modify(move |mut camera| {
                    camera.clear_color = if battle_drawn {
                        ClearColorConfig::None
                    } else {
                        // Ordinary 5878/56A8 draws an opaque RGB128 quad
                        // through 4AB14's doubled/clamped TEV color.
                        ClearColorConfig::Custom(Color::WHITE)
                    };
                });
        }
        if let Some(entity) = self.entity {
            // Retain the nonempty GPU allocation even after BBA0 stops drawing.
            commands.entity(entity).insert(if transition.active() {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
        }
        if self.last_timer != Some(transition.timer())
            && let Some(mesh) = &self.mesh
        {
            *meshes
                .get_mut(mesh)
                .context("battle entry mesh disappeared")? = geometry(transition);
            self.last_timer = Some(transition.timer());
        }
        Ok(())
    }
    pub fn dispose(self, world: &mut World) {
        if let Ok(mut field) = world.get_entity_mut(self.field_camera) {
            field.remove::<Capture>();
        }
        if let Some(camera) = self.camera {
            world.despawn(camera);
        }
        if let Some(entity) = self.entity {
            world.despawn(entity);
        }
        if let Some(mesh) = self.mesh {
            world.resource_mut::<Assets<Mesh>>().remove(mesh.id());
        }
        if let Some(material) = self.material {
            world
                .resource_mut::<Assets<crate::field_ui::Surface>>()
                .remove(material.id());
        }
        let mut images = world.resource_mut::<Assets<Image>>();
        images.remove(self.capture.image.id());
        if let Some(warm) = self.warm_target {
            images.remove(warm.id());
        }
    }
}

/// BBA0 uses SDK Z*Y and PSMTXMultVec, then emits fixed z (not perspective).
fn geometry(transition: &EntryTransition) -> Mesh {
    let mut positions = Vec::with_capacity(186);
    let mut uv = Vec::with_capacity(186);
    let [width, height] = transition.viewport();
    for piece in transition.pieces() {
        let [y, z] = transition.rotation(piece);
        let (sz, cz) = z.sin_cos();
        let cy = y.cos();
        let matrix = [[cz * cy, -sz], [sz * cy, cz]];
        for (point, texel) in piece.points.iter().zip(piece.uv) {
            // SDK paired-single dot adds the translation to its second lane
            // before summing the lanes. Do not replace with a quaternion.
            let xy = [0, 1].map(|axis| {
                (matrix[axis][0] * point[0]) + (matrix[axis][1] * point[1] + piece.center[axis])
            });
            positions.push([
                xy[0] - width * 0.5,
                height * 0.5 - xy[1],
                transition.draw_depth(),
            ]);
            uv.push(texel);
        }
    }
    let color = [
        128. / 255.,
        128. / 255.,
        128. / 255.,
        f32::from(transition.alpha()) / 255.,
    ];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vec![color; transition.pieces().len() * 3],
    )
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "battle_entry.wgsl");
    app.add_plugins((
        ExtractComponentPlugin::<Capture>::default(),
        ExtractComponentPlugin::<DrawReady>::default(),
    ));
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, pipeline)
        .add_systems(Core2d, capture.in_set(Core2dSystems::EarlyPostProcess))
        .add_systems(Render, submitted.in_set(RenderSystems::Cleanup));
}
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
fn pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/battle-entry-copy",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resonance/battle-entry-copy".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: server.load("embedded://resonance_presentation/battle_entry.wgsl"),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Bgra8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(Pipeline {
        layout,
        id,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
    });
}
fn capture(
    view: ViewQuery<(&Capture, &ViewTarget)>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    mut context: RenderContext,
) {
    let (request, source) = view.into_inner();
    if request.status.encoded.load(Ordering::Acquire) {
        return;
    }
    if let CachedPipelineState::Err(error) = cache.get_render_pipeline_state(pipeline.id) {
        *request.status.error.lock().unwrap() = Some(format!("battle entry capture: {error}"));
        return;
    }
    let (Some(pipeline_id), Some(image)) = (
        cache.get_render_pipeline(pipeline.id),
        images.get(&request.image),
    ) else {
        return;
    };
    let binding = context.render_device().create_bind_group(
        "resonance/battle-entry-copy",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((source.main_texture_view(), &pipeline.sampler)),
    );
    {
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("resonance/battle-entry-copy"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &image.texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(default()),
                        store: StoreOp::Store,
                    },
                })],
                ..default()
            });
        pass.set_pipeline(pipeline_id);
        pass.set_bind_group(0, &binding, &[]);
        pass.draw(0..3, 0..1);
    }
    request.status.encoded.store(true, Ordering::Release);
}
#[allow(clippy::too_many_arguments)]
fn submitted(
    captures: Query<&Capture>,
    reports: Query<&DrawReady>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    quads: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let _ = device.poll(PollType::Poll);
    for capture in &captures {
        if capture.status.encoded.load(Ordering::Acquire)
            && !capture.status.submitted.swap(true, Ordering::AcqRel)
        {
            let status = capture.status.clone();
            queue.on_submitted_work_done(move || status.completed.store(true, Ordering::Release));
        }
    }
    for ready in &reports {
        crate::model_preview::gpu::render_report(
            &mut ready.0.lock().unwrap(),
            &phases,
            &quads,
            &cache,
            &device,
            &queue,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transition() -> EntryTransition {
        use resonance_content::battle_profile::ScreenBreak;
        use resonance_game::battle::entry::Random;
        let mut points = vec![[0.; 3]; 43];
        points[..4].copy_from_slice(&[
            [0., 0., 0.],
            [640., 0., 0.],
            [640., 480., 0.],
            [0., 480., 0.],
        ]);
        EntryTransition::new(
            &ScreenBreak {
                points,
                triangles: (0..62)
                    .map(|i| if i % 2 == 0 { [0, 1, 2] } else { [2, 3, 0] })
                    .collect(),
                viewport: [640., 480.],
                viewport_center: [320., 240.],
                center_weight: 0.333,
                center_expansion: 0.025,
                velocity_scale: 2.5,
                angular_base: 1.,
                angular_variation: 0.15,
                radians_per_degree: 0.017453292,
                secondary_rotation_scale: 1.5,
                draw_depth: -0.5,
            },
            [128; 3],
            &mut Random::from_state(55023),
            resonance_battle::SoundBinding {
                resource: 0,
                index: 130,
            },
        )
        .unwrap()
    }

    #[test]
    fn dispatch_initializes_inside_first_visit_and_waits_without_consuming_age() {
        let mut dispatch = Dispatch::default();
        let mut transition = transition();
        let mut sounds = Vec::new();
        dispatch.advance(&mut transition);
        transition.advance(false);
        assert_eq!(dispatch, Dispatch::Loading(1));
        for _ in 1..80 {
            dispatch.advance(&mut transition);
            if let Some(sound) = transition.advance(false) {
                sounds.push((transition.timer(), sound.index));
            }
        }
        assert_eq!(dispatch, Dispatch::Loading(80));
        for _ in 0..100 {
            assert!(!dispatch.admitted(false));
        }
        assert_eq!(transition.timer(), 80);
        assert!(transition.fade().is_none());
        assert_eq!(sounds, [(31, 130)]);
        assert_eq!(dispatch, Dispatch::Loading(80));
        dispatch.advance(&mut transition);
        transition.advance(false);
        assert_eq!(dispatch, Dispatch::Actors);
        assert_eq!(transition.fade().unwrap().alpha, 255);
        assert!(dispatch.admitted(true));
        dispatch.advance(&mut transition);
        transition.advance(false);
        assert_eq!(dispatch, Dispatch::Camera);
        assert_eq!(
            transition.timer(),
            82,
            "completed40C8/P0 follows82 actual owner callbacks"
        );
        assert!(transition.active());
        assert_eq!(transition.alpha(), 155);
        assert_eq!(transition.fade().unwrap().alpha, 255);
        for _ in 0..10 {
            transition.advance(true);
        }
        assert_eq!(transition.timer(), 82);
        for presentation in 1..=99 {
            dispatch.advance(&mut transition);
            transition.advance(false);
            assert_eq!(
                transition.fade().map_or(0, |fade| fade.alpha),
                255_u16.saturating_sub(presentation * 16) as u8,
            );
            if presentation == 3 {
                assert_eq!(transition.timer(), 85);
                assert_eq!(transition.alpha(), 143);
                assert_eq!(transition.fade().unwrap().alpha, 207);
            }
        }
        assert!(!transition.active());
    }

    #[test]
    fn mesh_keeps_table_order_uvs_and_sdk_rotation_without_perspective() {
        use bevy::mesh::VertexAttributeValues;
        let mut transition = transition();
        for _ in 0..58 {
            transition.advance(false);
        }
        let mesh = geometry(&transition);
        assert!(mesh.indices().is_none());
        assert_eq!(mesh.count_vertices(), 186);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("uv");
        };
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors");
        };
        for (i, piece) in transition.pieces().iter().enumerate() {
            let [y, z] = transition.rotation(piece).map(f64::from);
            for j in 0..3 {
                // Independent scalar Z(Y(point)) reference, then translation.
                let x = f64::from(piece.points[j][0]) * y.cos();
                let py = f64::from(piece.points[j][1]);
                let expected = [
                    z.cos() * x - z.sin() * py + f64::from(piece.center[0]) - 320.,
                    240. - (z.sin() * x + z.cos() * py + f64::from(piece.center[1])),
                ];
                for axis in 0..2 {
                    assert!(
                        (f64::from(positions[i * 3 + j][axis]) - expected[axis]).abs() < 0.0001
                    );
                }
                assert_eq!(positions[i * 3 + j][2], -0.5);
                assert_eq!(uvs[i * 3 + j], piece.uv[j]);
                assert_eq!(
                    colors[i * 3 + j],
                    [128. / 255., 128. / 255., 128. / 255., 251. / 255.]
                );
            }
        }
    }

    #[test]
    fn publication_arms_one_immutable_copy_and_disposal_removes_it() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let field_camera = world.spawn_empty().id();
        let image = world.resource_mut::<Assets<Image>>().add(Image::default());
        let mut view = View {
            capture: Capture {
                image: image.clone(),
                status: Arc::default(),
            },
            field_camera,
            field_tick: 39062,
            published: false,
            camera: None,
            entity: None,
            mesh: None,
            material: None,
            warm_target: None,
            report: DrawReady(Arc::default()),
            switched: false,
            last_timer: None,
        };
        assert!(view.needs_field_publication());
        view.publish(&mut world.commands());
        world.flush();
        assert!(!view.needs_field_publication());
        let request = world.get::<Capture>(field_camera).unwrap().clone();
        request.status.encoded.store(true, Ordering::Release);
        request.status.completed.store(true, Ordering::Release);
        for _ in 0..10 {
            view.publish(&mut world.commands());
            world.flush();
        }
        let retained = world.get::<Capture>(field_camera).unwrap();
        assert!(Arc::ptr_eq(&request.status, &retained.status));
        assert_eq!(retained.image.id(), image.id());
        assert!(view.copied());
        view.dispose(&mut world);
        assert!(world.get::<Capture>(field_camera).is_none());
        assert!(!world.resource::<Assets<Image>>().contains(&image));
    }
}
