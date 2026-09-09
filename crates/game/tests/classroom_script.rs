//! Original assets remain local. This is behavior evidence, not image/audio acceptance.
use resonance_content::field::FieldAssets;
use resonance_game::field::{FieldInput, FieldSession};
use std::{fs, path::PathBuf};

fn asset_root() -> PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"))
}
fn classroom(entry: resonance_game::field::FieldEntry) -> FieldSession {
    let root = asset_root();
    let assets: FieldAssets =
        serde_json::from_slice(&fs::read(root.join("fields/iselia-classroom.json")).unwrap())
            .unwrap();
    FieldSession::enter(
        &fs::read(root.join(&assets.script.path)).unwrap(),
        serde_json::from_slice(&fs::read(root.join(&assets.messages)).unwrap()).unwrap(),
        &assets,
        entry,
    )
    .unwrap()
}
fn readable(page: &resonance_game::dialogue::DialoguePlayer) -> bool {
    !page.closed && !page.persistent && page.fully_revealed()
}
fn ready(session: &FieldSession) -> bool {
    session.dialogue.values().any(readable)
}
fn skip_movie(session: &mut FieldSession) {
    if let Some(movie) = &session.events.world.movie
        && movie.operation.is_pending()
    {
        movie.operation.complete(None).unwrap();
    }
}
fn advance_to(
    session: &mut FieldSession,
    reached: impl Fn(&FieldSession) -> bool,
    mut accept: impl FnMut(&FieldSession, u32) -> bool,
) {
    for update in 0..20_000 {
        if reached(session) {
            return;
        }
        skip_movie(session);
        let interact = accept(session, update);
        session
            .step(FieldInput {
                interact,
                ..Default::default()
            })
            .unwrap();
        session.events.world.audio_commands.clear();
    }
    assert!(reached(session), "classroom checkpoint not reached");
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn steady_keyboard_walking_keeps_the_walk_clip_across_loops() {
    let mut session = classroom(Default::default());
    advance_to(
        &mut session,
        |s| s.events.world.input_enabled,
        |_, tick| tick % 120 == 0,
    );
    assert!(session.events.world.input_enabled);
    let player = session.events.world.controlled_actor;
    let actor = session.events.world.actors.get_mut(&player).unwrap();
    actor.position = [-52., -619., 0.];
    actor.face(180.);
    for _ in 0..240 {
        session.step(FieldInput::default()).unwrap();
    }
    let mut binding = None;
    let mut previous_sample = 0.;
    let mut loops = 0;
    for update in 0..535 {
        // Recorded near-camera Dolphin route: down to the front wall, then
        // repeated left/right passes across the classroom's guarded triggers.
        let direction = match update {
            10..40 => [0., -1.],
            90..150 | 310..430 => [1., 0.],
            170..290 | 450..535 => [-1., 0.],
            _ => [0.; 2],
        };
        session
            .step(FieldInput {
                direction,
                ..Default::default()
            })
            .unwrap();
        session.events.world.audio_commands.clear();
        if direction[0] == 0. {
            binding = None;
            continue;
        }
        assert!(
            session.events.world.input_enabled,
            "spurious input lock at update {update}"
        );
        let animation = session.events.world.actors[&player]
            .animation
            .as_ref()
            .unwrap();
        assert_eq!(animation.slot, 36, "walk interrupted at update {update}");
        if let Some(start) = binding {
            assert_eq!(
                animation.start_tick, start,
                "walk restarted at update {update}"
            );
        }
        binding = Some(animation.start_tick);
        let sample = animation.sample(session.events.tick(), 0, animation.duration_ticks as f32);
        loops += u32::from(sample < previous_sample);
        previous_sample = sample;
    }
    assert!(loops >= 6);
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn eraser_impact_keeps_sound_motion_and_seeded_dust_together() {
    use resonance_events::AudioCommand;
    use resonance_game::field::replay::InputReplay;
    let mut session = classroom(Default::default());
    let anchor = InputReplay {
        actor: 100,
        position: [0., -10., 0.],
        animation_slot: 80,
        animation_sample: 20.,
        duration_updates: 21,
        accept_updates: Vec::new(),
    };
    advance_to(&mut session, |s| anchor.matches(s), |s, _| ready(s));
    assert!(anchor.matches(&session));
    assert!(session.events.world.billboards.is_empty());
    // Recovered from the independent impact checkpoint's RNG state. The
    // next sixteen original draws produce these eight growth/spin pairs.
    session.events.world.random_state = 0xdf7fa20d;
    session.step(FieldInput::default()).unwrap();
    let world = &session.events.world;
    assert_eq!(world.billboards.len(), 8);
    assert_eq!(
        world
            .audio_commands
            .iter()
            .filter(|c| matches!(c, AudioCommand::Sound { id: 236, .. }))
            .count(),
        1
    );
    let animation = world.actors[&100].animation.as_ref().unwrap();
    assert_eq!(world.tick - animation.phase_tick, 40);
    assert_eq!(animation.sample(world.tick, 0, 70.), 20.5);
    let observed = [
        ([90., -635., 140.], 1.3, 1.),
        ([70., -635., 140.], 2.48, 1.),
        ([80., -635., 150.], 2.4, -1.),
        ([80., -635., 130.], 2.82, -1.),
        ([85., -635., 145.], 2.98, -1.),
        ([85., -635., 135.], 1.49, -1.),
        ([75., -635., 145.], 1.84, -1.),
        ([75., -635., 135.], 2.29, -1.),
    ];
    for (p, &(position, growth, spin)) in world.billboards.values().zip(&observed) {
        assert_eq!(p.born, world.tick);
        assert_eq!(p.position, position);
        assert_eq!(p.size, [10.; 2]);
        assert!((p.size_delta - growth).abs() < 0.00001);
        assert_eq!(p.angular_velocity, [0., 0., spin]);
        assert_eq!(p.alpha(world.tick), 75.);
    }
    for _ in 0..20 {
        session.step(FieldInput::default()).unwrap();
    }
    for (p, &(_, growth, spin)) in session.events.world.billboards.values().zip(&observed) {
        assert!((p.size[0] - (10. + growth * 20.)).abs() < 0.0001);
        assert_eq!(p.rotation, [0., 0., spin * 20.]);
        assert_eq!(p.alpha(session.events.world.tick), 55.);
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn raine_walk_matches_observed_service_boundaries_and_ramp_motion() {
    use resonance_game::field::replay::InputReplay;
    use std::collections::BTreeMap;
    let mut session = classroom(Default::default());
    let replay: InputReplay = serde_json::from_str(include_str!(
        "../../../tools/oracle/cases/raine-mithos-input.json"
    ))
    .unwrap();
    replay.validate().unwrap();
    let mut pages = BTreeMap::new();
    advance_to(
        &mut session,
        |s| replay.matches(s),
        |s, _| {
            let tick = s.events.tick();
            s.dialogue.values().any(|p| {
                readable(p)
                    && tick - *pages.entry((p.operation.id(), p.page)).or_insert(tick) >= 180
            })
        },
    );
    assert!(replay.matches(&session));
    // Independent MemoryWatcher samples from raine-wait-stages-silent and
    // raine-walk-position-silent. The first two confirmations were consumed at
    // updates 21/171; DTM polling happened one VI before the window consumed A.
    let observed = BTreeMap::from([
        (189, ([238., 445., 0.], 181.)),
        (201, ([218.146_06, 434.411_25, 0.], 298.)),
        (233, ([168.762_68, 436.302_86, 0.7296725], 267.)),
        (250, ([147.415_25, 437.295_53, 16.153194], 267.)),
        (387, ([-60., 445., 27.141], 267.)),
        (400, ([-60., 445., 27.141], 267.)),
    ]);
    for update in 1..=400 {
        session
            .step(FieldInput {
                interact: replay.accept_at(update).unwrap(),
                ..Default::default()
            })
            .unwrap();
        session.events.world.audio_commands.clear();
        let actor = &session.events.world.actors[&4];
        if let Some(&(position, heading)) = observed.get(&update) {
            for (actual, expected) in actor.position.into_iter().zip(position) {
                assert!(
                    (actual - expected).abs() < 0.0001,
                    "update {update}: {:?} != {position:?}",
                    actor.position
                );
            }
            assert_eq!(actor.heading, heading, "update {update}");
        }
        if update == 189 {
            assert_eq!(
                actor.motion.as_ref().unwrap().target,
                [208., 429., 0.],
                "repositioning must preserve the active walk"
            );
        }
        if update == 201 {
            assert_eq!(actor.motion.as_ref().unwrap().target, [-60., 445., 27.]);
        }
        if update == 387 {
            assert!(actor.motion.is_none());
        }
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn conversations_wait_for_facing_then_return_smoothly_for_colette_and_a_classmate() {
    for (id, position) in [(2, [-88., -229., 0.]), (305, [-60., -619., 0.])] {
        let mut session = classroom(Default::default());
        advance_to(
            &mut session,
            |s| s.events.world.input_enabled,
            |s, _| ready(s),
        );
        assert!(session.events.world.input_enabled);
        let previous = session.events.world.actors[&id].target_heading;
        // Register the same observer position as the oracle; earlier tests
        // cover walking/collision. This test isolates conversation sequencing.
        let player = session.events.world.actors.get_mut(&1).unwrap();
        player.position = position;
        player.face(180.);
        assert_eq!(session.interaction_target(), Some(id));
        let initial_heading = session.events.world.actors[&id].heading;
        session
            .step(FieldInput {
                interact: true,
                ..Default::default()
            })
            .unwrap();
        assert!(!session.events.world.input_enabled);
        let request = session.events.world.dialogue[&0].clone();
        assert_eq!(request.opening_actor, Some(id));
        let facing = session.events.world.actors[&id].target_heading;
        if id == 2 {
            assert_eq!(facing, 351.);
        }
        // Interaction itself advances the first turn update.
        let mut outbound_updates =
            usize::from(session.events.world.actors[&id].heading != initial_heading);
        let mut previous_heading = session.events.world.actors[&id].heading;
        for _ in 0..120 {
            if session
                .dialogue
                .get(&0)
                .is_some_and(|p| p.operation.id() == request.operation.id())
            {
                break;
            }
            assert!(
                session.talking.is_empty(),
                "mouth moved before the window opened"
            );
            session.step(FieldInput::default()).unwrap();
            let heading = session.events.world.actors[&id].heading;
            if heading != previous_heading {
                outbound_updates += 1;
            }
            previous_heading = heading;
        }
        assert_eq!(session.events.world.actors[&id].heading, facing);
        // The paired held-conversation state uses Lloyd's event idle +0x74
        // (30 authored frames), not his ordinary +0x0c idle (29 frames).
        assert_eq!(
            session.events.world.actors[&1]
                .animation
                .as_ref()
                .unwrap()
                .slot,
            116
        );
        if id == 2 {
            // Independent Dolphin VI observations 126..159: 180 -> 351.
            // Counting only motion also excludes the window's opening stages.
            assert_eq!(outbound_updates, 34, "Colette's approach turn timing");
        }
        assert!(
            session
                .dialogue
                .get(&0)
                .is_some_and(|p| p.operation.id() == request.operation.id())
        );
        // Let the normal page player reveal, hold, and dismiss the line.
        for _ in 0..1000 {
            if session.events.world.input_enabled {
                break;
            }
            let ready = session
                .dialogue
                .values()
                .any(|p| !p.closed && p.fully_revealed());
            session
                .step(FieldInput {
                    interact: ready,
                    ..Default::default()
                })
                .unwrap();
            if request.operation.is_pending() {
                assert_eq!(session.events.world.actors[&id].target_heading, facing);
            }
        }
        assert!(session.events.world.input_enabled);
        assert_eq!(session.events.world.actors[&id].target_heading, previous);
        assert_ne!(
            session.events.world.actors[&id].heading, previous,
            "return turn snapped"
        );
        let mut return_updates = 0;
        for _ in 0..120 {
            let before = session.events.world.actors[&id].heading;
            session.step(FieldInput::default()).unwrap();
            let after = session.events.world.actors[&id].heading;
            let distance = (after - before + 180.).rem_euclid(360.) - 180.;
            if distance != 0. {
                return_updates += 1;
            }
            assert!(
                distance.abs() <= 6.,
                "actor {id} jumped from {before} to {after}"
            );
        }
        assert_eq!(session.events.world.actors[&id].heading, previous);
        assert_eq!(
            session.events.world.actors[&1]
                .animation
                .as_ref()
                .unwrap()
                .slot,
            12
        );
        if id == 2 {
            // Independent Dolphin VI observations 332..365: 351 -> 180.
            assert_eq!(return_updates, 34, "Colette's return turn timing");
        }
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets; no devices"]
fn walking_to_the_door_runs_both_choices_and_joins_the_party_once() {
    use resonance_events::{PersistentState, party::Party};
    use resonance_game::field::FieldEntry;
    use std::sync::Arc;
    let root = asset_root();
    let assets: FieldAssets =
        serde_json::from_slice(&fs::read(root.join("fields/iselia-classroom.json")).unwrap())
            .unwrap();
    let data: Arc<resonance_content::session::SessionData> = Arc::new(
        serde_json::from_slice(&fs::read(root.join("game/session-data.json")).unwrap()).unwrap(),
    );
    let effects: resonance_content::effect::FieldEffects =
        serde_json::from_slice(&fs::read(root.join(&assets.effects)).unwrap()).unwrap();
    for stay in [false, true] {
        let mut session = classroom(FieldEntry {
            persistent: PersistentState {
                party: Some(Party::new(&data, Default::default()).unwrap()),
                ..Default::default()
            },
            data: Some(data.clone()),
            ..Default::default()
        });
        advance_to(
            &mut session,
            |s| s.events.world.input_enabled,
            |s, _| ready(s),
        );
        assert_eq!(session.story_progress().unwrap(), 1000);
        // Ordinary camera-relative input across the aisle and up to the door.
        // This must detect the registered line; no direct trigger invocation.
        for (direction, updates) in [([-1., 0.], 138), ([0., 1.], 90), ([-1., 0.], 60)] {
            for _ in 0..updates {
                session
                    .step(FieldInput {
                        direction,
                        ..Default::default()
                    })
                    .unwrap();
                session.events.world.audio_commands.clear();
            }
        }
        assert!(
            !session.events.world.input_enabled,
            "door never acquired control"
        );
        let mut saw_question = false;
        let mut saw_choice = false;
        let mut joins = 0;
        let mut checked_clips = std::collections::BTreeSet::new();
        for _ in 0..20000 {
            let choosing = session
                .events
                .world
                .choices
                .values()
                .any(|c| c.operation.is_pending());
            saw_choice |= choosing;
            let move_choice = stay
                && session
                    .events
                    .world
                    .choices
                    .values()
                    .any(|c| c.operation.is_pending() && c.selected_line < c.last_line);
            for p in session.dialogue.values() {
                let text: String = p.current().text();
                saw_question |= text.contains("Where are you going?");
            }
            session
                .step(FieldInput {
                    interact: !move_choice && ready(&session),
                    direction: if move_choice { [0., -1.] } else { [0., 0.] },
                    ..Default::default()
                })
                .unwrap();
            for emote in session.events.world.emotes.values() {
                assert!(
                    effects.emotes.contains_key(&emote.kind),
                    "uncooked emote {}",
                    emote.kind
                );
            }
            for (&id, actor) in &session.events.world.actors {
                if let Some(animation) = &actor.animation
                    && checked_clips.insert((actor.resource, animation.resource, animation.slot))
                    && let Some(model) = assets.actors.iter().find(|m| m.resource == actor.resource)
                {
                    for (part, spec) in model.parts.iter().enumerate() {
                        assert!(
                            spec.clips.iter().any(|c| c.resource_slot == animation.slot
                                && c.animation_resource.unwrap_or(actor.resource)
                                    == animation.resource),
                            "actor {id}, part {part}: uncooked binding {:#x}/{}",
                            animation.resource,
                            animation.slot
                        );
                    }
                }
            }
            for command in session.events.world.audio_commands.drain(..) {
                if matches!(
                    command,
                    resonance_events::AudioCommand::Sound { id: 80, .. }
                ) {
                    joins += 1;
                }
            }
            if session.story_progress().unwrap() == 2000 && session.events.world.input_enabled {
                break;
            }
        }
        assert!(saw_question && saw_choice);
        assert!(
            session.events.world.input_enabled,
            "doorway stalled (stay={stay}): waits {:?}; pages {:?}",
            session.events.pending_operations(),
            session
                .dialogue
                .values()
                .map(|p| (
                    p.closed,
                    p.visible,
                    p.current().glyphs.len(),
                    p.current().text()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(session.story_progress().unwrap(), 2000);
        assert_eq!(
            session.events.world.party.as_ref().unwrap().formation,
            [1, 2, 3]
        );
        assert_eq!(joins, 1);
        assert!(!session.events.world.actors.contains_key(&2));
        assert!(!session.events.world.actors.contains_key(&3));
        for _ in 0..120 {
            session.step(FieldInput::default()).unwrap();
        }
        assert!(
            session.events.world.input_enabled,
            "completed doorway scene retriggered"
        );
        assert_eq!(
            session.events.world.party.as_ref().unwrap().formation,
            [1, 2, 3]
        );
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets"]
fn original_chosen_answer_waits_for_its_complete_spoken_audio() {
    let root = asset_root();
    let audio: resonance_content::field_audio::FieldAudio =
        serde_json::from_slice(&fs::read(root.join("fields/iselia-classroom-audio.json")).unwrap())
            .unwrap();
    let mut session = classroom(Default::default());
    session.voice_durations = std::sync::Arc::new(
        audio
            .voices
            .iter()
            .map(|(&id, v)| {
                (
                    id,
                    (f64::from(v.frames) / f64::from(v.sample_rate)
                        * resonance_game::clock::UPDATE_HZ)
                        .ceil() as u32,
                )
            })
            .collect(),
    );
    let required = session.voice_durations[&655379];
    let mut started = None;
    let mut mouth_moved = false;
    for _ in 0..20000 {
        if let Some(movie) = &session.events.world.movie
            && movie.operation.is_pending()
        {
            movie.operation.complete(None).unwrap();
        }
        let interact = session
            .dialogue
            .values()
            .any(|d| !d.closed && !d.persistent && d.fully_revealed() && d.voice_finished());
        session
            .step(FieldInput {
                interact,
                ..Default::default()
            })
            .unwrap();
        let tick = session.events.tick();
        if started.is_some() && session.talking.contains_key(&2) {
            mouth_moved = true;
        }
        for command in session.events.world.audio_commands.drain(..) {
            if let resonance_events::AudioCommand::Voice(id) = command {
                if id == 655379 {
                    started = Some(tick);
                }
                if id == 655380 {
                    let elapsed = tick - started.expect("Colette spoke first");
                    assert!(
                        elapsed >= required,
                        "Raine interrupted after {elapsed} updates; voice needs {required}"
                    );
                    assert!(mouth_moved, "Colette's voice must drive her mouth");
                    return;
                }
            }
        }
    }
    panic!("original scenario never reached Raine's reply");
}

#[test]
#[ignore = "requires locally cooked GQSEAF classroom assets"]
fn original_classroom_reaches_control_walks_and_runs_every_child_conversation() {
    let root = asset_root();
    let mut session = classroom(Default::default());
    advance_to(
        &mut session,
        |s| s.events.world.input_enabled,
        |_, update| update % 30 == 10,
    );
    assert!(
        session.events.world.input_enabled,
        "script failed to hand control to player"
    );
    assert_eq!(
        session
            .events
            .memory()
            .read(0x40, symphonia_script::Width::S32)
            .unwrap(),
        1000
    );
    assert_eq!(session.events.world.actors[&1].position, [92., -679., 0.]);
    let camera = session.events.world.field_camera.as_ref().unwrap();
    // Independent classroom-lesson-four-silent Dolphin checkpoint.
    for (actual, expected) in camera.position.into_iter().zip([75.44142, -1278., 186.]) {
        assert!((actual - expected).abs() < 0.002);
    }
    assert_eq!(camera.target, [92., -329., 87.]);
    // The window-side attribute regions select a different script light.
    // These are the current colors in the same independent Dolphin state.
    for id in [1, 2, 301, 303, 304, 305] {
        let light = session.character_light(id);
        assert_eq!(light.shade, [45; 3], "actor {id}");
        assert_eq!(light.bright, [60; 3], "actor {id}");
    }
    for id in [3, 302, 306] {
        let light = session.character_light(id);
        assert_eq!(light.shade, [48; 3], "actor {id}");
        assert_eq!(light.bright, [64; 3], "actor {id}");
    }
    assert!((session.events.world.actors[&301].position[2] - 27.141).abs() < 0.001);
    // Walk along the clear aisle, then toward NPC 305. Input, collision, and
    // target selection use the same field service as presentation.
    for _ in 0..38 {
        session
            .step(FieldInput {
                direction: [-1., 0.],
                ..Default::default()
            })
            .unwrap();
    }
    for _ in 0..15 {
        session
            .step(FieldInput {
                direction: [0., 1.],
                ..Default::default()
            })
            .unwrap();
    }
    assert_eq!(session.interaction_target(), Some(305));
    let previous_heading = session.events.world.actors[&305].target_heading;
    session
        .step(FieldInput {
            interact: true,
            ..Default::default()
        })
        .unwrap();
    assert!(!session.events.world.input_enabled);
    let player = &session.events.world.actors[&1];
    let npc = &session.events.world.actors[&305];
    let expected_heading = (player.position[0] - npc.position[0])
        .atan2(npc.position[1] - player.position[1])
        .to_degrees()
        .rem_euclid(360.)
        .trunc();
    assert_eq!(npc.target_heading, expected_heading);
    let dialogue = &session.events.world.dialogue[&0];
    let text = format!("{:?}", dialogue.body.tokens);
    assert!(text.contains("Let's leave everything to"));
    let operation = dialogue.operation.clone();
    for _ in 0..10 {
        session
            .step(FieldInput {
                direction: [1., 0.],
                ..Default::default()
            })
            .unwrap();
    }
    assert!(!session.events.world.input_enabled);
    let waiting = session.events.world.actors[&1].position;
    for _ in 0..120 {
        if session
            .dialogue
            .get(&0)
            .is_some_and(|p| p.operation.id() == operation.id())
        {
            break;
        }
        session.step(FieldInput::default()).unwrap();
    }
    assert!(
        session
            .dialogue
            .get(&0)
            .is_some_and(|p| p.operation.id() == operation.id())
    );
    // Opening, glyph fade, dismissal, and service completion are distinct
    // visible stages even when confirm is pressed on every update.
    for _ in 0..32 {
        session
            .step(FieldInput {
                interact: true,
                ..Default::default()
            })
            .unwrap();
        if operation.progress().outcome.is_some() {
            break;
        }
    }
    for _ in 0..4 {
        if session.events.world.input_enabled {
            break;
        }
        session.step(FieldInput::default()).unwrap();
    }
    assert!(session.events.world.input_enabled);
    assert_eq!(
        session.events.world.actors[&305].target_heading,
        previous_heading
    );
    assert_eq!(session.events.world.actors[&1].position, waiting);
    assert_eq!(
        session
            .events
            .memory()
            .read(0x40, symphonia_script::Width::S32)
            .unwrap(),
        1000
    );
    // Enumerate the other child scripts directly after the ordinary walk
    // above. This covers dialogue/glyph behavior, not their navigation paths.
    let art: resonance_content::font::DialogueArt =
        serde_json::from_slice(&fs::read(root.join("ui/dialogue.json")).unwrap()).unwrap();
    let font: resonance_content::font::BitmapFont =
        serde_json::from_slice(&fs::read(root.join(art.font)).unwrap()).unwrap();
    let mut saw_curly_quote = false;
    for actor in 301..=306 {
        assert!(session.events.interact(actor).unwrap(), "child {actor}");
        let mut saw_dialogue = false;
        for update in 0..3000 {
            session
                .step(FieldInput {
                    interact: update % 30 == 10,
                    ..Default::default()
                })
                .unwrap();
            for player in session.dialogue.values().filter(|p| !p.closed) {
                saw_dialogue = true;
                for glyph in &player.current().glyphs {
                    let ch = glyph.character;
                    saw_curly_quote |= ch == '“';
                    assert!(
                        ch == '\n' || font.glyphs.contains_key(&ch),
                        "child {actor} requested uncooked glyph {ch:?}"
                    );
                }
            }
            session.events.world.audio_commands.clear();
            if session.events.world.input_enabled {
                break;
            }
        }
        assert!(saw_dialogue, "child {actor} never opened dialogue");
        assert!(
            session.events.world.input_enabled,
            "child {actor} did not finish"
        );
    }
    assert!(
        saw_curly_quote,
        "reported curly-quote conversation was not exercised"
    );
}
