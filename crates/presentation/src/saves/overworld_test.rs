//! Temporary Rheaird playground; deliberately separate from normal new-game state.
use super::*;
use resonance_content::overworld::{MapDisplay, Mount, Position, TravelState, World};
use std::{fs, io::Write, path::Path, sync::Arc};

pub fn prepare_overworld_test_fixture(
    root: &Path,
    mods: &[std::path::PathBuf],
    script_root: Option<&Path>,
    output: &Path,
) -> Result<()> {
    prepare_test_fixture(root, mods, script_root, output, false)
}

/// Post-fusion sandbox using the united-world mod's physical landmark placements.
pub fn prepare_united_world_test_fixture(
    root: &Path,
    mods: &[std::path::PathBuf],
    script_root: Option<&Path>,
    output: &Path,
) -> Result<()> {
    prepare_test_fixture(root, mods, script_root, output, true)
}

fn prepare_test_fixture(
    root: &Path,
    mods: &[std::path::PathBuf],
    script_root: Option<&Path>,
    output: &Path,
    united: bool,
) -> Result<()> {
    ensure!(!output.exists(), "overworld test fixture already exists");
    let library = resonance_content::library::Library::open_paths(root, mods)?;
    let mut cache = crate::loading::Cache {
        scripts: script_root.map(|root| resonance_game::authored::FieldScripts::new(root.into())),
        ..Default::default()
    };
    let package = crate::overworld::Package::capture(
        &library,
        &mut cache,
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?
    .world;
    let data = package
        .resources
        .session_data
        .as_ref()
        .context("missing world party data")?;
    let mut party = resonance_events::party::Party::new(data, Default::default())?;
    party.gald = 100_000;
    // Early Sylvarant party, reunited after the Sylvarant Base rescue.
    // Kratos belongs in reserve; the first four members ride in the test view.
    party.formation = if united {
        vec![1, 2, 3, 4, 5, 6, 7, 8]
    } else {
        vec![1, 2, 3, 4, 9]
    };
    if united {
        for index in 0..party.members.len() {
            party
                .raise_level(data, index, 60, None, || 0)
                .map_err(anyhow::Error::msg)?;
        }
    }
    party.field_leader = 1;
    party.travel.saved_formation = party.formation.clone();
    party.travel.visited_locations.insert(2);
    let spawn = if united {
        ensure!(
            package.definition.landmarks.worlds[2]
                .iter()
                .any(|entry| entry.source_location == Some(257)),
            "--test-united-world requires the generated united-world mod"
        );
        party.travel.visited_locations.extend(
            package.definition.landmarks.worlds[2]
                .iter()
                .filter(|e| e.map_listed)
                .map(|e| e.id),
        );
        let iselia = package.definition.landmarks.worlds[2]
            .iter()
            .find(|e| e.source_location == Some(2))
            .context("united world has no Iselia")?;
        [iselia.position[0], iselia.position[1] - 900., 20.]
    } else {
        [9770., 23500., 0.]
    };
    party.items.insert(58, 1); // Flight shortcut, independent of story progress.
    let mut persistent = resonance_events::PersistentState {
        party: Some(party),
        event_flags: [22].into(),
        ..Default::default()
    };
    persistent.memory.write(
        0x40,
        symphonia_script::Width::S32,
        if united { 14_000_000 } else { 1_101_000 },
    )?;
    if united {
        persistent.event_flags.extend(922..=928);
    }
    let state = TravelState {
        world: if united {
            World::United
        } else {
            World::Sylvarant
        },
        position: Position::from_map(spawn)?,
        heading: std::f32::consts::PI,
        camera_yaw: std::f32::consts::PI,
        alternate_perspective: false,
        map_display: MapDisplay::Small,
        mount: Mount::Rheairds,
        altitude: 600.,
    };
    let session = resonance_game::overworld::Session::enter(
        package.assets(state.world, &persistent)?,
        state,
        persistent,
        Default::default(),
    )?;
    let checkpoint = SceneCheckpoint::World(session.checkpoint()?);
    let header = Header {
        version: resonance_persistence::VERSION,
        label: if united {
            "United world — post-fusion"
        } else {
            "Rheaird playground"
        }
        .into(),
        location: if united { "United World" } else { "Sylvarant" }.into(),
        played_ticks: 0,
        saved_unix_seconds: 0,
    };
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(&resonance_persistence::encode(&header, &checkpoint)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS and both discs; validates world save creation/admission with mod layers and an external project"]
    fn named_world_mod_and_project_match_save_creation_browsing_and_restore() -> Result<()> {
        use resonance_content::{library::Library, prepared::Files};
        let fixture = crate::test_support::named_world_fixture()?;
        let root = fixture.path();
        let work = tempfile::tempdir()?;
        let project = work.path().join("project");
        fs::create_dir(&project)?;
        fs::write(
            project.join("fields.json"),
            br#"{"330":{"module":"entry","task":"run"}}"#,
        )?;
        let source = "script field; use game::story; pub task run() { story::set_flag(41, true); }";
        fs::write(project.join("entry.sym"), source)?;
        let modification = work.path().join("mod");
        fs::create_dir_all(modification.join("fields/renamed-entrance"))?;
        fs::write(
            modification.join("fields/mappings.json"),
            br#"{"330":"renamed-entrance"}"#,
        )?;
        let mut base = Library::open(root, &[])?.snapshot();
        let field = base.field(330)?;
        fs::write(
            modification.join("fields/renamed-entrance/field.json"),
            &*base.read(&field, 1024 * 1024)?,
        )?;
        let mods = [modification];
        let library = Library::open_paths(root, &mods)?;
        let output = work.path().join("world.json");
        prepare_overworld_test_fixture(root, &mods, Some(&project), &output)?;
        let bytes = fs::read(output)?;
        resonance_persistence::decode::<SceneCheckpoint>(&bytes)?.into_parts();
        let resident = crate::loading::Resident::default();
        *resident.files.write().unwrap() = Some(Arc::new(Files::from_snapshot(
            library.snapshot(),
            || false,
            resident.diagnostics.clone(),
        )?));
        let load = |scripts| -> Result<new_game::Session> {
            let task = crate::loading::Pending::start(
                root.into(),
                scripts,
                Some(bytes.clone()),
                None,
                &resident,
            )?;
            crate::test_support::await_preparation(&task)
        };
        let restored = load(Some(project.clone()))?;
        assert!(restored.overworld().is_some());
        assert_eq!(
            restored.files().library().unwrap().snapshot().field(330)?,
            "fields/renamed-entrance/field.json"
        );
        fs::write(project.join("entry.sym"), source.replace("41", "42"))?;
        for scripts in [Some(project), None] {
            assert!(load(scripts)?.overworld().is_some());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; named test-save creation, world restore and original skit timing"]
    fn named_world_playground_uses_early_sylvarant_events_with_flight_and_gald() -> Result<()> {
        let work = crate::test_support::named_world_fixture()?;
        let root = work.path().to_path_buf();
        let output = tempfile::tempdir()?;
        let file = output.path().join("world.json");
        prepare_overworld_test_fixture(&root, &[], None, &file)?;
        let (_, checkpoint): (_, SceneCheckpoint) =
            resonance_persistence::decode(&fs::read(file)?)?.into_parts();
        let SceneCheckpoint::World(checkpoint) = checkpoint else {
            anyhow::bail!("playground did not produce a world checkpoint");
        };
        let party = &checkpoint.progress.party;
        assert_eq!(party.gald, 100_000);
        assert_eq!(party.formation, [1, 2, 3, 4, 9]);
        assert_eq!(party.items.get(&58), Some(&1));
        assert_eq!(checkpoint.state.mount, Mount::Rheairds);
        let package = crate::overworld::Package::prepare(
            &root,
            &mut Default::default(),
            || false,
            resonance_content::diagnostics::Diagnostics::new(true),
        )?
        .world;
        let persistent = checkpoint
            .progress
            .clone()
            .into_state(package.resources.session_data.as_ref().unwrap())?;
        let story = persistent.memory.read(0x40, symphonia_script::Width::S32)?;
        assert_eq!(story, 1_101_000);
        let catalog = package.resources.skits.as_ref().unwrap();
        let later_journey = catalog.skits.iter().find(|s| s.id == 829).unwrap();
        let [start, end] = later_journey.story.unwrap();
        assert!(!(start..=end).contains(&story));
        assert!((start..=end).contains(&14_000_000));
        let mut session = resonance_game::overworld::Session::restore(
            package.assets(World::Sylvarant, &persistent)?,
            checkpoint,
        )?;
        // No camp is scheduled between the base rescue and the early voyage.
        // In particular the later Linkite-tree camp must not remain in the world.
        for id in [53, 54, 55, 57, 58, 60] {
            let appearance = session.locations.appearance(id).unwrap();
            assert_eq!(
                appearance.marker,
                resonance_content::overworld::Marker::None
            );
            assert_eq!(
                appearance.interaction,
                resonance_content::overworld::Interaction::Disabled
            );
        }
        // Cross the ambient refresh, rather than only inspecting initial data.
        for _ in 0..1250 {
            session.step(Default::default())?;
            assert!(session.player_has_control());
            if let Some(prompt) = session.skit_prompt() {
                assert_ne!(prompt.id, 829);
                let definition = catalog.skits.iter().find(|s| s.id == prompt.id).unwrap();
                assert!(
                    definition
                        .story
                        .is_none_or(|[a, b]| (a..=b).contains(&story))
                );
            }
        }
        Ok(())
    }
}

/// Silent live-loader regression observer; the source save is disposable.
pub fn run_overworld_field_probe(
    root: &Path,
    landmark: u16,
    direction: u8,
    exit_trigger: Option<u32>,
    field_override: Option<u32>,
    output: &Path,
) -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let checkpoint = temporary.path().join("world.json");
    prepare_overworld_test_fixture(root, &[], None, &checkpoint)?;
    let mut app = super::probe::app(root, &checkpoint, output, crate::Resolution::default())?;
    app.world_mut()
        .resource_mut::<crate::RunOptions>()
        .skip_battles = true;
    app.world_mut()
        .resource_mut::<crate::RunOptions>()
        .allow_incomplete_scripts = true;
    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.insert_resource(FieldProbe {
        landmark,
        direction,
        exit_trigger,
        field_override,
        exit_requested: false,
        saw_field: false,
        entered: false,
        settled: 0,
        updates: 0,
        started: Instant::now(),
        output: output.into(),
        completed: completed.clone(),
    })
    .add_systems(Update, drive_field_probe);
    ensure!(
        app.run() == AppExit::Success && completed.load(std::sync::atomic::Ordering::Acquire),
        "world field probe did not finish"
    );
    Ok(())
}
#[derive(Resource)]
struct FieldProbe {
    landmark: u16,
    direction: u8,
    exit_trigger: Option<u32>,
    field_override: Option<u32>,
    exit_requested: bool,
    saw_field: bool,
    entered: bool,
    settled: u32,
    updates: u32,
    started: Instant,
    output: PathBuf,
    completed: Arc<std::sync::atomic::AtomicBool>,
}
fn drive_field_probe(world: &mut bevy::prelude::World) {
    let mut probe = world.remove_resource::<FieldProbe>().unwrap();
    probe.updates += 1;
    let result = (|| -> Result<()> {
        ensure!(
            probe.started.elapsed().as_secs() < 240,
            "world field probe timed out"
        );
        ensure!(
            !world.contains_resource::<new_game::TransitionFailure>(),
            "field preparation failed"
        );
        if !probe.entered {
            if !crate::overworld::ready(world) {
                return Ok(());
            }
            let mut session = world.resource_mut::<new_game::Session>();
            let scene = session.overworld_mut().context("world missing")?;
            ensure!(
                scene
                    .session
                    .events
                    .enter_landmark(probe.landmark, probe.direction)?,
                "landmark unavailable"
            );
            if let Some(map) = probe.field_override {
                for _ in 0..600 {
                    if scene.session.events.world.field_transition.is_some() {
                        break;
                    }
                    scene.session.step(Default::default())?;
                }
                let transition = scene
                    .session
                    .events
                    .world
                    .field_transition
                    .as_mut()
                    .context("landmark did not request a field")?;
                transition.map = map;
                transition.position = [0.; 3];
                transition.heading = 0.;
                transition.camera = None;
            }
            probe.entered = true;
            return Ok(());
        }
        let owner = world.resource::<new_game::Session>();
        let in_world = owner.overworld().is_some();
        probe.saw_field |= !in_world;
        let dialogue = !in_world && owner.field().dialogue.values().any(|d| !d.closed);
        let controlled = !in_world && owner.field().player_has_control();
        let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
        if dialogue && probe.updates.is_multiple_of(4) {
            keys.press(KeyCode::Enter);
        } else {
            keys.release(KeyCode::Enter);
        }
        if !probe.saw_field {
            return Ok(());
        }
        if let Some(trigger) = probe.exit_trigger {
            if !in_world {
                if controlled && crate::field_view::ready(world) && !probe.exit_requested {
                    probe.settled += 1;
                    if probe.settled == 30 {
                        ensure!(
                            world
                                .resource_mut::<new_game::Session>()
                                .field_mut()
                                .events
                                .trigger(trigger, true)?,
                            "exit trigger missing"
                        );
                        probe.exit_requested = true;
                        probe.settled = 0;
                    }
                }
                return Ok(());
            }
            ensure!(probe.exit_requested, "field returned before requested exit");
        }
        if in_world && !crate::overworld::ready(world) {
            return Ok(());
        }
        let state = match super::scene_checkpoint(world) {
            Ok(state) => serde_json::to_value(state)?,
            Err(error) => {
                // A valid authored camera can forbid quicksaving while still
                // allowing exploration. Capture it too, so camera regressions
                // cannot hide behind checkpoint eligibility.
                if crate::field_view::ready(world)
                    && world
                        .resource::<new_game::Session>()
                        .field()
                        .player_has_control()
                {
                    let owner = world.resource::<new_game::Session>();
                    serde_json::json!({
                        "map_id": owner.map_id(),
                        "camera": format!("{:?}", owner.field().events.world.field_camera),
                        "player_position": owner.field().events.world.actors.get(&owner.field().events.world.controlled_actor).map(|a| a.position),
                        "checkpoint_error": format!("{error:#}"),
                    })
                } else {
                    if probe.updates.is_multiple_of(300) {
                        info!("Waiting for field control: {error:#}");
                    }
                    let dialogue = world.get_resource::<new_game::Session>().is_some_and(|s| {
                        s.is_field() && s.field().dialogue.values().any(|d| !d.closed)
                    });
                    let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                    if dialogue && probe.updates.is_multiple_of(4) {
                        keys.press(KeyCode::Enter);
                    } else {
                        keys.release(KeyCode::Enter);
                    }
                    return Ok(());
                }
            }
        };
        probe.settled += 1;
        if probe.settled != 30 {
            return Ok(());
        }
        fs::write(
            probe.output.join("field.json"),
            serde_json::to_vec_pretty(&state)?,
        )?;
        let target = world.resource::<crate::Framebuffer>().0.clone();
        let path = probe.output.join("field.png");
        let completed = probe.completed.clone();
        world
            .spawn(bevy::render::view::screenshot::Screenshot(target))
            .observe(
                move |capture: On<bevy::render::view::screenshot::ScreenshotCaptured>,
                      mut exit: MessageWriter<AppExit>| {
                    capture
                        .image
                        .clone()
                        .try_into_dynamic()
                        .unwrap()
                        .save(&path)
                        .unwrap();
                    completed.store(true, std::sync::atomic::Ordering::Release);
                    exit.write(AppExit::Success);
                },
            );
        Ok(())
    })();
    world.insert_resource(probe);
    if let Err(error) = result {
        error!("World field probe failed: {error:#}");
        world.write_message(AppExit::error());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and mods/united-world/build.py output; audits every united-world entrance"]
fn united_world_entrance_audit() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("RESONANCE_TEST_ASSETS")?,
    );
    let modification = std::env::var_os("RESONANCE_UNITED_MOD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../mods/united-world/generated")
        });
    let mods = [modification];
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("world/landmarks.json"))?)?;
    let modified: serde_json::Value =
        serde_json::from_slice(&fs::read(mods[0].join("world/landmarks.json"))?)?;
    assert_eq!(original["sylvarant"], modified["sylvarant"]);
    assert_eq!(original["tethe_alla"], modified["tethe_alla"]);
    let mapping: serde_json::Value =
        serde_json::from_slice(&fs::read(mods[0].join("world/mappings.json"))?)?;
    assert!(
        mapping.get("0").is_none() && mapping.get("1").is_none(),
        "original overworld slots must not be overridden"
    );
    let work = tempfile::tempdir()?;
    let checkpoint = work.path().join("united.json");
    prepare_united_world_test_fixture(&root, &mods, None, &checkpoint)?;
    let (_, saved): (_, SceneCheckpoint) =
        resonance_persistence::decode(&fs::read(checkpoint)?)?.into_parts();
    let SceneCheckpoint::World(saved) = saved else {
        anyhow::bail!("world save missing")
    };
    let library = resonance_content::library::Library::open_paths(&root, &mods)?;
    let package = crate::overworld::Package::capture(
        &library,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?
    .world;
    let persistent = saved
        .progress
        .clone()
        .into_state(package.resources.session_data.as_ref().unwrap())?;
    // Enabling the mod must also preserve both original world sessions.
    for world in [World::Sylvarant, World::TetheAlla] {
        let mut progress = saved
            .progress
            .clone()
            .into_state(package.resources.session_data.as_ref().unwrap())?;
        progress
            .memory
            .write(0x50, symphonia_script::Width::S32, world.index() as i32)?;
        let mut state = saved.state.clone();
        state.world = world;
        let landmark = &package.definition.landmarks.worlds[world.index()][0];
        state.position = Position::from_map([landmark.position[0], landmark.position[1], 0.])?;
        let mut session = resonance_game::overworld::Session::enter(
            package.assets(world, &progress)?,
            state,
            progress,
            Default::default(),
        )?;
        for _ in 0..30 {
            session.step(Default::default())?;
        }
        assert_eq!(session.travel.state().world, world);
        assert_eq!(session.checkpoint()?.state.world, world);
    }
    let composition: serde_json::Value =
        serde_json::from_slice(&fs::read(mods[0].join("world/united/composition.json"))?)?;
    let entrances: std::collections::BTreeSet<u16> = composition["entrances"]
        .as_array()
        .context("authored entrances")?
        .iter()
        .map(|id| id.as_u64().unwrap() as u16)
        .collect();
    let assets = package.assets(World::United, &persistent)?;
    for landmark in package.definition.landmarks.worlds[2]
        .iter()
        .filter(|entry| {
            entrances.contains(&entry.id)
                && entry.interaction != resonance_content::overworld::Interaction::Disabled
        })
    {
        // Retail gates may be inside a wall, forest or floating settlement.
        // Validate a real contact approach, rather than flattening the gate's
        // center until the town loses its authored ground.
        let mut reachable = false;
        for angle in (0..32).map(|i| i as f32 * std::f32::consts::TAU / 32.) {
            let radius = landmark.radius + 35.;
            let position = Position::from_map([
                landmark.position[0] + radius * angle.cos(),
                landmark.position[1] + radius * angle.sin(),
                0.,
            ])?;
            reachable |= assets
                .terrain
                .query(position, assets.movement.collision.radius)?
                .surface(resonance_game::overworld::collision::Mode::Ground)
                .is_some_and(|surface| surface.kind.walkable());
        }
        let airborne = landmark
            .entry_overrides
            .iter()
            .any(|entry| entry.mount.is_some_and(|mount| mount.airborne()))
            || matches!(landmark.source_location, Some(11 | 25 | 265));
        ensure!(
            reachable || airborne,
            "{} has no walkable or airborne entrance approach",
            landmark.name
        );
    }
    let mut failures = Vec::new();
    let mut report = Vec::new();
    let interiors = std::env::var_os("RESONANCE_UNITED_INTERIORS").is_some();
    let mut field_cache = crate::loading::Cache::default();
    let selected: Option<std::collections::BTreeSet<u16>> =
        std::env::var("RESONANCE_UNITED_LOCATIONS")
            .ok()
            .map(|text| text.split(',').map(|id| id.parse().unwrap()).collect());
    for landmark in package.definition.landmarks.worlds[2].iter().filter(|e| {
        entrances.contains(&e.id) && selected.as_ref().is_none_or(|ids| ids.contains(&e.id))
    }) {
        let result = (|| -> Result<()> {
            let mut scene = resonance_game::overworld::Session::enter(
                assets.clone(),
                saved.state.clone(),
                saved
                    .progress
                    .clone()
                    .into_state(package.resources.session_data.as_ref().unwrap())?,
                Default::default(),
            )?;
            ensure!(
                scene.events.enter_landmark(landmark.id, 2)?,
                "no entrance event"
            );
            for _ in 0..600 {
                scene.step(Default::default())?;
                if scene.events.world.field_transition.is_some() {
                    break;
                }
            }
            let request = scene
                .events
                .world
                .field_transition
                .as_ref()
                .context("entrance did not request an interior")?;
            let map = request.map;
            report.push(serde_json::json!({"id":landmark.id,"name":landmark.name,"map":map,"position":request.position,"heading":request.heading}));
            if interiors {
                use resonance_events::input::{Button, Buttons};
                use resonance_game::field::FieldInput;
                let field_package = crate::new_game::FieldPackage::capture(
                    &library,
                    map,
                    &mut field_cache,
                    || false,
                    resonance_content::diagnostics::Diagnostics::new(true),
                )?;
                let mut entry = scene.field_entry()?;
                entry.allow_incomplete_scripts = true;
                let mut field = field_package.enter(entry)?;
                for _ in 0..2400 {
                    field.step(FieldInput {
                        pressed_buttons: [Button::Accept].into(),
                        held_buttons: Buttons::default().with(Button::Accept, true),
                        ..Default::default()
                    })?;
                    field.events.world.audio_commands.clear();
                    if field.player_has_control()
                        || field.events.world.field_transition.is_some()
                        || field.events.world.world_transition.is_some()
                    {
                        break;
                    }
                }
                ensure!(
                    field.player_has_control()
                        || field.events.world.field_transition.is_some()
                        || field.events.world.world_transition.is_some(),
                    "interior remained suspended"
                );
                println!(
                    "{} {}: field {map}, control={}, field handoff={}",
                    landmark.id,
                    landmark.name,
                    field.player_has_control(),
                    field.events.world.field_transition.is_some()
                );
                if [1026, 1035, 1049, 1289, 1400].contains(&landmark.id) {
                    for _ in 0..30 {
                        field.step(Default::default())?;
                    }
                    ensure!(
                        field.player_has_control(),
                        "authored entrance did not retain walking control"
                    );
                    let position =
                        field.events.world.actors[&field.events.world.controlled_actor].position;
                    ensure!(
                        field
                            .collision()
                            .surface_below([position[0], position[1], position[2] + 10.])
                            .is_some(),
                        "authored entrance has no supporting floor at {position:?}"
                    );
                }
                if map == 2900 {
                    ensure!(
                        field.player_has_control(),
                        "Confluence Hollow did not release control"
                    );
                    let checkpoint = SceneCheckpoint::Field(field.checkpoint()?);
                    let header = Header {
                        version: resonance_persistence::VERSION,
                        label: "Confluence Hollow".into(),
                        location: "Confluence Hollow".into(),
                        played_ticks: 0,
                        saved_unix_seconds: 0,
                    };
                    fs::write(
                        root.parent().unwrap().join("united-dungeon.json"),
                        resonance_persistence::encode(&header, &checkpoint)?,
                    )?;
                    ensure!(
                        field
                            .events
                            .world
                            .actors
                            .get(&201)
                            .is_some_and(|actor| actor.enemy.is_some()),
                        "first enemy missing"
                    );
                    ensure!(
                        field
                            .events
                            .world
                            .actors
                            .get(&202)
                            .is_some_and(|actor| actor.enemy.is_some()),
                        "second enemy missing"
                    );
                    if landmark.id == 1400 && std::env::var_os("RESONANCE_UNITED_COMBAT").is_some()
                    {
                        let mut owner = crate::new_game::Session::load_prepared(
                            Arc::new(field_package.clone()),
                            Some(field.checkpoint()?),
                            None,
                        )?;
                        ensure!(
                            owner.field_mut().events.contact_enemy(201)?,
                            "restored enemy cannot start combat"
                        );
                        for _ in 0..60 {
                            owner.field_mut().step(Default::default())?;
                            if owner.events().world.battle_request.is_some() {
                                break;
                            }
                        }
                        let request = owner
                            .events()
                            .world
                            .battle_request
                            .as_ref()
                            .context("restored dungeon did not request combat")?;
                        let resident = crate::loading::Resident::default();
                        *resident.files.write().unwrap() = Some(owner.files());
                        let worker = crate::loading::BattlePending::battle(
                            crate::battle::Entry::capture(&owner, request)?,
                            &resident,
                        )?;
                        let battle = crate::test_support::await_preparation(&worker)?;
                        ensure!(
                            battle
                                .prepared
                                .core
                                .snapshot()
                                .actors
                                .iter()
                                .any(|actor| actor.side == resonance_battle::Side::Enemy),
                            "prepared battle has no enemies"
                        );
                        println!(
                            "Confluence Hollow: production battle worker prepared enemies, arena, party, effects and audio"
                        );
                    }
                    ensure!(
                        field.events.contact_enemy(201)?,
                        "enemy contact did not start its event"
                    );
                    for _ in 0..60 {
                        field.step(Default::default())?;
                        if field.events.world.battle_request.is_some() {
                            break;
                        }
                    }
                    ensure!(
                        field.events.world.battle_request.is_some(),
                        "enemy contact did not request combat"
                    );
                    field
                        .events
                        .world
                        .skip_battle_as_victory()
                        .map_err(anyhow::Error::msg)?;
                    for _ in 0..180 {
                        field.step(Default::default())?;
                        if field.player_has_control() {
                            break;
                        }
                    }
                    ensure!(
                        field.player_has_control(),
                        "battle did not restore field control"
                    );
                    ensure!(
                        field.events.trigger(1000, true)?,
                        "dungeon exit trigger missing"
                    );
                    for _ in 0..180 {
                        field.step(Default::default())?;
                        if field.events.world.world_transition.is_some() {
                            break;
                        }
                    }
                    let request = field
                        .events
                        .world
                        .world_transition
                        .as_ref()
                        .context("dungeon exit did not return to world")?;
                    let persistent = field.events.persistent_state()?;
                    let resumed = resonance_game::overworld::Session::return_from_field(
                        package.assets(World::United, &persistent)?,
                        request,
                        persistent,
                        field.play_time,
                    )?;
                    ensure!(
                        resumed.travel.state().world == World::United,
                        "dungeon exit switched to a split world"
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{} {}: {error:#}", landmark.id, landmark.name));
        }
    }
    fs::write(
        root.parent().unwrap().join("united-entrances.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    for failure in &failures {
        println!("{failure}");
    }
    println!(
        "{} interiors resolved; {} failed",
        report.len(),
        failures.len()
    );
    ensure!(
        failures.is_empty(),
        "united-world entrances remain incomplete"
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and the authored united-world composition"]
fn united_world_preserves_native_foundations() -> Result<()> {
    use resonance_game::overworld::{TileCoordinate, collision};
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("RESONANCE_TEST_ASSETS")?,
    );
    let modification = std::env::var_os("RESONANCE_UNITED_MOD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../mods/united-world/generated")
        });
    let composition: serde_json::Value = serde_json::from_slice(&fs::read(
        modification.join("world/united/composition.json"),
    )?)?;
    let library = resonance_content::library::Library::open_paths(&root, &[modification])?;
    let package = crate::overworld::Package::capture(
        &library,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?
    .world;
    let terrains = package
        .definition
        .worlds
        .iter()
        .map(|tiles| {
            collision::Terrain::new(
                tiles
                    .iter()
                    .map(|tile| {
                        Ok((
                            TileCoordinate::new(tile.column, tile.row)?,
                            collision::Mesh::new(&tile.base.ground)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
                package.definition.movement.collision,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let mut floors = 0;
    let mut roots = 0;
    let mut max_error = 0f32;
    let radius = package.definition.movement.collision.radius;
    let sample = |world: usize, point: [f32; 2]| -> Result<Option<f32>> {
        Ok(terrains[world]
            .query(Position::from_map([point[0], point[1], 0.])?, radius)?
            .surface(collision::Mode::Ground)
            .map(|s| s.height))
    };
    for district in composition["districts"].as_array().context("districts")? {
        let anchor = district["anchor"].as_u64().unwrap() as u16;
        let world = usize::from(anchor >= 256);
        let offset = [
            district["translation"][0].as_f64().unwrap() as f32,
            district["translation"][1].as_f64().unwrap() as f32,
        ];
        let polygon: Vec<[f32; 2]> = district["polygon"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| [p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32])
            .collect();
        let low = [
            polygon.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
            polygon.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
        ];
        let high = [
            polygon
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max),
            polygon
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max),
        ];
        let inside = |p: [f32; 2]| {
            polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .all(|(a, b)| {
                    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
                        > 100. * (b[0] - a[0]).hypot(b[1] - a[1])
                })
        };
        let mut y = low[1] + 173.;
        while y < high[1] {
            let mut x = low[0] + 227.;
            while x < high[0] {
                if inside([x, y]) {
                    let source = [x - offset[0], y - offset[1]];
                    if let Some(expected) = sample(world, source)? {
                        let actual = sample(2, [x, y])?.with_context(|| {
                            format!("{} lost supporting ground at {x},{y}", district["name"])
                        })?;
                        let error = (expected - actual).abs();
                        max_error = max_error.max(error);
                        ensure!(
                            error < 0.1,
                            "{} changed native foundation by {error} at {x},{y}: {expected} -> {actual}",
                            district["name"]
                        );
                        floors += 1;
                    }
                }
                x += 417.;
            }
            y += 389.;
        }
        for id in district["members"].as_array().unwrap() {
            let id = id.as_u64().unwrap() as u16;
            let native = package.definition.landmarks.worlds[world]
                .iter()
                .find(|e| e.id == id)
                .unwrap();
            let relocated = package.definition.landmarks.worlds[2]
                .iter()
                .find(|e| e.source_location == Some(id))
                .unwrap();
            if matches!(
                native.marker,
                resonance_content::overworld::Marker::Model(_)
            ) {
                ensure!(
                    matches!(
                        relocated.marker,
                        resonance_content::overworld::Marker::Model(_)
                    ),
                    "{} lost its native model",
                    native.name
                );
                let expected = native
                    .height
                    .unwrap_or(sample(world, native.position)?.unwrap_or(0.));
                let actual = relocated
                    .height
                    .unwrap_or(sample(2, relocated.position)?.unwrap_or(0.));
                ensure!(
                    (actual - expected).abs() < 0.1,
                    "{} model root moved off its native datum: {expected} -> {actual}",
                    native.name
                );
                roots += 1;
            }
        }
    }
    ensure!(
        package.definition.encounters.united_travel_points.len() == 8,
        "missing authored sailing landing"
    );
    for dock in &package.definition.encounters.united_travel_points {
        let point = |side: usize| {
            Position::from_map([
                dock[side].map_x as f32,
                dock[side].map_z as f32,
                dock[side].height,
            ])
        };
        let land = terrains[2]
            .query(point(0)?, radius)?
            .surface(collision::Mode::Ground)
            .context("landing lost its pier")?;
        ensure!(
            land.kind == resonance_content::overworld::TerrainSurface::Pier,
            "sailing landing is not on a pier"
        );
        ensure!(
            (land.height - dock[0].height).abs() < 0.1,
            "sailing pier elevation changed"
        );
        ensure!(
            terrains[2]
                .query(point(1)?, radius)?
                .surface(collision::Mode::Ship)
                .is_some(),
            "sailing landing has no navigable water"
        );
    }
    ensure!(
        floors > 1000 && roots >= 10,
        "insufficient foundation coverage"
    );
    println!(
        "{floors} native support probes and {roots} model datums preserved; maximum drift {max_error:.5}"
    );
    Ok(())
}
