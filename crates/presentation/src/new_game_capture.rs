//! A device-free development replay of the same application used by players.
//! This checks the handoff and interaction; oracle comparison remains separate.
use super::*;
use bevy::{
    app::PluginsState,
    input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent},
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    time::TimeUpdateStrategy,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicU32;
use std::{collections::BTreeSet, path::Path, thread};

pub fn record_new_game(root: &Path, output: &Path) -> Result<()> {
    record_new_game_with_gamepad(root, output, false)
}

/// Use synthetic raw gamepad events to exercise the ordinary controller path.
/// No physical controller or audio device is opened by either recording mode.
pub fn record_new_game_with_gamepad(root: &Path, output: &Path, gamepad: bool) -> Result<()> {
    record_new_game_until(root, output, gamepad, None, None)
}

/// Stop at an observed checkpoint for a shorter diagnostic, not route acceptance.
pub fn record_new_game_until(
    root: &Path,
    output: &Path,
    gamepad: bool,
    stop_at: Option<&str>,
    input_replay: Option<&resonance_game::field::replay::InputReplay>,
) -> Result<()> {
    record(
        root,
        output,
        gamepad,
        stop_at,
        input_replay,
        Resolution::default(),
        None,
    )
}

/// Continue from the completed classroom exit scene through real keyboard input.
pub fn record_new_game_exploration(
    root: &Path,
    output: &Path,
    replay: &crate::CheckpointReplay,
) -> Result<()> {
    record(
        root,
        output,
        false,
        None,
        None,
        Resolution::default(),
        Some(replay),
    )
}

/// Presentation-size diagnostic only. Oracle recorders always use native size.
pub fn record_new_game_display(
    root: &Path,
    output: &Path,
    resolution: Resolution,
    stop_at: &str,
) -> Result<()> {
    record(root, output, false, Some(stop_at), None, resolution, None)
}

fn record(
    root: &Path,
    output: &Path,
    gamepad: bool,
    stop_at: Option<&str>,
    input_replay: Option<&resonance_game::field::replay::InputReplay>,
    resolution: Resolution,
    exploration: Option<&crate::CheckpointReplay>,
) -> Result<()> {
    if let Some(replay) = input_replay {
        replay.validate()?;
    }
    if let Some(replay) = exploration {
        replay.validate()?;
    }
    anyhow::ensure!(
        stop_at != Some("input-replay-end") || input_replay.is_some(),
        "input-replay-end needs an input replay"
    );
    // Bind the running build before another Cargo invocation can replace its
    // pathname during this several-minute replay.
    let executable_sha256 = digest_file(&std::env::current_exe()?)?;
    anyhow::ensure!(
        !output.exists(),
        "New Game recording directory already exists"
    );
    let names = resonance_events::ResourceLibrary {
        actor_names: resonance_events::ResourceLibrary::character_names(),
        text: Arc::new(serde_json::from_slice(&fs::read(
            root.join("game/text.json"),
        )?)?),
        ..Default::default()
    }
    .names(None);
    let exit_dialogue = [
        (
            format!("{}! Where are you going?", names[&1]),
            "classroom-exit-genis",
        ),
        ("It's research.".into(), "classroom-exit-choice"),
        ("...Huh? Um, okay.".into(), "classroom-exit-colette"),
        (
            format!("{} and {}", names[&2], names[&3]),
            "classroom-party-joined",
        ),
    ];
    fs::create_dir_all(output)?;
    let (mut app, _) = build_app_with_display(
        RunOptions {
            script_root: None,
            saves: crate::SaveOptions {
                directory: Some(output.join("slots")),
                ..Default::default()
            },
            assets: root.into(),
            tick: None,
            presentation_start: None,
            capture: None,
            reveal: false,
            selected: 0,
            silent: true,
            replay: None,
            movie_frame: None,
            boot_frame: None,
            skip_intro: true,
            record_playthrough: Some(output.into()),
            record_title_ticks: 1000,
        },
        resolution,
    )?;
    performance::install(
        &mut app,
        PerformanceOptions {
            overlay: false,
            dump: Some(output.join("performance.jsonl")),
        },
        true,
    )?;
    app.world_mut()
        .resource_mut::<playthrough::Recording>()
        .output = output.into();
    let began = Instant::now();
    while app.plugins_state() == PluginsState::Adding {
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(60),
            "New Game plugin setup timed out"
        );
        bevy::tasks::tick_global_task_pools_on_main_thread();
        thread::sleep(Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    let pad = gamepad.then(|| app.world_mut().spawn(Gamepad::default()).id());
    audio::validate_startup(&app, true, true)?;
    app.init_resource::<field_audio::Trace>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let mut prepared = 0;
    while prepared < 30 {
        anyhow::ensure!(
            began.elapsed() < Duration::from_secs(60),
            "New Game renderer preparation timed out"
        );
        app.update();
        playthrough::check_exit(&app)?;
        prepared = if app.world().resource::<timing::Ready>().0 {
            prepared + 1
        } else {
            0
        };
        thread::sleep(resonance_game::clock::UPDATE_STEP);
    }
    app.world_mut()
        .resource_mut::<playthrough::Recording>()
        .started = true;
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        resonance_game::clock::UPDATE_STEP,
    ));
    let (mixer, mut audio) = resonance_playback::Offline::new();
    let mut wave =
        hound::WavWriter::create(output.join("audio.partial.wav"), field_audio::PCM_SPEC)?;
    let mut frames = 0;
    let mut shots = BTreeSet::new();
    let failure = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicU32::new(0));
    let mut control_tick = None;
    let mut interaction = None;
    let mut completed = false;
    let mut colette_route_tick = None;
    let mut colette_interaction = None;
    let mut colette_completed = false;
    let mut exit_tick = None;
    let mut exit_completed = false;
    let mut exit_control_tick = None;
    let mut inputs = Vec::new();
    let mut previous_keys = Vec::new();
    let mut readable_since = std::collections::BTreeMap::new();
    let mut input_replay_start = None;
    let started = Instant::now();
    for step in 0..36000u64 {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(600),
            "New Game recording timed out"
        );
        let title_tick = app.world().resource::<Menu>().0.tick;
        let mut keys = Vec::new();
        if !app.world().contains_resource::<new_game::Session>() {
            if matches!(title_tick, 1 | 80) {
                keys.push(KeyCode::Enter);
            }
        } else if let Some(session) = app.world().get_resource::<new_game::Session>()
            && session.ready_for_field
        {
            let field = &session.field;
            let tick = field.events.tick();
            if input_replay_start.is_none()
                && input_replay.is_some_and(|replay| replay.matches(field))
            {
                input_replay_start = Some(tick);
            }
            if session.assets.map_id == 5 {
                // Hold the original setup prompt for a camera/pose checkpoint,
                // then select its "Not right now" route using normal input.
                if tick >= 360
                    && let Some(choice) = field.events.world.choices.get(&1)
                    && choice.operation.is_pending()
                {
                    if choice.selected_line == 0 {
                        keys.push(KeyCode::ArrowDown);
                    } else if tick >= 390 && tick.is_multiple_of(30) {
                        keys.push(KeyCode::Enter);
                    }
                }
            } else {
                if field.events.world.input_enabled && control_tick.is_none() {
                    control_tick = Some(tick);
                }
                if completed && !colette_completed {
                    let start = *colette_route_tick.get_or_insert(tick);
                    let elapsed = tick.saturating_sub(start);
                    if colette_interaction.is_none() {
                        if elapsed < 82 {
                            keys.push(KeyCode::ArrowLeft);
                        } else if elapsed < 172 {
                            keys.push(KeyCode::ArrowUp);
                        } else if elapsed < 245 {
                            keys.push(KeyCode::ArrowRight);
                        } else if elapsed < 252 {
                            keys.push(KeyCode::ArrowUp);
                        } else if elapsed == 260 {
                            anyhow::ensure!(
                                field.interaction_target() == Some(2),
                                "classroom walk did not reach Colette"
                            );
                            keys.push(KeyCode::Enter);
                        }
                        if elapsed > 260
                            && let Some(dialogue) = field.events.world.dialogue.get(&0)
                            && dialogue.operation.is_pending()
                        {
                            colette_interaction = Some(dialogue.operation.clone());
                        }
                    } else {
                        if dialogue_ready(field, &mut readable_since, DialogueWait::Text)
                            && !previous_keys.contains(&KeyCode::Enter)
                        {
                            keys.push(KeyCode::Enter);
                        }
                        colette_completed = field.events.world.input_enabled
                            && colette_interaction
                                .as_ref()
                                .is_some_and(|op| op.progress().outcome.is_some())
                            && field.events.world.actors[&2].heading == 180.;
                    }
                } else if completed {
                    let start = *exit_tick.get_or_insert(tick);
                    let elapsed = tick.saturating_sub(start);
                    if field.events.world.input_enabled {
                        if field.story_progress()? == 2000 {
                            exit_control_tick.get_or_insert(tick);
                            // The paired Dolphin endpoint observes normal
                            // idle at source frame 9. Keep the handoff image
                            // separately, then let the unchanged game reach
                            // that authored sample (18 cooked ticks).
                            exit_completed = field.events.world.actors[&1]
                                .animation
                                .as_ref()
                                .is_some_and(|a| {
                                    a.slot == 12
                                        && tick.saturating_sub(a.start_tick) >= a.blend_ticks
                                        && (a.sample(tick, 0, a.duration_ticks as f32) - 18.).abs()
                                            < 0.01
                                });
                        } else if elapsed < 75 {
                            keys.push(KeyCode::ArrowLeft);
                        } else if elapsed < 88 {
                            keys.push(KeyCode::ArrowDown);
                        } else {
                            keys.push(KeyCode::ArrowLeft);
                        }
                    } else if dialogue_ready(field, &mut readable_since, DialogueWait::Speech)
                        && !previous_keys.contains(&KeyCode::Enter)
                    {
                        keys.push(KeyCode::Enter);
                    }
                } else if let Some(start) = control_tick {
                    let elapsed = tick.saturating_sub(start);
                    if elapsed < 38 {
                        keys.push(KeyCode::ArrowLeft);
                    } else if elapsed < 53 {
                        keys.push(KeyCode::ArrowUp);
                    } else if elapsed == 53 {
                        anyhow::ensure!(
                            field.interaction_target() == Some(305),
                            "classroom walk did not reach NPC 305"
                        );
                        keys.push(KeyCode::Enter);
                    } else {
                        if let Some(dialogue) = field.events.world.dialogue.get(&0)
                            && dialogue.operation.is_pending()
                            && interaction.is_none()
                        {
                            interaction = Some(dialogue.operation.clone());
                        }
                        // Hold the complete conversation while the actor turns
                        // and its dialogue anchor settles. An immediate reveal
                        // checkpoint is not equivalent to the held oracle page.
                        if elapsed >= 233
                            && tick.is_multiple_of(30)
                            && interaction.as_ref().is_some_and(|op| op.is_pending())
                        {
                            keys.push(KeyCode::Enter);
                        }
                        completed = interaction
                            .as_ref()
                            .is_some_and(|op| op.progress().outcome.is_some())
                            && field.events.world.input_enabled
                            && field.events.world.actors.get(&305).is_some_and(|actor| {
                                (actor.heading - actor.target_heading).abs() < 0.01
                            });
                    }
                } else if dialogue_ready(field, &mut readable_since, DialogueWait::Speech)
                    && !previous_keys.contains(&KeyCode::Enter)
                {
                    keys.push(KeyCode::Enter);
                }
            }
            if let Some(accept) = input_replay_start
                .and_then(|start| input_replay.unwrap().accept_at(tick + 1 - start))
            {
                keys.clear();
                if accept {
                    keys.push(KeyCode::Enter);
                }
            }
        }
        if keys != previous_keys {
            inputs.push(serde_json::json!({"step":step,"keys":keys.iter().map(|key|format!("{key:?}")).collect::<Vec<_>>(),
                "title_tick":title_tick,"field_tick":app.world().get_resource::<new_game::Session>().map(|s|s.field.events.tick())}));
            previous_keys.clone_from(&keys);
        }
        const BUTTONS: [(KeyCode, GamepadButton); 5] = [
            (KeyCode::Enter, GamepadButton::South),
            (KeyCode::ArrowLeft, GamepadButton::DPadLeft),
            (KeyCode::ArrowRight, GamepadButton::DPadRight),
            (KeyCode::ArrowUp, GamepadButton::DPadUp),
            (KeyCode::ArrowDown, GamepadButton::DPadDown),
        ];
        if let Some(pad) = pad {
            let mut events = app.world_mut().resource_mut::<Messages<RawGamepadEvent>>();
            for (key, button) in BUTTONS {
                events.write(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                    pad,
                    button,
                    f32::from(keys.contains(&key)),
                )));
            }
        } else {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for (key, _) in BUTTONS {
                if keys.contains(&key) {
                    input.press(key);
                } else {
                    input.release(key);
                }
            }
        }
        {
            let mut record = app.world_mut().resource_mut::<playthrough::Recording>();
            record.step = step;
            record.audio_frames = frames;
        }
        app.update();
        playthrough::check_exit(&app)?;
        playthrough::attach::<movie::MovieAudio>(app.world_mut(), &mixer)?;
        playthrough::attach::<GameAudio>(app.world_mut(), &mixer)?;
        playthrough::attach::<field_audio::FieldSource>(app.world_mut(), &mixer)?;
        let end = (step + 1) * 32028 * resonance_game::clock::UPDATE_RATE_DENOMINATOR
            / resonance_game::clock::UPDATE_RATE_NUMERATOR;
        app.world()
            .resource::<movie::Playback>()
            .wait_for_audio(end - frames)?;
        for _ in frames..end {
            for _ in 0..2 {
                let sample = audio.next().unwrap_or(0.);
                anyhow::ensure!(sample.is_finite(), "nonfinite New Game audio");
                wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
            }
        }
        frames = end;
        if let Some(control) = app.world().get_resource::<field_audio::Control>() {
            control.check()?;
        }
        let movie = app.world().resource::<movie::Playback>();
        let shot = if movie.active && movie.presented_frame == Some(0) {
            Some("story-movie")
        } else if let Some(session) = app.world().get_resource::<new_game::Session>() {
            let field = &session.field;
            if session.assets.map_id == 5 {
                // Match the observed cursor bob phase (3904 % 24 == 16),
                // independently of omitted startup waits.
                (field.events.tick() >= 304
                    && field
                        .events
                        .world
                        .choices
                        .get(&1)
                        .is_some_and(|c| c.operation.is_pending())
                    && field
                        .dialogue
                        .values()
                        .all(|d| d.fully_revealed() && d.accepts_input()))
                .then_some("new-game-setup")
            } else if completed {
                Some("interaction-complete")
            } else if interaction.is_some()
                && control_tick.is_some_and(|start| field.events.tick() >= start + 203)
                && field
                    .dialogue
                    .values()
                    .any(|d| !d.closed && d.fully_revealed() && d.accepts_input())
            {
                Some("classmate-conversation")
            } else if field.events.world.input_enabled {
                Some("player-control")
            } else if field
                .dialogue
                .values()
                .any(|d| !d.closed && d.visible >= 18)
            {
                if field.events.world.brightness() < 0.01 {
                    Some("opening-dialogue")
                } else {
                    Some("classroom-dialogue")
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(name) = shot
            && shots.insert(name)
        {
            screenshot(
                &mut app,
                output.join(format!("{name}.png")),
                failure.clone(),
                written.clone(),
            )?;
        }
        // Observe the actual application state: these checkpoints do not
        // fast-forward scripts or manufacture presentation requests.
        let mut details = Vec::new();
        let movie = app.world().resource::<movie::Playback>();
        if movie.active
            && movie
                .presented_frame
                .is_some_and(|f| (400..410).contains(&f))
        {
            details.push("story-subtitles");
        }
        if let Some(session) = app.world().get_resource::<new_game::Session>()
            && session.assets.map_id == 340
            && session.ready_for_field
        {
            let world = &session.field.events.world;
            if input_replay_start.is_some_and(|start| {
                world.tick.saturating_sub(start) >= input_replay.unwrap().duration_updates
            }) {
                details.push("input-replay-end");
            }
            if colette_route_tick.is_some() && exit_tick.is_none() {
                if world
                    .dialogue
                    .get(&0)
                    .is_some_and(|d| d.opening_actor == Some(2))
                {
                    details.push("colette-conversation-turn");
                }
                if let Some(operation) = &colette_interaction {
                    if session.field.dialogue.get(&0).is_some_and(|p| {
                        p.operation.id() == operation.id() && p.opening_fraction() == Some(0.5)
                    }) {
                        details.push("colette-conversation-opening");
                    }
                    if session.field.dialogue.get(&0).is_some_and(|p| {
                        p.operation.id() == operation.id()
                            && !p.closed
                            && p.fully_revealed()
                            && p.accepts_input()
                    }) {
                        details.push("colette-conversation");
                    }
                    if operation.progress().outcome.is_some() && world.actors[&2].heading != 180. {
                        details.push("colette-conversation-return");
                    }
                }
                if colette_completed {
                    details.push("colette-conversation-complete");
                }
            }
            if exit_tick.is_some() {
                for dialogue in session
                    .field
                    .dialogue
                    .values()
                    .filter(|p| !p.closed && p.fully_revealed() && p.accepts_input())
                {
                    let text: String = dialogue.current().text();
                    for (prefix, name) in &exit_dialogue {
                        if text.starts_with(prefix) {
                            details.push(*name);
                        }
                    }
                }
                if exit_control_tick.is_some() {
                    details.push("classroom-exit-handoff");
                }
                if exit_completed {
                    details.push("classroom-exit-complete");
                }
            }
            if session
                .field
                .dialogue
                .values()
                .any(|d| !d.closed && d.current().text().starts_with("*Sigh* Never mind."))
            {
                if session
                    .field
                    .dialogue
                    .values()
                    .any(|d| !d.closed && d.fully_revealed() && d.accepts_input())
                {
                    details.push("raine-question");
                    // The independent paired question state observes Raine
                    // walking at source sample 30.5. Keep the text-complete
                    // checkpoint and this authored-motion phase separately.
                    if let Some(actor) = world.actors.get(&4)
                        && actor.motion.is_some()
                        && actor.position[1] > 0.
                        && let Some(animation) = &actor.animation
                        && animation.slot == 36
                    {
                        // Neighboring natural frames expose capture latency;
                        // they never alter the gameplay or animation clock.
                        match animation.sample(world.tick, 0, 80.) {
                            59. => details.push("raine-question-before-two"),
                            60. => details.push("raine-question-before"),
                            61. => details.push("raine-question-oracle"),
                            62. => details.push("raine-question-after"),
                            63. => details.push("raine-question-after-two"),
                            _ => {}
                        }
                    }
                }
                if let Some(start) = session.field.talking.get(&4) {
                    match world.tick.saturating_sub(*start) {
                        90 => details.push("raine-walk-090"),
                        120 => details.push("raine-walk-120"),
                        150 => details.push("raine-walk-150"),
                        180 => details.push("raine-walk-180"),
                        210 => details.push("raine-walk-210"),
                        _ => {}
                    }
                }
            }
            for emote in world.emotes.values() {
                if world.tick.saturating_sub(emote.start_tick) >= 24 {
                    match emote.kind {
                        12 => details.push("lloyd-sleep"),
                        1 => details.push("raine-emote-buckets"),
                        14 => details.push("genis-emote"),
                        _ => {}
                    }
                }
            }
            if world
                .billboards
                .values()
                .any(|b| world.tick.saturating_sub(b.born) >= 20)
            {
                details.push("eraser-dust");
            }
            if world.actors.get(&2).is_some_and(|a| {
                a.appearance
                    .bone_adjustments
                    .values()
                    .any(|b| b.angles[0] < -20. && world.tick.saturating_sub(b.start_tick) >= 20)
            }) {
                details.push("colette-head-turn");
            }
            if world
                .fade
                .as_ref()
                .is_some_and(|f| f.white && f.alpha(world.tick) >= 220.)
            {
                details.push("oracle-white-flash");
            }
            if let Some(fade) = world.fade.as_ref().filter(|f| f.white) {
                match world.tick.saturating_sub(fade.start_tick) {
                    1 => details.push("oracle-white-flash-1"),
                    2 => details.push("oracle-white-flash-2"),
                    _ => {}
                }
            }
            if session
                .field
                .dialogue
                .values()
                .any(|d| !d.closed && d.current().text().starts_with("Yes, Raine."))
                && let Some(start) = session.field.talking.get(&3)
            {
                match world.tick.saturating_sub(*start) {
                    6 => details.push("genis-mouth-a"),
                    12 => details.push("genis-mouth-b"),
                    24 => details.push("genis-mouth-next-a"),
                    _ => {}
                }
            }
        }
        for name in details {
            if shots.insert(name) {
                screenshot(
                    &mut app,
                    output.join(format!("{name}.png")),
                    failure.clone(),
                    written.clone(),
                )?;
            }
        }
        if exit_completed || stop_at.is_some_and(|name| shots.contains(name)) {
            break;
        }
        if step.is_multiple_of(1200) {
            info!(
                step,
                title_tick, "Recording New Game without an audio device"
            );
        }
        // The movie decoder needs real sample cadence. Once it has finished,
        // the field mixer is synchronous and can render faster than wall time;
        // the recorded audio and every gameplay step retain the same clocks.
        if !app
            .world()
            .resource::<movie::Playback>()
            .completed_naturally
        {
            thread::sleep(
                Duration::from_secs_f64((step + 1) as f64 / resonance_game::clock::UPDATE_HZ)
                    .saturating_sub(started.elapsed()),
            );
        }
    }
    anyhow::ensure!(
        exit_completed || stop_at.is_some_and(|name| shots.contains(name)),
        "New Game did not reach the requested endpoint"
    );
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let drain = Instant::now();
    while written.load(Ordering::Acquire) as usize != shots.len() {
        anyhow::ensure!(
            drain.elapsed() < Duration::from_secs(10) && !failure.load(Ordering::Acquire),
            "New Game image readback did not complete"
        );
        app.update();
        playthrough::check_exit(&app)?;
        thread::sleep(Duration::from_millis(2));
    }
    anyhow::ensure!(
        !failure.load(Ordering::Acquire),
        "New Game screenshot failed"
    );
    wave.finalize()?;
    fs::rename(output.join("audio.partial.wav"), output.join("audio.wav"))?;
    let session = app.world().resource::<new_game::Session>();
    for required in [
        "new-game-setup",
        "story-movie",
        "opening-dialogue",
        "classroom-dialogue",
        "player-control",
        "classmate-conversation",
        "interaction-complete",
        "story-subtitles",
        "lloyd-sleep",
        "eraser-dust",
        "raine-emote-buckets",
        "genis-emote",
        "colette-head-turn",
        "oracle-white-flash",
        "oracle-white-flash-1",
        "oracle-white-flash-2",
        "genis-mouth-a",
        "genis-mouth-b",
        "genis-mouth-next-a",
        "raine-question",
        "raine-question-oracle",
        "colette-conversation-turn",
        "colette-conversation-opening",
        "colette-conversation",
        "colette-conversation-return",
        "colette-conversation-complete",
        "classroom-exit-genis",
        "classroom-exit-choice",
        "classroom-exit-colette",
        "classroom-exit-complete",
    ] {
        anyhow::ensure!(
            stop_at.is_some() || shots.contains(required),
            "New Game missed checkpoint {required}"
        );
    }
    fs::write(
        output.join("field-audio-events.json"),
        serde_json::to_vec_pretty(&app.world().resource::<field_audio::Trace>().0)?,
    )?;
    fs::write(
        output.join("input.json"),
        serde_json::to_vec_pretty(&inputs)?,
    )?;
    let manifests = [
        "title.json",
        "title-audio.json",
        "title-sounds.json",
        "movies/1.json",
        "fields/map-340.json",
        "fields/map-5.json",
        "game/session-data.json",
        "fields/map-340-audio.json",
        "ui/dialogue.json",
        "ui/story-subtitles.json",
        "effects/field.json",
    ]
    .into_iter()
    .map(|path| Ok((path, digest_file(&root.join(path))?)))
    .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    let image_hashes = shots
        .iter()
        .map(|name| Ok((*name, digest_file(&output.join(format!("{name}.png")))?)))
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    let mut metadata = serde_json::json!({
        "kind":"new-game-development-replay", "audio_device":false, "window":false,
        "output_stage":app.world().resource::<display::OutputStage>(),
        "diagnostic_stop_at":stop_at,
        "resolution":resolution,
        "native_oracle_size":resolution == Resolution::default(),
        "input_replay":input_replay,
        "input_replay_anchor_tick":input_replay_start,
        "input_device":if gamepad { "synthetic-raw-gamepad" } else { "keyboard" },
        "input_log_keys_are_actions":gamepad,
        "field_audio_implemented":true, "field_audio_frames":app.world().get_resource::<field_audio::Control>().map(|c| c.rendered_frames()),
        "movie_completed_naturally":app.world().resource::<movie::Playback>().completed_naturally,
        "field_tick":session.field.events.tick(), "control_tick":control_tick,
        "setup_completed":session.assets.map_id == 340,
        "party_gald":session.field.events.world.party.as_ref().map(|p|p.gald),
        "player_position":session.field.events.world.actors[&1].position,
        "interaction_actor":305, "interaction_completed":completed, "images":shots,
        "colette_conversation_completed":colette_completed,
        "field_checkpoints_freeze_gameplay":true,
        "exit_scene_completed":exit_completed,
        "exit_control_tick":exit_control_tick,
        "exit_reference_idle_sample":18,
        "story_progress":session.field.story_progress()?,
        "party_formation":session.field.events.world.party.as_ref().map(|p| &p.formation),
        "audio_frames":frames,
        "sample_rate":32028,"update_rate_ratio":[resonance_game::clock::UPDATE_RATE_NUMERATOR,resonance_game::clock::UPDATE_RATE_DENOMINATOR],
        "manifests":manifests,"image_sha256":image_hashes,
        "executable_sha256":executable_sha256,
        "audio_sha256":digest_file(&output.join("audio.wav"))?,
        "input_sha256":digest_file(&output.join("input.json"))?,
        "field_audio_events_sha256":digest_file(&output.join("field-audio-events.json"))?,
        "scene_teardown_checked":true,
    });
    if let Some(replay) = exploration {
        crate::saves::record_live(
            &mut app,
            &output.join("exploration"),
            replay,
            &mixer,
            &mut audio,
        )?;
        metadata["exploration"] = serde_json::json!({
            "continued_same_session":true,
            "recording":"exploration/recording.json",
            "save_directory":"slots",
        });
    }
    app.world_mut().remove_resource::<new_game::Session>();
    app.update();
    playthrough::check_exit(&app)?;
    anyhow::ensure!(
        !app.world().contains_resource::<field_audio::Control>()
            && !app.world().contains_resource::<field_ui::Artwork>(),
        "field resources survived session teardown"
    );
    anyhow::ensure!(
        app.world_mut()
            .query_filtered::<Entity, With<field_view::ActorPart>>()
            .iter(app.world())
            .next()
            .is_none(),
        "field actors survived session teardown"
    );
    anyhow::ensure!(
        !field_view::has_live_shadows(app.world_mut()),
        "field shadows survived session teardown"
    );
    anyhow::ensure!(
        app.world_mut()
            .query_filtered::<Entity, With<AudioPlayer<field_audio::FieldSource>>>()
            .iter(app.world())
            .next()
            .is_none(),
        "field audio player survived session teardown"
    );
    // Exercise disconnection on the same manually consumed mixer as playback.
    for _ in 0..1024 {
        audio.next();
    }
    anyhow::ensure!(
        (0..2048).all(|_| audio.next().unwrap_or(0.) == 0.),
        "retired scene still produces audio"
    );
    if stop_at.is_some() {
        fs::write(
            output.join("recording.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        return Ok(());
    }
    // Restore the supported fresh-session entry in the same application and
    // mixer. This catches retained actors/input/audio that a process restart
    // would conceal. The ordinary New Game service owns initialization.
    app.insert_resource(new_game::Request(None));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        resonance_game::clock::UPDATE_STEP,
    ));
    let restart_began = Instant::now();
    loop {
        app.update();
        playthrough::check_exit(&app)?;
        playthrough::attach::<field_audio::FieldSource>(app.world_mut(), &mixer)?;
        for _ in 0..1068 {
            audio.next();
        }
        if app
            .world()
            .get_resource::<new_game::Session>()
            .is_some_and(|s| s.field.events.tick() >= 300)
        {
            break;
        }
        anyhow::ensure!(
            restart_began.elapsed() < Duration::from_secs(60),
            "session restart timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let restarted = app.world().resource::<new_game::Session>();
    let field = &restarted.field;
    anyhow::ensure!(
        restarted.assets.map_id == 5
            && field.events.tick() >= 300
            && field.story_progress()? == 0
            && field
                .events
                .world
                .party
                .as_ref()
                .is_some_and(|p| p.formation == [1])
            && field
                .events
                .world
                .actors
                .keys()
                .copied()
                .collect::<Vec<_>>()
                == [1, resonance_events::camera::ANCHOR_ACTOR, 999996]
            && field
                .events
                .world
                .choices
                .get(&1)
                .is_some_and(|c| { c.selected_line == 0 && c.operation.is_pending() })
            && field.events.world.voice.is_none()
            && field.events.world.movie.is_none()
            && !field.events.world.input_enabled,
        "fresh New Game retained prior scene state or stale confirmation input; actors: {:?}",
        field.events.world.actors.keys().collect::<Vec<_>>()
    );
    metadata["restart_validation"] = serde_json::json!({
        "same_application_and_mixer":true,
        "entry":"New Game service",
        "map":restarted.assets.map_id,
        "tick":field.events.tick(),
        "story_progress":field.story_progress()?,
        "party_formation":[1],
        "pending_first_choice":true,
        "retired_audio_is_silent":true,
    });
    fs::write(
        output.join("recording.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    info!("New Game recording completed: {}", output.display());
    Ok(())
}
enum DialogueWait {
    Text,
    Speech,
}
fn dialogue_ready(
    field: &resonance_game::field::FieldSession,
    readable_since: &mut std::collections::BTreeMap<(u64, usize), u32>,
    wait: DialogueWait,
) -> bool {
    let tick = field.events.tick();
    field.dialogue.values().any(|page| {
        !page.closed
            && page.fully_revealed()
            && (matches!(wait, DialogueWait::Text) || (!page.persistent && page.voice_finished()))
            && tick
                - *readable_since
                    .entry((page.operation.id(), page.page))
                    .or_insert(tick)
                >= 60
    })
}

fn digest_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file =
        fs::File::open(path).with_context(|| format!("hash recording input {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn screenshot(
    app: &mut App,
    path: PathBuf,
    failed: Arc<AtomicBool>,
    written: Arc<AtomicU32>,
) -> Result<()> {
    let fixed_tick = app
        .world()
        .get_resource::<new_game::Session>()
        .filter(|s| s.ready_for_field && !app.world().resource::<movie::Playback>().active)
        .map(|s| s.field.events.tick());
    if fixed_tick.is_some() {
        // Readback happens on a later render submission. Keep the gameplay
        // snapshot fixed so an effect/opening image cannot depict tick N+1
        // while its sidecar describes tick N. No audio samples are consumed.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        app.update();
        playthrough::check_exit(app)?;
        super::model_preview::synchronize_capture(app)?;
    }
    let completed = Arc::new(AtomicBool::new(false));
    let captured = completed.clone();
    let failure = failed.clone();
    let framebuffer = app.world().resource::<Framebuffer>().0.clone();
    let secondary = super::secondary_motion::diagnostic(app.world_mut());
    let shadows = field_view::shadow_diagnostic(app.world_mut());
    let metadata = app.world().get_resource::<new_game::Session>().map(|session| {
        let field = &session.field;
        serde_json::json!({"tick":field.events.tick(),"input_enabled":field.events.world.input_enabled,
            "output_stage":app.world().resource::<display::OutputStage>(),
            "talking":field.talking, "state_tick_locked":fixed_tick.is_some(),
            "secondary_chains":secondary,
            "contact_shadows":shadows,
            "camera":field.events.world.field_camera.as_ref().map(|c|serde_json::json!({"position":c.position,"target":c.target,"fov_degrees":c.fov_degrees()})),
            "actors":field.events.world.actors.iter().map(|(id,a)|serde_json::json!({"id":id,"resource":a.resource,"position":a.position,"heading":a.heading,"target_heading":a.target_heading,"animation":format!("{:?}",a.animation),"attachment":format!("{:?}",a.attachment),"bone_adjustments":format!("{:?}",a.appearance.bone_adjustments)})).collect::<Vec<_>>(),
            "dialogue_layouts":app.world().resource::<super::field_ui::Artwork>().diagnostic_layouts(field),
            "emotes":format!("{:?}",field.events.world.emotes), "billboards":format!("{:?}",field.events.world.billboards),
            "fade":format!("{:?}",field.events.world.fade),
            "dialogue":field.dialogue.values().filter(|d| !d.closed).map(|d|d.current().glyphs.iter().take(d.visible).map(|g|g.character).collect::<String>()).collect::<Vec<_>>()})
    });
    app.world_mut()
        .spawn(Screenshot(framebuffer))
        .observe(move |event: On<ScreenshotCaptured>| {
            if let Err(error) = crate::screenshot::write(&event.image, &path, metadata.as_ref()) {
                error!("New Game screenshot failed: {error:#}");
                failed.store(true, Ordering::Release);
            } else {
                written.fetch_add(1, Ordering::Release);
                captured.store(true, Ordering::Release);
            }
        });
    if let Some(tick) = fixed_tick {
        let began = Instant::now();
        while !completed.load(Ordering::Acquire) {
            anyhow::ensure!(
                began.elapsed() < Duration::from_secs(10) && !failure.load(Ordering::Acquire),
                "field checkpoint readback failed"
            );
            app.update();
            playthrough::check_exit(app)?;
            anyhow::ensure!(
                app.world()
                    .resource::<new_game::Session>()
                    .field
                    .events
                    .tick()
                    == tick,
                "field checkpoint advanced gameplay during readback"
            );
            thread::sleep(Duration::from_millis(1));
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
    }
    Ok(())
}
