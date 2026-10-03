use super::*;
use resonance_game::field::FieldInput;

#[path = "exploration_tests.rs"]
mod exploration;

#[test]
#[ignore = "requires cooked Iselia escape movies; no window or audio device"]
fn field_movies_play_after_startup_and_can_be_requested_again() -> Result<()> {
    use bevy::ecs::system::RunSystemOnce;
    let mut app = super::super::movie::tests::fixture();
    let root = app.world().resource::<RunOptions>().assets.clone();
    app.world_mut().resource_mut::<movie::Playback>().active = false;
    let mut session = Session::load(&root)?;
    let package = Arc::new(FieldPackage::prepare(
        &root,
        80,
        &mut Default::default(),
        || false,
    )?);
    session.fields.insert(80, package.clone());
    app.insert_resource(session);
    for playback in 0..2 {
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            let mut entry = FieldEntry {
                data: Some(session.data.clone()),
                available_fields: session.available_fields.clone(),
                persistent: resonance_events::PersistentState {
                    party: Some(resonance_events::party::Party::new(
                        &session.data,
                        Default::default(),
                    )?),
                    ..Default::default()
                },
                ..Default::default()
            };
            // Declared scene-entry fixture: the ranch exit writes this story
            // stage before the original field-80 script requests movie 4.
            entry
                .persistent
                .memory
                .write(0x40, symphonia_script::Width::S32, 20_308_000)?;
            session.activate(package.enter(entry)?, &package, false);
            for _ in 0..120 {
                session.field.step(Default::default())?;
                if session.field.events.world.movie.is_some() {
                    break;
                }
            }
            assert_eq!(
                session.field.events.world.movie.as_ref().unwrap().resource,
                4
            );
        }
        let start = std::time::Instant::now();
        let (mixer, mut output) = resonance_playback::Offline::new();
        while !app.world().resource::<movie::Playback>().active {
            app.world_mut().run_system_once(super::advance).unwrap();
            ensure!(
                start.elapsed() < std::time::Duration::from_secs(30),
                "field movie did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let operation = app
            .world()
            .resource::<Session>()
            .field
            .events
            .world
            .movie
            .as_ref()
            .unwrap()
            .operation
            .clone();
        assert!(!app.world().resource::<Session>().ready_for_field);
        assert_eq!(app.world().resource::<movie::Playback>().resource, Some(4));
        while app.world().resource::<movie::Playback>().active {
            app.world_mut().run_system_once(movie::update).unwrap();
            super::super::playthrough::attach::<movie::MovieAudio>(app.world_mut(), &mixer)?;
            let movie = app.world().resource::<movie::Playback>();
            if movie.is_presenting() {
                movie.wait_for_audio(534)?;
                for _ in 0..534 * 2 {
                    output.next().context("offline movie output stopped")?;
                }
            }
            if playback == 1 && movie.presented_frame.is_some() {
                break;
            }
            ensure!(
                start.elapsed() < std::time::Duration::from_secs(60),
                "field movie playback stalled"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if playback == 0 {
            let movie = app.world().resource::<movie::Playback>();
            assert!(movie.completed_naturally);
            assert_eq!(
                movie.presented_frame,
                Some(movie.asset.as_ref().unwrap().frames - 1)
            );
        } else {
            assert!(operation.is_pending());
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Enter);
            app.world_mut().run_system_once(movie::update).unwrap();
        }
        assert!(!app.world().resource::<movie::Playback>().active);
        assert_eq!(
            operation.progress().outcome,
            Some(resonance_events::Outcome::Completed(None))
        );
        app.world_mut().run_system_once(movie_handoff).unwrap();
        assert!(app.world().resource::<Session>().ready_for_field);
        app.world_mut()
            .resource_mut::<Session>()
            .field
            .step(Default::default())?;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS with current field inventories; no window or audio device"]
fn original_world_checkpoint_restores_all_mounts_without_running_field_entry() -> Result<()> {
    use super::super::saves::{SceneCheckpoint, WorldCheckpoint};
    use resonance_content::overworld::{Mount, Position, TravelState, World};
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let identity = Session::identity(&root)?;
    let mut cache = super::super::loading::Cache::default();
    let prepared = resonance_game::overworld::Prepared::load(
        &root,
        &mut cache.bytes,
        available_fields(&root)?,
        || false,
    )?;
    let data = prepared.resources.session_data.as_ref().unwrap();
    let header = resonance_persistence::Header {
        identity,
        label: "World test".into(),
        location: "Sylvarant".into(),
        played_ticks: 12345,
        saved_unix_seconds: 0,
    };
    for mount in [Mount::Foot, Mount::Noishe, Mount::Rheairds, Mount::Ship] {
        let mut persistent = resonance_events::PersistentState {
            party: Some(resonance_events::party::Party::new(
                data,
                Default::default(),
            )?),
            ..Default::default()
        };
        persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
        let state = TravelState {
            world: World::Sylvarant,
            position: Position::from_map([9770., 24160., 0.])?,
            heading: 1.25,
            camera_yaw: 2.5,
            alternate_perspective: true,
            map_display: resonance_content::overworld::MapDisplay::Full,
            mount,
            altitude: if mount.airborne() { 600. } else { 0. },
        };
        let session = resonance_game::overworld::Session::enter(
            prepared.assets(state.world, &persistent)?,
            state.clone(),
            persistent,
            resonance_game::clock::PlayTime::resume(12345),
        )?;
        let state = session.travel.state().clone();
        let saved = SceneCheckpoint::World(WorldCheckpoint {
            overworld: session.checkpoint()?,
            anchor_field: 330,
        });
        let bytes = resonance_persistence::encode(&header, &saved)?;
        let (_, decoded): (_, SceneCheckpoint) =
            resonance_persistence::decode(&bytes, &header.identity)?;
        let SceneCheckpoint::World(saved) = decoded else {
            panic!("world save decoded as a field")
        };
        let field = FieldPackage::prepare(&root, saved.anchor_field, &mut cache, || false)?;
        let mut restored =
            Session::load_world_prepared(&root, field.files, saved, &mut cache, || false)?;
        let scene = restored.overworld.as_ref().unwrap();
        assert_eq!(scene.session.travel.state(), &state);
        assert_eq!(scene.session.play_time.total(), 12345);
        assert_eq!(scene.session.events.tick(), session.events.tick());
        assert!(!restored.field.player_has_control());
        assert!(!restored.ready_for_field);
        assert!(restored.story_movie.is_none());
        assert!(restored.audio.is_some());
        let world = &mut restored.overworld.as_mut().unwrap().session;
        let tick = world.events.tick();
        world.step(resonance_game::overworld::Input {
            menu: FieldInput {
                menu: true,
                ..Default::default()
            },
            ..Default::default()
        })?;
        assert!(world.menu.is_some());
        assert!(!world.player_has_control());
        assert!(world.checkpoint().is_err());
        for _ in 0..20 {
            world.step(resonance_game::overworld::Input {
                travel: resonance_game::overworld::travel::Input {
                    cycle_map: true,
                    toggle_perspective: true,
                    ..Default::default()
                },
                ..Default::default()
            })?;
        }
        assert_eq!(world.travel.state(), &state);
        assert_eq!(world.events.tick(), tick);
        assert_eq!(world.travel.state(), &state);
        let menu_save = SceneCheckpoint::World(WorldCheckpoint {
            overworld: world.menu_checkpoint()?,
            anchor_field: 330,
        });
        let menu_bytes = resonance_persistence::encode(&header, &menu_save)?;
        let (_, saved): (_, SceneCheckpoint) =
            resonance_persistence::decode(&menu_bytes, &header.identity)?;
        let SceneCheckpoint::World(saved) = saved else {
            panic!("world menu wrote a field save")
        };
        assert_eq!(saved.overworld.state, state);
        assert_eq!(
            saved.overworld.progress.party.travel.overworld.as_ref(),
            Some(&state)
        );
        world.step(resonance_game::overworld::Input {
            menu: FieldInput {
                cancel: true,
                ..Default::default()
            },
            ..Default::default()
        })?;
        // Stop when the menu closes so no contact/event can advance this probe.
        for _ in 0..30 {
            if world.menu.is_none() {
                break;
            }
            world.step(Default::default())?;
        }
        assert!(world.menu.is_none());
        assert!(world.player_has_control());
        if mount == Mount::Foot {
            let scene = restored.overworld.as_mut().unwrap();
            scene.session.events.set_global(16, 900_000)?;
            // The original world script selects ISA_T00 from the south;
            // octants 6/7 instead enter the northern ISA_T02 map (332).
            assert!(scene.session.events.enter_landmark(2, 2)?);
            for _ in 0..120 {
                scene.session.step(Default::default())?;
                if scene.session.events.world.field_transition.is_some() {
                    break;
                }
            }
            let request = restored
                .events()
                .world
                .field_transition
                .clone()
                .context("Iselia entry did not request a field")?;
            assert_eq!(request.map, 330);
            // A failed asynchronous preparation leaves both the request and its
            // source session intact, and does not automatically retry each frame.
            let mut owner = bevy::prelude::World::new();
            owner.insert_resource(super::super::loading::Resident::default());
            owner.insert_resource(ButtonInput::<KeyCode>::default());
            owner.insert_resource(restored);
            transition_failed(&mut owner, anyhow::anyhow!("missing destination fixture"));
            for _ in 0..3 {
                transition(&mut owner);
            }
            assert!(owner.contains_resource::<TransitionFailure>());
            assert!(request.operation.is_pending());
            assert!(owner.resource::<Session>().overworld.is_some());
            restored = owner.remove_resource::<Session>().unwrap();
            let package = Arc::new(FieldPackage::prepare(
                &root,
                request.map,
                &mut cache,
                || false,
            )?);
            restored.change_field(package)?;
            assert!(!request.operation.is_pending());
            assert!(restored.overworld.is_none());
            assert_eq!(
                restored
                    .field
                    .events
                    .world
                    .party
                    .as_ref()
                    .unwrap()
                    .travel
                    .overworld
                    .as_ref(),
                Some(&state)
            );
            for _ in 0..120 {
                restored.field.step(Default::default())?;
                if restored.field.player_has_control() {
                    break;
                }
            }
            assert!(
                restored.field.player_has_control(),
                "Iselia arrival did not release control"
            );
            // Original ISA_T00's confirmed south exit is registry (2,1000).
            assert!(restored.field.events.trigger(1000, true)?);
            for _ in 0..120 {
                restored.field.step(Default::default())?;
                if restored.events().world.world_transition.is_some() {
                    break;
                }
            }
            let request = restored
                .events()
                .world
                .world_transition
                .clone()
                .context("Iselia exit did not request the world")?;
            let package = restored.world_package.clone().unwrap();
            restored.change_world(package)?;
            assert!(!request.operation.is_pending());
            assert!(restored.overworld.is_some());
        }
    }
    Ok(())
}

fn asset_root() -> std::path::PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        Into::into,
    )
}

#[test]
#[ignore = "requires cooked fields and the captured slope quicksave; no window or audio device"]
fn authored_entries_prepare_on_loading_and_refresh_cached_fields() {
    use super::super::loading::{FieldPending, Pending, Resident, Task};
    use std::{
        fs, thread,
        time::{Duration, Instant},
    };
    fn finish<T: Send + 'static>(task: Task<T>) -> Result<T> {
        let started = Instant::now();
        loop {
            if let Some(result) = task.poll()? {
                return result;
            }
            ensure!(
                started.elapsed() < Duration::from_secs(30),
                "field preparation timed out"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let source_root = Directory(
        std::env::temp_dir().join(format!("resonance-authored-loading-{}", std::process::id())),
    );
    fs::create_dir(&source_root.0).unwrap();
    fs::write(
        source_root.0.join("fields.json"),
        r#"{"332":{"module":"entry","task":"run","on":"entry"}}"#,
    )
    .unwrap();
    let source = "use game::story; use game::field; pub task run() { await field::wait_ticks(2ticks); story::set_flag(2000, true); }";
    fs::write(source_root.0.join("entry.sym"), source).unwrap();
    let root = asset_root();
    let identity = Session::identity(&root).unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/milestone-3/slope-quicksave.json");
    let (header, mut saved): (_, FieldCheckpoint) =
        resonance_persistence::decode(&fs::read(fixture).unwrap(), &identity).unwrap();
    saved.progress.event_flags.remove(&2000);
    saved.progress.event_flags.remove(&2001);
    let bytes = resonance_persistence::encode(&header, &saved).unwrap();
    let resident = Resident::default();
    let mut session = finish(
        Pending::start(
            root.clone(),
            Some(source_root.0.clone()),
            Some(bytes),
            &resident,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!session.field.player_has_control());
    assert!(session.field.checkpoint().is_err());
    assert!(!session.field.events.world.event_flags.contains(&2000));
    for _ in 0..3 {
        session.field.step(Default::default()).unwrap();
    }
    assert!(session.field.events.world.event_flags.contains(&2000));
    assert!(session.field.player_has_control());

    let previous = session.fields[&332].clone();
    fs::write(
        source_root.0.join("entry.sym"),
        source.replace("2000", "2001"),
    )
    .unwrap();
    let refreshed = finish(
        FieldPending::field(
            root.clone(),
            Some(source_root.0.clone()),
            332,
            Some(previous.clone()),
            &resident,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&previous.files, &refreshed.files));
    assert!(Arc::ptr_eq(&previous.audio, &refreshed.audio));
    session.fields.insert(332, Arc::new(refreshed));
    session.restore(saved).unwrap();
    for _ in 0..3 {
        session.field.step(Default::default()).unwrap();
    }
    assert!(session.field.events.world.event_flags.contains(&2001));
    assert!(!session.field.events.world.event_flags.contains(&2000));

    fs::write(
        source_root.0.join("entry.sym"),
        "use game::field; pub task run() { await field::notice(\"☃\"); }",
    )
    .unwrap();
    let rejected = finish(
        FieldPending::field(
            root,
            Some(source_root.0.clone()),
            332,
            Some(previous),
            &resident,
        )
        .unwrap(),
    );
    assert!(
        rejected
            .err()
            .unwrap()
            .to_string()
            .contains("prepare authored entry")
    );
    assert!(
        session.field.player_has_control(),
        "failed preparation leaves the active field usable"
    );
}

#[test]
#[ignore = "requires cooked classroom assets; no window or audio device"]
fn checkpoint_restarts_live_session_and_rejects_invalid_loads_atomically() {
    let root = asset_root();
    let session = Session::load(&root).unwrap();
    assert!(
        Arc::ptr_eq(&session.fields[&5].audio, &session.fields[&340].audio),
        "setup and classroom should share their decoded audio bank"
    );
    let mut app = App::new();
    app.insert_resource(session)
        .init_resource::<super::super::loading::Resident>()
        .add_systems(Update, transition);
    for tick in 0..3000 {
        if app.world().resource::<Session>().assets.map_id == 340 {
            break;
        }
        app.world_mut()
            .resource_mut::<Session>()
            .field
            .step(FieldInput {
                interact: tick % 120 == 0,
                ..Default::default()
            })
            .unwrap();
        app.update();
    }
    let mut session = app.world_mut().resource_mut::<Session>();
    assert_eq!(session.assets.map_id, 340);
    assert!(session.field.checkpoint().is_err());
    for tick in 0..20_000 {
        if session.field.checkpoint().is_ok() {
            break;
        }
        if let Some(movie) = &session.field.events.world.movie
            && movie.operation.is_pending()
        {
            movie.operation.complete(None).unwrap();
        }
        session
            .field
            .step(FieldInput {
                interact: tick % 120 == 0,
                ..Default::default()
            })
            .unwrap();
        session.field.events.world.audio_commands.clear();
    }
    let checkpoint = session.field.checkpoint().unwrap();
    // Menu ownership freezes the field and rejects saving immediately. It must
    // never turn a request made in a menu into a deferred exploration save.
    session
        .field
        .step(FieldInput {
            menu: true,
            ..Default::default()
        })
        .unwrap();
    assert!(session.field.menu.is_some());
    let mut menu_updates = 1;
    assert!(
        session
            .field
            .checkpoint()
            .unwrap_err()
            .to_string()
            .contains("menu")
    );
    let paused_tick = session.field.events.tick();
    for _ in 0..30 {
        session
            .field
            .step(FieldInput {
                direction: [1., 0.],
                ..Default::default()
            })
            .unwrap();
        menu_updates += 1;
        assert_eq!(session.field.events.tick(), paused_tick);
        assert!(session.field.checkpoint().is_err());
    }
    session
        .field
        .step(FieldInput {
            cancel: true,
            ..Default::default()
        })
        .unwrap();
    menu_updates += 1;
    for _ in 0..60 {
        if session.field.menu.is_none() {
            break;
        }
        assert!(session.field.checkpoint().is_err());
        session.field.step(Default::default()).unwrap();
        menu_updates += 1;
        assert_eq!(session.field.events.tick(), paused_tick);
    }
    assert!(session.field.menu.is_none(), "menu did not finish closing");
    let resumed = session.field.checkpoint().unwrap();
    assert_eq!(resumed.position, checkpoint.position);
    assert_eq!(resumed.progress.tick, checkpoint.progress.tick);
    assert_eq!(
        resumed.played_ticks(),
        checkpoint.played_ticks() + menu_updates
    );
    let header = resonance_persistence::Header {
        identity: session.identity.clone(),
        label: "Classroom exploration".into(),
        location: "Iselia school".into(),
        played_ticks: checkpoint.played_ticks(),
        saved_unix_seconds: 0,
    };
    let bytes = resonance_persistence::encode(&header, &checkpoint).unwrap();
    if let Some(output) = std::env::var_os("RESONANCE_CHECKPOINT_OUTPUT") {
        std::fs::write(output, &bytes).unwrap();
    }
    let mut times = Vec::new();
    for _ in 0..100 {
        let started = std::time::Instant::now();
        let (_, saved) = resonance_persistence::decode(&bytes, &session.identity).unwrap();
        session.restore(saved).unwrap();
        times.push(started.elapsed());
        let restored = session.field.checkpoint().unwrap();
        assert_eq!(restored.played_ticks(), checkpoint.played_ticks());
        assert_eq!(session.field.play_time.session(), 0);
        assert_eq!(restored.position, checkpoint.position);
        assert_eq!(restored.heading, checkpoint.heading);
        assert_eq!(restored.camera, checkpoint.camera);
        assert_eq!(
            restored.progress.script_globals,
            checkpoint.progress.script_globals
        );
        assert_eq!(
            serde_json::to_value(&restored.progress.party).unwrap(),
            serde_json::to_value(&checkpoint.progress.party).unwrap()
        );
        assert!(!session.movie_owns_audio());
        assert!(session.prepared_movie.is_none());
    }
    times.sort();
    eprintln!(
        "100 field initializations + JSON decode: p95={:.3} ms; {} bytes (no renderer)",
        times[94].as_secs_f64() * 1000.,
        bytes.len()
    );
    // Both aisle crossings have empty source handlers. A neutral pose update
    // can queue one, but it must not turn a valid exploration save into an error.
    for position in [[-88., -229., 0.], [-147., 79., 0.]] {
        let mut aisle = checkpoint.clone();
        aisle.position = position;
        aisle.heading = 180.;
        session.restore(aisle).unwrap();
        let restored = session.field.checkpoint().unwrap();
        assert_eq!(restored.position, position);
        assert_eq!(restored.heading, 180.);
        assert_eq!(
            restored.progress.script_globals,
            checkpoint.progress.script_globals
        );
        assert_eq!(restored.played_ticks(), checkpoint.played_ticks());
        session.field.step(Default::default()).unwrap();
        assert!(session.field.checkpoint().is_ok());
    }
    session.restore(checkpoint.clone()).unwrap();
    session
        .field
        .step(FieldInput {
            direction: [1., 0.],
            ..Default::default()
        })
        .unwrap();
    let walking = session.field.checkpoint().expect("walking permits saving");
    assert_ne!(walking.position, checkpoint.position);
    session.restore(walking.clone()).unwrap();
    assert_eq!(
        session.field.checkpoint().unwrap().position,
        walking.position
    );
    session.restore(checkpoint.clone()).unwrap();
    let mut invalid = checkpoint.clone();
    invalid.map_id = u32::MAX;
    assert!(session.restore(invalid).is_err());
    let mut invalid = checkpoint.clone();
    invalid.position[0] = f32::NAN;
    assert!(session.restore(invalid).is_err());
    let mut invalid = checkpoint.clone();
    invalid.camera.as_mut().unwrap().fov_degrees = f32::NAN;
    assert!(session.restore(invalid).is_err());
    let mut foreground = checkpoint.clone();
    foreground.position = [-540., -320., 0.];
    let error = session.restore(foreground).unwrap_err();
    // The doorway scene takes control and stages Genis before speaking. Its
    // movement can outlast initialization's bound before dialogue is visible.
    assert!(
        matches!(
            error.root_cause().to_string().as_str(),
            "saved progression restarts a foreground event"
                | "quicksave unavailable during a scripted event"
        ),
        "the doorway scene must reject loading while it owns control: {error:#}"
    );
    let after = session.field.checkpoint().unwrap();
    assert_eq!(after.position, checkpoint.position);
    assert_eq!(
        after.progress.script_globals,
        checkpoint.progress.script_globals
    );

    // The real doorway event, village exits and classroom revisit share the
    // same package/entry path as the player. Dialogue advances normally here;
    // visual timing and walking across trigger boundaries have separate cases.
    let mut cache = Default::default();
    for (key, confirmed, map, story) in [
        (3001, false, 340, 2000),
        (1000, true, 332, 2500),
        (1001, false, 330, 2500),
        (1002, false, 332, 2500),
        (1011, true, 340, 2500),
    ] {
        assert!(
            session.field.events.trigger(key, confirmed).unwrap(),
            "trigger {key} is unavailable"
        );
        for tick in 0..20_000 {
            if let Some(request) = &session.field.events.world.field_transition {
                let package = session
                    .fields
                    .get(&request.map)
                    .cloned()
                    .unwrap_or_else(|| {
                        Arc::new(
                            FieldPackage::prepare(&root, request.map, &mut cache, || false)
                                .unwrap(),
                        )
                    });
                session.change_field(package).unwrap();
            }
            session
                .field
                .step(FieldInput {
                    interact: tick % 120 == 0,
                    ..Default::default()
                })
                .unwrap();
            session.field.events.world.audio_commands.clear();
            if session.assets.map_id == map
                && session.field.story_progress().unwrap() == story
                && session.field.checkpoint().is_ok()
            {
                break;
            }
        }
        assert_eq!(session.assets.map_id, map);
        assert_eq!(session.field.story_progress().unwrap(), story);
        // Every authored scenery layer keeps its reserved script identity.
        for (actor, section) in [(999996, 0), (999997, 10), (999980, 12), (999998, 2)] {
            if session
                .assets
                .parts
                .iter()
                .any(|part| u32::from(part.resource) == section)
            {
                assert_eq!(
                    session.field.events.world.actors[&actor].resource,
                    resonance_content::field::SCENERY_RESOURCE_BASE + section
                );
            }
        }
        let checkpoint = session.field.checkpoint().unwrap();
        session.restore(checkpoint.clone()).unwrap_or_else(|error| {
            panic!("restore after trigger {key} to field {map}, story {story}: {error:#}")
        });
        assert_eq!(
            session.field.checkpoint().unwrap().position,
            checkpoint.position
        );
        assert!(!session.movie_owns_audio());
        if key == 1000 {
            // Enter the memory circle before its first-use notice has been seen.
            // Quicksave must reject the notice, then preserve its completion
            // without serializing a dialogue operation or VM stack.
            let player = session.field.events.world.controlled_actor;
            session
                .field
                .events
                .world
                .actors
                .get_mut(&player)
                .unwrap()
                .position = [1968.5966, 1005.5622, 0.];
            session.field.step(FieldInput::default()).unwrap();
            assert!(session.field.checkpoint().is_err());
            assert!(!session.field.events.world.event_flags.contains(&0x208));
            for _ in 0..1200 {
                session.field.step(FieldInput::default()).unwrap();
                assert!(session.field.checkpoint().is_err());
                if session
                    .field
                    .dialogue
                    .values()
                    .any(|d| d.fully_revealed() && d.accepts_input())
                {
                    break;
                }
            }
            session
                .field
                .step(FieldInput {
                    interact: true,
                    ..Default::default()
                })
                .unwrap();
            assert!(session.field.checkpoint().is_err());
            for _ in 0..16 {
                session.field.step(FieldInput::default()).unwrap();
                if session.field.checkpoint().is_ok() {
                    break;
                }
            }
            let at_circle = session.field.checkpoint().unwrap();
            assert!(at_circle.progress.event_flags.contains(&0x208));
            session.restore(at_circle.clone()).unwrap();
            assert_eq!(
                session.field.checkpoint().unwrap().position,
                at_circle.position
            );
            assert!(
                session
                    .field
                    .events
                    .world
                    .dialogue
                    .values()
                    .all(|d| !d.operation.is_pending())
            );
            session.restore(checkpoint).unwrap();
        }
    }
}

#[test]
#[ignore = "requires all cooked Iselia fields; registry/loader coverage, no output devices"]
fn connected_iselia_packages_preserve_locks_shop_and_both_cooking_choices() {
    use super::super::{field_audio::validation::Playback, loading::Cache};
    use resonance_events::{PersistentState, party::Party};
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct Observed {
        pages: BTreeSet<String>,
        shops: BTreeSet<u8>,
        choices: BTreeMap<u64, u8>,
    }
    fn assert_camera(field: &FieldSession, expected: [f32; 6]) {
        let camera = field.events.world.field_camera.as_ref().unwrap();
        for (actual, expected) in camera
            .position
            .into_iter()
            .chain(camera.target)
            .zip(expected)
        {
            assert!(
                (actual - expected).abs() < 0.001,
                "camera {actual} != {expected}"
            );
        }
    }
    fn settle(
        root: &Path,
        cache: &mut Cache,
        session: &mut Session,
        audio: &mut Playback,
        choice: u8,
    ) -> Observed {
        let mut observed = Observed::default();
        for tick in 0..20_000 {
            if let Some(request) = &session.field.events.world.field_transition {
                let neighborhood_entry = session.assets.map_id == 332 && request.map == 331;
                let package = session
                    .fields
                    .get(&request.map)
                    .cloned()
                    .unwrap_or_else(|| {
                        Arc::new(FieldPackage::prepare(root, request.map, cache, || false).unwrap())
                    });
                session.change_field(package).unwrap();
                if neighborhood_entry {
                    // The first view already resolves the authored entrance
                    // orbit (336, 0, 346), distance 1661, and its X bounds.
                    assert_camera(
                        &session.field,
                        [-511., 1550.6742, 762.5896, -146., 3023., 87.],
                    );
                }
                audio
                    .enter(session.audio.take().unwrap(), &mut session.field)
                    .unwrap();
            }
            let field = &mut session.field;
            for page in field.dialogue.values().filter(|page| !page.closed) {
                observed.pages.insert(page.current().text());
            }
            if let Some(shop) = &field.shop {
                observed.shops.insert(shop.id);
            }
            let mut direction = [0., 0.];
            let mut choosing = false;
            for pending in field
                .events
                .world
                .choices
                .values()
                .filter(|c| c.operation.is_pending())
            {
                choosing = true;
                let selected = pending.selected_line - pending.first_line;
                assert!(choice <= pending.last_line - pending.first_line);
                observed.choices.insert(pending.operation.id(), selected);
                direction[1] = match selected.cmp(&choice) {
                    std::cmp::Ordering::Less => -1.,
                    std::cmp::Ordering::Greater => 1.,
                    std::cmp::Ordering::Equal => 0.,
                };
            }
            let ready = field.dialogue.values().any(|page| {
                !page.closed && !page.persistent && page.fully_revealed() && page.voice_finished()
            });
            let in_menu = field.menu_is_open();
            field
                .step(if tick % 30 == 10 {
                    FieldInput {
                        direction,
                        interact: !in_menu && direction == [0., 0.] && (choosing || ready),
                        cancel: in_menu,
                        ..Default::default()
                    }
                } else {
                    FieldInput::default()
                })
                .unwrap();
            audio.step(field).unwrap();
            if field.checkpoint().is_ok() {
                return observed;
            }
        }
        panic!(
            "field {} stalled: {:?}",
            session.assets.map_id,
            session.field.events.pending_operations()
        );
    }

    let root = asset_root();
    let mut cache = Cache::default();
    let package = FieldPackage::prepare(&root, 340, &mut cache, || false).unwrap();
    let data: Arc<resonance_content::session::SessionData> =
        Arc::new(package.files.json("game/session-data.json").unwrap());
    let mut persistent = PersistentState {
        party: Some(Party::new(&data, Default::default()).unwrap()),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 1000)
        .unwrap();
    let mut field = package
        .enter(FieldEntry {
            persistent,
            data: Some(data),
            position: [-52., -619., 0.],
            available_fields: available_fields(&root).unwrap(),
            ..Default::default()
        })
        .unwrap();
    let mut audio = Playback::new((*package.audio).clone(), &mut field);
    for _ in 0..1000 {
        field.step(Default::default()).unwrap();
        audio.step(&mut field).unwrap();
        if field.checkpoint().is_ok() {
            break;
        }
    }
    let mut session = Session::load_prepared(
        &root,
        package.files,
        Some(field.checkpoint().unwrap()),
        &mut cache,
    )
    .unwrap();
    audio
        .enter(session.audio.take().unwrap(), &mut session.field)
        .unwrap();
    let mut visited = BTreeSet::from([340]);
    macro_rules! hop {
        ($key:expr, $confirmed:expr, $map:expr, $choice:expr) => {{
            assert!(session.field.events.trigger($key, $confirmed).unwrap());
            let observed = settle(&root, &mut cache, &mut session, &mut audio, $choice);
            assert_eq!(session.assets.map_id, $map, "trigger {}", $key);
            visited.insert($map);
            observed
        }};
        ($key:expr, $confirmed:expr, $map:expr) => {
            hop!($key, $confirmed, $map, 0)
        };
    }
    // Exact source registry transitions, not a claim of natural walking coverage.
    hop!(3001, false, 340);
    assert_eq!(session.field.story_progress().unwrap(), 2000);
    hop!(1000, true, 332);
    hop!(1005, false, 331);
    hop!(1002, false, 332);
    hop!(1001, false, 330);
    assert_eq!(session.field.story_progress().unwrap(), 2500);
    assert!(
        hop!(2002, true, 330)
            .pages
            .iter()
            .any(|p| p.contains("locked"))
    );
    hop!(2001, true, 333);
    // The shop entrance supplies no camera template. Its script relies on the
    // standard shoulder-height target before locking the view to the counter.
    let camera = session.field.events.world.field_camera.as_ref().unwrap();
    assert_eq!(camera.current().offset, [0., 0., 87.]);
    assert_camera(
        &session.field,
        [-152., -845., 136.5519, -152., 551., 38.892956],
    );
    assert!(session.field.events.interact(501).unwrap());
    assert_eq!(
        settle(&root, &mut cache, &mut session, &mut audio, 0).shops,
        [1].into()
    );
    assert!(session.field.player_has_control());
    hop!(1000, true, 330);
    hop!(2003, true, 338);
    assert!(session.field.events.interact(202).unwrap());
    assert!(
        settle(&root, &mut cache, &mut session, &mut audio, 0)
            .pages
            .iter()
            .any(|p| p.contains("locked"))
    );
    hop!(1000, true, 330);
    hop!(1001, false, 331);
    assert!(
        hop!(1003, true, 331)
            .pages
            .iter()
            .any(|p| p.contains("locked"))
    );
    hop!(1004, true, 336);
    hop!(1002, false, 337);
    hop!(1001, false, 336);
    hop!(1001, true, 331);
    hop!(1002, false, 332);
    assert!(
        hop!(1010, true, 332)
            .pages
            .iter()
            .any(|p| p.contains("locked"))
    );
    hop!(1011, true, 340);
    hop!(1000, true, 332);
    hop!(1001, false, 330);

    let mut later = session.field.checkpoint().unwrap();
    later.progress.script_globals[0x40 / 4] = 112000;
    session.restore(later).unwrap();
    audio
        .enter(session.audio.take().unwrap(), &mut session.field)
        .unwrap();
    hop!(2002, true, 334);
    hop!(1000, true, 330);
    hop!(1001, false, 331);
    hop!(1003, true, 335);
    hop!(1000, true, 331);
    hop!(1002, false, 332);
    hop!(1010, true, 339);
    hop!(1000, true, 332);
    hop!(1001, false, 330);
    assert_eq!(visited, (330..=340).collect());

    let mut tutorial = session.field.checkpoint().unwrap();
    tutorial.progress.script_globals[0x40 / 4] = 202000;
    tutorial.progress.event_flags.remove(&275);
    for choice in [0, 1] {
        session.restore(tutorial.clone()).unwrap();
        audio
            .enter(session.audio.take().unwrap(), &mut session.field)
            .unwrap();
        let before = &tutorial.progress.party.items;
        let expected: BTreeMap<_, _> = [121, 100, 86]
            .into_iter()
            .map(|id| (id, before.get(&id).copied().unwrap_or(0) + 3))
            .collect();
        let observed = hop!(2003, true, 338, choice);
        assert_eq!(observed.choices.into_values().collect::<Vec<_>>(), [choice]);
        assert!(session.field.events.world.event_flags.contains(&275));
        for (&id, &count) in &expected {
            assert_eq!(
                session.field.events.world.party.as_ref().unwrap().items[&id],
                count
            );
        }
        let saved = session.field.checkpoint().unwrap();
        session.restore(saved).unwrap();
        audio
            .enter(session.audio.take().unwrap(), &mut session.field)
            .unwrap();
        hop!(1000, true, 330);
        assert!(
            hop!(2003, true, 338).choices.is_empty(),
            "tutorial repeated on re-entry"
        );
        for (&id, &count) in &expected {
            assert_eq!(
                session.field.events.world.party.as_ref().unwrap().items[&id],
                count
            );
        }
        hop!(1000, true, 330);
    }
    hop!(1001, false, 331);
    assert!(
        !hop!(1004, true, 331).pages.is_empty(),
        "later Colette-house refusal disappeared"
    );
}

#[test]
#[ignore = "requires cooked field 332 and the captured slope quicksave; no output devices"]
fn moving_slope_checkpoint_survives_cold_and_warm_loads() {
    use sha2::{Digest, Sha256};
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = asset_root();
    let bytes = std::fs::read(project.join("local/milestone-3/slope-quicksave.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "a63b605494bc8e659bd3c901079f7122e62f8a0f7374560c3827ee686b4639ee"
    );
    let (_, saved): (_, FieldCheckpoint) =
        resonance_persistence::decode(&bytes, &Session::identity(&root).unwrap()).unwrap();
    let mut cache = super::super::loading::Cache::default();
    let package = FieldPackage::prepare(&root, saved.map_id, &mut cache, || false).unwrap();
    let ground = resonance_game::field::navigation::WalkMesh::new(&package.assets.ground).unwrap();
    assert!((ground.height(saved.position, 32.).unwrap() - saved.position[2]).abs() > 0.001);
    let mut session =
        Session::load_prepared(&root, package.files, Some(saved.clone()), &mut cache).unwrap();
    for _ in 0..2 {
        let restored = session.field.checkpoint().unwrap();
        assert_eq!(restored.position, saved.position);
        assert_eq!(restored.heading, saved.heading);
        assert_eq!(restored.played_ticks(), saved.played_ticks());
        assert_eq!(
            restored.progress.script_globals,
            saved.progress.script_globals
        );
        assert_eq!(restored.progress.event_flags, saved.progress.event_flags);
        session.field.step(Default::default()).unwrap();
        let grounded = session.field.checkpoint().unwrap();
        assert_ne!(grounded.position[2], saved.position[2]);
        session.restore(saved.clone()).unwrap();
    }
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; exercises original cinematics without a window or audio device"]
fn original_world_cinematics_prepare_chain_and_return_without_resuming_the_caller() -> Result<()> {
    use resonance_content::overworld::{Mount, Position, TravelState, World};
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let mut cache = super::super::loading::Cache::default();
    let prepared = resonance_game::overworld::Prepared::load(
        &root,
        &mut cache.bytes,
        available_fields(&root)?,
        || false,
    )?;
    let mut persistent = resonance_events::PersistentState {
        party: Some(resonance_events::party::Party::new(
            prepared.resources.session_data.as_ref().unwrap(),
            Default::default(),
        )?),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
    let state = TravelState {
        world: World::Sylvarant,
        position: Position::from_map([9770., 24160., 0.])?,
        heading: 0.,
        camera_yaw: 0.,
        alternate_perspective: false,
        map_display: Default::default(),
        mount: Mount::Foot,
        altitude: 0.,
    };
    let world = resonance_game::overworld::Session::enter(
        prepared.assets(state.world, &persistent)?,
        state,
        persistent,
        Default::default(),
    )?;
    let saved = super::super::saves::WorldCheckpoint {
        overworld: world.checkpoint()?,
        anchor_field: 330,
    };
    let field = FieldPackage::prepare(&root, 330, &mut cache, || false)?;
    let mut owner = Session::load_world_prepared(&root, field.files, saved, &mut cache, || false)?;
    let package = owner.world_package.clone().unwrap();
    for id in 513..=526 {
        let position = owner
            .overworld
            .as_ref()
            .unwrap()
            .session
            .travel
            .state()
            .position;
        let request = owner
            .events_mut()
            .world
            .request_world(
                id,
                0,
                Some(resonance_events::SceneDestination {
                    map: 3000,
                    position: [0.; 3],
                    heading: 0.,
                }),
            )
            .map_err(anyhow::Error::msg)?;
        owner.change_world(package.clone())?;
        assert_eq!(
            request.progress().outcome,
            Some(resonance_events::Outcome::Cancelled)
        );
        let scenes = if id == 516 { vec![516, 517] } else { vec![id] };
        for expected in scenes {
            let scene = owner.overworld.as_mut().unwrap();
            assert_eq!(scene.session.cinematic.as_ref().unwrap().id, expected);
            assert!(scene.session.checkpoint().is_err());
            assert!(!scene.session.player_has_control());
            for _ in 0..2200 {
                scene.session.step(Default::default())?;
                if scene.session.events.world.world_transition.is_some() {
                    break;
                }
            }
            let next = owner
                .events()
                .world
                .world_transition
                .as_ref()
                .context("cinematic did not finish")?
                .operation
                .clone();
            let ticks = owner
                .overworld
                .as_ref()
                .unwrap()
                .session
                .cinematic
                .as_ref()
                .unwrap()
                .ticks();
            for _ in 0..30 {
                owner
                    .overworld
                    .as_mut()
                    .unwrap()
                    .session
                    .step(Default::default())?;
            }
            assert_eq!(
                owner
                    .overworld
                    .as_ref()
                    .unwrap()
                    .session
                    .cinematic
                    .as_ref()
                    .unwrap()
                    .ticks(),
                ticks
            );
            owner.change_world(package.clone())?;
            assert_eq!(
                next.progress().outcome,
                Some(resonance_events::Outcome::Cancelled)
            );
        }
        let world = &owner.overworld.as_ref().unwrap().session;
        assert!(world.cinematic.is_none());
        // A normal world return resolves terrain height again; horizontal
        // placement and the stored field-return pose must survive the film.
        assert_eq!(
            &world.travel.state().position.map()[..2],
            &position.map()[..2]
        );
        if id == 518 {
            assert_eq!(world.travel.state().mount, Mount::Rheairds);
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires the complete cooked field catalogue in RESONANCE_WORLD_ASSETS"]
fn original_world_landmarks_prepare_and_enter_their_fields() -> Result<()> {
    use resonance_content::overworld::{Mount, Position, TravelState, World};
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let fields = available_fields(&root)?;
    ensure!(fields.len() > 490, "full field catalogue required");
    let prepared =
        resonance_game::overworld::Prepared::load(&root, &mut Default::default(), fields, || {
            false
        })?;
    let selected: Option<BTreeSet<u16>> = std::env::var("RESONANCE_WORLD_LANDMARKS")
        .ok()
        .map(|s| s.split(',').map(|v| v.parse().unwrap()).collect());
    let mut failures = Vec::new();
    let mut destinations = BTreeSet::new();
    for world in [World::Sylvarant, World::TetheAlla] {
        for landmark in &prepared.definition.landmarks.worlds[world.index()] {
            if selected
                .as_ref()
                .is_some_and(|ids| !ids.contains(&landmark.id))
            {
                continue;
            }
            // Field points are handled by world reward/skit services, not the
            // script's town registry (some IDs intentionally alias its entries).
            if matches!(
                landmark.marker,
                resonance_content::overworld::Marker::FieldPoint
            ) {
                continue;
            }
            let result = (|| -> Result<()> {
                let mut party = resonance_events::party::Party::new(
                    prepared.resources.session_data.as_ref().unwrap(),
                    Default::default(),
                )?;
                party.formation = vec![1, 2, 3, 4];
                party.travel.saved_formation = party.formation.clone();
                let mut progress = resonance_events::PersistentState {
                    party: Some(party),
                    ..Default::default()
                };
                progress
                    .memory
                    .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
                progress
                    .memory
                    .write(0x50, symphonia_script::Width::S32, world.index() as i32)?;
                let mut scene = resonance_game::overworld::Session::enter(
                    prepared.assets(world, &progress)?,
                    TravelState {
                        world,
                        position: Position::from_map([
                            landmark.position[0],
                            landmark.position[1],
                            0.,
                        ])?,
                        heading: 0.,
                        camera_yaw: 0.,
                        alternate_perspective: false,
                        map_display: Default::default(),
                        mount: Mount::Foot,
                        altitude: 0.,
                    },
                    progress,
                    Default::default(),
                )?;
                if !scene.events.enter_landmark(landmark.id, 2)? {
                    return Ok(());
                }
                for _ in 0..180 {
                    scene.step(Default::default())?;
                    if scene.events.world.field_transition.is_some() {
                        break;
                    }
                }
                let Some(request) = scene.events.world.field_transition.as_ref() else {
                    return Ok(());
                };
                let map = request.map;
                let mut cache = super::super::loading::Cache::default();
                let package = FieldPackage::prepare(&root, map, &mut cache, || false)
                    .with_context(|| format!("prepare map {map}"))?;
                let mut entry = scene.field_entry()?;
                if let Some(camera) = &entry.camera {
                    ensure!(
                        camera.camera.actor
                            == i32::from(entry.persistent.party.as_ref().unwrap().field_leader),
                        "world camera retained a non-field actor for map {map}"
                    );
                }
                entry.allow_incomplete_scripts = true;
                let mut field = package
                    .enter(entry)
                    .with_context(|| format!("enter map {map}"))?;
                for _ in 0..2400 {
                    field
                        .step(FieldInput {
                            interact: true,
                            accelerate_dialogue: true,
                            ..Default::default()
                        })
                        .with_context(|| format!("step map {map}"))?;
                    field.events.world.audio_commands.clear();
                    if field.player_has_control()
                        || field.events.world.field_transition.is_some()
                        || field.events.world.world_transition.is_some()
                    {
                        break;
                    }
                }
                println!(
                    "Landmark {} ({}) entered map {map}; control={}",
                    landmark.id,
                    landmark.name,
                    field.player_has_control()
                );
                if let Some(reason) = &field.events.exploration_error {
                    println!("map {map} exploration fallback: {reason}");
                }
                if !field.player_has_control() {
                    println!("map {map} pending: {:?}", field.events.pending_operations());
                    println!(
                        "map {map} services: movie={:?}, skit={:?}, battle={:?}, menu={:?}, field={:?}, world={:?}",
                        field.events.world.movie,
                        field.events.world.skit_request,
                        field.events.world.battle_request,
                        field.events.world.menu_request,
                        field.events.world.field_transition,
                        field.events.world.world_transition
                    );
                }
                ensure!(
                    field.player_has_control()
                        || field.events.world.field_transition.is_some()
                        || field.events.world.world_transition.is_some(),
                    "map {map} entrance remained suspended without a scene handoff"
                );
                if landmark.id == 53 {
                    ensure!(
                        field.player_has_control(),
                        "caravan introduction did not finish"
                    );
                    ensure!(
                        field.events.interact(102)?,
                        "caravan NPC interaction missing"
                    );
                    let mut dialogue_seen = false;
                    for _ in 0..2400 {
                        field.step(FieldInput {
                            interact: true,
                            accelerate_dialogue: true,
                            ..Default::default()
                        })?;
                        dialogue_seen |= !field.events.world.dialogue.is_empty();
                        field.events.world.audio_commands.clear();
                        if field.player_has_control() {
                            break;
                        }
                    }
                    ensure!(
                        dialogue_seen && field.player_has_control(),
                        "caravan NPC dialogue did not finish"
                    );
                    ensure!(field.events.trigger(1000, true)?, "caravan exit missing");
                    for _ in 0..180 {
                        field.step(Default::default())?;
                        if field.events.world.world_transition.is_some() {
                            break;
                        }
                    }
                    ensure!(
                        field.events.world.world_transition.is_some(),
                        "caravan did not return to the world"
                    );
                    println!("Nova's Caravan NPC dialogue and world return passed");
                }
                field.step(FieldInput {
                    start: true,
                    ..Default::default()
                })?;
                let return_request = field
                    .events
                    .world
                    .world_transition
                    .as_ref()
                    .context("test return control did not request the overworld")?;
                ensure!(
                    return_request.location == 0,
                    "test return lost its saved world position"
                );
                let progress = field.events.persistent_state()?;
                let resumed = resonance_game::overworld::Session::return_from_field(
                    prepared.assets(world, &progress)?,
                    return_request,
                    progress,
                    field.play_time,
                )?;
                ensure!(
                    resumed.travel.state().world == world,
                    "test returned to the wrong world"
                );
                destinations.insert(map);
                Ok(())
            })();
            if let Err(error) = result {
                failures.push(format!("{} ({}): {error:#}", landmark.id, landmark.name));
            }
        }
    }
    println!(
        "Prepared and entered {} distinct destination fields",
        destinations.len()
    );
    ensure!(
        failures.is_empty(),
        "landmark failures:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[test]
#[ignore = "requires the complete cooked field catalogue in RESONANCE_WORLD_ASSETS"]
fn original_all_fields_have_complete_preparation_inventories() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let fields = available_fields(&root)?;
    ensure!(fields.len() > 490, "full field catalogue required");
    let selected: Option<BTreeSet<u32>> = std::env::var("RESONANCE_FIELD_MAPS")
        .ok()
        .map(|s| s.split(',').map(|v| v.parse().unwrap()).collect());
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut walking = 0;
    for map in fields
        .iter()
        .copied()
        .filter(|&map| map < 3000 && selected.as_ref().is_none_or(|ids| ids.contains(&map)))
    {
        checked += 1;
        let mut cache = super::super::loading::Cache::default();
        let result = (|| -> Result<()> {
            let package = FieldPackage::prepare(&root, map, &mut cache, || false)?;
            if package.assets.ground.is_empty() && package.assets.parts.is_empty() {
                println!("Map {map} has no room geometry or collision; checked its inventory only");
                return Ok(());
            }
            let data = Arc::new(
                package
                    .files
                    .json::<resonance_content::session::SessionData>("game/session-data.json")?,
            );
            let mut party = resonance_events::party::Party::new(&data, Default::default())?;
            party.formation = vec![1, 2, 3, 4];
            party.travel.saved_formation = party.formation.clone();
            let mut progress = resonance_events::PersistentState {
                party: Some(party),
                ..Default::default()
            };
            progress
                .memory
                .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
            let mut field = package.enter(FieldEntry {
                allow_incomplete_scripts: true,
                persistent: progress,
                data: Some(data),
                available_fields: fields.clone(),
                ..Default::default()
            })?;
            field.enter_exploration("Field catalogue walking check".into())?;
            ensure!(
                field.player_has_control(),
                "exploration did not grant control"
            );
            let initial = field.events.world.actors[&field.events.world.controlled_actor].position;
            let mut walked = false;
            for direction in [[1., 0.], [-1., 0.], [0., 1.], [0., -1.]] {
                for _ in 0..8 {
                    field.step(resonance_game::field::FieldInput {
                        direction,
                        ..Default::default()
                    })?;
                    field.events.world.audio_commands.clear();
                    let current =
                        field.events.world.actors[&field.events.world.controlled_actor].position;
                    walked |= current != initial;
                }
            }
            ensure!(walked, "player could not walk from the exploration spawn");
            field.checkpoint()?;
            walking += 1;
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("map {map}: {error:#}"));
        }
    }
    println!("Checked {checked} field preparation inventories and {walking} walking sessions");
    ensure!(
        failures.is_empty(),
        "field preparation failures:\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; original Salvation interior handoff"]
fn original_salvation_interior_handoff_keeps_its_destination() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let mut cache = super::super::loading::Cache::default();
    let package = FieldPackage::prepare(&root, 81, &mut cache, || false)?;
    let data: Arc<resonance_content::session::SessionData> =
        Arc::new(package.files.json("game/session-data.json")?);
    let mut persistent = resonance_events::PersistentState {
        party: Some(resonance_events::party::Party::new(
            &data,
            Default::default(),
        )?),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
    let mut field = package.enter(FieldEntry {
        allow_incomplete_scripts: true,
        persistent,
        data: Some(data.clone()),
        available_fields: available_fields(&root)?,
        position: [557., 83., 124.],
        ..Default::default()
    })?;
    for _ in 0..1200 {
        field.step(FieldInput {
            interact: true,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        if field.player_has_control() {
            break;
        }
    }
    ensure!(
        field.player_has_control(),
        "Salvation outside did not release control"
    );
    ensure!(
        field.events.trigger(1002, true)?,
        "Salvation doorway trigger missing"
    );
    for _ in 0..600 {
        field.step(Default::default())?;
        if field.events.exploration_error.is_some() || field.events.world.field_transition.is_some()
        {
            break;
        }
    }
    ensure!(
        field.events.exploration_error.is_none(),
        "door script failed: {:?}",
        field.events.exploration_error
    );
    let request = field
        .events
        .world
        .field_transition
        .as_ref()
        .context("door did not request interior")?;
    ensure!(request.map == 95, "wrong Salvation interior");
    let package = FieldPackage::prepare(&root, request.map, &mut cache, || false)?;
    let mut inside = package.enter(FieldEntry {
        allow_incomplete_scripts: true,
        persistent: field.events.persistent_state()?,
        data: Some(data),
        available_fields: available_fields(&root)?,
        position: request.position,
        heading: request.heading,
        camera: request.camera.clone(),
        ..Default::default()
    })?;
    for _ in 0..1200 {
        inside.step(FieldInput {
            interact: true,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        if inside.player_has_control() {
            break;
        }
    }
    ensure!(
        inside.player_has_control(),
        "Salvation interior did not release control"
    );
    ensure!(
        inside.events.exploration_error.is_none(),
        "interior unexpectedly fell back"
    );
    ensure!(
        inside.events.trigger(1000, true)?,
        "Salvation interior exit missing"
    );
    for _ in 0..180 {
        inside.step(Default::default())?;
        if inside.events.world.field_transition.is_some() {
            break;
        }
    }
    ensure!(
        inside
            .events
            .world
            .field_transition
            .as_ref()
            .is_some_and(|request| request.map == 81),
        "Salvation interior failed to return outside"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_WORLD_ASSETS; original overworld discovery skit choices"]
fn original_world_discovery_skits_show_dialogue_and_complete_choices() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let prepared = resonance_game::overworld::Prepared::load(
        &root,
        &mut Default::default(),
        available_fields(&root)?,
        || false,
    )?;
    let data = prepared.resources.session_data.as_ref().unwrap();
    let mut world = resonance_events::GameWorld::default();
    world.party = Some(resonance_events::party::Party::new(
        data,
        Default::default(),
    )?);
    world.input_enabled = true;
    let program = Arc::new(symphonia_script::Program::decode(
        &prepared.files.read(&prepared.definition.script.path)?,
    )?);
    let mut parent = resonance_events::EventRuntime::with_state(
        program,
        prepared.resources.clone(),
        world,
        Default::default(),
    )?;
    let skits = resonance_game::skit::Prepared::load(
        prepared.resources.skits.as_ref().unwrap().clone(),
        &prepared.files,
    )?;
    const STATE: &str = "test::skit::visits";
    parent.world.script_state.insert(STATE.into(), 0);
    for (&id, skit) in skits.range(502..=534) {
        let before = parent.world.script_state[STATE];
        let mut playback =
            resonance_game::skit::Playback::start(skit, &mut parent, true, false, None)?;
        assert_eq!(playback.id, id);
        assert_eq!(playback.events.world.script_state[STATE], before);
        playback
            .events
            .world
            .script_state
            .insert(STATE.into(), before + 1);
        let (mut text_seen, mut choice_seen, mut done) = (false, false, false);
        for tick in 0..36_000 {
            text_seen |= playback
                .dialogue
                .values()
                .any(|p| p.window_visible() && !p.current().glyphs.is_empty());
            choice_seen |= !playback.events.world.choices.is_empty();
            done = playback.step(
                &mut parent,
                resonance_game::skit::Input {
                    confirm: tick % 30 == 10,
                    ..Default::default()
                },
            )?;
            parent.world.audio_commands.clear();
            if done {
                break;
            }
        }
        ensure!(
            done && text_seen && choice_seen,
            "skit {id} failed: complete={done}, text={text_seen}, choice={choice_seen}"
        );
        assert!(playback.step(&mut parent, Default::default())?);
        assert_eq!(parent.world.script_state[STATE], before + 1);
    }
    let before = parent.save_progress()?;
    let mut preview =
        resonance_game::skit::Playback::start(&skits[&502], &mut parent, true, true, None)?;
    preview.events.world.script_state.clear();
    preview.events.world.event_flags.clear();
    preview.events.world.party.as_mut().unwrap().gald = before.party.gald + 1;
    preview.events.set_global(16, 99)?;
    for _ in 0..120 {
        preview.step(
            &mut parent,
            resonance_game::skit::Input {
                skip: true,
                ..Default::default()
            },
        )?;
    }
    assert!(preview.step(&mut parent, Default::default())?);
    let after = parent.save_progress()?;
    assert_eq!(after.script_state, before.script_state);
    assert_eq!(after.script_globals, before.script_globals);
    assert_eq!(after.event_flags, before.event_flags);
    assert_eq!(after.party.gald, before.party.gald);
    // Exercise the coastal Raine discovery through the owning world VM too:
    // completion must retire the skit and release the suspended contact event.
    let mut persistent = parent.persistent_state()?;
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 14_000_000)?;
    persistent.party.as_mut().unwrap().formation = vec![1, 2, 3, 4];
    let mut scene = resonance_game::overworld::Session::enter(
        prepared.assets(resonance_game::overworld::World::Sylvarant, &persistent)?,
        resonance_content::overworld::TravelState {
            world: resonance_game::overworld::World::Sylvarant,
            position: resonance_game::overworld::Position::from_map([2763., 49152., 0.])?,
            heading: 0.,
            camera_yaw: 0.,
            alternate_perspective: false,
            map_display: Default::default(),
            mount: resonance_content::overworld::Mount::Foot,
            altitude: 0.,
        },
        persistent,
        Default::default(),
    )?;
    let mut started = false;
    for tick in 0..12_000 {
        scene.step(resonance_game::overworld::Input {
            confirm: tick % 30 == 10,
            ..Default::default()
        })?;
        scene.events.world.audio_commands.clear();
        started |= scene.active_skit.is_some();
        if started && scene.active_skit.is_none() && scene.player_has_control() {
            break;
        }
    }
    ensure!(
        started && scene.player_has_control() && scene.active_skit.is_none(),
        "coastal discovery did not restore world control"
    );
    ensure!(
        scene
            .events
            .world
            .party
            .as_ref()
            .unwrap()
            .travel
            .visited_locations
            .contains(&65),
        "coastal discovery was not consumed"
    );
    Ok(())
}
