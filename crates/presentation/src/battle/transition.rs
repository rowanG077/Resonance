//! Hold the retained field image until the battle is ready, then shatter it.
use super::*;
use crate::gpu_prepare as gpu;
use crate::native::MeshMaterial2d;
use bevy::{asset::RenderAssetUsages, image::ImageSampler, mesh::PrimitiveTopology};
use std::hash::{DefaultHasher, Hash, Hasher};

const LAYER: usize = 25;
const HOLD_TICKS: u16 = 22;
const FADE_START: u16 = 43;
const END_TICK: u16 = 90;
const ENTRY_VOICE_TICK: u16 = HOLD_TICKS + (END_TICK - HOLD_TICKS) / 2;
const FADE_STEP: f32 = 1. / (END_TICK - FADE_START) as f32;
const EDGE_SPEED: f32 = 2.5;
const CRACK_SPREAD: f32 = 1.025;
const MIN_SPIN_DEGREES: f32 = 1.;
const SPIN_STEP_DEGREES: f32 = 0.15;
const SPIN_VARIANTS: u64 = 20;
const ROLL_MULTIPLIER: f32 = 1.5;

/// Only the diagnostic benchmark can remove animation's minimum duration.
#[cfg(all(test, feature = "preparation-trace"))]
#[derive(Resource)]
pub(super) struct SkipMinimumDuration;

struct Piece {
    corners: [Vec2; 3],
    center: Vec2,
    spin: f32,
}

#[derive(Resource)]
pub(super) struct State {
    capture: crate::menu_backdrop::Capture,
    warmup: gpu::Shared,
    capture_camera: Entity,
    camera: Entity,
    entity: Entity,
    mesh: Handle<Mesh>,
    pieces: Vec<Piece>,
    size: Vec2,
    age: Option<u16>,
    mesh_age: f32,
    crack_color: Color,
    battle_ready: bool,
    #[cfg(all(test, feature = "preparation-trace"))]
    skip_minimum_duration: bool,
}

impl State {
    pub(super) fn start_shatter(&mut self) {
        self.battle_ready = true;
        #[cfg(all(test, feature = "preparation-trace"))]
        if self.skip_minimum_duration {
            // Called only after capture, view activation and menu preparation.
            self.age = Some(END_TICK);
        }
    }

    pub(super) fn finished(&self) -> bool {
        self.age == Some(END_TICK)
    }

    fn fragment_age(&self, fraction: f32) -> f32 {
        let hold = f32::from(HOLD_TICKS);
        if self.battle_ready {
            (f32::from(self.age.unwrap_or_default()) + fraction).clamp(hold, f32::from(END_TICK))
        } else {
            hold
        }
    }

    pub(super) fn entry_voice_ready(&self) -> bool {
        self.age.is_some_and(|age| age >= ENTRY_VOICE_TICK)
    }

    pub(super) fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
            "age": self.age,
            "captured": self.capture.ready(),
            "activated": self.age.is_some(),
        })
    }

    pub(super) fn ready(&self) -> Result<bool> {
        let warmup = self.warmup.0.lock().unwrap();
        if let Some(error) = &warmup.error {
            anyhow::bail!("battle transition: {error}");
        }
        Ok(self.capture.ready() && warmup.completed.load(Ordering::Acquire))
    }
}

pub(super) fn install(app: &mut App) {
    app.add_systems(FixedUpdate, advance.after(super::advance))
        .add_systems(
            PostUpdate,
            render
                .in_set(PublishBattle)
                .before(bevy::camera::CameraUpdateSystems),
        );
}

fn crack_color(formations: usize) -> Color {
    match formations {
        0 | 1 => Color::WHITE,
        2 => Color::linear_rgb(1., 0.5, 0.),
        _ => Color::linear_rgb(1., 0., 0.),
    }
}

fn field_cover(field: Option<&resonance_events::GameWorld>) -> Vec4 {
    field.map_or(Vec4::ZERO, |field| {
        let white = field.fade.as_ref().is_some_and(|fade| fade.white);
        Vec3::splat(if white { 1. } else { 0. }).extend(1. - field.brightness())
    })
}

pub(super) fn entry_color(field: Option<&resonance_events::GameWorld>) -> [u8; 3] {
    let cover = field_cover(field);
    // Entries without a shatter retain their completed field cover.
    // A completed white fade stays white until the battle fades in.
    if cover.w == 1. && cover.x == 1. {
        [255; 3]
    } else {
        [0; 3]
    }
}

/// Scripted opening events reveal the battle themselves. Ordinary openings
/// shatter the outgoing image, including any completed field fade.
fn shatter_cover(route: [i32; 5], field: Option<&resonance_events::GameWorld>) -> Option<Vec4> {
    (route[0] as u8 == 0).then(|| field_cover(field))
}

pub(super) fn begin(world: &mut World, seed: u64, request: &Request) -> Result<()> {
    let session = world.resource::<crate::new_game::Session>();
    let field = session.is_field().then_some(&session.events().world);
    let Some(cover) = shatter_cover(request.setup.route, field) else {
        return Ok(());
    };
    let source = world.resource::<crate::display::Targets>().source.clone();
    let descriptor = world
        .resource::<Assets<Image>>()
        .get(&source)
        .context("battle transition lost its outgoing image")?
        .texture_descriptor
        .size;
    let (capture_camera, camera_active) = world
        .query_filtered::<(Entity, &Camera), With<crate::FieldCamera>>()
        .iter(world)
        .next()
        .map(|(entity, camera)| (entity, camera.is_active))
        .context("battle transition lost its outgoing camera")?;
    let session = world.resource::<crate::new_game::Session>();
    let field = session.events();
    debug!(
        target: "resonance_presentation::battle_entry",
        field_tick = field.tick(),
        fade_alpha = field.world.fade.as_ref().map(|fade| fade.alpha(field.tick())),
        camera_active,
        "Battle transition retains the field and its active fade cover"
    );
    let capture_format = crate::renderer::SCENE_COLOR_FORMAT;
    let mut image =
        Image::new_target_texture(descriptor.width, descriptor.height, capture_format, None);
    image.sampler = ImageSampler::linear();
    let image = world.resource_mut::<Assets<Image>>().add(image);
    let capture = crate::menu_backdrop::Capture::once(image.clone());
    world
        .entity_mut(capture_camera)
        .insert(crate::menu_backdrop::SceneCapture(capture.clone()));
    let size = world.resource::<crate::display::Display>().0.ui_size();
    let pieces = pieces(seed);
    // Warm the cracked pose used at activation and preserve it through the hold.
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(mesh(&pieces, size, f32::from(HOLD_TICKS)));
    let material = world
        .resource_mut::<Assets<crate::menu_backdrop::Material>>()
        .add(crate::menu_backdrop::Material::visible(image).with_cover(cover));
    let mut report = gpu::Report {
        armed: true,
        ..default()
    };
    // The shatter draws in the scene pass once the field image is captured.
    report.prepare(
        gpu::View::Field,
        mesh.id(),
        crate::native::Prepare::Backdrop,
        false,
        None,
    );
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
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            Camera {
                order: -2,
                is_active: false,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderTarget::Image(source.into()),
            RenderLayers::layer(LAYER),
            projection,
        ))
        .id();
    let warmup = gpu::Shared::new(world);
    *warmup.0.lock().unwrap() = report;
    #[cfg(all(test, feature = "preparation-trace"))]
    let skip_minimum_duration = world.contains_resource::<SkipMinimumDuration>();
    world.insert_resource(State {
        capture,
        warmup,
        capture_camera,
        camera,
        entity,
        mesh,
        pieces,
        size,
        age: None,
        mesh_age: f32::from(HOLD_TICKS),
        crack_color: crack_color(request.formation_count()),
        battle_ready: false,
        #[cfg(all(test, feature = "preparation-trace"))]
        skip_minimum_duration,
    });
    Ok(())
}

pub(super) fn dispose(world: &mut World) {
    if let Some(state) = world.remove_resource::<State>() {
        state.warmup.0.lock().unwrap().armed = false;
        if let Ok(mut camera) = world.get_entity_mut(state.capture_camera) {
            camera.remove::<crate::menu_backdrop::SceneCapture>();
        }
        world.despawn(state.entity);
        world.despawn(state.camera);
    }
}

fn advance(
    state: Option<ResMut<State>>,
    audio: Option<Res<crate::field_audio::Control>>,
    mut exit: crate::diagnostics::Fatal,
) {
    if let Some(mut state) = state
        && let Some(age) = state.age
    {
        // Keep the cracked field intact until the battle's first frame has
        // completed on the GPU, then animate both scenes together.
        if age >= HOLD_TICKS && !state.battle_ready {
            return;
        }
        if age == HOLD_TICKS
            && let Some(audio) = audio
            && let Err(error) = audio.battle_entry()
        {
            exit.fail("Battle entry sound failed", error);
        }
        state.age = Some(age.saturating_add(1).min(END_TICK));
    }
}

fn render(
    mut commands: Commands,
    state: Option<ResMut<State>>,
    owner: Option<Res<Owner>>,
    prediction: Res<crate::field_view::Prediction>,
    (mut cameras, field_cameras): (Query<&mut Camera>, Query<Entity, crate::FieldCameraFilter>),
    mut meshes: ResMut<Assets<Mesh>>,
    targets: Option<Res<crate::display::Targets>>,
    mut images: ResMut<Assets<Image>>,
    mut exit: crate::diagnostics::Fatal,
) {
    let Some(mut state) = state else {
        return;
    };
    if !state.capture.ready()
        && let Some(targets) = targets
        && state.capture.fit_source(&targets.source, &mut images)
    {
        commands
            .entity(state.capture_camera)
            .insert(crate::menu_backdrop::SceneCapture(state.capture.clone()));
    }
    match state.ready() {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            exit.fail("battle transition", error);
            return;
        }
    }
    commands
        .entity(state.capture_camera)
        .remove::<crate::menu_backdrop::SceneCapture>();
    let battle_visible = owner.as_ref().is_some_and(|owner| owner.presenting());
    if battle_visible && state.finished() {
        state.warmup.0.lock().unwrap().armed = false;
        commands.entity(state.entity).despawn();
        commands.entity(state.camera).despawn();
        commands.remove_resource::<State>();
        return;
    }
    if state.age.is_none() {
        if let Ok(mut camera) = cameras.get_mut(state.camera) {
            camera.is_active = true;
        }
        // The GPU copy now owns the outgoing image; the field no longer needs
        // to render while the battle warms behind the shatter.
        for entity in &field_cameras {
            if let Ok(mut camera) = cameras.get_mut(entity) {
                camera.is_active = false;
            }
        }
        state.age = Some(0);
        debug!(
            target: "resonance_presentation::battle_entry",
            age = ?state.age,
            "Battle transition retained field image"
        );
    }
    if let Ok(mut camera) = cameras.get_mut(state.camera) {
        let battle_frame = owner.as_ref().is_some_and(|owner| {
            matches!(&owner.phase, Phase::Scene(scene) if scene.state != SceneState::Warming)
        });
        camera.clear_color = if !battle_frame || state.age.is_some_and(|age| age < HOLD_TICKS) {
            // Only the narrow cracks expose this backing; motion reveals battle.
            ClearColorConfig::Custom(state.crack_color)
        } else {
            // Draw the held battle pose so its first-frame fence can complete
            // before the shatter is allowed to move past HOLD_TICKS.
            ClearColorConfig::None
        };
    }
    // Rebuild only once the pieces start moving.
    let age = state.fragment_age(prediction.alpha);
    if age != state.mesh_age
        && let Some(mut target) = meshes.get_mut(&state.mesh)
    {
        *target = mesh(&state.pieces, state.size, age);
        state.mesh_age = age;
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

fn mesh(pieces: &[Piece], size: Vec2, age: f32) -> Mesh {
    let motion = (age - f32::from(HOLD_TICKS)).max(0.) * 90. / f32::from(END_TICK - HOLD_TICKS);
    let alpha = ((f32::from(END_TICK) - age) * FADE_STEP).clamp(0., 1.);
    let mut positions = Vec::with_capacity(pieces.len() * 3);
    let mut uv = Vec::with_capacity(positions.capacity());
    for piece in pieces {
        let spread = (piece.center - Vec2::splat(0.5)) * (CRACK_SPREAD - 1.);
        let center = (piece.center - Vec2::splat(0.5) + spread) * size;
        let offset = center / (size * 0.5) * EDGE_SPEED * motion;
        let angle = piece.spin * motion;
        let rotation = Mat2::from_angle(angle * ROLL_MULTIPLIER);
        for corner in piece.corners {
            let mut local = (corner - piece.center) * size;
            local.x *= angle.cos();
            let point = center + offset + rotation * local;
            positions.push([point.x, -point.y, 0.]);
            // Opening the gaps clips the retained picture in place. Sample at
            // the displaced screen coordinate so the picture itself does not
            // jump with the pieces until their motion starts after the hold.
            uv.push((corner + spread).to_array());
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
    fn fragments_wait_for_the_battle_and_keep_the_full_animation() {
        for loading_ticks in [0, END_TICK * 2] {
            let mut app = App::new();
            app.add_message::<AppExit>()
                .add_systems(FixedUpdate, advance)
                .insert_resource(State {
                    capture: crate::menu_backdrop::Capture::once(Handle::default()),
                    warmup: gpu::Shared::default(),
                    capture_camera: Entity::PLACEHOLDER,
                    camera: Entity::PLACEHOLDER,
                    entity: Entity::PLACEHOLDER,
                    mesh: Handle::default(),
                    pieces: Vec::new(),
                    size: Vec2::ZERO,
                    age: Some(0),
                    mesh_age: f32::from(HOLD_TICKS),
                    crack_color: Color::WHITE,
                    battle_ready: false,
                    #[cfg(feature = "preparation-trace")]
                    skip_minimum_duration: false,
                });
            for tick in 0..loading_ticks {
                app.world_mut().run_schedule(FixedUpdate);
                assert_eq!(
                    app.world().resource::<State>().age,
                    Some((tick + 1).min(HOLD_TICKS)),
                );
                let state = app.world().resource::<State>();
                assert_eq!(state.fragment_age(0.5), f32::from(HOLD_TICKS));
                assert!(!state.entry_voice_ready());
            }
            app.world_mut().resource_mut::<State>().start_shatter();
            let age = app.world().resource::<State>().age.unwrap();
            for next in age + 1..=END_TICK {
                app.world_mut().run_schedule(FixedUpdate);
                let state = app.world().resource::<State>();
                assert_eq!(state.age, Some(next));
                if next == HOLD_TICKS + 1 {
                    assert_eq!(state.fragment_age(0.5), f32::from(next) + 0.5);
                }
                assert_eq!(state.finished(), next == END_TICK);
                assert_eq!(state.entry_voice_ready(), next >= 56);
            }
            app.world_mut().run_schedule(FixedUpdate);
            assert!(app.world().resource::<State>().finished());
        }
    }

    #[test]
    fn opening_route_selects_whether_to_shatter_the_field_cover() {
        // Both Raybit encounters and Mithos use the ordinary route under full white.
        let ordinary = [0; 5];
        let vidarr = [2, 3, 600, 0, 3];
        // No field owner means an overworld encounter, without a field UI cover.
        assert_eq!(shatter_cover(ordinary, None), Some(Vec4::ZERO));
        assert_eq!(entry_color(None), [0; 3]);
        let mut field = resonance_events::GameWorld::default();
        // Before the first fade command, the field UI covers the scene in black.
        assert_eq!(shatter_cover(ordinary, Some(&field)), Some(Vec4::W));
        assert_eq!(shatter_cover(vidarr, Some(&field)), None);
        assert_eq!(entry_color(Some(&field)), [0; 3]);
        for white in [false, true] {
            let shade = if white { 1. } else { 0. };
            field.fade = Some(resonance_events::Fade {
                start_tick: 10,
                duration: 10,
                from: 0.,
                to: 256.,
                white,
            });
            for (tick, alpha) in [(9, 0.), (10, 25.), (14, 128.), (30, 255.)] {
                field.tick = tick;
                let expected = Vec3::splat(shade).extend(alpha / 255.);
                let cover = shatter_cover(ordinary, Some(&field)).unwrap();
                assert!((cover - expected).abs().max_element() < 0.00001);
                assert_eq!(shatter_cover(vidarr, Some(&field)), None);
                assert_eq!(
                    entry_color(Some(&field)),
                    if white && alpha == 255. {
                        [255; 3]
                    } else {
                        [0; 3]
                    },
                );
            }
        }
    }

    #[test]
    fn captured_image_cracks_then_holds_before_motion() {
        assert_eq!(crack_color(1), Color::WHITE);
        assert_eq!(crack_color(2), Color::linear_rgb(1., 0.5, 0.));
        assert_eq!(crack_color(3), crack_color(10));
        let pieces = pieces(7);
        let area: f32 = pieces
            .iter()
            .map(|piece| {
                let [a, b, c] = piece.corners;
                (b - a).perp_dot(c - a).abs() * 0.5
            })
            .sum();
        assert!((area - 1.).abs() < 0.00001);
        let size = Vec2::new(640., 480.);
        let held = mesh(&pieces, size, 0.);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            held.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("fragment positions");
        };
        let Some(VertexAttributeValues::Float32x2(uv)) = held.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("fragment texture coordinates");
        };
        for (position, uv) in positions.iter().zip(uv) {
            // Every visible fragment still samples the outgoing screen at its
            // original pixel coordinate, even after gaps open between pieces.
            let expected = (Vec2::from_array(*uv) - Vec2::splat(0.5)) * Vec2::new(size.x, -size.y);
            assert!(Vec2::new(position[0], position[1]).distance(expected) < 0.0001);
        }
        let before = mesh(&pieces, size, f32::from(HOLD_TICKS));
        let moving = mesh(&pieces, size, f32::from(HOLD_TICKS) + 0.5);
        assert_eq!(
            held.attribute(Mesh::ATTRIBUTE_POSITION),
            before.attribute(Mesh::ATTRIBUTE_POSITION)
        );
        assert_ne!(
            held.attribute(Mesh::ATTRIBUTE_POSITION),
            moving.attribute(Mesh::ATTRIBUTE_POSITION)
        );
        assert_eq!(
            held.attribute(Mesh::ATTRIBUTE_UV_0),
            moving.attribute(Mesh::ATTRIBUTE_UV_0)
        );
        let end = mesh(&pieces, size, f32::from(END_TICK));
        let Some(VertexAttributeValues::Float32x4(colors)) = end.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("fragment colors");
        };
        assert!(colors.iter().all(|color| color[3] == 0.));
    }
}
