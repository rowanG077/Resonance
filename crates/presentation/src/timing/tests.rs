use super::*;

fn fixture() -> App {
    fixture_at("assets")
}

fn fixture_at(path: &str) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: path.into(),
            ..Default::default()
        },
    ))
    .register_asset_loader(bevy::image::ImageLoader::new(
        bevy::image::CompressedImageFormats::NONE,
    ))
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::ZERO,
    ))
    .insert_resource(crate::diagnostics::Diagnostics(
        resonance_content::diagnostics::Diagnostics::new(true),
    ))
    .add_message::<AppExit>()
    .init_resource::<Ready>()
    .init_resource::<RenderReady>()
    .init_resource::<FieldAssets>()
    .init_resource::<movie::Playback>()
    .init_resource::<boot::Playback>()
    .init_resource::<PendingInput>()
    .init_resource::<audio::MenuSounds>()
    .init_resource::<Assets<GameAudio>>()
    .init_asset::<Image>()
    .init_resource::<Assets<glow::GlowMaterial>>()
    .init_resource::<Assets<TitleText>>()
    .insert_resource(PendingAudio(None))
    .init_resource::<Clock>()
    .insert_resource(Menu(TitleState::default()))
    .insert_resource(Art {
        manifest: TitleAssets {
            version: resonance_content::CONTENT_VERSION,
            game_id: "GQSEAF".into(),
            revision: 0,
            source_sha256: String::new(),
            textures: Vec::new(),
            scene: None,
        },
        images: Vec::new(),
    })
    .insert_resource(RunOptions {
        script_root: None,
        saves: Default::default(),
        assets: PathBuf::new(),
        capture_at: None,
        capture: None,
        reveal: false,
        selected: 0,
        silent: true,
        paranoid: true,
        skip_intro: true,
        skip_battles: false,
        allow_incomplete_scripts: false,
        record_playthrough: None,
        record_title_ticks: 1000,
    })
    .add_systems(
        FixedUpdate,
        (advance_clock, boot::advance, crate::advance).chain(),
    )
    .add_systems(Update, (prepare, start_audio).chain());
    app
}

fn assert_frozen(app: &mut App, ticks: usize) {
    let initial = (
        app.world().resource::<Clock>().0.tick(),
        app.world().resource::<Menu>().0.tick,
    );
    for _ in 0..ticks {
        app.world_mut().run_schedule(FixedUpdate);
        app.update();
        assert_eq!(
            (
                app.world().resource::<Clock>().0.tick(),
                app.world().resource::<Menu>().0.tick,
            ),
            initial,
            "preparation consumed gameplay or presentation time"
        );
    }
}

#[test]
fn loading_does_not_advance_title_or_presentation() {
    for delay in [1, 75, 260] {
        let mut app = fixture();
        assert_frozen(&mut app, delay);
        app.world_mut().resource_mut::<FieldAssets>().ready = true;
        assert_frozen(&mut app, delay);
        // A missing overlay still prevents playback after meshes and pipelines
        // are ready. This handle has no I/O request or device behind it.
        app.world_mut()
            .resource_mut::<Art>()
            .images
            .push(Handle::default());
        app.world()
            .resource::<RenderReady>()
            .0
            .lock()
            .unwrap()
            .completed
            .store(true, Ordering::Release);
        assert_frozen(&mut app, delay);
        app.world_mut().resource_mut::<Art>().images.clear();
        app.update();
        assert!(app.world().resource::<Ready>().0);
        assert_eq!(app.world().resource::<Menu>().0.tick, 0);
        assert_eq!(app.world().resource::<Clock>().0.tick(), 0);
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(app.world().resource::<Menu>().0.tick, 1);
        assert_eq!(app.world().resource::<Clock>().0.tick(), 1);
    }
}

#[test]
fn recorder_preparation_and_static_readbacks_consume_no_ticks() {
    let mut app = fixture();
    app.world_mut().resource_mut::<Ready>().0 = true;
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(app.world().resource::<Menu>().0.tick, 0);
    assert_eq!(app.world().resource::<Clock>().0.tick(), 0);
    app.world_mut().resource_mut::<Ready>().0 = true;
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Menu>().0.tick, 1);
    assert_eq!(app.world().resource::<Clock>().0.tick(), 1);
    // Held readbacks use zero elapsed time through the normal Update schedule.
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(app.world().resource::<Menu>().0.tick, 1);
    assert_eq!(app.world().resource::<Clock>().0.tick(), 1);
}

#[test]
fn title_return_requires_a_fresh_submission_of_visible_startup_draws() {
    use bevy::ecs::system::RunSystemOnce;
    let mut app = fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Mesh>>();
    world.resource_mut::<FieldAssets>().ready = true;
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1., 1.));
    let output = world
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d::<TitleOutput>(Handle::default()),
            ViewVisibility::VISIBLE,
        ))
        .id();
    world.spawn((
        Mesh2d(mesh),
        MeshMaterial2d::<TitleText>(Handle::default()),
        ViewVisibility::HIDDEN,
    ));
    let root = world
        .spawn((scene::PartRoot, WorldAssetRoot(Handle::default())))
        .id();
    world.run_system_once(prepare_draws).unwrap();
    assert!(
        !world.resource::<RenderReady>().0.lock().unwrap().armed,
        "asset metadata alone does not mean the title scene has instantiated"
    );
    world.entity_mut(root).insert(scene::Instantiated);
    world.run_system_once(prepare_draws).unwrap();
    let submitted = {
        let report = world.resource::<RenderReady>().0.lock().unwrap();
        assert_eq!(report.expected, [MainEntity::from(output)].into());
        assert!(!report.completed.load(Ordering::Acquire));
        report.completed.clone()
    };
    submitted.store(true, Ordering::Release);
    world.run_system_once(prepare).unwrap();
    assert!(world.resource::<Ready>().0);
    world.run_system_once(prepare_draws).unwrap();

    world.resource_mut::<Ready>().0 = false;
    world.run_system_once(prepare_draws).unwrap();
    world.run_system_once(prepare).unwrap();
    assert!(
        !world.resource::<Ready>().0,
        "the preceding title submission must not release a new title visit"
    );
    let report = world.resource::<RenderReady>().0.lock().unwrap();
    assert_eq!(report.expected, [MainEntity::from(output)].into());
    assert!(!Arc::ptr_eq(&submitted, &report.completed));
}

#[test]
fn startup_counts_authored_frames_and_releases_title_input_after_completion() {
    let mut app = fixture();
    app.world_mut().resource_mut::<Ready>().0 = true;
    app.world_mut().resource_mut::<boot::Playback>().logos = Some(Default::default());
    app.world_mut().resource_mut::<movie::Playback>().active = true;
    // An earlier press must not remain queued until the Namco skip window.
    app.world_mut()
        .resource_mut::<PendingInput>()
        .pressed
        .accept = true;
    for _ in 0..resonance_game::boot::LOGO_TICKS {
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(app.world().resource::<Menu>().0.tick, 0);
    }
    assert!(!app.world().resource::<boot::Playback>().active());
    assert_eq!(
        app.world().resource::<Clock>().0.tick(),
        resonance_game::boot::LOGO_TICKS
    );
    assert_frozen(&mut app, 100); // Movie has not prebuffered yet.
    app.world_mut().resource_mut::<movie::Playback>().active = false;
    let mut menu = app.world_mut().resource_mut::<Menu>();
    menu.0.revealed = true;
    menu.0.opacity = 255;
    app.world_mut().resource_mut::<PendingInput>().pressed.down = true;
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(
        app.world().resource::<Menu>().0.selected,
        1,
        "completed startup consumed the title's navigation press"
    );
    assert_eq!(
        app.world().resource::<Clock>().0.tick(),
        resonance_game::boot::LOGO_TICKS + 1
    );
}

#[test]
fn missing_and_corrupt_startup_images_honor_policy_without_consuming_time() {
    use bevy::ecs::system::RunSystemOnce;
    let path = std::env::temp_dir().join(format!(
        "resonance-startup-images-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    fs::write(path.join("broken.png"), b"not a PNG").unwrap();
    for paranoid in [false, true] {
        for image_path in ["missing.png", "broken.png"] {
            for logo in [false, true] {
                let mut app = fixture_at(path.to_str().unwrap());
                let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
                app.insert_resource(crate::diagnostics::Diagnostics(diagnostics.clone()));
                app.world_mut().resource_mut::<FieldAssets>().ready = true;
                app.world()
                    .resource::<RenderReady>()
                    .0
                    .lock()
                    .unwrap()
                    .completed
                    .store(true, Ordering::Release);
                if logo {
                    app.init_resource::<Assets<Mesh>>()
                        .init_resource::<Assets<TitleText>>();
                    let mut boot = app.world_mut().resource_mut::<boot::Playback>();
                    boot.logos = Some(Default::default());
                    boot.asset = Some(resonance_content::BootAssets {
                        version: 1,
                        source_sha256: String::new(),
                        textures: vec![
                            resonance_content::BootTexture {
                                path: image_path.into(),
                                width: 1,
                                height: 1,
                                background: [0; 3],
                            };
                            4
                        ],
                    });
                    app.world_mut()
                        .run_system_once(
                            |mut commands: Commands,
                             server: Res<AssetServer>,
                             mut meshes: ResMut<Assets<Mesh>>,
                             mut materials: ResMut<Assets<TitleText>>,
                             mut boot: ResMut<boot::Playback>| {
                                boot::setup(
                                    &mut commands,
                                    &server,
                                    &mut meshes,
                                    &mut materials,
                                    &mut boot,
                                    Handle::default(),
                                );
                            },
                        )
                        .unwrap();
                    app.add_systems(Update, boot::update.after(prepare));
                } else {
                    let handle = app.world().resource::<AssetServer>().load(image_path);
                    app.world_mut().resource_mut::<Art>().images.push(handle);
                }
                let began = Instant::now();
                while !diagnostics.has_errors() {
                    assert!(
                        began.elapsed() < Duration::from_secs(5),
                        "image failure was not observed"
                    );
                    app.update();
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(app.world().resource::<Ready>().0, !paranoid);
                assert_eq!(
                    app.world().resource::<Messages<AppExit>>().is_empty(),
                    !paranoid
                );
                assert_eq!(app.world().resource::<Clock>().0.tick(), 0);
                assert_eq!(app.world().resource::<Menu>().0.tick, 0);
                if logo {
                    assert!(!app.world().resource::<boot::Playback>().active());
                    assert!(
                        app.world_mut()
                            .query_filtered::<&Camera, With<boot::BootCamera>>()
                            .iter(app.world())
                            .all(|camera| !camera.is_active)
                    );
                } else {
                    let handle = &app.world().resource::<Art>().images[0];
                    assert_eq!(
                        app.world().resource::<Assets<Image>>().contains(handle),
                        !paranoid
                    );
                }
            }
        }
    }
    fs::remove_dir_all(path).unwrap();
}
