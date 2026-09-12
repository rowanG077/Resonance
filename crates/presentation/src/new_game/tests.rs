use super::*;
use resonance_game::field::FieldInput;

fn asset_root() -> std::path::PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        Into::into,
    )
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
            let in_menu = field.menu.is_some() || field.shop.is_some();
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
            available_fields: PLAYABLE_FIELDS.into(),
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
        &mut cache.audio,
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
    assert_eq!(visited, PLAYABLE_FIELDS.into());

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
