//! Break the retained field image into tumbling pieces during battle loading.
use super::*;
use crate::model_preview::gpu;
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d},
    image::ImageSampler,
    mesh::PrimitiveTopology,
    render::{
        Render, RenderApp, RenderSystems,
        render_phase::ViewSortedRenderPhases,
        render_resource::{PipelineCache, TextureFormat},
        renderer::{RenderDevice, RenderQueue},
    },
    sprite_render::AlphaMode2d,
};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Mutex,
};

const LAYER: usize = 25;
const HOLD_TICKS: u16 = 30;
const FADE_START: u16 = 57;
const END_TICK: u16 = 120;
const FADE_STEP: f32 = 4. / 255.;
const EDGE_SPEED: f32 = 2.5;
const CRACK_SPREAD: f32 = 1.025;
const MIN_SPIN_DEGREES: f32 = 1.;
const SPIN_STEP_DEGREES: f32 = 0.15;
const SPIN_VARIANTS: u64 = 20;
const ROLL_MULTIPLIER: f32 = 1.5;

struct Piece {
    corners: [Vec2; 3],
    center: Vec2,
    spin: f32,
}

#[derive(Resource, Clone, Default)]
struct Warmup(Arc<Mutex<gpu::Report>>);

#[derive(Resource)]
pub(super) struct State {
    capture: crate::menu_backdrop::Capture,
    warmup: Warmup,
    target: Handle<Image>,
    capture_camera: Entity,
    camera: Entity,
    entity: Entity,
    mesh: Handle<Mesh>,
    pieces: Vec<Piece>,
    size: Vec2,
    color: Color,
    age: u16,
    activated: bool,
}

impl State {
    pub(super) fn ready(&self) -> Result<bool> {
        let warmup = self.warmup.0.lock().unwrap();
        if let Some(error) = &warmup.error {
            anyhow::bail!("battle transition: {error}");
        }
        Ok(self.capture.ready() && warmup.completed.load(Ordering::Acquire))
    }
}

pub(super) fn install(app: &mut App) {
    let warmup = Warmup::default();
    app.insert_resource(warmup.clone());
    app.add_systems(FixedUpdate, advance.after(super::advance))
        .add_systems(
            PostUpdate,
            render
                .in_set(PublishBattle)
                .before(bevy::camera::CameraUpdateSystems),
        );
    app.sub_app_mut(RenderApp)
        .insert_resource(warmup)
        .add_systems(Render, rendered.in_set(RenderSystems::Cleanup));
}

fn rendered(
    warmup: Res<Warmup>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    quads: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    gpu::render_report(
        &mut warmup.0.lock().unwrap(),
        &phases,
        &quads,
        &cache,
        &device,
        &queue,
    );
}

pub(super) fn begin(world: &mut World, white: bool, seed: u64) -> Result<()> {
    let source = world.resource::<crate::display::Targets>().source.clone();
    let descriptor = world
        .resource::<Assets<Image>>()
        .get(&source)
        .context("battle transition lost its outgoing image")?
        .texture_descriptor
        .size;
    let capture_camera = world
        .query_filtered::<Entity, With<crate::FieldOverlayCamera>>()
        .iter(world)
        .next()
        .context("battle transition lost its outgoing camera")?;
    let mut image = Image::new_target_texture(
        descriptor.width,
        descriptor.height,
        TextureFormat::Bgra8Unorm,
        None,
    );
    image.sampler = ImageSampler::linear();
    let image = world.resource_mut::<Assets<Image>>().add(image);
    let capture = crate::menu_backdrop::Capture::once(source.clone(), image.clone());
    world.entity_mut(capture_camera).insert(capture.clone());
    let size = world.resource::<crate::display::Display>().0.ui_size();
    let pieces = pieces(seed);
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(mesh(&pieces, size, 0));
    let material = world
        .resource_mut::<Assets<ColorMaterial>>()
        .add(ColorMaterial {
            texture: Some(image),
            alpha_mode: AlphaMode2d::Blend,
            ..default()
        });
    let entity = world
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::default(),
            RenderLayers::layer(LAYER),
            NoFrustumCulling,
        ))
        .id();
    let projection = overlay_projection(world.resource::<crate::display::Display>().0);
    let warm_target = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            1,
            1,
            TextureFormat::Bgra8Unorm,
            None,
        ));
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            Tonemapping::None,
            Camera {
                order: -2,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderTarget::Image(warm_target.into()),
            RenderLayers::layer(LAYER),
            projection,
        ))
        .id();
    let warmup = world.resource::<Warmup>().clone();
    *warmup.0.lock().unwrap() = gpu::Report {
        armed: true,
        expected: [entity.into()].into(),
        ..default()
    };
    world.insert_resource(State {
        capture,
        warmup,
        target: source,
        capture_camera,
        camera,
        entity,
        mesh,
        pieces,
        size,
        color: if white { Color::WHITE } else { Color::BLACK },
        age: 0,
        activated: false,
    });
    Ok(())
}

pub(super) fn dispose(world: &mut World) {
    if let Some(state) = world.remove_resource::<State>() {
        state.warmup.0.lock().unwrap().armed = false;
        if let Ok(mut camera) = world.get_entity_mut(state.capture_camera) {
            camera.remove::<crate::menu_backdrop::Capture>();
        }
        world.despawn(state.entity);
        world.despawn(state.camera);
    }
}

fn advance(state: Option<ResMut<State>>) {
    if let Some(mut state) = state
        && state.ready().unwrap_or(false)
    {
        state.age = state.age.saturating_add(1).min(END_TICK);
    }
}

fn render(
    mut commands: Commands,
    state: Option<ResMut<State>>,
    owner: Option<Res<Owner>>,
    display: Res<crate::display::Display>,
    mut cameras: Query<&mut Camera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut state) = state else {
        return;
    };
    match state.ready() {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            error!("{error:#}");
            exit.write(AppExit::error());
            return;
        }
    }
    commands
        .entity(state.capture_camera)
        .remove::<crate::menu_backdrop::Capture>();
    let battle_visible = owner.as_ref().is_some_and(|owner| owner.presenting());
    if battle_visible && state.age == END_TICK {
        state.warmup.0.lock().unwrap().armed = false;
        commands.entity(state.entity).despawn();
        commands.entity(state.camera).despawn();
        commands.remove_resource::<State>();
        return;
    }
    if !state.activated {
        // Retargeting also refreshes the computed viewport and projection.
        commands.entity(state.camera).insert((
            RenderTarget::Image(state.target.clone().into()),
            overlay_projection(display.0),
        ));
        state.activated = true;
    }
    if let Ok(mut camera) = cameras.get_mut(state.camera) {
        camera.clear_color = if battle_visible {
            ClearColorConfig::None
        } else {
            ClearColorConfig::Custom(state.color)
        };
    }
    if let Some(mut target) = meshes.get_mut(&state.mesh) {
        *target = mesh(&state.pieces, state.size, state.age);
    }
}

fn random(seed: u64, index: usize) -> u64 {
    let mut hash = DefaultHasher::new();
    (seed, index).hash(&mut hash);
    hash.finish()
}

fn pieces(seed: u64) -> Vec<Piece> {
    const COLUMNS: usize = 8;
    const ROWS: usize = 4;
    let point = |x: usize, y: usize| {
        let index = y * (COLUMNS + 1) + x;
        let jitter =
            |axis| (random(seed, index * 2 + axis) as f64 / u64::MAX as f64 - 0.5) as f32 * 0.5;
        Vec2::new(
            (x as f32 + if x > 0 && x < COLUMNS { jitter(0) } else { 0. }) / COLUMNS as f32,
            (y as f32 + if y > 0 && y < ROWS { jitter(1) } else { 0. }) / ROWS as f32,
        )
    };
    let mut result = Vec::with_capacity(COLUMNS * ROWS * 2);
    for y in 0..ROWS {
        for x in 0..COLUMNS {
            let [a, b, c, d] = [
                point(x, y),
                point(x + 1, y),
                point(x + 1, y + 1),
                point(x, y + 1),
            ];
            let triangles = if (x + y) % 2 == 0 {
                [[a, b, c], [a, c, d]]
            } else {
                [[a, b, d], [b, c, d]]
            };
            for corners in triangles {
                let center = corners.iter().sum::<Vec2>() / 3.;
                let spin = (MIN_SPIN_DEGREES
                    + SPIN_STEP_DEGREES * (random(seed, result.len()) % SPIN_VARIANTS) as f32)
                    .to_radians();
                result.push(Piece {
                    corners,
                    center,
                    spin,
                });
            }
        }
    }
    result
}

fn mesh(pieces: &[Piece], size: Vec2, age: u16) -> Mesh {
    let motion = age.saturating_sub(HOLD_TICKS) as f32;
    let alpha = if age >= END_TICK {
        0.
    } else {
        1. - age.saturating_sub(FADE_START) as f32 * FADE_STEP
    };
    let mut positions = Vec::with_capacity(pieces.len() * 3);
    let mut uv = Vec::with_capacity(positions.capacity());
    for piece in pieces {
        let center = (piece.center - Vec2::splat(0.5)) * size;
        let center = center * CRACK_SPREAD;
        let offset = center / (size * 0.5) * EDGE_SPEED * motion;
        let angle = piece.spin * motion;
        let rotation = Mat2::from_angle(angle * ROLL_MULTIPLIER);
        for corner in piece.corners {
            let mut local = (corner - piece.center) * size;
            local.x *= angle.cos();
            let point = center + offset + rotation * local;
            positions.push([point.x, -point.y, 0.]);
            uv.push(corner.to_array());
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1., 1., 1., alpha]; count])
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn fragments_hold_then_tumble_and_finish_transparent() {
        let pieces = pieces(7);
        let area: f32 = pieces
            .iter()
            .map(|piece| {
                let [a, b, c] = piece.corners;
                (b - a).perp_dot(c - a).abs() * 0.5
            })
            .sum();
        assert!((area - 1.).abs() < 0.00001);
        let held = mesh(&pieces, Vec2::new(640., 480.), 0);
        let before = mesh(&pieces, Vec2::new(640., 480.), HOLD_TICKS);
        let moving = mesh(&pieces, Vec2::new(640., 480.), HOLD_TICKS + 1);
        assert_eq!(
            held.attribute(Mesh::ATTRIBUTE_POSITION),
            before.attribute(Mesh::ATTRIBUTE_POSITION)
        );
        assert_ne!(
            held.attribute(Mesh::ATTRIBUTE_POSITION),
            moving.attribute(Mesh::ATTRIBUTE_POSITION)
        );
        let end = mesh(&pieces, Vec2::new(640., 480.), END_TICK);
        let Some(VertexAttributeValues::Float32x4(colors)) = end.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("fragment colors");
        };
        assert!(colors.iter().all(|color| color[3] == 0.));
    }
}
