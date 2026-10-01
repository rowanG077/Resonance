//! Temporary Rheaird playground; deliberately separate from normal new-game state.
use super::*;
use resonance_content::overworld::{MapDisplay, Mount, Position, TravelState, World};
use std::{fs, io::Write, path::Path, sync::Arc};

pub fn prepare_overworld_test_fixture(root: &Path, output: &Path) -> Result<()> {
    ensure!(!output.exists(), "overworld test fixture already exists");
    let package = Arc::new(resonance_game::overworld::Prepared::load(
        root,
        resonance_content::prepared::Files::load(root, &[], &mut Default::default(), || false)?,
        &mut Default::default(),
        new_game::available_fields(root)?,
        || false,
    )?);
    let data = package
        .resources
        .session_data
        .as_ref()
        .context("missing world party data")?;
    let mut party = resonance_events::party::Party::new(data, Default::default())?;
    party.gald = 100_000;
    // Early Sylvarant party, reunited after the Sylvarant Base rescue.
    // Kratos belongs in reserve; the first four members ride in the test view.
    party.formation = vec![1, 2, 3, 4, 9];
    party.field_leader = 1;
    party.travel.saved_formation = party.formation.clone();
    party.travel.visited_locations.insert(2);
    party.items.insert(58, 1); // Flight shortcut, independent of story progress.
    let mut persistent = resonance_events::PersistentState {
        party: Some(party),
        event_flags: [22].into(),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 1_101_000)?;
    let state = TravelState {
        world: World::Sylvarant,
        position: Position::from_map([9770., 23500., 0.])?,
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
        identity: new_game::save_context(
            root,
            resonance_content::diagnostics::Diagnostics::new(true),
        )?
        .0,
        label: "Rheaird playground".into(),
        location: "Sylvarant".into(),
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
    #[ignore = "requires RESONANCE_WORLD_ASSETS; original test fixture and skit catalogue"]
    fn original_playground_uses_early_sylvarant_events_with_flight_and_gald() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
        );
        let output = tempfile::tempdir()?;
        let file = output.path().join("world.json");
        prepare_overworld_test_fixture(&root, &file)?;
        let (_, checkpoint): (_, SceneCheckpoint) =
            resonance_persistence::decode(&fs::read(file)?)?.admit(
                &new_game::save_context(
                    &root,
                    resonance_content::diagnostics::Diagnostics::new(true),
                )?
                .0,
            )?;
        let SceneCheckpoint::World(checkpoint) = checkpoint else {
            anyhow::bail!("playground did not produce a world checkpoint");
        };
        let party = &checkpoint.progress.party;
        assert_eq!(party.gald, 100_000);
        assert_eq!(party.formation, [1, 2, 3, 4, 9]);
        assert_eq!(party.items.get(&58), Some(&1));
        assert_eq!(checkpoint.state.mount, Mount::Rheairds);
        let package = resonance_game::overworld::Prepared::load(
            &root,
            resonance_content::prepared::Files::load(&root, &[], &mut Default::default(), || {
                false
            })?,
            &mut Default::default(),
            new_game::available_fields(&root)?,
            || false,
        )?;
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
    prepare_overworld_test_fixture(root, &checkpoint)?;
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
