use super::super::saves::assert_checkpoint;
use super::*;
use resonance_events::input::{Button, Buttons};
use resonance_game::field::FieldInput;

use crate::test_support::field_checkpoint;

fn assert_shared_definitions(session: &Session) {
    let resources = session.field.events.resources();
    assert!(Arc::ptr_eq(
        &session.data,
        resources.session_data.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        session.data.rules.as_ref().unwrap(),
        session.field.menu_data().unwrap()
    ));
}

fn asset_root() -> std::path::PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
        Into::into,
    )
}

#[test]
#[ignore = "requires current opening field/menu assets; no output devices"]
fn malformed_optional_label_is_admitted_before_startup_and_rejected_on_use() -> Result<()> {
    use resonance_content::{
        diagnostics::Diagnostics,
        menu_data::{ItemUse, MenuData},
    };
    use resonance_game::menu::{Menu, Page, Resources};
    let root = asset_root();
    let healthy = Files::load(
        &root,
        &["fields/map-332.preload.json"],
        &mut Default::default(),
        || false,
    )?;
    let mut checkpoint = field_checkpoint(&healthy)?;
    let original: MenuData = healthy.json("game/menu-data.json")?;
    assert!(
        serde_json::to_value(MenuData::load(&healthy)?)? == serde_json::to_value(&original)?,
        "optional-caption admission changed healthy menu definitions"
    );
    let item = original
        .items
        .iter()
        .position(|item| matches!(item.field_use, Some(ItemUse::EncounterRate { rate: 1 })))
        .context("fixture has no Holy Bottle")?;
    checkpoint.progress.party.items = [(u16::try_from(item)?, 1)].into();
    let mut document: serde_json::Value = healthy.json("game/menu-data.json")?;
    document["presentation"]["labels"]["holy_aura"] = serde_json::json!({"unexpected": "object"});
    let location = document["presentation"]["world_map"]["locations"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    document["presentation"]["world_map"]["locations"][&location]["point"] =
        serde_json::json!(false);
    document["presentation"]["titles"][0][0]["description"] = false.into();
    document["presentation"]["strategy"]["keyboard"] = serde_json::json!([1, 2]);
    document["presentation"]["ex_skills"]["labels"]["title"] =
        serde_json::json!({"unexpected": "object"});
    document["presentation"]["status"]["technical_type"] = false.into();
    document["presentation"]["cooking"]["recipes"][0]["description"] = false.into();
    for paranoid in [false, true] {
        let diagnostics = Diagnostics::new(paranoid);
        let mut files = Files::load_with_diagnostics(
            &root,
            &["fields/map-332.preload.json"],
            &mut Default::default(),
            || false,
            diagnostics.clone(),
        )?;
        files.insert(
            "game/menu-data.json".into(),
            serde_json::to_vec(&document)?.into(),
        );
        let loaded = Session::load_prepared(
            &root,
            Arc::new(files),
            Some(checkpoint.clone()),
            None,
            &mut Default::default(),
        );
        assert!(
            diagnostics
                .entries()
                .iter()
                .any(|entry| entry.scope == "menu label holy_aura")
        );
        if paranoid {
            assert!(loaded.is_err());
            continue;
        }
        let session = loaded?;
        assert_shared_definitions(&session);
        assert!(
            session
                .field
                .menu_data()
                .unwrap()
                .label("holy_aura")
                .is_err()
        );
        assert!(
            session
                .field
                .menu_data()
                .unwrap()
                .presentation
                .world_map
                .is_none()
        );
        assert!(session.field.menu_data().unwrap().world_map_text().is_err());
        assert!(session.field.menu_data().unwrap().title_text(1, 1).is_err());
        assert_eq!(
            session.field.menu_data().unwrap().title_text(1, 2)?.name,
            original.title_text(1, 2)?.name
        );
        assert_eq!(
            session.field.menu_data().unwrap().shop_text(0)?,
            original.shop_text(0)?
        );
        assert!(
            session
                .field
                .menu_data()
                .unwrap()
                .strategy_text()?
                .keyboard()
                .is_err()
        );
        assert_eq!(
            session.field.menu_data().unwrap().strategy_presets()?,
            original.strategy_presets()?
        );
        assert!(session.field.menu_data().unwrap().ex_skill_text().is_err());
        assert!(session.field.menu_data().unwrap().status_text().is_err());
        assert!(
            session
                .field
                .menu_data()
                .unwrap()
                .cooking_text()?
                .recipe(0)
                .is_err()
        );
        assert_eq!(
            session
                .field
                .menu_data()
                .unwrap()
                .cooking_text()?
                .recipe(1)?
                .name,
            original.cooking_text()?.recipe(1)?.name
        );
        assert_eq!(
            session.field.menu_data().unwrap().items_text()?.items[item]
                .as_ref()
                .unwrap()
                .name,
            original.items_text()?.items[item].as_ref().unwrap().name
        );
        let mut menu = Menu::new(Page::Items, Some(checkpoint.clone()), false);
        menu.resources = Some(Arc::new(Resources {
            session: session.data.clone(),
            data: session.field.menu_data().unwrap().clone(),
            files: session.files(),
        }));
        menu.inventory.category = original.items[item].inventory_category().unwrap();
        let before = serde_json::to_vec(&menu.checkpoint.as_ref().unwrap().progress.party)?;
        assert_eq!(menu.inventory_items(), [u16::try_from(item)?]);
        assert_eq!(
            menu.step(FieldInput {
                pressed_buttons: [resonance_events::input::Button::Accept].into(),
                ..Default::default()
            }),
            Some(4)
        );
        assert!(
            menu.notice
                .as_deref()
                .is_some_and(|notice| notice.contains("Item notice unavailable"))
        );
        assert!(menu.take_failure().is_none());
        assert_eq!(
            serde_json::to_vec(&menu.checkpoint.as_ref().unwrap().progress.party)?,
            before
        );
        assert_eq!(menu.page, Page::Items);
        assert!(
            diagnostics
                .entries()
                .iter()
                .any(|entry| entry.scope == "Item notice unavailable")
        );
        menu.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Cancel].into(),
            ..Default::default()
        });
        assert!(menu.notice.is_none());
        menu.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Cancel].into(),
            ..Default::default()
        });
        menu.step(FieldInput {
            pressed_buttons: [resonance_events::input::Button::Cancel].into(),
            ..Default::default()
        });
        assert_eq!(menu.page, Page::Main);

        let mut invalid_gameplay = (*session.files()).clone();
        document["items"] = serde_json::json!("malformed gameplay records");
        invalid_gameplay.insert(
            "game/menu-data.json".into(),
            serde_json::to_vec(&document)?.into(),
        );
        assert!(MenuData::load(&invalid_gameplay).is_err());
        document["items"] = serde_json::to_value(&original.items)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked fields; no window or audio device"]
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
    let source_root = tempfile::tempdir().unwrap();
    fs::write(
        source_root.path().join("fields.json"),
        r#"{"332":{"module":"entry","task":"run","on":"entry"}}"#,
    )
    .unwrap();
    let source = "script field; use game::story; use game::field; pub task run() { await field::wait_ticks(ticks(2)); story::set_flag(2000, true); }";
    fs::write(source_root.path().join("entry.sym"), source).unwrap();
    let root = asset_root();
    let files = Files::load(
        &root,
        &[manifest_path(332).as_str()],
        &mut Default::default(),
        || false,
    )
    .unwrap();
    let mut saved = field_checkpoint(&files).unwrap();
    let header = resonance_persistence::Header {
        identity: resonance_persistence::Identity::load(&files).unwrap(),
        label: "Authored entry fixture".into(),
        location: "Iselia".into(),
        played_ticks: saved.played_ticks,
        saved_unix_seconds: 0,
    };
    saved.progress.event_flags.remove(&2000);
    saved.progress.event_flags.remove(&2001);
    let bytes = resonance_persistence::encode(
        &header,
        &crate::saves::SceneCheckpoint::Field(saved.clone()),
    )
    .unwrap();
    let resident = Resident::default();
    let mut session = finish(
        Pending::start(
            root.clone(),
            Some(source_root.path().to_path_buf()),
            Some(bytes),
            None,
            &resident,
        )
        .unwrap(),
    )
    .unwrap();
    assert_shared_definitions(&session);
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
        source_root.path().join("entry.sym"),
        source.replace("2000", "2001"),
    )
    .unwrap();
    let refreshed = finish(
        FieldPending::field(
            root.clone(),
            Some(source_root.path().to_path_buf()),
            332,
            Some(previous.clone()),
            &resident,
        )
        .unwrap(),
    )
    .unwrap();
    session.fields.insert(332, Arc::new(refreshed));
    session.restore(saved).unwrap();
    assert_shared_definitions(&session);
    for _ in 0..3 {
        session.field.step(Default::default()).unwrap();
    }
    assert!(session.field.events.world.event_flags.contains(&2001));
    assert!(!session.field.events.world.event_flags.contains(&2000));

    fs::write(
        source_root.path().join("entry.sym"),
        "script field; use game::field; pub task run() { await field::notice(\"☃\"); }",
    )
    .unwrap();
    let rejected = finish(
        FieldPending::field(
            root,
            Some(source_root.path().to_path_buf()),
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
    assert_shared_definitions(&session);
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
                pressed_buttons: Buttons::default().with(Button::Accept, tick % 120 == 0),
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
                pressed_buttons: Buttons::default().with(Button::Accept, tick % 120 == 0),
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
            pressed_buttons: [Button::Menu].into(),
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
            pressed_buttons: [Button::Cancel].into(),
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
    assert_eq!(resumed.played_ticks, checkpoint.played_ticks + menu_updates);
    let header = resonance_persistence::Header {
        identity: session.identity.clone(),
        label: "Classroom exploration".into(),
        location: "Iselia school".into(),
        played_ticks: checkpoint.played_ticks,
        saved_unix_seconds: 0,
    };
    let bytes = resonance_persistence::encode(
        &header,
        &crate::saves::SceneCheckpoint::Field(checkpoint.clone()),
    )
    .unwrap();
    if let Some(output) = std::env::var_os("RESONANCE_CHECKPOINT_OUTPUT") {
        std::fs::write(output, &bytes).unwrap();
    }
    let assert_restored = |loaded: &Session| {
        assert_shared_definitions(loaded);
        assert_checkpoint(&loaded.field.checkpoint().unwrap(), &checkpoint).unwrap();
        assert_eq!(loaded.field.play_time.session(), 0);
        assert!(!loaded.movie_owns_audio());
        assert!(loaded.prepared_movie.is_none());
    };
    // A cold load constructs its field package independently; a warm restore
    // reuses the live session's prepared field. Both publish the same save state.
    let mut cold_cache = super::super::loading::Cache::default();
    let package =
        FieldPackage::prepare(&root, checkpoint.map_id, &mut cold_cache, || false).unwrap();
    let (_, saved) = resonance_persistence::decode(&bytes)
        .unwrap()
        .admit(&session.identity)
        .unwrap();
    let cold =
        Session::load_prepared(&root, package.files, Some(saved), None, &mut cold_cache).unwrap();
    assert_restored(&cold);
    let (_, saved) = resonance_persistence::decode(&bytes)
        .unwrap()
        .admit(&session.identity)
        .unwrap();
    session.restore(saved).unwrap();
    assert_restored(&session);
    let loaded_fields = cold.fields.keys().copied().collect::<Vec<_>>();
    assert!(session.fields.len() > loaded_fields.len());
    session.replace_loaded(cold);
    assert_eq!(
        session.fields.keys().copied().collect::<Vec<_>>(),
        loaded_fields
    );
    assert_restored(&session);
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
        assert_eq!(restored.played_ticks, checkpoint.played_ticks);
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
    assert_checkpoint(&session.field.checkpoint().unwrap(), &checkpoint).unwrap();
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
                assert_shared_definitions(&session);
            }
            session
                .field
                .step(FieldInput {
                    pressed_buttons: Buttons::default().with(Button::Accept, tick % 120 == 0),
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
                    pressed_buttons: [Button::Accept].into(),
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
            let mut cold = Session::load_prepared(
                &root,
                session.fields[&map].files.clone(),
                Some(at_circle.clone()),
                None,
                &mut cache,
            )
            .unwrap();
            for field in [&mut session.field, &mut cold.field] {
                let published = field.checkpoint().unwrap();
                assert_checkpoint(&published, &at_circle).unwrap();
                assert!(
                    field
                        .events
                        .world
                        .save_points
                        .iter()
                        .any(|point| point.active)
                );
                for _ in 0..8 {
                    field.step(Default::default()).unwrap();
                }
                let continued = field.checkpoint().unwrap();
                assert!(continued.progress.tick > published.progress.tick);
                assert!(continued.played_ticks > published.played_ticks);
                assert_ne!(
                    continued.progress.random_state,
                    published.progress.random_state
                );
            }
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
    use super::super::loading::Cache;
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
    fn settle(root: &Path, cache: &mut Cache, session: &mut Session, choice: u8) -> Observed {
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
                assert_shared_definitions(session);
                if neighborhood_entry {
                    // The first view already resolves the authored entrance
                    // orbit (336, 0, 346), distance 1661, and its X bounds.
                    assert_camera(
                        &session.field,
                        [-511., 1550.6742, 762.5896, -146., 3023., 87.],
                    );
                }
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
                let selected = pending.selection.lines().unwrap().selected_line
                    - pending.selection.lines().unwrap().first_line;
                assert!(
                    choice
                        <= pending.selection.lines().unwrap().last_line
                            - pending.selection.lines().unwrap().first_line
                );
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
                        pressed_buttons: Buttons::default()
                            .with(
                                Button::Accept,
                                !in_menu && direction == [0., 0.] && (choosing || ready),
                            )
                            .with(Button::Cancel, in_menu),
                        ..Default::default()
                    }
                } else {
                    FieldInput::default()
                })
                .unwrap();
            field.events.world.audio_commands.clear();
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
    let (data, menus) = admit_definitions(
        |path| Ok(package.files.read(path)?.to_vec()),
        package.files.diagnostics(),
    )
    .unwrap();
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
            menu_data: Some(menus),
            text: Arc::new(package.files.json("game/text.json").unwrap()),
            position: [-52., -619., 0.],
            available_fields: available_fields(&root).unwrap(),
            ..Default::default()
        })
        .unwrap();
    for _ in 0..1000 {
        field.step(Default::default()).unwrap();
        field.events.world.audio_commands.clear();
        if field.checkpoint().is_ok() {
            break;
        }
    }
    let mut session = Session::load_prepared(
        &root,
        package.files,
        Some(field.checkpoint().unwrap()),
        None,
        &mut cache,
    )
    .unwrap();
    let mut visited = BTreeSet::from([340]);
    macro_rules! hop {
        ($key:expr, $confirmed:expr, $map:expr, $choice:expr) => {{
            assert!(session.field.events.trigger($key, $confirmed).unwrap());
            let observed = settle(&root, &mut cache, &mut session, $choice);
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
    assert_eq!(settle(&root, &mut cache, &mut session, 0).shops, [1].into());
    assert!(session.field.player_has_control());
    hop!(1000, true, 330);
    hop!(2003, true, 338);
    assert!(session.field.events.interact(202).unwrap());
    assert!(
        settle(&root, &mut cache, &mut session, 0)
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
fn failed_entry_leaves_title_usable_in_diagnostic_mode_and_exits_in_paranoid_mode() {
    for paranoid in [false, true] {
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
        let mut resident = super::super::loading::Resident::default();
        resident.diagnostics = diagnostics.clone();
        let mut app = App::new();
        app.add_message::<AppExit>();
        app.insert_resource(resident);
        app.insert_resource(Request(None));
        app.insert_resource(super::super::Menu(resonance_game::TitleState {
            selected: 1,
            revealed: true,
            opacity: 255,
            ..Default::default()
        }));
        entry_failed(
            app.world_mut(),
            "test field entry",
            anyhow::anyhow!("missing mandatory script"),
        );
        assert!(!app.world().contains_resource::<Request>());
        assert!(
            !app.world()
                .contains_resource::<super::super::loading::Pending>()
        );
        assert_eq!(app.world().resource::<super::super::Menu>().0.selected, 1);
        assert_eq!(
            app.world().resource::<Messages<AppExit>>().len(),
            usize::from(paranoid)
        );
        assert_eq!(diagnostics.entries().len(), 1);
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
                            pressed_buttons: [Button::Accept].into(),
                            held_buttons: Buttons::default().with(Button::Accept, true),
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
                            pressed_buttons: [Button::Accept].into(),
                            held_buttons: Buttons::default().with(Button::Accept, true),
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
                    pressed_buttons: [Button::Start].into(),
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
#[ignore = "requires RESONANCE_WORLD_ASSETS; no devices"]
fn salvation_doorways_return_to_the_requested_field() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let mut field = crate::field_test::Scene::story(&root, 81, 14_000_000, |entry| {
        entry.allow_incomplete_scripts = true;
        entry.position = [557., 83., 124.];
        Ok(())
    })?;
    for (trigger, destination) in [(1002, 95), (1000, 81)] {
        field.advance_until(|f| f.player_has_control())?;
        ensure!(
            field.events.exploration_error.is_none(),
            "arrival script failed"
        );
        ensure!(
            field.events.trigger(trigger, true)?,
            "doorway trigger missing"
        );
        field.until(Default::default(), |f| {
            ensure!(f.events.exploration_error.is_none(), "door script failed");
            Ok(f.events.world.field_transition.is_some())
        })?;
        assert_eq!(
            field.events.world.field_transition.as_ref().unwrap().map,
            destination
        );
        field.follow_transition(&root)?;
    }
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

#[test]
#[ignore = "requires cooked setup script and menus; no window or audio device"]
fn clear_save_runs_grade_shop_and_starts_a_fresh_story() -> Result<()> {
    use resonance_content::grade::Benefit;
    use resonance_game::menu::Page;
    let mut session = Session::load(&asset_root())?;
    let mut progress = session.field.events.save_progress()?;
    progress.script_globals[0x40 / 4] = 1;
    progress.party.new_game_plus.cleared = true;
    progress.party.game_clears = 1;
    progress.party.grade_hundredths = 100_000;
    progress.party.gald = 9876;
    session.restore(FieldCheckpoint {
        allow_incomplete_scripts: false,
        map_id: 5,
        position: [0.; 3],
        heading: 0.,
        camera: None,
        progress,
        played_ticks: 9000,
    })?;
    let accept = FieldInput {
        pressed_buttons: [Button::Accept].into(),
        skip_dialogue: true,
        ..Default::default()
    };
    for _ in 0..600 {
        if session.field.menu.is_some() {
            break;
        }
        session.field.step(accept)?;
    }
    let menu = session
        .field
        .menu
        .as_mut()
        .context("Grade Shop did not open")?;
    assert_eq!(menu.page, Page::GradeShop);
    let shop = &menu.resources.as_ref().unwrap().data.grade_shop;
    let gald_row = shop
        .options
        .iter()
        .position(|p| p.benefit == Benefit::Gald)
        .unwrap();
    let cost = shop.options[gald_row].price * 100;
    menu.grade_shop.row = gald_row;
    session.field.step(accept)?;
    assert!(
        session
            .field
            .menu
            .as_ref()
            .unwrap()
            .grade_shop
            .selected
            .contains(&Benefit::Gald)
    );
    session.field.step(FieldInput {
        pressed_buttons: [Button::Start].into(),
        ..Default::default()
    })?;
    session.field.step(accept)?;
    assert_eq!(
        session.field.menu.as_ref().unwrap().grade_shop.confirmation,
        Some(false)
    );
    session.field.step(FieldInput {
        direction: [-1., 0.],
        ..Default::default()
    })?;
    session.field.step(accept)?;
    assert!(session.field.menu.is_none());
    for _ in 0..600 {
        if session.field.events.world.field_transition.is_some() {
            break;
        }
        session.field.step(accept)?;
    }
    let world = &session.field.events.world;
    assert_eq!(
        world
            .field_transition
            .as_ref()
            .context("new story did not start")?
            .map,
        340
    );
    let party = world.party.as_ref().unwrap();
    // The opening script grants another 500 Gald after applying carryover.
    assert_eq!(
        (party.gald, party.grade_hundredths),
        (9876 + 500, 100_000 - cost)
    );
    assert_eq!(party.new_game_plus.benefits, [Benefit::Gald].into());
    assert!(!party.new_game_plus.cleared);
    assert!(session.field.play_time.total() < 9000);
    assert_eq!(
        session.field.events.save_progress()?.script_globals[0x40 / 4],
        0
    );
    Ok(())
}

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
    let identity = save_context(
        &root,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?
    .0;
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
            resonance_persistence::decode(&bytes)?.admit(&header.identity)?;
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
                pressed_buttons: [Button::Menu].into(),
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
            resonance_persistence::decode(&menu_bytes)?.admit(&header.identity)?;
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
                pressed_buttons: [Button::Cancel].into(),
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
