use super::*;
use bevy::{
    camera::RenderTarget,
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{PollType, TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
    window::ExitCondition,
    winit::WinitPlugin,
};

// The import screen must work without a single cooked game asset. This manual
// GPU test captures its real UI and verifies the spoiler boundary and focus.
#[test]
#[ignore = "requires a graphics adapter; writes setup review captures to target/setup-review"]
fn setup_screen_spoiler_gate_and_layout() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    )
    .add_plugins(TabNavigationPlugin)
    .insert_resource(ClearColor(Color::srgb(0.06, 0.09, 0.14)))
    .insert_resource(Setup::new(
        "target/setup-review/absent-assets".into(),
        "target/setup-review/mods".into(),
        None,
    ))
    .insert_resource(YggdrasilPlayback {
        paused: true,
        progress: Some(0.),
        ..default()
    })
    .add_plugins(YggdrasilPlugin)
    .add_systems(Startup, (scene, ui::load_art))
    .add_systems(
        Update,
        (
            layout_scene,
            ui::rebuild,
            ui::refresh,
            ui::button_appearance,
        )
            .chain(),
    )
    .add_observer(ui::activate);
    configure_schedules(&mut app);
    app.finish();
    app.cleanup();
    let mut image = Image::new_target_texture(1024, 800, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.update();
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera2d>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(camera).insert((
        RenderTarget::Image(target.clone().into()),
        IsDefaultUiCamera,
    ));

    capture(&mut app, &target, "selection");
    // Outside the ornate frame, the panel must leave the scene untouched.
    let panel = app
        .world_mut()
        .query_filtered::<&ChildOf, With<ui::PanelBackground>>()
        .single(app.world())
        .unwrap()
        .parent();
    let size = app.world().get::<ComputedNode>(panel).unwrap().size();
    let center = app
        .world()
        .get::<UiGlobalTransform>(panel)
        .unwrap()
        .affine()
        .translation;
    let min = center - size / 2.;
    let max = center + size / 2.;
    let root = app
        .world_mut()
        .query_filtered::<Entity, With<ui::Root>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(root).insert(Visibility::Hidden);
    capture(&mut app, &target, "selection-underlay");
    app.world_mut()
        .entity_mut(root)
        .insert(Visibility::Inherited);
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/setup-review");
    let panel_image = image::open(output.join("selection.png"))
        .unwrap()
        .into_rgba8();
    let underlay = image::open(output.join("selection-underlay.png"))
        .unwrap()
        .into_rgba8();
    for x in [(min.x + 1.).ceil() as u32, (max.x - 2.).floor() as u32] {
        for y in [(min.y + 1.).ceil() as u32, (max.y - 2.).floor() as u32] {
            assert_eq!(
                panel_image.get_pixel(x, y),
                underlay.get_pixel(x, y),
                "panel background leaks through a transparent frame corner at ({x}, {y})"
            );
        }
    }
    {
        let mut state = app.world_mut().resource_mut::<Setup>();
        state.phase = Phase::Importing;
        state.pacing.shown = 0.42;
        state.stage = "Preparing your world · step 39 of 65".into();
        state.output("SPOILER_SENTINEL: a story asset".into());
        state.rebuild = true;
    }
    {
        let mut playback = app.world_mut().resource_mut::<YggdrasilPlayback>();
        playback.progress = Some(0.42);
        playback.seconds = 149.;
    }
    capture(&mut app, &target, "importing");
    trigger(&mut app, |a| matches!(a, ui::Action::Details));
    capture(&mut app, &target, "warning");
    assert!(
        !app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .any(|t| t.contains("SPOILER_SENTINEL"))
    );
    let focus = app
        .world()
        .resource::<bevy::input_focus::InputFocus>()
        .get()
        .unwrap();
    assert!(matches!(
        app.world().get::<ui::Action>(focus),
        Some(ui::Action::Hide)
    ));
    trigger(&mut app, |a| matches!(a, ui::Action::Reveal));
    capture(&mut app, &target, "output");
    assert!(
        app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .any(|t| t.contains("SPOILER_SENTINEL"))
    );
    trigger(&mut app, |a| matches!(a, ui::Action::Hide));
    {
        let mut state = app.world_mut().resource_mut::<Setup>();
        state.phase = Phase::Complete;
        state.pacing.shown = 1.;
        state.stage = "Your world is ready. A new journey awaits.".into();
        state.rebuild = true;
        let mut playback = app.world_mut().resource_mut::<YggdrasilPlayback>();
        playback.progress = Some(1.);
        playback.seconds = 460.;
    }
    capture(&mut app, &target, "complete");
    let backdrop = app
        .world_mut()
        .query_filtered::<&Transform, With<SceneBackdrop>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        backdrop.scale,
        Vec3::ONE,
        "default artwork stays native size"
    );
    assert!(
        !app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .any(|t| t.contains("SPOILER_SENTINEL"))
    );
    // The compact supported size must retain every action within the viewport.
    *app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&target)
        .unwrap() = {
        let mut image = Image::new_target_texture(800, 640, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    };
    {
        let mut state = app.world_mut().resource_mut::<Setup>();
        state.phase = Phase::Select;
        state.notice = "Expected disc 2, found disc 1.".into();
        state.discs[0] =
            Some("Tales of Symphonia (North American GameCube release) (Disc 1).rvz".into());
        state.rebuild = true;
    }
    {
        let mut playback = app.world_mut().resource_mut::<YggdrasilPlayback>();
        playback.progress = Some(0.);
        playback.seconds = 0.;
    }
    capture(&mut app, &target, "selection-small");
    assert_actions_visible(&mut app, Vec2::new(800., 640.));
    for cached in [true, false] {
        {
            let mut state = app.world_mut().resource_mut::<Setup>();
            state.reimport = true;
            state.extracted = cached.then(|| "local/extracted-discs".into());
            state.notice.clear();
            state.rebuild = true;
            assert_eq!(state.source().is_some(), cached);
        }
        capture(
            &mut app,
            &target,
            if cached {
                "reimport-cached-small"
            } else {
                "reimport-discs-small"
            },
        );
        assert_actions_visible(&mut app, Vec2::new(800., 640.));
        assert!(
            app.world().resource::<Setup>().job.is_none(),
            "the prompt must wait for the import button"
        );
        let actions: Vec<_> = app
            .world_mut()
            .query::<(&ui::Action, Has<bevy::ui::InteractionDisabled>)>()
            .iter(app.world())
            .map(|(a, disabled)| (*a, disabled))
            .collect();
        assert!(
            actions
                .iter()
                .any(|(a, disabled)| matches!(a, ui::Action::Start) && *disabled != cached)
        );
        assert_eq!(
            actions
                .iter()
                .filter(|(a, _)| matches!(a, ui::Action::Disc(_)))
                .count(),
            if cached { 0 } else { 2 }
        );
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|t| t.contains("Re-import assets"))
        );
    }
}

fn assert_actions_visible(app: &mut App, viewport: Vec2) {
    for (node, transform) in app
        .world_mut()
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ui::Action>>()
        .iter(app.world())
    {
        let center = transform.affine().translation;
        let half = node.size() / 2.;
        assert!(
            center.x - half.x >= 0.
                && center.x + half.x <= viewport.x
                && center.y - half.y >= 0.
                && center.y + half.y <= viewport.y,
            "setup action outside compact viewport: {center:?}, size {:?}",
            node.size()
        );
    }
}

fn trigger(app: &mut App, select: impl Fn(&ui::Action) -> bool) {
    let entity = app
        .world_mut()
        .query::<(Entity, &ui::Action)>()
        .iter(app.world())
        .find(|(_, a)| select(a))
        .unwrap()
        .0;
    app.world_mut()
        .trigger(bevy::ui_widgets::Activate { entity });
    app.update();
}

fn capture(app: &mut App, target: &Handle<Image>, name: &str) {
    for _ in 0..12 {
        app.update();
    }
    let image = app.world().resource::<Assets<Image>>().get(target).unwrap();
    let (width, height) = (image.width(), image.height());
    let result = Arc::new(Mutex::new(None));
    let saved = result.clone();
    app.world_mut()
        .spawn(Readback::texture(target.clone()))
        .observe(move |event: On<ReadbackComplete>, mut commands: Commands| {
            let row = width as usize * 4;
            let bytes: Vec<u8> = event
                .data
                .chunks(row.div_ceil(256) * 256)
                .take(height as usize)
                .flat_map(|r| r[..row].iter().copied())
                .collect();
            *saved.lock().unwrap() = Some(bytes);
            commands.entity(event.entity).despawn();
        });
    let started = std::time::Instant::now();
    loop {
        app.update();
        let _ = app
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            });
        if let Some(bytes) = result.lock().unwrap().take() {
            let output =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/setup-review");
            std::fs::create_dir_all(&output).unwrap();
            image::save_buffer(
                output.join(format!("{name}.png")),
                &bytes,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "capture timed out"
        );
    }
}
