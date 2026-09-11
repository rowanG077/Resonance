use super::*;

fn fixture() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::ZERO,
        ))
        .init_resource::<Ready>()
        .init_resource::<RenderReady>()
        .init_resource::<FieldAssets>()
        .init_resource::<movie::Playback>()
        .init_resource::<boot::Playback>()
        .init_resource::<PendingInput>()
        .init_resource::<audio::MenuSounds>()
        .init_resource::<Assets<GameAudio>>()
        .insert_resource(PendingAudio(None))
        .insert_resource(Clock(PresentationClock::new(2365)))
        .insert_resource(Menu(TitleState::default()))
        .insert_resource(Replay(Some(
            serde_json::from_str(include_str!(
                "../../../../tools/oracle/cases/native-navigation.json"
            ))
            .unwrap(),
        )))
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
            saves: Default::default(),
            assets: PathBuf::new(),
            tick: None,
            presentation_start: Some(2365),
            capture: None,
            reveal: false,
            selected: 0,
            silent: true,
            replay: None,
            movie_frame: None,
            boot_frame: None,
            skip_intro: true,
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
fn loading_duration_does_not_change_replay_or_blink_phase() {
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
            .store(true, Ordering::Release);
        assert_frozen(&mut app, delay);
        app.world_mut().resource_mut::<Art>().images.clear();
        app.update();
        assert!(app.world().resource::<Ready>().0);
        assert_eq!(app.world().resource::<Menu>().0.tick, 0);
        assert_eq!(app.world().resource::<Clock>().0.tick(), 2365);
        let mut changes = Vec::new();
        for tick in 1..=968 {
            let selected = app.world().resource::<Menu>().0.selected;
            app.world_mut().run_schedule(FixedUpdate);
            let state = &app.world().resource::<Menu>().0;
            if state.selected != selected {
                changes.push(tick);
            }
            assert_eq!(state.tick, tick);
            let clock = app.world().resource::<Clock>().0;
            assert_eq!(clock.tick(), 2365 + tick);
            assert_eq!(state.disc_label_visible(clock), (2365 + tick) & 0x40 != 0);
        }
        // These changes and final state are independently observed in Dolphin.
        assert_eq!(changes, [912, 943, 947]);
        let state = &app.world().resource::<Menu>().0;
        assert_eq!((state.selected, state.pulse_tick), (0, 111));
    }
}

#[test]
fn recorder_preparation_and_static_readbacks_consume_no_ticks() {
    let mut app = fixture();
    app.world_mut().resource_mut::<Ready>().0 = true;
    app.init_resource::<playthrough::Recording>();
    assert_frozen(&mut app, 128);
    app.world_mut()
        .resource_mut::<playthrough::Recording>()
        .started = true;
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Menu>().0.tick, 1);
    assert_eq!(app.world().resource::<Clock>().0.tick(), 2366);
    let mut options = app.world_mut().resource_mut::<RunOptions>();
    options.tick = Some(1);
    options.capture = Some("unused.png".into());
    assert_frozen(&mut app, 128);
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
    assert_eq!(app.world().resource::<Clock>().0.tick(), 2365 + 976);
    assert_frozen(&mut app, 100); // Movie has not prebuffered yet.
    app.world_mut().resource_mut::<movie::Playback>().active = false;
    app.world_mut().resource_mut::<Replay>().0 = None;
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
    assert_eq!(app.world().resource::<Clock>().0.tick(), 2365 + 977);
}
