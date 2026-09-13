//! Small driver regression: a sunlit cube must cast a ray-traced floor shadow.
use bevy::{
    camera::{CameraMainTextureUsages, Hdr, RenderTarget},
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        settings::{WgpuFeatures, WgpuSettings},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    solari::prelude::*,
    window::ExitCondition,
};

fn compatible(mut mesh: Mesh) -> Mesh {
    mesh.generate_tangents().unwrap();
    mesh.insert_indices(Indices::U32(
        mesh.indices().unwrap().iter().map(|i| i as u32).collect(),
    ));
    assert!(matches!(
        mesh.attribute(Mesh::ATTRIBUTE_TANGENT),
        Some(VertexAttributeValues::Float32x4(_))
    ));
    mesh
}

fn main() {
    resonance_presentation::prepare_ray_tracing_process().expect("Solari driver settings");
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(bevy::render::RenderPlugin {
                    render_creation: WgpuSettings {
                        disabled_features: Some(WgpuFeatures::EXPERIMENTAL_COOPERATIVE_MATRIX),
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::gilrs::GilrsPlugin>(),
        )
        .add_plugins((
            SolariPlugins,
            bevy::solari::pathtracer::PathtracingPlugin,
            bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(10)),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .add_systems(PostUpdate, reset_history)
        .run();
}

fn reset_history(mut views: Query<&mut SolariLighting>) {
    if !std::env::args().any(|arg| arg == "--keep-history") {
        for mut lighting in &mut views {
            lighting.reset = true;
        }
    }
}

#[derive(Resource)]
struct Target(Handle<Image>);
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut image = Image::new_target_texture(256, 256, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = images.add(image);
    commands.insert_resource(Target(target.clone()));
    let mut camera = commands.spawn((
        Camera3d::default(),
        Hdr,
        bevy::core_pipeline::tonemapping::Tonemapping::ReinhardLuminance,
        Msaa::Off,
        CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
        RenderTarget::Image(target.into()),
        Transform::from_xyz(4., 3., 5.).looking_at(Vec3::new(0., 0.4, 0.), Vec3::Y),
    ));
    if std::env::args().any(|arg| arg == "--pathtracer") {
        camera.insert(bevy::solari::pathtracer::Pathtracer::default());
    } else {
        camera.insert(SolariLighting::default());
    }
    let ground = meshes.add(compatible(Mesh::from(
        Plane3d::default().mesh().size(10., 10.),
    )));
    commands.spawn((
        Mesh3d(ground.clone()),
        RaytracingMesh3d(ground),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.7),
            ..default()
        })),
    ));
    let cube = meshes.add(compatible(Mesh::from(Cuboid::new(1., 1., 1.))));
    let mut cube_entity = commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.15, 0.05),
            ..default()
        })),
        Transform::from_xyz(0., 0.5, 0.),
    ));
    if !std::env::args().any(|arg| arg == "--no-occluder") {
        cube_entity.insert(RaytracingMesh3d(cube));
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 20000.,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(4., 7., 3.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn capture(mut frames: Local<u32>, mut commands: Commands, target: Res<Target>) {
    *frames += 1;
    if *frames == 80 {
        let path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "local/solari-probe.png".into());
        commands
            .spawn(Screenshot(RenderTarget::Image(target.0.clone().into())))
            .observe(
                move |capture: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    capture
                        .image
                        .clone()
                        .try_into_dynamic()
                        .unwrap()
                        .save(&path)
                        .unwrap();
                    exit.write(AppExit::Success);
                },
            );
    }
}
