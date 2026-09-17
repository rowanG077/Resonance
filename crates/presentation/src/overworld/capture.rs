//! Render a documented world observer using the production package and renderer.
use super::*;
use bevy::{
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};
use resonance_content::{HEIGHT, SCENE_HEIGHT, WIDTH};
use resonance_events::input::Button;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub state: resonance_content::overworld::TravelState,
    pub story: i32,
    /// Settle the camera without stepping scripts, travel, or audio.
    #[serde(default = "camera_ticks")]
    pub camera_ticks: u16,
    /// Observe the shared failed-transition notice after artwork becomes ready.
    #[serde(default)]
    pub transition_failure: bool,
    #[serde(default)]
    pub menu: bool,
    /// Run stationary gameplay before capturing, for observing enemy symbols.
    #[serde(default)]
    pub simulation_ticks: u16,
    /// Confirm on specific simulation updates (for dialogue/choice observers).
    #[serde(default)]
    pub confirm_ticks: Vec<u16>,
    /// Party order, including the four visible Rheaird riders.
    #[serde(default)]
    pub formation: Option<Vec<u8>>,
    /// Drive the prepared scene, observing vehicle effects at gameplay cadence.
    #[serde(default)]
    pub throttle_ticks: u16,
    #[serde(default)]
    pub stick: [f32; 2],
    #[serde(default)]
    pub cinematic: Option<u16>,
    /// Observe a prepared skit's dialogue through the world UI.
    #[serde(default)]
    pub skit: Option<u16>,
    /// Advance gameplay after artwork is ready, including prompt animation.
    #[serde(default)]
    pub settle_simulation_ticks: u16,
}
fn camera_ticks() -> u16 {
    300
}
#[derive(Resource)]
pub(super) struct Scene(pub super::Scene);
#[derive(Resource)]
struct Capture {
    path: std::path::PathBuf,
    start: std::time::Instant,
    settled: u32,
    requested: bool,
    transition_failure: bool,
    simulation_ticks: u16,
}

pub fn capture_overworld(root: &Path, output: &Path, probe: &Probe) -> Result<()> {
    let root = root.canonicalize()?;
    let package = Arc::new(game::Prepared::load(
        &root,
        &mut Default::default(),
        (0..547).collect(),
        || false,
    )?);
    let mut persistent = resonance_events::PersistentState {
        party: Some(resonance_events::party::Party::new(
            package.resources.session_data.as_ref().unwrap(),
            Default::default(),
        )?),
        ..Default::default()
    };
    if let Some(formation) = &probe.formation {
        let party = persistent.party.as_mut().unwrap();
        anyhow::ensure!(!formation.is_empty(), "capture needs a party leader");
        party.formation = formation.clone();
        party.field_leader = formation[0];
        party.validate(package.resources.session_data.as_ref().unwrap())?;
    }
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, probe.story)?;
    persistent.memory.write(
        0x50,
        symphonia_script::Width::S32,
        probe.state.world.index() as i32,
    )?;
    let mut session = if let Some(id) = probe.cinematic {
        let definition = package
            .definition
            .visuals
            .cinematics
            .get(&id)
            .context("capture cinematic missing")?;
        let mut source = resonance_events::GameWorld::default();
        source
            .request_world(
                id,
                0,
                Some(resonance_events::SceneDestination {
                    map: 1,
                    position: [0.; 3],
                    heading: 0.,
                }),
            )
            .map_err(anyhow::Error::msg)?;
        game::Session::play_cinematic(
            package.assets(definition.world, &persistent)?,
            source.world_transition.as_ref().unwrap(),
            Arc::new(definition.clone()),
            persistent,
            Default::default(),
        )?
    } else {
        game::Session::enter(
            package.assets(probe.state.world, &persistent)?,
            probe.state.clone(),
            persistent,
            Default::default(),
        )?
    };
    for _ in 0..if probe.cinematic.is_none() {
        probe.camera_ticks
    } else {
        0
    } {
        session.update_camera()?;
    }
    if let Some(id) = probe.skit {
        let skits = resonance_game::skit::Prepared::load(
            package.resources.skits.as_ref().unwrap().clone(),
            &package.files,
        )?;
        session.active_skit = Some(resonance_game::skit::Playback::start(
            &skits[&id],
            &mut session.events,
            true,
            true,
            None,
        )?);
    }
    for tick in 0..probe.simulation_ticks {
        session.step(game::Input {
            confirm: probe.confirm_ticks.contains(&tick),
            ..Default::default()
        })?;
    }
    if probe.menu {
        session.step(game::Input {
            menu: resonance_game::field::FieldInput {
                pressed_buttons: [Button::Menu].into(),
                ..Default::default()
            },
            ..Default::default()
        })?;
        for _ in 0..20 {
            session.step(Default::default())?;
        }
    }
    let mut scene = super::Scene::new(session, package.clone())?;
    for _ in 0..probe.throttle_ticks {
        scene.step(game::Input {
            travel: game::travel::Input {
                throttle: true,
                stick: probe.stick,
                ..Default::default()
            },
            ..Default::default()
        })?;
    }
    let mut app = App::new();
    crate::loading::install(&mut app, &root);
    *app.world()
        .resource::<crate::loading::Resident>()
        .files
        .write()
        .unwrap() = Some(package.files.clone());
    app.add_plugins(
        DefaultPlugins
            .set(bevy::pbr::PbrPlugin {
                gltf_enable_standard_materials: false,
                ..default()
            })
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
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
    .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
        resonance_game::clock::UPDATE_STEP,
    ))
    .add_plugins((
        crate::materials::install_surface_material,
        crate::materials::install_output_materials,
    ))
    .add_plugins(crate::draw_order::DrawOrderPlugin)
    .init_resource::<crate::display::Display>()
    .init_resource::<crate::scene::SampledImages>()
    .insert_resource(Scene(scene))
    .insert_resource(Capture {
        path: output.into(),
        start: std::time::Instant::now(),
        settled: 0,
        requested: false,
        transition_failure: probe.transition_failure,
        simulation_ticks: probe.settle_simulation_ticks,
    })
    .insert_resource(ClearColor(Color::srgb(0.32, 0.52, 0.68)))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            load,
            prepare,
            instances,
            bind,
            pose,
            terrain_order,
            camera,
            ui,
            crate::skin_bounds,
            capture,
        )
            .chain(),
    )
    .add_systems(
        PostUpdate,
        animate.before(bevy::transform::TransformSystems::Propagate),
    );
    app.add_systems(
        Update,
        crate::field_ui::transition_failure
            .after(ui)
            .before(capture),
    );
    crate::field_ui::install(&mut app);
    crate::materials::install(&mut app);
    sparse_animation::install(&mut app);
    crate::renderer::configure(&mut app);
    let ready = crate::RenderReady::default();
    app.insert_resource(ready.clone());
    app.add_systems(
        PostUpdate,
        crate::timing::prepare_draws
            .after(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
    );
    app.sub_app_mut(bevy::render::RenderApp)
        .insert_resource(ready)
        .add_systems(
            bevy::render::Render,
            crate::timing::rendered.in_set(bevy::render::RenderSystems::Cleanup),
        );
    anyhow::ensure!(app.run() == AppExit::Success, "overworld capture failed");
    Ok(())
}
fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut outputs: ResMut<Assets<TitleOutput>>,
) {
    let mut final_image =
        Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Bgra8UnormSrgb, None);
    final_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let final_image = images.add(final_image);
    commands.insert_resource(crate::Framebuffer(RenderTarget::Image(
        final_image.clone().into(),
    )));
    let mut source =
        Image::new_target_texture(WIDTH, SCENE_HEIGHT, TextureFormat::Bgra8Unorm, None);
    source.sampler = bevy::image::ImageSampler::linear();
    source.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let source = images.add(source);
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderTarget::Image(source.clone().into()),
        crate::camera::overlay_alignment(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(WIDTH as f32, HEIGHT as f32))),
        MeshMaterial2d(outputs.add(TitleOutput {
            source: source.clone(),
            brightness: Vec4::new(1., 0., 0., 0.),
            screen_offset: Vec2::ZERO,
        })),
        RenderLayers::layer(2),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(2),
        Camera {
            order: 1,
            ..default()
        },
        RenderTarget::Image(final_image.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Msaa::Off,
        RenderTarget::Image(source.into()),
        crate::FieldCamera,
        Camera {
            order: -1,
            ..default()
        },
        Transform::default(),
    ));
}
#[allow(clippy::too_many_arguments)]
fn capture(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    mut scene: ResMut<Scene>,
    art: Option<Res<Art>>,
    parts: Query<(Entity, &Instance)>,
    children: Query<&Children>,
    geometry: Query<(&super::super::materials::MaterialSlot, &Visibility)>,
    framebuffer: Res<crate::Framebuffer>,
    ready: Res<crate::RenderReady>,
    resident: Res<crate::loading::Resident>,
    mut exit: MessageWriter<AppExit>,
) {
    if capture.requested {
        return;
    }
    if capture.start.elapsed().as_secs() > 180 {
        error!("World capture timed out");
        exit.write(AppExit::error());
        return;
    }
    if art.is_none_or(|a| !a.ready)
        || parts.is_empty()
        || parts.iter().any(|(_, p)| !p.prepared)
        || !resident.active.load(Ordering::Acquire)
        || !ready.0.lock().unwrap().completed.load(Ordering::Acquire)
    {
        capture.settled = 0;
        return;
    }
    if capture.settled == 0 && capture.transition_failure {
        commands.insert_resource(crate::new_game::TransitionFailure);
    }
    if capture.simulation_ticks > 0 {
        capture.simulation_ticks -= 1;
        if let Err(error) = scene.0.step(Default::default()) {
            error!("World capture simulation failed: {error:#}");
            exit.write(AppExit::error());
        }
        return;
    }
    capture.settled += 1;
    if capture.settled < 20 {
        return;
    }
    capture.requested = true;
    let path = capture.path.clone();
    let hidden_geometry: Vec<_> = parts
        .iter()
        .filter(|(_, p)| p.model == Model::Actor(scene.0.leader()))
        .map(|(root, p)| {
            serde_json::json!({"part": p.part, "hidden_materials": children.iter_descendants(root)
            .filter_map(|entity| geometry.get(entity).ok())
            .filter(|(_, visibility)| **visibility == Visibility::Hidden)
            .map(|(slot, _)| slot.0).collect::<Vec<_>>()})
        })
        .collect();
    let state = serde_json::json!({"kind":"overworld-development-observer", "state": scene.0.session.travel.state(), "camera_angle":scene.0.session.camera.angle(), "cinematic":scene.0.session.cinematic.as_ref().map(|c| serde_json::json!({"id":c.id, "ticks":c.ticks(), "camera":c.camera()})), "transition_failure": capture.transition_failure, "instances": parts.iter().count(), "hidden_geometry": hidden_geometry, "late_asset_reads": resident.unprepared_reads.load(Ordering::Acquire)});
    commands.spawn(Screenshot(framebuffer.0.clone())).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            match crate::screenshot::write(&event.image, &path, Some(&state)) {
                Ok(()) => {
                    exit.write(AppExit::Success);
                }
                Err(error) => {
                    error!("World capture failed: {error:#}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}
