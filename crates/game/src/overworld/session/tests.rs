use super::*;
use crate::overworld::{
    landmarks::tests::definitions,
    travel::tests::{parameters, state, terrain},
};
use resonance_content::{
    overworld::{Interaction, Marker},
    session::SessionData,
};
use resonance_events::input::Button;
use resonance_events::party::Party;

fn data() -> Arc<SessionData> {
    // Small valid party data; no original files are needed for scene ownership.
    Arc::new(serde_json::from_value(serde_json::json!({
        "version":1,"executable_sha256":"0".repeat(64),"experience":[0,0,10],
        "items":vec![serde_json::json!({"equipment_kind":null,"allowed_characters":511,"stack_limit":20});60],
        "characters":vec![serde_json::json!({"level":1,"experience":0,"affinity":0,
            "base_stats":[100,20,30,40,50,60,70],"luck":10,"overlimit":0,
                "equipment":vec![0;6],"techniques":[],"allowed_techniques":[],"shortcuts":vec![0;4],
            "growth":vec![serde_json::json!({"base":1,"random":1,"title_bonus":0});7],"level_techniques":{}});9]
    })).unwrap())
}
fn start(mut landmarks: Landmarks, guideposts: Vec<Guidepost>) -> Result<Session> {
    let data = data();
    let mut party = Party::new(&data, Default::default())?;
    party.travel.saved_formation = vec![1];
    landmarks.worlds[0][0].position = [3200., 3200.];
    landmarks.worlds[0][0].interaction = Interaction::Active;
    // Main ends. Landmark 1 requests field 100 at [10,20,30], heading 90.
    let mut code = vec![0x20ffu16];
    for value in [100, 10, 20, 30, 90] {
        code.extend([value, 0x3000, 0x4000]);
    }
    code.extend([0x2040, 0x20ff]);
    let mut words = vec![10, 0, 0, 1, 0, 1, 0, 1, 0, 1];
    words.extend(code);
    let program = Arc::new(Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )?);
    let resources = Arc::new(ResourceLibrary {
        session_data: Some(data),
        fields: [100].into(),
        ..Default::default()
    });
    let assets = Arc::new(Assets {
        world: crate::overworld::World::Sylvarant,
        terrain: Arc::new(terrain(1)),
        rules: Arc::new(Rules::new(crate::overworld::tests::tables())?),
        movement: parameters(),
        landmarks: Arc::new(landmarks),
        guideposts: Arc::new(guideposts),
        program,
        story_rules: crate::overworld::scripts::fixture(),
        resources,
        skits: Default::default(),
    });
    Session::enter(
        assets,
        state(Mount::Foot),
        PersistentState {
            party: Some(party),
            ..Default::default()
        },
        Default::default(),
    )
}

fn visible_camps(session: &Session) -> Vec<u16> {
    session
        .locations
        .visible(crate::overworld::World::Sylvarant)
        .filter_map(|(location, _)| {
            [53, 54, 55, 57, 58, 60]
                .contains(&location.id)
                .then_some(location.id)
        })
        .collect()
}

#[test]
fn roaming_caravan_moves_on_world_entry_but_not_refresh_or_restore() -> Result<()> {
    let initial = start(definitions(), vec![])?;
    let assets = initial.assets.clone();
    let mut state = initial.travel.state().clone();
    state.mount = Mount::Rheairds;
    state.altitude = 600.;
    let mut persistent = initial.events.persistent_state()?;
    // Entry before the roaming flag leaves the script's default stop unchanged.
    assert!(persistent.script_state.is_empty());
    persistent.event_flags.insert(1152);
    for expected in [55, 57, 58, 60, 54, 55] {
        let mut session = Session::enter(
            assets.clone(),
            state.clone(),
            persistent,
            Default::default(),
        )?;
        for _ in 0..3 {
            session.step(Input::default())?;
            assert_eq!(visible_camps(&session), [expected]);
        }
        let checkpoint = serde_json::to_vec(&session.checkpoint()?)?;
        let restored = Session::restore(assets.clone(), serde_json::from_slice(&checkpoint)?)?;
        assert_eq!(visible_camps(&restored), [expected]);
        persistent = restored.events.persistent_state()?;

        // Visiting Tethe'alla must preserve the Sylvarant camp.
        let mut other_assets = (*assets).clone();
        other_assets.world = crate::overworld::World::TetheAlla;
        let mut other_state = state.clone();
        other_state.world = other_assets.world;
        let other = Session::enter(
            Arc::new(other_assets),
            other_state,
            persistent,
            Default::default(),
        )?;
        assert_eq!(visible_camps(&other), [expected]);
        persistent = other.events.persistent_state()?;
    }
    Ok(())
}

#[test]
fn temporary_battle_bypass_resumes_world_and_scripted_encounters_as_victories() -> Result<()> {
    use resonance_events::battle::Outcome;
    let mut session = start(definitions(), vec![])?;
    session.contact(
        Contact {
            id: 94,
            direction: 0,
            blocked: false,
        },
        false,
    )?;
    let request = session.battle_request().unwrap().clone();
    assert!(
        session
            .events
            .world
            .skip_battle_as_victory()
            .map_err(anyhow::Error::msg)?
    );
    assert_eq!(
        request.result().map_err(anyhow::Error::msg)?,
        Some(Outcome::Victory)
    );
    session.step(Input::default())?;
    assert!(session.player_has_control());
    assert!(session.battle_request().is_none());

    // A field script receives the same result in both native return registers,
    // then continues to its next instruction without a combat scene.
    let mut words = vec![10u16, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for value in [96i32, 5, 0, -1, 1, 2, 3, 4, 5, 0, 0, 0] {
        words.extend([
            0x0200,
            value as u16,
            (value as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    words.push(0x20cd);
    // The resumed field caller mutes the battle transition before the combat
    // owner restores the retained track (the Tower of Salvation route).
    for value in [0_i32, 0, 1] {
        words.extend([0x0200, value as u16, 0, 0x3000, 0x4000]);
    }
    words.extend([
        0x2000 | symphonia_script::NativeCall::SetAudioFade as u16,
        0x20ff,
    ]);
    let program = Arc::new(Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )?);
    let mut world = resonance_events::GameWorld::default();
    world.input_enabled = true;
    world.party = session.events.world.party.take();
    let mut events = resonance_events::EventRuntime::with_state(
        program,
        Arc::new(ResourceLibrary::default()),
        world,
        Default::default(),
    )?;
    let request = events.world.battle_request.as_ref().unwrap().clone();
    assert!(!events.player_has_control());
    events.step()?;
    assert!(request.is_pending());
    assert!(
        events
            .world
            .skip_battle_as_victory()
            .map_err(anyhow::Error::msg)?
    );
    assert!(
        !events
            .world
            .skip_battle_as_victory()
            .map_err(anyhow::Error::msg)?
    );
    events.step()?;
    assert!(events.player_has_control());
    assert!(
        matches!(
            events.world.audio_commands.as_slice(),
            [
                resonance_events::AudioCommand::MusicVolume {
                    volume: 0,
                    duration_ticks: 1
                },
                resonance_events::AudioCommand::MusicVolume {
                    volume: 127,
                    duration_ticks: 0
                }
            ]
        ),
        "a skipped battle restores music once"
    );
    for address in [0x20, 0x24] {
        assert_eq!(
            events.memory().read(address, Width::S32)?,
            Outcome::Victory as i32
        );
    }
    Ok(())
}

#[test]
fn sandworm_handoff_freezes_the_source_and_resumes_once_after_results() -> Result<()> {
    use resonance_events::battle::{DefeatPolicy, Outcome};
    for outcome in [Outcome::Victory, Outcome::Escaped] {
        let mut session = start(definitions(), vec![])?;
        let position = session.travel.state().clone();
        session.contact(
            Contact {
                id: 94,
                direction: 0,
                blocked: false,
            },
            false,
        )?;
        assert_eq!(
            session.special_encounter(),
            Some(SpecialEncounter::Sandworm)
        );
        let request = session.events.world.battle_request.take().unwrap();
        assert_eq!(
            (request.setup.encounter, request.setup.arena),
            (resonance_events::battle::Encounter::Formation(96), 5)
        );
        assert_eq!(request.setup.defeat, DefeatPolicy::GameOver);
        assert_eq!(session.battle_request().unwrap().id(), request.id());
        assert!(session.checkpoint().is_err());
        assert!(request.complete(Outcome::Defeat).is_err());
        let tick = session.events.tick();
        let clock = session.play_time.total();
        for _ in 0..120 {
            session.step(Input {
                travel: travel::Input {
                    stick: [1., 0.],
                    ..Default::default()
                },
                ..Default::default()
            })?;
        }
        assert!(request.is_pending());
        assert_eq!(session.events.tick(), tick);
        assert_eq!(session.play_time.total(), clock);
        assert_eq!(session.travel.state(), &position);
        // The combat owner writes back party results before completing the token.
        session.events.world.party.as_mut().unwrap().gald = 345;
        request.complete(outcome).map_err(anyhow::Error::msg)?;
        assert!(request.complete(outcome).is_err());
        session.step(Input::default())?;
        assert!(session.battle_request().is_none());
        assert!(session.player_has_control());
        assert_eq!(session.events.tick(), tick);
        let checkpoint = session.checkpoint()?;
        assert_eq!(checkpoint.state, position);
        assert_eq!(checkpoint.progress.party.gald, 345);
        assert!(
            checkpoint
                .progress
                .party
                .travel
                .visited_locations
                .contains(&94)
        );
        assert_eq!(checkpoint.progress.party.travel.overworld, Some(position));
    }
    Ok(())
}

#[test]
fn world_encounter_callbacks_retire_with_the_scene_and_cannot_resume_a_new_one() -> Result<()> {
    use resonance_events::battle::Outcome;
    let mut old = start(definitions(), vec![])?;
    old.begin_battle(
        resonance_events::battle::Encounter::Formation(96),
        5,
        Some(SpecialEncounter::Sandworm),
    )?;
    let request = old.events.world.battle_request.take().unwrap();
    old.events.cancel();
    assert!(request.complete(Outcome::Victory).is_err());
    assert!(request.result().is_err());
    let mut next = start(definitions(), vec![])?;
    next.begin_battle(resonance_events::battle::Encounter::Pool(1), 2, None)?;
    assert_ne!(request.id(), next.battle_request().unwrap().id());
    assert!(next.battle_request().unwrap().is_pending());
    let request = next.battle_request().unwrap().clone();
    drop(next);
    assert!(request.complete(Outcome::Victory).is_err());
    Ok(())
}

#[test]
fn view_controls_obey_prompt_event_and_battle_ownership() -> Result<()> {
    let mut session = start(definitions(), vec![])?;
    let input = Input {
        travel: travel::Input {
            toggle_perspective: true,
            cycle_map: true,
            ..Default::default()
        },
        ..Default::default()
    };
    session.prompt = Some(Prompt::Guidepost {
        name: "Locked".into(),
    });
    let before = session.travel.state().clone();
    session.step(input)?;
    assert_eq!(session.travel.state(), &before);
    session.prompt = None;
    session.events.world.input_enabled = false;
    session.step(input)?;
    assert_eq!(session.travel.state(), &before);
    session.events.world.input_enabled = true;
    session.begin_battle(resonance_events::battle::Encounter::Pool(1), 2, None)?;
    session.step(input)?;
    assert_eq!(session.travel.state(), &before);
    Ok(())
}

#[test]
fn symbol_handoffs_observe_mount_surface_and_existing_control_owners() -> Result<()> {
    for mount in [Mount::Foot, Mount::Noishe, Mount::Rheairds, Mount::Ship] {
        let mut session = start(definitions(), vec![])?;
        // Populate the current response through the ordinary ground controller.
        session
            .travel
            .reject_displacement(super::super::Position::from_map([4000., 3200., 0.])?);
        session.step(Input::default())?;
        let mut pose = session.travel.state().clone();
        pose.mount = mount;
        if matches!(mount, Mount::Rheairds | Mount::Ship) {
            session.travel = Travel::new(pose, parameters())?;
        } else if mount == Mount::Noishe {
            session.events.world.event_flags.extend(900..=922);
            let cues = session.step(Input {
                travel: travel::Input {
                    toggle_noishe: true,
                    ..Default::default()
                },
                ..Default::default()
            })?;
            let token = cues
                .iter()
                .find_map(|cue| match cue {
                    travel::Cue::Animation { token, .. } => Some(*token),
                    _ => None,
                })
                .unwrap();
            session.travel.finish_animation(token);
            for _ in 0..60 {
                if session.travel.player_has_control() {
                    break;
                }
                session.step(Input::default())?;
            }
        }
        let position = session.travel.state().position;
        assert!(session.encounter_symbol(position, 2).is_err());
        let accepted = session.encounter_symbol(position, 1)?;
        assert_eq!(accepted, matches!(mount, Mount::Foot | Mount::Noishe));
        if accepted {
            let request = session.battle_request().unwrap().clone();
            assert_eq!(
                (request.setup.encounter, request.setup.arena),
                (resonance_events::battle::Encounter::Pool(1007), 0)
            );
            assert!(!session.encounter_symbol(position, 0)?);
            assert_eq!(session.battle_request().unwrap().id(), request.id());
        }
    }
    Ok(())
}

#[test]
fn field_arrivals_resolve_height_before_a_town_prompt_can_pause_travel() -> Result<()> {
    use crate::overworld::{TileCoordinate, collision};
    let original = start(definitions(), vec![])?;
    let mut assets = (*original.assets).clone();
    let center = TileCoordinate::new(0, 0)?;
    assets.terrain = Arc::new(collision::Terrain::new(
        (-1..=1)
            .flat_map(|x| (-1..=1).map(move |y| [x, y]))
            .map(|offset| {
                Ok((
                    center.neighbor(offset),
                    collision::Mesh::new(&[collision::tests::rectangle(
                        1, [-400.; 2], [400.; 2], 200.,
                    )])?,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
        collision::tests::tables(),
    )?);
    let mut session = Session::enter(
        Arc::new(assets),
        state(Mount::Foot),
        original.events.persistent_state()?,
        Default::default(),
    )?;
    assert_eq!(session.travel.state().position.map()[2], 200.);
    assert_eq!(session.travel.state().altitude, 200.);
    session.step(Input::default())?;
    assert!(matches!(
        session.prompt(),
        Some(Prompt::Enter { location: 1, .. })
    ));
    assert_eq!(session.travel.state().position.map()[2], 200.);
    Ok(())
}

#[test]
fn rheaird_portals_require_progress_and_latch_until_leaving_the_gate() -> Result<()> {
    use crate::overworld::{Position, World};
    let mut landmarks = definitions();
    landmarks.worlds[0][14].position = [3200., 3200.];
    landmarks.worlds[1][14].position = [4000., 3500.];
    let mut session = start(landmarks, vec![])?;
    session.travel = Travel::new(state(Mount::Rheairds), parameters())?;
    session.portal_prompt()?;
    assert!(session.prompt().is_none());
    session.events.set_global(16, 13_503_000)?;
    session.refresh_locations()?;
    session.portal_prompt()?;
    assert_eq!(
        session.prompt(),
        Some(&Prompt::ChangeWorld {
            destination: World::TetheAlla
        })
    );
    session.step(Input {
        cancel: true,
        ..Default::default()
    })?;
    session.portal_prompt()?;
    assert!(session.prompt().is_none());
    session
        .travel
        .reject_displacement(Position::from_map([3501., 3200., 0.])?);
    session.portal_prompt()?;
    session
        .travel
        .reject_displacement(Position::from_map([3200., 3200., 0.])?);
    session.portal_prompt()?;
    session.step(Input {
        confirm: true,
        ..Default::default()
    })?;
    assert_eq!(session.world_destination(), Some(World::TetheAlla));
    assert!(session.checkpoint().is_err());
    let before = session.travel.state().clone();
    assert!(session.change_world(session.assets.clone()).is_err());
    assert_eq!(session.travel.state(), &before);
    let mut assets = (*session.assets).clone();
    assets.world = World::TetheAlla;
    let mut arrival = session.change_world(Arc::new(assets))?;
    assert_eq!(arrival.travel.state().position.map(), [4000., 3500., 0.]);
    assert_eq!(arrival.travel.state().mount, Mount::Rheairds);
    assert_eq!(arrival.events.memory().read(0x50, Width::S32)?, 1);
    arrival.portal_prompt()?;
    assert!(arrival.prompt().is_none());
    assert_eq!(session.world_destination(), Some(World::TetheAlla));
    Ok(())
}

#[test]
fn entry_prompt_cancel_and_preparation_keep_the_source_and_return_location() -> Result<()> {
    let mut session = start(definitions(), vec![])?;
    let original = session.travel.state().clone();
    session.step(Input::default())?;
    assert!(matches!(
        session.prompt(),
        Some(Prompt::Enter { location: 1, .. })
    ));
    assert!(session.checkpoint().is_err());
    session.step(Input {
        cancel: true,
        ..Default::default()
    })?;
    assert!(session.prompt().is_none());
    assert!(session.events.world.field_transition.is_none());
    session.step(Input::default())?;
    session.step(Input {
        confirm: true,
        ..Default::default()
    })?;
    let request = session
        .events
        .world
        .field_transition
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(
        (request.map, request.position, request.heading),
        (100, [10., 20., 30.], 90.)
    );
    for _ in 0..2 {
        let entry = session.field_entry()?;
        assert_eq!(
            entry.persistent.party.unwrap().travel.overworld,
            Some(original.clone())
        );
        assert!(request.operation.is_pending());
    }
    assert_eq!(session.travel.state(), &original);
    assert!(!session.player_has_control());
    Ok(())
}

#[test]
fn discovery_rewards_only_once_and_checkpoint_preserves_mount_and_flags() -> Result<()> {
    let mut landmarks = definitions();
    landmarks.item_rewards.insert(63, 2);
    landmarks.party_requirements.clear();
    landmarks.worlds[0][62].position = [3200., 3200.];
    landmarks.worlds[0][62].marker = Marker::FieldPoint;
    let mut session = start(landmarks, vec![])?;
    // Move town 1 away from the player's position to reach the discovery.
    let mut definitions = (*session.assets.landmarks).clone();
    definitions.worlds[0][0].position = [6000., 3200.];
    session.locations = Locations::new(
        Arc::new(definitions),
        Default::default(),
        crate::overworld::scripts::fixture(),
    )?;
    session.step(Input {
        travel: travel::Input {
            stick: [1., 0.],
            ..Default::default()
        },
        ..Default::default()
    })?;
    assert_eq!(session.travel.speed(), 0.);
    assert_eq!(session.discovery().map(|(id, _)| id), Some(63));
    assert_eq!(
        session.prompt(),
        Some(&Prompt::Item {
            item: 2,
            received: true
        })
    );
    session.step(Input {
        confirm: true,
        ..Default::default()
    })?;
    session.step(Input::default())?;
    assert!(session.prompt().is_none());
    assert!(session.discovery().is_none());
    assert_eq!(
        session.events.world.party.as_ref().unwrap().items.get(&2),
        Some(&1)
    );
    let saved = session.checkpoint()?;
    let encoded = serde_json::to_string(&saved)?;
    let decoded = serde_json::from_str(&encoded)?;
    let restored = Session::restore(session.assets.clone(), decoded)?;
    assert_eq!(restored.travel.state(), &saved.state);
    assert!(
        restored
            .events
            .world
            .party
            .as_ref()
            .unwrap()
            .travel
            .visited_locations
            .contains(&63)
    );
    assert_eq!(restored.play_time.total(), 3);
    Ok(())
}

#[test]
fn guidepost_sets_all_regional_flags_and_full_inventory_retains_discovery() -> Result<()> {
    let mut data = definitions();
    data.item_rewards.insert(1, 2);
    let mut session = start(data, vec![])?;
    session
        .events
        .world
        .party
        .as_mut()
        .unwrap()
        .items
        .insert(2, 20);
    session.step(Input::default())?;
    assert_eq!(
        session.prompt(),
        Some(&Prompt::Item {
            item: 2,
            received: false
        })
    );
    assert!(
        !session
            .events
            .world
            .party
            .as_ref()
            .unwrap()
            .travel
            .visited_locations
            .contains(&1)
    );
    let post = Guidepost {
        name: "Region".into(),
        name_id: 0,
        location: 1,
        event_flags: [
            std::num::NonZeroU16::new(900),
            std::num::NonZeroU16::new(901),
            None,
        ],
    };
    let mut session = start(definitions(), vec![post])?;
    session.step(Input::default())?;
    assert_eq!(
        session.prompt(),
        Some(&Prompt::Guidepost {
            name: "Region".into()
        })
    );
    assert_eq!(session.events.world.event_flags, [900, 901].into());
    session.step(Input {
        confirm: true,
        ..Default::default()
    })?;
    session.step(Input::default())?;
    assert!(session.prompt().is_none());
    assert!(session.events.world.field_transition.is_none());
    Ok(())
}

#[test]
fn field_return_is_prepared_without_consuming_the_fields_pending_operation() -> Result<()> {
    let session = start(definitions(), vec![])?;
    let mut persistent = session.events.persistent_state()?;
    let mut previous = session.travel.state().clone();
    previous.mount = Mount::Noishe;
    persistent.party.as_mut().unwrap().travel.overworld = Some(previous.clone());
    let mut words = vec![4u16, 0, 0, 0];
    for value in [3000i32, 1, 123, -456, 0] {
        words.extend([
            0x0200,
            value as u16,
            (value as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    words.extend([0x2040, 0x20ff]);
    let program = Arc::new(Program::decode(
        &words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    )?);
    let resources = Arc::new(ResourceLibrary {
        fields: [3000].into(),
        ..Default::default()
    });
    let (world, memory) = persistent.into_world();
    let field = EventRuntime::with_state(program, resources, world, memory)?;
    let request = field.world.world_transition.as_ref().unwrap();
    let mut wrong_world = request.clone();
    wrong_world.location = 257;
    assert!(
        Session::return_from_field(
            session.assets.clone(),
            &wrong_world,
            field.persistent_state()?,
            Default::default()
        )
        .is_err()
    );
    assert!(request.operation.is_pending());
    assert_eq!(
        field
            .world
            .party
            .as_ref()
            .unwrap()
            .travel
            .overworld
            .as_ref(),
        Some(&previous)
    );
    let returned = Session::return_from_field(
        session.assets.clone(),
        request,
        field.persistent_state()?,
        Default::default(),
    )?;
    assert_eq!(returned.travel.state().mount, Mount::Noishe);
    let point = returned.travel.state().position.map();
    assert_eq!(
        point[0],
        3200. + session.assets.landmarks.worlds[0][0].radius + 100.
    );
    assert_eq!(point[1], 3200.);
    assert!(request.operation.is_pending());
    assert!(field.world.world_transition.is_some());
    Ok(())
}

#[test]
fn restored_play_time_unlocks_overworld_skits_as_exploration_continues() -> Result<()> {
    const REQUIRED_SECONDS: u64 = 18 * 60 * 60;
    let mut session = start(definitions(), vec![])?;
    session
        .travel
        .reject_displacement(crate::overworld::Position::from_map([4000., 4000., 0.])?);
    let mut checkpoint = session.checkpoint()?;
    checkpoint.played_ticks = (REQUIRED_SECONDS - 1) * 60;
    let catalog = Arc::new(serde_json::from_value(serde_json::json!({
        "version": 2, "preview_order": [], "portrait_recipes": [],
        "skits": [{"id": 788, "title": "Time to talk", "party_mask": 0,
            "location": "anywhere", "story": null,
            "condition": {"range": {"value": "played_seconds", "min": REQUIRED_SECONDS}}}]
    }))?);
    let mut assets = (*session.assets).clone();
    assets.resources = Arc::new(ResourceLibrary {
        session_data: assets.resources.session_data.clone(),
        fields: assets.resources.fields.clone(),
        skits: Some(catalog),
        ..Default::default()
    });
    let mut session = Session::restore(Arc::new(assets), checkpoint.clone())?;
    assert_eq!(session.events.world.played_ticks, checkpoint.played_ticks);
    for _ in 0..60 * 60 {
        session.step(Input::default())?;
        assert_eq!(session.events.world.played_ticks, session.play_time.total());
        if session.skits.prompt().is_some() {
            break;
        }
    }
    assert!(session.play_time.total() >= REQUIRED_SECONDS * 60);
    assert_eq!(session.skits.prompt().map(|prompt| prompt.id), Some(788));
    let checkpoint = session.checkpoint()?;
    let restored = Session::restore(session.assets.clone(), checkpoint.clone())?;
    assert_eq!(restored.events.world.played_ticks, checkpoint.played_ticks);
    Ok(())
}

#[test]
fn event_only_skit_suspends_world_and_returns_progress_once() -> Result<()> {
    use resonance_content::skit::{SkitCatalog, SkitResourcePaths};
    let mut session = start(definitions(), vec![])?;
    let mut words = vec![4u16, 0, 0, 0];
    // Set flag 42, wait two updates, end. No notification definition is needed.
    words.extend([
        42, 0x3000, 0x4000, 0x2068, 0, 0x3000, 0x4000, 2, 0x3000, 0x4000, 0x2064, 0x20ff,
    ]);
    let catalog = Arc::new(SkitCatalog {
        preview_order: Vec::new(),
        version: 2,
        skits: vec![],
        resources: [(
            450,
            SkitResourcePaths {
                script: "skit.ssb".into(),
                messages: "messages.json".into(),
                title: Some("Event scene".into()),
            },
        )]
        .into(),
        portraits: Default::default(),
        portrait_recipes: vec![],
        media: Default::default(),
    });
    catalog.validate()?;
    let mut files = resonance_content::prepared::Files::default();
    files.bytes.insert(
        "skit.ssb".into(),
        words
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>()
            .into(),
    );
    files
        .bytes
        .insert("messages.json".into(), b"[]".to_vec().into());
    files.bytes.insert(
        "game/text.json".into(),
        serde_json::to_vec(&resonance_content::session::GameText::default())?.into(),
    );
    files.bytes.insert(
        "game/session-data.json".into(),
        serde_json::to_vec(data().as_ref())?.into(),
    );
    Arc::get_mut(&mut session.assets).unwrap().skits =
        Arc::new(crate::skit::Prepared::load(catalog, &files)?);
    session.start_skit(450, true, false, None)?;
    assert_eq!(session.active_skit.as_ref().unwrap().title, "Event scene");
    assert!(session.checkpoint().is_err());
    session
        .active_skit
        .as_mut()
        .unwrap()
        .events
        .set_global(16, 999)?;
    let parent_tick = session.events.tick();
    let position = session.travel.state().position;
    for _ in 0..4 {
        session.step(Input {
            travel: travel::Input {
                stick: [1., 0.],
                ..Default::default()
            },
            ..Default::default()
        })?;
        if session.active_skit.is_none() {
            break;
        }
    }
    assert!(session.active_skit.is_none());
    assert_eq!(session.events.world.played_ticks, session.play_time.total());
    assert!(session.play_time.total() > 0);
    assert_eq!(session.events.tick(), parent_tick);
    assert_eq!(session.travel.state().position, position);
    assert_eq!(session.events.memory().read(0x40, Width::S32)?, 999);
    assert!(session.events.world.event_flags.contains(&42));
    assert_eq!(
        session.events.world.party.as_ref().unwrap().viewed_skits,
        [450].into()
    );
    assert_eq!(
        session
            .events
            .world
            .audio_commands
            .iter()
            .filter(|command| matches!(command, resonance_events::AudioCommand::StopVoice))
            .count(),
        1
    );
    assert!(session.checkpoint().is_ok());
    Ok(())
}

#[test]
fn cinematic_completion_preserves_return_pose_and_publishes_each_destination_once() -> Result<()> {
    for id in [513, 516, 518] {
        let source = start(definitions(), vec![])?;
        let saved = source.travel.checkpoint()?;
        let mut persistent = source.events.persistent_state()?;
        persistent.party.as_mut().unwrap().travel.overworld = Some(saved.clone());
        let following = resonance_events::SceneDestination {
            map: if id == 518 { 3001 } else { 100 },
            position: if id == 518 {
                [271., 0., 0.]
            } else {
                [10., 20., 30.]
            },
            heading: 90.,
        };
        let mut issuer = resonance_events::GameWorld::default();
        issuer
            .request_world(id, 0, Some(following.clone()))
            .map_err(anyhow::Error::msg)?;
        let definition = Arc::new(resonance_content::overworld::Cinematic {
            world: source.assets.world,
            camera: vec![
                resonance_content::CameraKey {
                    time: 0.,
                    position: [0., -500., 300.],
                    target: [0.; 3],
                },
                resonance_content::CameraKey {
                    time: 60.,
                    position: [100., -500., 300.],
                    target: [100., 0., 0.],
                },
            ],
            actors: Default::default(),
            dialogue: vec![],
        });
        let mut movie = Session::play_cinematic(
            source.assets.clone(),
            issuer.world_transition.as_ref().unwrap(),
            definition,
            persistent,
            source.play_time,
        )?;
        assert!(movie.checkpoint().is_err());
        for _ in 0..60 {
            movie.step(Input {
                travel: travel::Input {
                    stick: [1., 1.],
                    toggle_noishe: true,
                    toggle_perspective: true,
                    cycle_map: true,
                    ..Default::default()
                },
                confirm: true,
                menu: crate::field::FieldInput {
                    pressed_buttons: [Button::Menu].into(),
                    ..Default::default()
                },
                ..Default::default()
            })?;
        }
        assert_eq!(movie.cinematic.as_ref().unwrap().camera().position[0], 50.);
        assert_eq!(
            movie.events.world.played_ticks,
            source.play_time.total() + 60
        );
        assert_eq!(movie.play_time.total(), movie.events.world.played_ticks);
        assert!(movie.menu.is_none());
        assert!(!movie.player_has_control());
        assert_eq!(
            movie
                .events
                .world
                .party
                .as_ref()
                .unwrap()
                .travel
                .overworld
                .as_ref(),
            Some(&saved)
        );
        for _ in 0..150 {
            movie.step(Default::default())?;
        }
        let (operation, tick) = if id == 513 {
            let request = movie.events.world.field_transition.as_ref().unwrap();
            assert_eq!(request.position, following.position);
            assert_eq!(request.heading, following.heading);
            assert_eq!(
                movie
                    .field_entry()?
                    .persistent
                    .party
                    .unwrap()
                    .travel
                    .overworld
                    .as_ref(),
                Some(&saved)
            );
            (request.operation.clone(), movie.events.tick())
        } else {
            let request = movie.events.world.world_transition.as_ref().unwrap();
            assert_eq!(request.location, if id == 516 { 517 } else { 271 });
            if id == 516 {
                assert_eq!(request.following.as_ref(), Some(&following));
                assert_eq!(movie.events.memory().read(0x50, Width::S32)?, 1);
            } else {
                assert!(request.following.is_none());
                let state = movie
                    .events
                    .world
                    .party
                    .as_ref()
                    .unwrap()
                    .travel
                    .overworld
                    .as_ref()
                    .unwrap();
                assert_eq!(state.mount, Mount::Rheairds);
                assert_eq!(state.position, saved.position);
            }
            (request.operation.clone(), movie.events.tick())
        };
        for _ in 0..120 {
            movie.step(Default::default())?;
        }
        assert_eq!(movie.events.tick(), tick);
        assert!(operation.is_pending());
        movie.events.cancel();
        assert_eq!(
            operation.progress().outcome,
            Some(resonance_events::Outcome::Cancelled)
        );
    }
    Ok(())
}
