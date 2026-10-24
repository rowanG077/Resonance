//! Original-asset checks are opt-in; synthetic collision tests run without a disc.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    library::Library,
    overworld::{Package, document},
};
use resonance_game::overworld::{
    Position, Rules, TileCoordinate, World,
    collision::{Mesh, Mode, Terrain},
};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    std::env::var_os("RESONANCE_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"))
}

#[test]
#[ignore = "requires a current named library; set RESONANCE_ASSETS"]
fn original_world_script_initializes_and_routes_all_landmark_handlers() -> Result<()> {
    use resonance_events::{EventRuntime, GameWorld, party::Party};
    use std::sync::Arc;
    use symphonia_script::{Program, Width};
    let prepared = prepare_world(&root())?;
    let data = prepared
        .resources
        .session_data
        .as_ref()
        .context("missing session data")?
        .clone();
    let bytes = prepared.files.read(&prepared.definition.script.path)?;
    let program = Arc::new(Program::decode(&bytes)?);
    let resources = prepared.resources.clone();
    let mut party = Party::new(&data, Default::default())?;
    party.items.insert(55, 1);
    party.travel.sorcerers_ring = resonance_events::ring::SorcerersRing::Lightning(
        resonance_events::ring::LightningColor::Red,
    );
    party.travel.saved_formation = vec![1];
    let start = |story| -> Result<EventRuntime> {
        let mut memory = symphonia_script_vm::Memory::default();
        memory.write(0x40, Width::S32, story)?;
        let mut world = GameWorld::default();
        world.input_enabled = true;
        world.party = Some(party.clone());
        world.field_camera = Some(Default::default());
        world.event_flags = [150, 151, 1700, 1701, 1800, 1801, 331, 900].into();
        EventRuntime::with_state(program.clone(), resources.clone(), world, memory)
    };
    let mut first = start(900000)?;
    let party = first.world.party.as_ref().unwrap();
    assert_eq!(
        party.travel.sorcerers_ring,
        resonance_events::ring::SorcerersRing::Fire
    );
    assert!(party.travel.saved_formation.is_empty());
    assert_eq!(first.world.event_flags, [900].into());
    assert!(first.enter_landmark(1, 6)?);
    assert!(!first.enter_landmark(2, 0)?);
    first.step()?;
    let destination = first
        .world
        .field_transition
        .as_ref()
        .context("Dirk's House did not request a field")?;
    assert_eq!(destination.map, 372);
    assert_eq!(destination.position, [-540., -333., 0.]);
    assert_eq!(destination.heading, 126.);
    assert!(destination.camera.is_some());
    let records = symphonia_script::scenario::analyze(&bytes)?.records;
    assert_eq!(records.len(), 160);
    for story in [0, 900000, 903000, 10405000, 12204000, 20801001, 99999999] {
        for record in &records {
            for direction in [0, 2, 4, 6] {
                let mut runtime = start(story)?;
                ensure!(
                    runtime.enter_landmark(record.key as u16, direction)?,
                    "landmark {} unavailable",
                    record.key
                );
                for _ in 0..8 {
                    runtime.step().with_context(|| {
                        format!(
                            "landmark {} direction {direction} story {story}",
                            record.key
                        )
                    })?;
                    if runtime.world.field_transition.is_some()
                        || runtime.world.skit_request.is_some()
                        || runtime.player_has_control()
                    {
                        break;
                    }
                }
                ensure!(
                    runtime.world.field_transition.is_some()
                        || runtime.world.skit_request.is_some()
                        || runtime.player_has_control(),
                    "landmark {} did not reach a scene boundary",
                    record.key
                );
            }
        }
    }
    Ok(())
}
fn world_package(root: &Path) -> Result<Package> {
    Ok(document::capture(&mut Library::open(root, &[])?.snapshot(), &|| false)?.1)
}

#[test]
#[ignore = "requires cooked original assets; set RESONANCE_ASSETS or cook local/all-assets"]
fn original_worlds_prepare_every_base_and_alternate_tile() -> Result<()> {
    let root = root();
    let package = world_package(&root)?;
    let collision = &package.collision;
    collision.validate()?;
    let mut variants = 0;
    for world in &package.worlds {
        let mut tiles = Vec::new();
        for tile in world {
            for (variant, asset) in std::iter::once(&tile.base)
                .chain(&tile.alternate)
                .enumerate()
            {
                let mesh = Mesh::new(&asset.ground).with_context(|| {
                    format!("terrain ({}, {}), variant {variant}", tile.column, tile.row)
                })?;
                if variant == 0 {
                    tiles.push((TileCoordinate::new(tile.column, tile.row)?, mesh));
                }
                variants += 1;
            }
        }
        let terrain = Terrain::new(tiles, collision.clone())?;
        // Exercise all tile centers, seams and quadrant corners, including wrap.
        for tile in world {
            for offset in [[0., 0.], [3200., 3200.], [6399., 6399.]] {
                let position = Position::from_map([
                    f32::from(tile.column) * 6400. + offset[0],
                    f32::from(tile.row) * 6400. + offset[1],
                    0.,
                ])?;
                let query = terrain.query(position, 1000.)?;
                for mode in [
                    Mode::Ground,
                    Mode::Ship,
                    Mode::AlternateGround,
                    Mode::RestrictedGround,
                ] {
                    if let Some(surface) = query.surface(mode) {
                        ensure!(
                            surface.height.is_finite()
                                && surface.normal.iter().all(|v| v.is_finite()),
                            "invalid prepared surface"
                        );
                    }
                    let motion = query.motion(20., 0., mode)?;
                    ensure!(
                        motion
                            .delta
                            .iter()
                            .chain(&motion.slope)
                            .all(|v| v.is_finite()),
                        "invalid world movement"
                    );
                }
            }
        }
    }
    assert_eq!(variants, 228);
    Ok(())
}

#[test]
#[ignore = "requires cooked original assets; set RESONANCE_ASSETS or cook local/all-assets"]
fn original_encounters_select_iselia_groups_and_the_later_tethealla_table() -> Result<()> {
    let root = root();
    let tables = world_package(&root)?.encounters;
    let region_zero = tables.area_grids[1]
        .iter()
        .enumerate()
        .find_map(|(row, cells)| {
            cells
                .iter()
                .position(|area| *area == 0)
                .map(|column| [column as f32 * 3200., row as f32 * 3200., 0.])
        })
        .context("missing Tethe'alla region zero")?;
    let encounters = Rules::new(tables)?;
    let iselia = Position::from_map([9600., 24600., 0.])?;
    let first = encounters
        .lookup(World::Sylvarant, iselia, 1, 0, 0)
        .unwrap();
    let second = encounters
        .lookup(World::Sylvarant, iselia, 1, 1, 0)
        .unwrap();
    assert_eq!(
        (first.group, second.group, first.arena, first.area),
        (501, 502, 1, 0)
    );
    assert!(
        encounters
            .lookup(World::Sylvarant, iselia, 11, 0, 0)
            .is_none()
    );
    assert!(
        encounters
            .lookup(World::Sylvarant, iselia, -1, 0, 0)
            .is_none()
    );
    assert!(
        encounters
            .lookup(World::Sylvarant, iselia, 1, 2, 0)
            .is_none()
    );
    // The authored fallback group 500 is a group, not the no-encounter sentinel.
    assert_eq!(
        encounters
            .lookup(World::Sylvarant, iselia, 3, 0, 0)
            .unwrap()
            .group,
        500
    );
    let position = Position::from_map(region_zero)?;
    let before = encounters
        .lookup(World::TetheAlla, position, 1, 0, 10_404_999)
        .unwrap();
    let after = encounters
        .lookup(World::TetheAlla, position, 1, 0, 10_405_000)
        .unwrap();
    assert_eq!(
        (before.group, after.group, before.arena, after.arena),
        (631, 646, 7, 7)
    );
    Ok(())
}

#[test]
#[ignore = "requires a prepared world package; set RESONANCE_WORLD_ASSETS"]
fn original_world_package_enters_towns_and_restores_travel_without_filesystem_reads() -> Result<()>
{
    use resonance_events::{PersistentState, party::Party};
    use resonance_game::overworld::{
        Input, Prompt, Session,
        travel::{Mount, State},
    };
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let prepared = prepare_world(&root)?;
    for world in [World::Sylvarant, World::TetheAlla] {
        for story in [900000, 10405000, 20201000, 22601000] {
            let data = prepared.resources.session_data.as_ref().unwrap();
            let mut persistent = PersistentState {
                party: Some(Party::new(data, Default::default())?),
                ..Default::default()
            };
            persistent
                .memory
                .write(0x40, symphonia_script::Width::S32, story)?;
            let selected = prepared.terrain(world, &persistent)?;
            assert_eq!(selected.len(), 108);
            let assets = prepared.assets(world, &persistent)?;
            let landmark = &prepared.definition.landmarks.worlds[world.index()][0];
            let position = Position::from_map([
                landmark.position[0] + landmark.radius + 40.,
                landmark.position[1],
                0.,
            ])?;
            let state = State {
                world,
                position,
                heading: 0.,
                camera_yaw: 0.,
                alternate_perspective: false,
                map_display: Default::default(),
                mount: Mount::Foot,
                altitude: 0.,
            };
            let mut session =
                Session::enter(assets.clone(), state, persistent, Default::default())?;
            let checkpoint = session.checkpoint()?;
            let bytes = serde_json::to_vec(&checkpoint)?;
            let restored = Session::restore(assets, serde_json::from_slice(&bytes)?)?;
            assert_eq!(restored.travel.state(), session.travel.state());
            session.step(Input::default())?;
            assert!(
                matches!(session.prompt(),Some(Prompt::Enter {location,..}) if *location == landmark.id)
            );
            session.step(Input {
                confirm: true,
                ..Default::default()
            })?;
            let destination = session
                .events
                .world
                .field_transition
                .as_ref()
                .context("town did not request a field")?;
            assert!(destination.operation.is_pending());
            let entry = session.field_entry()?;
            assert_eq!(
                entry
                    .persistent
                    .party
                    .as_ref()
                    .unwrap()
                    .travel
                    .overworld
                    .as_ref()
                    .unwrap(),
                session.travel.state()
            );
            assert!(destination.operation.is_pending());
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a prepared world package; set RESONANCE_WORLD_ASSETS"]
fn original_world_enemy_symbols_spawn_and_publish_native_encounters() -> Result<()> {
    use resonance_events::{
        PersistentState,
        battle::Outcome,
        party::{EncounterModifier, Party},
    };
    use resonance_game::overworld::{
        Session,
        travel::{Mount, State},
    };
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let prepared = prepare_world(&root)?;
    for world in [World::Sylvarant, World::TetheAlla] {
        let mut persistent = PersistentState {
            party: Some(Party::new(
                prepared.resources.session_data.as_ref().unwrap(),
                Default::default(),
            )?),
            ..Default::default()
        };
        let story = 14_000_000;
        persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, story)?;
        let assets = prepared.assets(world, &persistent)?;
        let mut found = None;
        'search: for z in (1600..57600).step_by(3200) {
            for x in (1600..76800).step_by(3200) {
                let position = Position::from_map([x as f32, z as f32, 0.])?;
                if prepared.definition.landmarks.worlds[world.index()]
                    .iter()
                    .any(|l| {
                        position.distance_to(
                            Position::from_map([l.position[0], l.position[1], 0.]).unwrap(),
                        ) < l.radius + 250.
                    })
                {
                    continue;
                }
                let query = assets
                    .terrain
                    .query(position, assets.movement.collision_radius)?;
                let Some(surface) = query.surface(Mode::Ground) else {
                    continue;
                };
                if surface.height <= 0.
                    || !query.has_clearance(Mode::Ground)
                    || assets
                        .rules
                        .lookup(world, position, surface.response, 0, story)
                        .is_none()
                {
                    continue;
                }
                let mut entry = PersistentState {
                    party: persistent.party.clone(),
                    ..Default::default()
                };
                entry
                    .memory
                    .write(0x40, symphonia_script::Width::S32, story)?;
                let mut session = Session::enter(
                    assets.clone(),
                    State {
                        world,
                        position,
                        heading: 0.,
                        camera_yaw: 0.,
                        alternate_perspective: false,
                        map_display: Default::default(),
                        mount: Mount::Foot,
                        altitude: 0.,
                    },
                    entry,
                    Default::default(),
                )?;
                for _ in 0..61 {
                    session.step(Default::default())?;
                }
                if session.enemies.iter().count() == 3 && session.player_has_control() {
                    found = Some(session);
                    break 'search;
                }
            }
        }
        let mut session =
            found.context("no original terrain candidate spawned all three enemy symbols")?;
        println!(
            "{world:?} enemy probe: {:?}",
            session.travel.state().position.map()
        );
        let symbol = session.enemies.iter().next().unwrap().clone();
        let expected = assets
            .rules
            .lookup(
                world,
                symbol.position,
                session.travel.response(),
                symbol.variant,
                story,
            )
            .unwrap();
        session
            .events
            .world
            .party
            .as_mut()
            .unwrap()
            .encounter_modifier = Some(EncounterModifier {
            rate: 1,
            remaining: 100,
        });
        assert!(session.encounter_symbol(symbol.position, symbol.variant)?);
        let request = session.events.world.battle_request.take().unwrap();
        assert_eq!(
            (&request.setup.encounter, request.setup.arena),
            (
                &resonance_events::battle::Encounter::Pool(expected.group),
                u16::from(expected.arena)
            )
        );
        let tick = session.events.tick();
        for _ in 0..120 {
            session.step(Default::default())?;
        }
        assert_eq!(session.events.tick(), tick);
        assert_eq!(
            session
                .events
                .world
                .party
                .as_ref()
                .unwrap()
                .encounter_modifier
                .as_ref()
                .unwrap()
                .remaining,
            100
        );
        request
            .complete(Outcome::Victory)
            .map_err(anyhow::Error::msg)?;
        session.step(Default::default())?;
        assert!(session.player_has_control());
        assert_eq!(session.enemies.iter().count(), 0);
        session.step(Default::default())?;
        assert_eq!(
            session
                .events
                .world
                .party
                .as_ref()
                .unwrap()
                .encounter_modifier
                .as_ref()
                .unwrap()
                .remaining,
            100
        );
        session.step(resonance_game::overworld::Input {
            travel: resonance_game::overworld::travel::Input {
                stick: [1., 0.],
                ..Default::default()
            },
            ..Default::default()
        })?;
        assert_eq!(
            session
                .events
                .world
                .party
                .as_ref()
                .unwrap()
                .encounter_modifier
                .as_ref()
                .unwrap()
                .remaining,
            99
        );
        session.checkpoint()?;
    }
    Ok(())
}

/// Semantic world checks use the current public documents and dependency discovery.
fn prepare_world(root: &std::path::Path) -> Result<resonance_game::overworld::Prepared> {
    use resonance_content::{
        diagnostics::Diagnostics,
        library::{FIELDS, Library},
        preparation,
        prepared::Files,
    };
    let mut snapshot = Library::open(root, &[])?.snapshot();
    let diagnostics = Diagnostics::new(true);
    let (_, world) = resonance_content::overworld::document::capture(&mut snapshot, &|| false)?;
    preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let skits = std::sync::Arc::new(resonance_content::skit::document::capture(
        &mut snapshot,
        &|| false,
    )?);
    let fields = snapshot
        .mappings(FIELDS)?
        .iter()
        .map(|(id, _)| id)
        .collect();
    let files = std::sync::Arc::new(Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        diagnostics,
    )?);
    resonance_game::overworld::Prepared::from_prepared(world, files, skits, fields)
}
