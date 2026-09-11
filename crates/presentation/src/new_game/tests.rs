use super::*;
use resonance_game::field::FieldInput;

#[test]
#[ignore = "requires cooked classroom assets; no window or audio device"]
fn checkpoint_restarts_live_session_and_rejects_invalid_loads_atomically() {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        Into::into,
    );
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
    let resumed = session.field.checkpoint().unwrap();
    assert_eq!(resumed.position, checkpoint.position);
    assert_eq!(resumed.progress.tick, checkpoint.progress.tick);
    assert_eq!(resumed.played_ticks(), checkpoint.played_ticks() + 32);
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
        if matches!(map, 330 | 332) {
            // The optional scenery layer must reach the VM and renderer, not
            // disappear through a fixed two-entry binding list.
            for (actor, section) in [(999996, 0), (999998, 2), (999980, 12)] {
                assert_eq!(
                    session.field.events.world.actors[&actor].resource,
                    resonance_content::field::SCENERY_RESOURCE_BASE + section
                );
            }
        }
        let checkpoint = session.field.checkpoint().unwrap();
        session.restore(checkpoint.clone()).unwrap();
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
#[ignore = "requires cooked field 332 and the captured slope quicksave; no output devices"]
fn moving_slope_checkpoint_survives_cold_and_warm_loads() {
    use sha2::{Digest, Sha256};
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = project.join("local/cooked");
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
        Session::load_prepared(&root, package.files, Some(saved.clone()), &mut cache.audio)
            .unwrap();
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
