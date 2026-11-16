//! Display-list renderer for gameplay frames.
//!
//! The authored game is an ordered list of a few hundred draws with
//! fixed-function style materials. Each rendered frame the main world compiles
//! the visible draws of the cameras that share the scene image into a [`Frame`]:
//! sorted draw items, one record per draw, joint palettes and streamed
//! vertices. The render world uploads that data once and records one render
//! pass for scene, overlay and UI, then one for the final output conversion.
//!
//! It draws every active camera of the display without changing camera activity.
//! The renderer plugin removes Bevy's per-view visibility and camera extraction;
//! the native frame owns those tasks.
use crate::{
    draw_order::DrawOrder,
    materials::{
        GeometryData, GeometryFrame, GeometryVertex, SurfaceKey, SurfaceRecords, WorldRecords,
    },
};
use bevy::{
    asset::AssetId,
    camera::{
        RenderTarget,
        primitives::{Aabb, Frustum, Sphere},
        visibility::{InheritedVisibility, NoFrustumCulling, RenderLayers},
    },
    math::Affine3A,
    mesh::{
        Mesh2d, Mesh3d, MeshTag,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::Buffer,
};
use std::{
    hash::{BuildHasher, Hash},
    ops::Range,
    sync::Mutex,
};

pub(crate) mod render;
pub(crate) use render::Drawn;

/// The material of a 3D mesh the native frame draws.
#[derive(Component)]
pub struct MeshMaterial3d<M: Asset>(pub Handle<M>);
/// The material of a 2D mesh the native frame draws.
#[derive(Component)]
pub struct MeshMaterial2d<M: Asset>(pub Handle<M>);

macro_rules! material_component {
    ($name:ident) => {
        impl<M: Asset> Clone for $name<M> {
            fn clone(&self) -> Self {
                Self(self.0.clone())
            }
        }
        impl<M: Asset> Default for $name<M> {
            fn default() -> Self {
                Self(Handle::default())
            }
        }
        impl<M: Asset> std::fmt::Debug for $name<M> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_tuple(stringify!($name)).field(&self.0).finish()
            }
        }
        impl<M: Asset> PartialEq for $name<M> {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }
        impl<M: Asset> Eq for $name<M> {}
        impl<M: Asset> From<Handle<M>> for $name<M> {
            fn from(handle: Handle<M>) -> Self {
                Self(handle)
            }
        }
    };
}
material_component!(MeshMaterial3d);
material_component!(MeshMaterial2d);

/// A camera's distance fog for surfaces that take the view's fog: its color
/// and the start and end of its range.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct DistanceFog {
    pub color: Color,
    pub start: f32,
    pub end: f32,
}
impl Default for DistanceFog {
    /// Bevy's default distance fog, which the cameras used before.
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            start: 0.,
            end: 100.,
        }
    }
}

/// A 3D camera drawing into its own image, which a later camera samples, as
/// the model preview does. The native frame draws it first, with the scene.
#[derive(Component, Default)]
pub(crate) struct Offscreen;

/// A CPU-drawn panel shown over the window (the performance panel, the
/// developer overlay): the window's output pass draws its image at this
/// rectangle in physical pixels (x, y, width, height); an empty one hides it.
#[derive(Component, Clone, Copy)]
pub(crate) struct Panel {
    pub image: AssetId<Image>,
    pub rect: [f32; 4],
}

/// A mesh drawn over the completed scene. Field captures use their authored UVs;
/// battle particles sample a displaced, masked half-resolution scene copy.
#[derive(Component, Clone)]
pub(crate) struct SceneSample {
    pub mask: Option<Handle<Image>>,
    pub tint: Vec4,
    pub vertex_color: bool,
    pub depth_test: bool,
}
impl Default for SceneSample {
    fn default() -> Self {
        Self {
            mask: None,
            tint: Vec4::ONE,
            vertex_color: true,
            depth_test: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Phase {
    Scene,
    Particles,
    Captures,
}

/// Everything that selects a bind group and, with the mesh, a pipeline.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum Material {
    /// Field, battle and world surfaces: color, sampler source, multiply, toon.
    Surface {
        key: SurfaceKey,
        textures: [Option<AssetId<Image>>; 4],
    },
    /// Dialogue, menu and HUD artwork: source, sampler source, frame and color masks.
    Ui {
        key: crate::field_ui::SurfaceKey,
        textures: [AssetId<Image>; 4],
    },
    /// The scene image converted for presentation, under the premultiplied
    /// pause overlay when there is one (flagged in the draw's fourth parameter).
    Output {
        source: AssetId<Image>,
        overlay: Option<AssetId<Image>>,
    },
    /// Title, boot and movie artwork: premultiplied text plus an additive
    /// pulse, with the opacity and pulse in the draw's parameters.
    Text {
        source: AssetId<Image>,
    },
    /// The frozen field image under a menu, with its cover color in the
    /// draw's parameters.
    Backdrop {
        source: AssetId<Image>,
    },
    /// An offscreen camera's premultiplied image, scaled by the opacity in
    /// the draw's parameters: the model preview.
    Composite {
        source: AssetId<Image>,
    },
    /// The title's glow billboards: texture times vertex color, fourfold RGB
    /// gain, added to the scene.
    Glow {
        source: AssetId<Image>,
    },
    /// Ymir's water: the reflection camera's image sampled in screen space,
    /// tinted by the draw's parameters.
    Water {
        reflection: AssetId<Image>,
    },
    Refraction {
        atlas: [AssetId<Image>; 3],
    },
    SceneSample {
        mask: Option<AssetId<Image>>,
        depth_test: bool,
    },
}
impl Material {
    pub(crate) fn phase(&self) -> Phase {
        match self {
            Self::SceneSample { mask: Some(_), .. } => Phase::Particles,
            Self::SceneSample { mask: None, .. } => Phase::Captures,
            _ => Phase::Scene,
        }
    }
    /// The part of this material that selects its pipeline.
    pub(crate) fn prepare(&self) -> Prepare {
        match self {
            Self::Surface { key, .. } => Prepare::Surface(*key),
            Self::Ui { key, .. } => Prepare::Ui(*key),
            Self::Output { .. } => Prepare::Output,
            Self::Text { .. } => Prepare::Text,
            Self::Backdrop { .. } => Prepare::Backdrop,
            Self::Composite { .. } => Prepare::Composite,
            Self::Glow { .. } => Prepare::Glow,
            Self::Water { .. } => Prepare::Water,
            Self::Refraction { .. } => Prepare::Refraction,
            Self::SceneSample { mask, depth_test } => Prepare::SceneSample {
                masked: mask.is_some(),
                depth_test: *depth_test,
            },
        }
    }
}

/// What selects a draw's pipeline apart from its mesh and pass: preparations
/// build that pipeline before the draw is first visible.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Prepare {
    Surface(SurfaceKey),
    Ui(crate::field_ui::SurfaceKey),
    Output,
    Text,
    Backdrop,
    Composite,
    Glow,
    Water,
    Refraction,
    SceneSample { masked: bool, depth_test: bool },
}

impl From<&crate::materials::TitleSurface> for Prepare {
    fn from(surface: &crate::materials::TitleSurface) -> Self {
        Self::Surface(SurfaceKey::from(surface))
    }
}

impl From<&crate::materials::WorldSurface> for Prepare {
    fn from(surface: &crate::materials::WorldSurface) -> Self {
        Self::Surface(SurfaceKey::from(surface))
    }
}

impl From<&crate::field_ui::Surface> for Prepare {
    fn from(surface: &crate::field_ui::Surface) -> Self {
        Self::Ui(surface.native().1)
    }
}

#[derive(Clone)]
pub(crate) struct Draw {
    pub entity: Entity,
    /// Authored mesh/layout identity; UI uses its fixed vertex layout.
    pub mesh: Option<AssetId<Mesh>>,
    /// Index into [`Frame::materials`].
    pub material: u32,
    pub skinned: bool,
    /// Range in `vertices`, or `ui_vertices` when this draw has no mesh.
    pub geometry: Option<Range<u32>>,
    /// Slot in the coverage table region; zero masks nothing.
    pub coverage: u32,
    /// Transparent sort key of a 3D draw: the view depth Bevy's phase would
    /// sort it at, or NaN when that is its mesh's center, which only the
    /// render world knows.
    pub depth: f32,
}

/// One row of the per-frame draw table; `native_frame.wesl` `NativeDraw`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRecord {
    pub world_from_local: [[f32; 4]; 3],
    /// Material record row, first joint matrix, mesh tag bits, reserved.
    pub ids: [u32; 4],
    pub params: [f32; 4],
}

pub(crate) struct View {
    pub clip_from_world: Mat4,
    pub view_from_world: Mat4,
    /// Linear color and start/end range of the camera's distance fog.
    pub fog: Option<([f32; 4], [f32; 2])>,
    pub draws: Range<u32>,
    /// Whether the render world still sorts the draws by depth.
    pub sort: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Image(AssetId<Image>),
    /// A window's swap chain, by main-world entity.
    Window(Entity),
}

pub(crate) struct Pass {
    pub target: Target,
    /// Warm replacement field draws while the display keeps the last completed scene.
    pub retain_output: bool,
    pub samples: u32,
    pub clear: [f64; 4],
    pub depth: bool,
    pub views: Range<u32>,
    /// The first overlay view: everything before it is the scene proper,
    /// which is completed in an image of its own when an effect needs it.
    pub scene_end: u32,
    /// The scene image is kept before UI for dissolves.
    pub dissolve: Option<crate::field_dissolve::NativeDissolve>,
    /// Particles that sample the completed scene before UI.
    pub particles: bool,
    pub captures: bool,
    /// Heat haze, ripples and captured meshes over the completed scene.
    pub refraction: Option<crate::field_refraction::NativeRefraction>,
    /// SMAA at the end of the scene part, after the dissolve, as Bevy runs
    /// it at the end of the last 3D camera.
    pub smaa: Option<resonance_content::menu_data::AntiAliasing>,
    /// An offscreen camera's pass, which keeps its own scene images.
    pub offscreen: bool,
}

/// Per-target pass assembly, retaining offscreen storage between frames.
#[derive(Default)]
struct Passes {
    offscreen: Vec<Pass>,
    scene: Option<Pass>,
    output: Option<Pass>,
}
impl Passes {
    fn clear(&mut self) {
        self.offscreen.clear();
        self.scene = None;
        self.output = None;
    }

    fn append(&mut self, next: Pass, three_d: bool) -> Option<&'static str> {
        let current = if next.offscreen {
            self.offscreen
                .iter_mut()
                .find(|pass| pass.target == next.target)
        } else if next.depth {
            self.scene.as_mut()
        } else {
            self.output.as_mut()
        };
        if let Some(pass) = current {
            pass.views.end = next.views.end;
            pass.retain_output |= next.retain_output;
            pass.particles |= next.particles;
            pass.captures |= next.captures;
            let mut issue = None;
            if three_d {
                pass.scene_end = next.scene_end;
                // SMAA runs once, after the last 3D view.
                if pass.smaa.is_some() {
                    issue = Some("a later 3D camera draws over an SMAA image, smoothed once");
                }
                pass.smaa = next.smaa;
            }
            return issue;
        }
        if next.offscreen {
            self.offscreen.push(next);
        } else if next.depth {
            self.scene = Some(next);
        } else {
            self.output = Some(next);
        }
        None
    }
}

/// One rendered frame's display list. Buffers keep their capacity between frames.
#[derive(Default)]
pub(crate) struct Frame {
    pub passes: Vec<Pass>,
    pub views: Vec<View>,
    pub draws: Vec<Draw>,
    pub records: Vec<DrawRecord>,
    pub joints: Vec<[f32; 16]>,
    pub materials: Vec<Material>,
    pub vertices: GeometryFrame<GeometryVertex>,
    pub ui_vertices: GeometryFrame<crate::field_ui::UiVertex>,
    /// Encoded coverage tables for slots one onward.
    pub coverage: Vec<u8>,
    pub title_states: Option<Buffer>,
    pub world_states: Option<Buffer>,
    /// Panels and their window rectangles in physical pixels, drawn last in
    /// the window's output pass.
    pub panels: Vec<Panel>,
    /// Freeze the field for this menu opening before composing its panels.
    pub capture: Option<crate::menu_backdrop::Capture>,
    /// Battle entry's copy of the completed scene part, before overlays.
    pub scene_capture: Option<crate::menu_backdrop::SceneCapture>,
}
impl Frame {
    fn clear(&mut self) {
        self.panels.clear();
        self.capture = None;
        self.scene_capture = None;
        self.passes.clear();
        self.views.clear();
        self.draws.clear();
        self.records.clear();
        self.joints.clear();
        self.materials.clear();
        self.vertices.begin();
        self.ui_vertices.begin();
        self.coverage.clear();
    }
}

/// The compiled frame, handed to the render world at extraction.
#[derive(Resource, Default)]
pub(crate) struct Source(Mutex<Frame>);

#[derive(Clone, Copy)]
struct Candidate {
    entity: Entity,
    order: isize,
    scene: bool,
    /// The image an `Offscreen` camera draws; such a camera is neither a
    /// scene nor an output camera.
    offscreen: Option<AssetId<Image>>,
}

#[derive(Resource, Default)]
struct State {
    /// Active cameras on the scene image or framebuffer, in draw order.
    candidates: Vec<Candidate>,
    passes: Passes,
    /// What the last compiled frame could not draw, if anything, for how many
    /// frames in a row, and what the log last said about it.
    issue: &'static str,
    issue_frames: u32,
    reported: &'static str,
    /// The entity behind the issue, for the log.
    culprit: Option<Entity>,
    materials: HashMap<Material, u32>,
    palettes: HashMap<(AssetId<SkinnedMeshInverseBindposes>, u64), u32>,
    /// Each skinned draw's joint-list hash, valid while its skin is unchanged.
    skins: bevy::ecs::entity::EntityHashMap<(bevy::ecs::change_detection::Tick, u64)>,
    pending: Vec<Pending>,
    order: Vec<u32>,
    /// This frame's visible draw entities, in query order.
    visible3d: Vec<Entity>,
    visible2d: Vec<Entity>,
}
/// The entities visible to each compiled camera, including draws whose
/// materials are not ready yet. Scene admission compares this independent
/// visibility requirement with the render world's completed draws.
#[derive(Resource, Default)]
pub(crate) struct Compiled {
    pub views: HashMap<Entity, Vec<Entity>>,
}

/// Per-frame counters for diagnostics and benchmarks.
#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct Stats {
    pub native_scene: bool,
    pub native_output: bool,
    pub draws: usize,
    /// Rendered frames since startup, how many of them drew the scene, and
    /// how many drew scene-sampling particles.
    pub frames: u64,
    pub native_frames: u64,
    pub particle_frames: u64,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Compile;

/// Point the native frame at its images, as the game's display does: cameras
/// drawing into `source` draw the scene, and cameras drawing into
/// `framebuffer` the output, which shows the scene through a [`crate::TitleOutput`].
pub fn set_display_targets(world: &mut World, source: Handle<Image>, framebuffer: RenderTarget) {
    let output = match &framebuffer {
        RenderTarget::Image(image) => Some(image.handle.clone()),
        _ => None,
    };
    world.insert_resource(crate::display::Targets { source, output });
    world.insert_resource(crate::Framebuffer(framebuffer));
}

/// Register a material asset the native frame draws, unless already
/// registered: each module that creates such materials and the renderer itself
/// register them, in either order.
pub(crate) fn init_material<A: Asset>(app: &mut App) {
    if !app.world().contains_resource::<Assets<A>>() {
        app.init_asset::<A>();
    }
}

pub(super) fn install(app: &mut App) {
    init_material::<crate::materials::TitleSurface>(app);
    init_material::<crate::materials::WorldSurface>(app);
    init_material::<crate::TitleOutput>(app);
    init_material::<crate::materials::TitleText>(app);
    init_material::<crate::field_ui::Surface>(app);
    init_material::<crate::menu_backdrop::Material>(app);
    init_material::<crate::model_preview::material::Composite>(app);
    init_material::<crate::glow::GlowMaterial>(app);
    init_material::<crate::field_view::water::Surface>(app);
    bevy::shader::load_shader_library!(app, "native_frame.wesl");
    bevy::asset::embedded_asset!(app, "field_ui.wesl");
    bevy::asset::embedded_asset!(app, "native_surface_vertex.wesl");
    bevy::asset::embedded_asset!(app, "native_flat.wesl");
    bevy::asset::embedded_asset!(app, "native_scene_sample.wesl");
    bevy::asset::embedded_asset!(app, "field_refraction.wesl");
    bevy::shader::load_shader_library!(app, "effect_depth.wesl");
    app.init_resource::<Source>()
        .init_resource::<State>()
        .init_resource::<Compiled>()
        .init_resource::<Stats>();
    app.add_systems(
        Last,
        (select_cameras, compile)
            .chain()
            .run_if(display_ready)
            .in_set(Compile)
            .after(crate::materials::SurfacePublication)
            .after(crate::anti_aliasing::ConfigureCameras),
    );
    render::install(app);
}

/// The frame compiler reads the display's settings; apps without the display
/// stack draw nothing.
fn display_ready(world: &World) -> bool {
    world.contains_resource::<crate::anti_aliasing::AntiAliasingSettings>()
        && world.contains_resource::<ClearColor>()
        && world.contains_resource::<Assets<SkinnedMeshInverseBindposes>>()
}

/// Read logical camera activity after all scene updates, without changing it.
#[allow(clippy::too_many_arguments)]
fn select_cameras(
    mut state: ResMut<State>,
    targets: Option<Res<crate::display::Targets>>,
    framebuffer: Option<Res<crate::Framebuffer>>,
    caps: Option<Res<crate::anti_aliasing::AntiAliasingCapabilities>>,
    cameras: Query<(Entity, &Camera, &RenderTarget)>,
    primary: Query<Entity, With<bevy::window::PrimaryWindow>>,
    offscreen: Query<(), With<Offscreen>>,
) {
    state.candidates.clear();
    let (Some(targets), Some(framebuffer), Some(_)) = (targets, framebuffer, caps) else {
        return;
    };
    let image = |target: &RenderTarget| match target {
        RenderTarget::Image(image) => Some(image.handle.id()),
        _ => None,
    };
    // A camera may name the primary window either way; both draw into the
    // same swap chain.
    let primary = primary.single().ok();
    let window = |target: &bevy::window::WindowRef| match target {
        bevy::window::WindowRef::Primary => primary,
        bevy::window::WindowRef::Entity(entity) => Some(*entity),
    };
    let same = |a: &RenderTarget, b: &RenderTarget| match (a, b) {
        (RenderTarget::Image(a), RenderTarget::Image(b)) => a.handle.id() == b.handle.id(),
        (RenderTarget::Window(a), RenderTarget::Window(b)) => {
            use bevy::window::WindowRef;
            matches!((a, b), (WindowRef::Primary, WindowRef::Primary))
                || window(a).is_some_and(|a| window(b) == Some(a))
        }
        _ => false,
    };
    let source = targets.source.id();
    for (entity, camera, target) in &cameras {
        if !camera.is_active {
            continue;
        }
        if offscreen.contains(entity) {
            if let Some(image) = image(target) {
                state.candidates.push(Candidate {
                    entity,
                    order: camera.order,
                    scene: false,
                    offscreen: Some(image),
                });
            }
        } else if image(target) == Some(source) {
            state.candidates.push(Candidate {
                entity,
                order: camera.order,
                scene: true,
                offscreen: None,
            });
        } else if same(target, &framebuffer.0) {
            state.candidates.push(Candidate {
                entity,
                order: camera.order,
                scene: false,
                offscreen: None,
            });
        }
    }
    // Offscreen images are sampled by later cameras, so they are drawn first,
    // and each pass's views stay contiguous.
    state
        .candidates
        .sort_by_key(|candidate| (candidate.offscreen.is_none(), candidate.order));
}

type SceneCamera = (
    &'static Camera,
    &'static GlobalTransform,
    Option<&'static RenderLayers>,
    Has<Camera3d>,
    Option<&'static DistanceFog>,
    &'static Msaa,
    Option<&'static Frustum>,
    Option<&'static crate::anti_aliasing::Smaa>,
);
type Draw3d = (
    Entity,
    &'static Mesh3d,
    &'static GlobalTransform,
    &'static InheritedVisibility,
    Option<&'static RenderLayers>,
    Option<&'static DrawOrder>,
    (
        Option<&'static MeshMaterial3d<crate::TitleSurface>>,
        Option<&'static MeshMaterial3d<crate::materials::WorldSurface>>,
        Option<&'static MeshMaterial3d<crate::glow::GlowMaterial>>,
        Option<&'static MeshMaterial3d<crate::field_view::water::Surface>>,
        Option<&'static SceneSample>,
    ),
    Option<&'static SkinnedMesh>,
    Option<&'static GeometryData>,
    Option<&'static MeshTag>,
    Option<&'static Aabb>,
    (
        Has<NoFrustumCulling>,
        Has<bevy::mesh::morph::MeshMorphWeights>,
    ),
);
type Draw2d = (
    Entity,
    Option<&'static Mesh2d>,
    Option<&'static crate::field_ui::UiGeometry>,
    &'static GlobalTransform,
    &'static InheritedVisibility,
    Option<&'static RenderLayers>,
    Option<&'static MeshMaterial2d<crate::field_ui::Surface>>,
    Option<&'static MeshMaterial2d<crate::TitleOutput>>,
    Option<&'static MeshMaterial2d<crate::materials::TitleText>>,
    Option<&'static MeshMaterial2d<crate::menu_backdrop::Material>>,
    Option<&'static MeshMaterial2d<crate::model_preview::material::Composite>>,
);

type Draw2dFilter = Or<(With<Mesh2d>, With<crate::field_ui::UiGeometry>)>;
type DrawLabel = (
    NameOrEntity,
    Option<&'static ChildOf>,
    Has<MeshMaterial2d<crate::materials::TitleText>>,
    Has<MeshMaterial2d<crate::menu_backdrop::Material>>,
);

/// A visible draw awaiting its place in the view's order.
struct Pending {
    entity: Entity,
    order: Option<DrawOrder>,
    depth: f32,
    mesh: Option<AssetId<Mesh>>,
    material: u32,
    transform: Affine3A,
    ids: [u32; 4],
    params: [f32; 4],
    skin: Option<Entity>,
    geometry: Option<Entity>,
    coverage: u32,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Content<'w, 's> {
    draws3d: Query<'w, 's, Draw3d>,
    draws2d: Query<'w, 's, Draw2d>,
    /// Narrow views of the same entities, to find the few visible ones without
    /// fetching every draw's components once per camera.
    visible3d: Query<'w, 's, (Entity, &'static InheritedVisibility), With<Mesh3d>>,
    visible2d: Query<'w, 's, (Entity, &'static InheritedVisibility), Draw2dFilter>,
    skins: Query<'w, 's, Ref<'static, SkinnedMesh>>,
    geometry: Query<'w, 's, &'static GeometryData>,
    ui_geometry: Query<'w, 's, &'static crate::field_ui::UiGeometry>,
    joints: Query<'w, 's, &'static GlobalTransform>,
    bindposes: Res<'w, Assets<SkinnedMeshInverseBindposes>>,
    title: Option<ResMut<'w, SurfaceRecords>>,
    world: Res<'w, Assets<crate::materials::WorldSurface>>,
    world_records: Option<Res<'w, WorldRecords>>,
    ui: Res<'w, Assets<crate::field_ui::Surface>>,
    outputs: Res<'w, Assets<crate::TitleOutput>>,
    texts: Res<'w, Assets<crate::materials::TitleText>>,
    backdrops: Res<'w, Assets<crate::menu_backdrop::Material>>,
    composites: Res<'w, Assets<crate::model_preview::material::Composite>>,
    glows: Res<'w, Assets<crate::glow::GlowMaterial>>,
    waters: Res<'w, Assets<crate::field_view::water::Surface>>,
    captures: Query<'w, 's, &'static crate::menu_backdrop::Capture>,
    scene_captures: Query<'w, 's, &'static crate::menu_backdrop::SceneCapture>,
    panels: Query<'w, 's, &'static Panel>,
    /// Names for the log when an entity keeps the frame from this renderer.
    names: Query<'w, 's, DrawLabel>,
}

fn intern(materials: &mut HashMap<Material, u32>, frame: &mut Frame, material: Material) -> u32 {
    if let Some(index) = materials.get(&material) {
        return *index;
    }
    let index = frame.materials.len() as u32;
    frame.materials.push(material.clone());
    materials.insert(material, index);
    index
}

/// Collect and order visible scene draws; mesh-center depths are resolved later.
fn collect_3d(
    state: &mut State,
    frame: &mut Frame,
    content: &mut Content,
    layers: &RenderLayers,
    frustum: Option<&Frustum>,
    origin: f32,
    visible: &mut Vec<Entity>,
) -> &'static str {
    let mut unsupported = "";
    for (
        entity,
        mesh,
        transform,
        inherited,
        draw_layers,
        order,
        (title, world, glow, water, sample),
        skin,
        geometry,
        tag,
        aabb,
        (no_culling, morphed),
    ) in content
        .draws3d
        .iter_many(&state.visible3d)
        .filter_map(Result::ok)
    {
        if !inherited.get() || !layers.intersects(draw_layers.unwrap_or(&RenderLayers::layer(0))) {
            continue;
        }
        let model = transform.affine();
        if let (false, Some(aabb), Some(frustum)) = (no_culling, aabb, frustum) {
            let sphere = Sphere {
                center: model.transform_point3a(aabb.center),
                radius: transform.radius_vec3a(aabb.half_extents),
            };
            if !frustum.intersects_sphere(&sphere, false)
                || !frustum.intersects_obb(aabb, &model, true, false)
            {
                continue;
            }
        }
        visible.push(entity);
        if morphed {
            unsupported = "a visible mesh has morph targets";
            continue;
        }
        let mut water_params = None;
        let resolved = if let Some(sample) = sample {
            Some((
                Material::SceneSample {
                    mask: sample.mask.as_ref().map(Handle::id),
                    depth_test: sample.depth_test,
                },
                u32::from(sample.vertex_color),
            ))
        } else if let Some(world) = world {
            content.world.get(&world.0).and_then(|surface| {
                Some((
                    Material::Surface {
                        key: SurfaceKey::from(surface),
                        textures: [
                            surface.color.as_ref().map(Handle::id),
                            surface.color.as_ref().map(Handle::id),
                            surface.multiply.as_ref().map(Handle::id),
                            None,
                        ],
                    },
                    surface.row?,
                ))
            })
        } else if let Some(title) = title {
            content
                .title
                .as_mut()
                .and_then(|records| records.native(title.0.id()))
                .map(|surface| {
                    (
                        Material::Surface {
                            key: surface.key,
                            textures: surface.textures,
                        },
                        surface.row,
                    )
                })
        } else if let Some(glow) = glow {
            content.glows.get(&glow.0).map(|glow| {
                (
                    Material::Glow {
                        source: glow.texture.id(),
                    },
                    0,
                )
            })
        } else if let Some(water) = water {
            content.waters.get(&water.0).map(|water| {
                let (reflection, tint, dimensions) = water.native();
                water_params = Some((tint.to_array(), dimensions));
                (Material::Water { reflection }, 0)
            })
        } else {
            None
        };
        let Some((material, row)) = resolved else {
            unsupported = "a visible mesh has no native surface record";
            continue;
        };
        let (ids, params) = match (sample, water_params) {
            (Some(sample), _) => ([row, 0, 0, 0], sample.tint.to_array()),
            (_, Some((tint, size))) => ([row, 0, size.x.to_bits(), size.y.to_bits()], tint),
            _ => ([row, 0, tag.map_or(0, |tag| tag.value), 0], [0.; 4]),
        };
        let material = intern(&mut state.materials, frame, material);
        state.pending.push(Pending {
            entity,
            order: order.copied(),
            depth: f32::NAN,
            mesh: Some(mesh.0.id()),
            material,
            transform: model,
            ids,
            params,
            skin: skin.map(|_| entity),
            geometry: geometry.map(|_| entity),
            coverage: 0,
        });
    }
    // As `draw_order::apply` and Bevy's phase sort: unordered draws
    // first, then authored orders; an ordered draw then sorts at the
    // view depth of the origin plus its rank, an unordered one at the
    // depth of its mesh center. Both sorts are stable; the render
    // world does the second once it has the mesh centers.
    state.order.clear();
    state.order.extend(0..state.pending.len() as u32);
    let pending = &state.pending;
    state.order.sort_by_key(|index| {
        let draw = &pending[*index as usize];
        (frame.materials[draw.material as usize].phase(), draw.order)
    });
    for (rank, index) in state.order.iter().enumerate() {
        let pending = &mut state.pending[*index as usize];
        if pending.order.is_some()
            || frame.materials[pending.material as usize].phase() != Phase::Scene
        {
            pending.depth = origin + rank as f32;
        }
    }
    unsupported
}

/// Collect overlay draws, retaining their authored z order.
fn collect_2d(
    state: &mut State,
    frame: &mut Frame,
    content: &Content,
    layers: &RenderLayers,
    scene: bool,
    visible: &mut Vec<Entity>,
) -> Option<Entity> {
    let mut culprit = None;
    for (
        entity,
        mesh,
        geometry,
        transform,
        inherited,
        draw_layers,
        ui,
        output,
        text,
        backdrop,
        composite,
    ) in content
        .draws2d
        .iter_many(&state.visible2d)
        .filter_map(Result::ok)
    {
        if !inherited.get() || !layers.intersects(draw_layers.unwrap_or(&RenderLayers::layer(0))) {
            continue;
        }
        let mut params = [0.; 4];
        let mut coverage = 0;
        let material = if let Some(surface) = ui.and_then(|ui| content.ui.get(&ui.0)) {
            let (textures, key) = surface.native();
            if surface.native_coverage(&mut frame.coverage) {
                coverage = (frame.coverage.len() / render::COVERAGE_BYTES) as u32;
            }
            Some(Material::Ui { key, textures })
        } else if let (Some(output), false) = (
            output.and_then(|output| content.outputs.get(&output.0)),
            scene,
        ) {
            params = output.brightness.to_array();
            params[3] = f32::from(u8::from(output.overlay.is_some()));
            Some(Material::Output {
                source: output.source.id(),
                overlay: output.overlay.as_ref().map(Handle::id),
            })
        } else if let Some(text) = text.and_then(|text| content.texts.get(&text.0)) {
            params = text.opacity_pulse.to_array();
            Some(Material::Text {
                source: text.source.id(),
            })
        } else if let Some(backdrop) =
            backdrop.and_then(|backdrop| content.backdrops.get(&backdrop.0))
        {
            // A disabled backdrop's zero alpha draws nothing.
            let Some((source, cover)) = backdrop.native() else {
                continue;
            };
            params = cover.to_array();
            Some(Material::Backdrop { source })
        } else if let Some(composite) =
            composite.and_then(|composite| content.composites.get(&composite.0))
        {
            let (source, opacity) = composite.native();
            params[0] = opacity;
            Some(Material::Composite { source })
        } else {
            None
        };
        visible.push(entity);
        let Some(material) = material else {
            culprit = Some(entity);
            continue;
        };
        let material = intern(&mut state.materials, frame, material);
        state.pending.push(Pending {
            entity,
            order: None,
            depth: transform.translation().z,
            mesh: mesh.map(|mesh| mesh.0.id()),
            material,
            transform: transform.affine(),
            ids: [0; 4],
            params,
            skin: None,
            geometry: geometry.map(|_| entity),
            coverage,
        });
    }
    let pending = &state.pending;
    state.order.clear();
    state.order.extend(0..pending.len() as u32);
    state.order.sort_by(|a, b| {
        pending[*a as usize]
            .depth
            .total_cmp(&pending[*b as usize].depth)
    });
    culprit
}

/// Publish ordered draw records, sharing palettes between a model's primitives.
fn append_draws(state: &mut State, frame: &mut Frame, content: &Content) -> Range<u32> {
    let start = frame.draws.len() as u32;
    for index in 0..state.order.len() {
        let pending = &state.pending[state.order[index] as usize];
        let mut ids = pending.ids;
        let mut skinned = false;
        if let Some((entity, skin)) = pending
            .skin
            .and_then(|entity| Some((entity, content.skins.get(entity).ok()?)))
            && let Some(bindposes) = content.bindposes.get(&skin.inverse_bindposes)
        {
            // Primitives of one model share a palette; hash each joint
            // list once per change rather than once per draw per frame.
            let changed = skin.last_changed();
            let hash = match state.skins.get(&entity) {
                Some((tick, hash)) if *tick == changed => *hash,
                _ => {
                    let hash = bevy::platform::hash::FixedHasher.hash_one(&skin.joints);
                    state.skins.insert(entity, (changed, hash));
                    hash
                }
            };
            let key = (skin.inverse_bindposes.id(), hash);
            let base = *state.palettes.entry(key).or_insert_with(|| {
                let base = frame.joints.len() as u32;
                for (joint, bindpose) in skin.joints.iter().zip(bindposes.iter()) {
                    let joint = content
                        .joints
                        .get(*joint)
                        .map_or(Affine3A::IDENTITY, GlobalTransform::affine);
                    frame.joints.push((joint * *bindpose).to_cols_array());
                }
                base
            });
            ids[1] = base;
            skinned = true;
        }
        let geometry = pending.geometry.and_then(|entity| {
            if pending.mesh.is_none() {
                content
                    .ui_geometry
                    .get(entity)
                    .ok()
                    .map(|data| data.append_to(&mut frame.ui_vertices))
            } else {
                content
                    .geometry
                    .get(entity)
                    .ok()
                    .map(|data| data.append_to(&mut frame.vertices))
            }
        });
        let m = pending.transform;
        frame.records.push(DrawRecord {
            world_from_local: [
                [m.x_axis.x, m.y_axis.x, m.z_axis.x, m.translation.x],
                [m.x_axis.y, m.y_axis.y, m.z_axis.y, m.translation.y],
                [m.x_axis.z, m.y_axis.z, m.z_axis.z, m.translation.z],
            ],
            ids,
            params: pending.params,
        });
        frame.draws.push(Draw {
            entity: pending.entity,
            mesh: pending.mesh,
            material: pending.material,
            skinned,
            geometry,
            coverage: pending.coverage,
            depth: pending.depth,
        });
    }
    start..frame.draws.len() as u32
}

/// Compile the selected cameras' visible draws.
#[allow(clippy::too_many_arguments)]
fn compile(
    mut state: ResMut<State>,
    mut stats: ResMut<Stats>,
    source: Res<Source>,
    targets: Option<Res<crate::display::Targets>>,
    framebuffer: Option<Res<crate::Framebuffer>>,
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    clear: Res<ClearColor>,
    cameras: Query<SceneCamera>,
    dissolves: Query<crate::field_dissolve::NativeQuery>,
    refractions: Query<crate::field_refraction::NativeQuery>,
    mut content: Content,
    mut compiled: ResMut<Compiled>,
) {
    let _timer = crate::performance::smoothing_profile::Timer::start(6);
    let mut frame = source.0.lock().unwrap();
    frame.clear();
    *stats = Stats {
        native_scene: false,
        native_output: false,
        draws: 0,
        frames: stats.frames + 1,
        ..*stats
    };
    let state = &mut *state;
    state.materials.clear();
    state.palettes.clear();
    state.passes.clear();
    let (Some(targets), Some(framebuffer)) = (targets, framebuffer) else {
        compiled.views.clear();
        return;
    };
    // Keep each active camera's draw capacity, but never publish a retired view.
    compiled.views.retain(|entity, draws| {
        draws.clear();
        state
            .candidates
            .iter()
            .any(|candidate| candidate.entity == *entity)
            && cameras.contains(*entity)
    });
    let output_target = match &framebuffer.0 {
        RenderTarget::Image(image) => Some(Target::Image(image.handle.id())),
        RenderTarget::Window(bevy::window::WindowRef::Primary) => {
            windows.single().ok().map(Target::Window)
        }
        RenderTarget::Window(bevy::window::WindowRef::Entity(entity)) => {
            Some(Target::Window(*entity))
        }
        _ => None,
    };
    frame.title_states = content.title.as_ref().and_then(|r| r.buffer().cloned());
    frame.world_states = content
        .world_records
        .as_ref()
        .and_then(|r| r.buffer().cloned());
    let mut unsupported = "";
    let mut culprit = None;
    // Hidden draws (pooled effects, resident variants) far outnumber visible
    // ones; find the visible ones once, in query order, for every camera.
    let visible3d = &mut state.visible3d;
    visible3d.clear();
    visible3d.extend(
        content
            .visible3d
            .iter()
            .filter_map(|(entity, visible)| visible.get().then_some(entity)),
    );
    let visible2d = &mut state.visible2d;
    visible2d.clear();
    visible2d.extend(
        content
            .visible2d
            .iter()
            .filter_map(|(entity, visible)| visible.get().then_some(entity)),
    );
    // A later camera that clears the shared image replaces everything before it.
    let first = state
        .candidates
        .iter()
        .rposition(|candidate| {
            candidate.scene
                && cameras
                    .get(candidate.entity)
                    .is_ok_and(|camera| !matches!(camera.0.clear_color, ClearColorConfig::None))
        })
        .unwrap_or(usize::MAX);
    // The view whose completed image the scene-sampling particles read.
    let mut sampled = None;
    let mut refracted = None;
    for index in 0..state.candidates.len() {
        let candidate = state.candidates[index];
        let Ok((camera, transform, layers, three_d, fog, msaa, frustum, smaa)) =
            cameras.get(candidate.entity)
        else {
            continue;
        };
        if candidate.scene && (first == usize::MAX || index < first) {
            if first == usize::MAX {
                unsupported = "no scene camera clears the image";
            }
            // A later camera clears what this one would draw.
            compiled.views.entry(candidate.entity).or_default();
            continue;
        }
        let layers = layers.cloned().unwrap_or_default();
        let world_from_view = transform.to_matrix();
        let view_from_world = world_from_view.inverse();
        let clip_from_world = camera.clip_from_view() * view_from_world;
        state.pending.clear();
        let visible = compiled.views.entry(candidate.entity).or_default();
        if three_d {
            let issue = collect_3d(
                state,
                &mut frame,
                &mut content,
                &layers,
                frustum,
                view_from_world.row(2).w,
                visible,
            );
            if !issue.is_empty() {
                unsupported = issue;
            }
        } else if let Some(entity) = collect_2d(
            state,
            &mut frame,
            &content,
            &layers,
            candidate.scene,
            visible,
        ) {
            unsupported = "a visible 2D mesh has no native material";
            culprit = Some(entity);
        }
        // Every compiled view is listed, even one that drew nothing.
        let draws = append_draws(state, &mut frame, &content);
        let view = frame.views.len() as u32;
        frame.views.push(View {
            clip_from_world,
            view_from_world,
            fog: fog.map(|fog| (fog.color.to_linear().to_f32_array(), [fog.start, fog.end])),
            draws,
            sort: three_d && state.pending.iter().any(|pending| pending.depth.is_nan()),
        });
        let target = if let Some(image) = candidate.offscreen {
            Target::Image(image)
        } else if candidate.scene {
            Target::Image(targets.source.id())
        } else if let Some(target) = output_target {
            target
        } else {
            continue;
        };
        let color = match camera.clear_color {
            ClearColorConfig::Custom(color) => color,
            _ => clear.0,
        };
        let dissolve = dissolves
            .get(candidate.entity)
            .ok()
            .filter(|_| candidate.scene)
            .map(|item| crate::field_dissolve::NativeDissolve::new(candidate.entity, item));
        let pass = Pass {
            target,
            retain_output: candidate.scene
                && matches!(camera.output_mode, bevy::camera::CameraOutputMode::Skip),
            samples: msaa.samples(),
            clear: color.to_linear().to_f32_array().map(f64::from),
            depth: candidate.scene || candidate.offscreen.is_some(),
            views: view..view + 1,
            scene_end: view + 1,
            dissolve,
            particles: state
                .pending
                .iter()
                .any(|draw| frame.materials[draw.material as usize].phase() == Phase::Particles),
            captures: state
                .pending
                .iter()
                .any(|draw| frame.materials[draw.material as usize].phase() == Phase::Captures),
            refraction: None,
            smaa: smaa.filter(|_| three_d).map(|smaa| smaa.0),
            offscreen: candidate.offscreen.is_some(),
        };
        let particles = pass.particles;
        if let Some(issue) = state.passes.append(pass, three_d) {
            unsupported = issue;
        }
        if candidate.scene && three_d && particles {
            sampled = Some(view);
        }
        if candidate.scene
            && three_d
            && let Ok(item) = refractions.get(candidate.entity)
            && let Some(mut refraction) = crate::field_refraction::NativeRefraction::new(item)
            && let Some(pass) = &mut state.passes.scene
        {
            refraction.material = intern(
                &mut state.materials,
                &mut frame,
                Material::Refraction {
                    atlas: refraction.atlas,
                },
            );
            pass.refraction = Some(refraction);
            refracted = Some(view);
        }
        if candidate.scene
            && three_d
            && let Ok(capture) = content.captures.get(candidate.entity)
            && !capture.ready()
        {
            frame.capture = Some(capture.clone());
        }
        if candidate.scene
            && three_d
            && let Ok(capture) = content.scene_captures.get(candidate.entity)
            && !capture.0.ready()
        {
            frame.scene_capture = Some(capture.clone());
        }
    }
    if let (Some(view), Some(pass)) = (sampled, &state.passes.scene)
        && view + 1 != pass.scene_end
    {
        unsupported = "a later camera draws over scene-sampling particles";
    }
    // Bevy refracts at the end of the field camera's own passes.
    if let (Some(view), Some(pass)) = (refracted, &state.passes.scene)
        && view + 1 != pass.scene_end
    {
        unsupported = "a later camera draws over the refracted scene";
    }
    state.issue_frames = if unsupported == state.issue {
        state.issue_frames.saturating_add(1)
    } else {
        0
    };
    state.issue = unsupported;
    state.culprit = culprit;
    if state.skins.len() > 16_384 {
        // Entries of despawned draws accumulate slowly; start over.
        state.skins.clear();
    }
    // Content can be visible a frame before its material publishes; only an
    // issue that lasts is reported.
    if state.issue != state.reported && (state.issue.is_empty() || state.issue_frames == 30) {
        state.reported = state.issue;
        let name = |entity| {
            content
                .names
                .get(entity)
                .map(|(name, _, text, backdrop)| {
                    let kind = match (text, backdrop) {
                        (true, _) => "title text",
                        (_, true) => "menu backdrop",
                        _ => "other material",
                    };
                    format!("{name}, {kind}")
                })
                .unwrap_or_else(|_| format!("{entity}"))
        };
        match state.culprit {
            _ if state.issue.is_empty() => info!("Native frame renderer draws everything again"),
            Some(entity) => {
                let parent = content
                    .names
                    .get(entity)
                    .ok()
                    .and_then(|(_, parent, _, _)| parent)
                    .map(|parent| name(parent.parent()));
                warn!(
                    "Native frame renderer skips content: {} ({}, child of {})",
                    state.issue,
                    name(entity),
                    parent.as_deref().unwrap_or("nothing")
                );
            }
            None => warn!("Native frame renderer skips content: {}", state.issue),
        }
    }
    frame.passes.clear();
    frame.passes.append(&mut state.passes.offscreen);
    if let Some(pass) = state.passes.scene.take() {
        frame.passes.push(pass);
        stats.native_scene = true;
    }
    if let Some(pass) = state.passes.output.take() {
        if matches!(pass.target, Target::Window(_)) {
            frame.panels.extend(
                content
                    .panels
                    .iter()
                    .filter(|panel| panel.rect[2] > 0. && panel.rect[3] > 0.)
                    .copied(),
            );
        }
        frame.passes.push(pass);
        stats.native_output = true;
    }
    if frame.passes.is_empty() {
        frame.clear();
    }
    frame.vertices.finish();
    frame.ui_vertices.finish();
    stats.draws = frame.draws.len();
    stats.native_frames += u64::from(stats.native_scene);
    stats.particle_frames +=
        u64::from(stats.native_scene && frame.passes.iter().any(|pass| pass.particles));
}
