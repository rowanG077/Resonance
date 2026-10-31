//! Exercise live resizing and recovery through a real field menu and GPU.
use super::*;
use resonance_content::{diagnostics::Diagnostics, library::Library, prepared::Files};
use resonance_game::{
    field::FieldInput,
    menu::{Menu, customize::Focus},
};

#[test]
#[ignore = "requires cooked assets and native 2x MSAA; checks live resolution preview, popup, Keep and timeout"]
fn live_resolution_preview_recovers_in_an_open_field_menu() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .expect("set RESONANCE_TEST_ASSETS");
    let mut snapshot = Library::open(&root, &[])?.snapshot();
    let diagnostics = Diagnostics::new(true);
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let mut checkpoint = crate::test_support::field_checkpoint(&files)?;
    let data = resonance_content::options::load(&files)?;
    let work = tempfile::tempdir()?;
    let save = work.path().join("checkpoint.json");
    crate::saves::prepare_checkpoint_fixture(&root, checkpoint.clone(), &save)?;
    // Desktop settings are deliberately omitted from game saves. Load a real
    // temporary user configuration so session activation mirrors interactive play.
    let config = work.path().join("config.toml");
    let initial = resonance_content::menu_data::CustomizeSettings {
        anti_aliasing: resonance_content::menu_data::AntiAliasing::SmaaS2x,
        ..default()
    };
    std::fs::write(&config, toml::to_string_pretty(&initial)?)?;
    let output = std::env::var_os("RESONANCE_DISPLAY_CAPTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| work.path().join("captures"));
    std::fs::create_dir_all(&output)?;
    let mut options = crate::test_support::run_options(root);
    options.capture = Some(output.join("unused.png"));
    options.saves.directory = Some(work.path().join("slots"));
    options.saves.load = Some(save);
    let (mut app, _) = crate::build_app_with_display(options, Resolution::default())?;
    app.insert_resource(InteractiveDisplay::default());
    // A logical window exercises the production display system without changing
    // the user's desktop. Actual cameras continue rendering to offscreen targets.
    let native = [3024, 1964];
    let monitor = app
        .world_mut()
        .spawn(Monitor {
            name: Some("Native resolution regression".into()),
            physical_width: native[0],
            physical_height: native[1],
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60000),
            scale_factor: 2.,
            video_modes: vec![bevy::window::VideoMode {
                physical_size: UVec2::from_array(native),
                bit_depth: 24,
                refresh_rate_millihertz: 60000,
            }],
        })
        .id();
    app.world_mut().spawn((
        window(Resolution::default()),
        PrimaryWindow,
        OnMonitor(monitor),
    ));
    app.insert_resource(crate::config::Preferences::load(config.clone())?);
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(began.elapsed().as_secs() < 180, "GPU setup timed out");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    loop {
        anyhow::ensure!(began.elapsed().as_secs() < 180, "field warmup timed out");
        app.update();
        crate::playthrough::check_exit(&app)?;
        if app
            .world()
            .get_resource::<crate::new_game::Session>()
            .is_some_and(|s| s.is_field() && s.ready_for_field)
            && app
                .world()
                .resource::<crate::loading::Resident>()
                .active
                .load(std::sync::atomic::Ordering::Acquire)
            && crate::new_game::scene_ready(app.world_mut())
        {
            break;
        }
    }
    anyhow::ensure!(
        app.world()
            .resource::<crate::anti_aliasing::AntiAliasingCapabilities>()
            .supports(resonance_content::menu_data::AntiAliasing::SmaaS2x),
        "native S2x is required"
    );
    assert_eq!(
        app.world()
            .resource::<crate::anti_aliasing::AntiAliasingSettings>()
            .0,
        resonance_content::menu_data::AntiAliasing::SmaaS2x,
    );
    let preferences = app
        .world()
        .resource::<crate::config::Preferences>()
        .settings
        .clone();
    checkpoint.progress.party.settings.preferences = preferences.clone();
    let mut menu = Menu::title_customize(preferences.clone(), data)?;
    menu.checkpoint = Some(resonance_game::Checkpoint::Field(checkpoint));
    menu.main_fade = 0;
    menu.customize.transition = Default::default();
    menu.customize.row = resonance_content::menu_data::RESOLUTION_OPTION;
    menu.customize.first = menu.customize.row - 8;
    app.world_mut()
        .resource_mut::<crate::new_game::Session>()
        .field_mut()
        .menu = Some(menu);
    settle(&mut app)?;
    {
        let mut session = app.world_mut().resource_mut::<crate::new_game::Session>();
        let menu = session.menu_mut().unwrap();
        menu.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Accept].into(),
            ..default()
        });
        menu.step(default());
        menu.step(FieldInput {
            direction: [0., 1.],
            ..default()
        });
    }
    settle(&mut app)?;
    assert_resolution(&app, [640, 480]);
    assert!(
        app.world()
            .resource::<InteractiveDisplay>()
            .confirmation
            .is_none()
    );
    capture(&mut app, &output.join("resolution-dropdown.png"))?;
    app.world_mut()
        .resource_mut::<crate::new_game::Session>()
        .menu_mut()
        .unwrap()
        .step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Cancel].into(),
            ..default()
        });
    change_resolution(&mut app);
    settle(&mut app)?;
    assert_resolution(&app, [800, 600]);
    assert_eq!(
        app.world()
            .resource::<crate::config::Preferences>()
            .settings
            .resolution,
        [640, 480]
    );
    assert_eq!(
        crate::config::Preferences::load(config)?
            .settings
            .resolution,
        [640, 480]
    );
    capture(&mut app, &output.join("resolution-preview.png"))?;
    app.world_mut()
        .resource_mut::<InteractiveDisplay>()
        .confirmation
        .as_mut()
        .unwrap()
        .2 = std::time::Instant::now() - std::time::Duration::from_secs(14);
    app.update();
    assert!(matches!(
        app.world()
            .resource::<crate::new_game::Session>()
            .menu()
            .unwrap()
            .customize
            .focus,
        Focus::DisplayConfirmation {
            remaining_secs: 1,
            ..
        }
    ));
    app.world_mut()
        .resource_mut::<InteractiveDisplay>()
        .confirmation
        .as_mut()
        .unwrap()
        .2 = std::time::Instant::now() - std::time::Duration::from_secs(15);
    settle(&mut app)?;
    assert_resolution(&app, [640, 480]);
    assert_eq!(
        app.world()
            .resource::<crate::new_game::Session>()
            .menu()
            .unwrap()
            .customize
            .focus,
        Focus::Options
    );
    capture(&mut app, &output.join("resolution-reverted.png"))?;
    change_resolution(&mut app);
    settle(&mut app)?;
    app.world_mut()
        .resource_mut::<crate::new_game::Session>()
        .menu_mut()
        .unwrap()
        .step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Accept].into(),
            ..default()
        });
    settle(&mut app)?;
    assert_resolution(&app, [800, 600]);
    assert!(
        app.world()
            .resource::<InteractiveDisplay>()
            .confirmation
            .is_none()
    );
    capture(&mut app, &output.join("resolution-kept.png"))?;
    {
        let mut session = app.world_mut().resource_mut::<crate::new_game::Session>();
        let state = &mut session.menu_mut().unwrap().customize;
        assert_eq!(state.native_resolution(), Some(native));
        assert!(state.resolution_choices().contains(&native));
        state.focus = Focus::DisplayConfirmation {
            previous_mode: state.draft.display_mode,
            previous_resolution: state.draft.resolution,
            remaining_secs: resonance_game::menu::customize::DISPLAY_CONFIRMATION_SECONDS,
            keep: true,
            revision: 3,
        };
        state.draft.display_mode = resonance_content::menu_data::DisplayMode::Fullscreen;
        state.draft.resolution = native;
    }
    settle(&mut app)?;
    assert_resolution(&app, native);
    capture(&mut app, &output.join("resolution-native.png"))?;
    app.world_mut()
        .resource_mut::<InteractiveDisplay>()
        .confirmation
        .as_mut()
        .unwrap()
        .2 = std::time::Instant::now() - std::time::Duration::from_secs(15);
    settle(&mut app)?;
    assert_resolution(&app, [800, 600]);
    Ok(())
}

fn change_resolution(app: &mut App) {
    let mut session = app.world_mut().resource_mut::<crate::new_game::Session>();
    let menu = session.menu_mut().unwrap();
    menu.step(default());
    for input in [
        FieldInput {
            pressed_buttons: [resonance_events::input::Button::Accept].into(),
            ..default()
        },
        FieldInput {
            direction: [0., -1.],
            ..default()
        },
        FieldInput {
            pressed_buttons: [resonance_events::input::Button::Accept].into(),
            ..default()
        },
    ] {
        menu.step(input);
        menu.step(default());
    }
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation { .. }
    ));
}
fn settle(app: &mut App) -> Result<()> {
    for _ in 0..40 {
        app.update();
        crate::playthrough::check_exit(app)?;
    }
    Ok(())
}
fn assert_resolution(app: &App, resolution: [u32; 2]) {
    assert_eq!(
        [
            app.world().resource::<Display>().0.width,
            app.world().resource::<Display>().0.height
        ],
        resolution
    );
    let target = &app.world().resource::<Targets>().source;
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(target)
        .unwrap()
        .size();
    assert_eq!(
        size,
        UVec2::new(
            resolution[0],
            Resolution {
                width: resolution[0],
                height: resolution[1]
            }
            .scene_height()
        )
    );
}
fn capture(app: &mut App, path: &std::path::Path) -> Result<()> {
    let failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let written = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    crate::new_game_capture::screenshot_held(
        app,
        path.to_owned(),
        failed.clone(),
        written.clone(),
    )?;
    assert!(!failed.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(written.load(std::sync::atomic::Ordering::Acquire), 1);
    Ok(())
}
