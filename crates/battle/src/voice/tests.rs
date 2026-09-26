use super::*;
use crate::{ActionPhase, ActionRequest, Battle, BattleInput, ResourceBinding, Side};
use std::sync::Arc;

fn sound(index: u16) -> SoundBinding {
    SoundBinding { resource: 1, index }
}

fn dispatch(voice: &mut Voice, enabled: bool, next: &mut u64) -> Vec<Cue> {
    let mut cues = Vec::new();
    voice
        .step(ActorId(0), [0.; 3], enabled, next, &mut cues)
        .unwrap();
    cues
}

fn battle(body: &str, phase: ActionPhase) -> Battle {
    let source = format!(
        r#"
        asset line: battle::Voice = "test/voice";
        pub task run() {{ {body} }}
    "#
    );
    let mut prepared = crate::tests::prepared(&source, vec![crate::tests::actor(Side::Party)], 10);
    Arc::get_mut(&mut prepared).unwrap().actions[0].phase = phase;
    Battle::new(prepared)
}

fn request() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: 99,
            target: ActorId(0),
        }],
        ..Default::default()
    }
}

fn playback(cues: &[Cue]) -> VoiceId {
    cues.iter()
        .find_map(|cue| match cue {
            Cue::Voice { playback, .. } => Some(*playback),
            _ => None,
        })
        .expect("voice starts")
}

#[test]
fn priority_compares_with_playback_and_last_accepted_pending_request_wins() {
    let mut voice = Voice::default();
    let mut next = 1;
    assert!(voice.request(sound(1), 2));
    assert!(voice.request(sound(2), 1)); // Pending priority is not the playing priority.
    let first = dispatch(&mut voice, true, &mut next);
    assert!(matches!(
        first.as_slice(),
        [Cue::Voice {
            sound: SoundBinding { index: 2, .. },
            ..
        }]
    ));
    let old = playback(&first);
    assert!(!voice.request(sound(3), 0));
    assert!(voice.pending.is_none());
    assert!(voice.request(sound(4), 1));
    let replaced = dispatch(&mut voice, true, &mut next);
    assert!(
        matches!(replaced.as_slice(), [Cue::VoiceStopped { playback }, Cue::Voice { sound: SoundBinding { index: 4, .. }, .. }] if *playback == old)
    );
    assert_ne!(playback(&replaced), old);
    voice.blocked = true;
    assert!(!voice.request(sound(5), 15));
    assert!(voice.pending.is_none());
}

#[test]
fn idle_observation_resets_priority_but_does_not_clear_the_playback_latch() {
    let mut voice = Voice::default();
    let mut next = 1;
    voice.request(sound(1), 2);
    dispatch(&mut voice, true, &mut next);
    voice.playing = None; // Audio host has observed completion.
    assert!(!voice.request(sound(2), 1)); // The common actor visit has not run yet.
    assert!(dispatch(&mut voice, true, &mut next).is_empty());
    assert!(voice.latched);
    assert_eq!(voice.priority, 0);
    assert!(voice.request(sound(2), 1));
    assert!(matches!(
        dispatch(&mut voice, true, &mut next).as_slice(),
        [Cue::Voice { .. }]
    ));
}

#[test]
fn disabled_voices_consume_requests_without_starting_audio() {
    let mut voice = Voice::default();
    let mut next = 1;
    voice.request(sound(1), 2);
    assert!(dispatch(&mut voice, false, &mut next).is_empty());
    assert!(voice.latched);
    assert!(voice.pending.is_none());
    assert!(voice.playing.is_none());
    assert_eq!(voice.priority, 0);
    assert_eq!(voice.mode, 2);
    assert_eq!(next, 1);
}

#[test]
fn centered_request_flag_survives_disabled_playback_and_clears_on_enabled_dispatch() {
    let mut voice = Voice {
        centered: true,
        ..Default::default()
    };
    let mut next = 1;
    voice.request(sound(1), 1);
    assert!(dispatch(&mut voice, false, &mut next).is_empty());
    assert!(voice.centered);
    assert!(voice.pending.is_none());
    voice.request(sound(2), 1);
    assert!(matches!(
        dispatch(&mut voice, true, &mut next).as_slice(),
        [Cue::Voice { centered: true, .. }]
    ));
    assert!(!voice.centered);
}

#[test]
fn actor_requests_dispatch_on_the_same_visit_and_resident_requests_on_the_next() {
    for phase in [ActionPhase::Actor, ActionPhase::Resident] {
        let mut battle = battle("battle::voice(battle::owner(), line, 1);", phase);
        let first = battle.step(request()).unwrap();
        if phase == ActionPhase::Actor {
            playback(&first.cues);
            assert!(battle.step(BattleInput::default()).unwrap().cues.is_empty());
        } else {
            assert!(
                !first
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Voice { .. }))
            );
            playback(&battle.step(BattleInput::default()).unwrap().cues);
        }
    }
}

#[test]
fn interruption_keeps_pending_and_playing_voices_while_menu_holds_dispatch() {
    let mut battle = battle(
        "battle::voice(battle::owner(), line, 1); await battle::wait_ticks(ticks(8));",
        ActionPhase::Resident,
    );
    let first = battle.step(request()).unwrap();
    let action = first
        .cues
        .iter()
        .find_map(|cue| match cue {
            Cue::Started { action, .. } => Some(*action),
            _ => None,
        })
        .unwrap();
    for _ in 0..3 {
        let held = battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap();
        assert!(held.cues.is_empty());
        assert!(battle.voices[0].pending.is_some());
    }
    let interrupted = battle
        .step(BattleInput {
            interrupt: vec![action],
            ..Default::default()
        })
        .unwrap();
    assert!(interrupted.cues.contains(&Cue::Interrupted { action }));
    let playing = playback(&interrupted.cues);
    assert_eq!(battle.voices[0].playing, Some(playing));
    battle
        .step(BattleInput {
            menu_open: true,
            voices_finished: vec![playing],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.voices[0].playing, None);
    assert_eq!(battle.voices[0].priority, 1); // Audio continues during a menu; callbacks wait.
    battle.step(BattleInput::default()).unwrap();
    assert_eq!(battle.voices[0].priority, 0);
}

#[test]
fn completions_validate_atomically_and_late_completion_cannot_stop_a_replacement() {
    let mut battle = battle(
        "battle::voice(battle::owner(), line, 1);",
        ActionPhase::Actor,
    );
    let old = playback(&battle.step(request()).unwrap().cues);
    battle.voices[0].request(sound(2), 1);
    let frame = battle.step(BattleInput::default()).unwrap();
    let current = playback(&frame.cues);
    assert!(
        battle
            .step(BattleInput {
                voices_finished: vec![current, VoiceId(0)],
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(
        battle
            .step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap()
            .update,
        frame.update
    );
    assert_eq!(battle.voices[0].playing, Some(current));
    battle
        .step(BattleInput {
            voices_finished: vec![old, old],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.voices[0].playing, Some(current));
    battle
        .step(BattleInput {
            voices_finished: vec![current],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(battle.voices[0].playing, None);
}

#[test]
fn invalid_priority_faults_and_clears_queued_and_playing_voices() {
    for priority in [-1, 16] {
        let mut battle = battle(
            &format!(
                "battle::voice(battle::owner(), line, 1); await battle::wait_ticks(ticks(1)); battle::voice(battle::owner(), line, {priority});"
            ),
            ActionPhase::Actor,
        );
        battle.step(request()).unwrap();
        assert!(battle.voices[0].playing.is_some());
        assert!(
            battle
                .step(BattleInput::default())
                .unwrap_err()
                .to_string()
                .contains("voice priority must be in 0..15")
        );
        assert!(battle.voices[0].playing.is_none());
        assert!(battle.voices[0].pending.is_none());
        assert!(battle.step(BattleInput::default()).is_err());
    }
}

#[test]
fn voice_bindings_cover_the_roster_and_absent_lines_are_quiet() {
    let mut prepared = crate::tests::prepared(
        "asset line: battle::Voice = \"test/voice\"; pub task run() { battle::voice(battle::owner(), line, 1); }",
        vec![crate::tests::actor(Side::Enemy)],
        5,
    );
    let definition = Arc::get_mut(&mut prepared).unwrap();
    definition.actions[0].phase = ActionPhase::Actor;
    let mut action = definition.actions[0].clone();
    action.resources = vec![ResourceBinding::Voice(vec![])];
    assert!(
        crate::PreparedBattle::new(definition.actors.clone(), vec![action], 1, vec![], vec![])
            .unwrap_err()
            .to_string()
            .contains("voice binding must cover the battle roster")
    );
    let mut battle = Battle::new(prepared);
    assert!(
        !battle
            .step(request())
            .unwrap()
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::Voice { .. }))
    );
}

#[test]
fn original_nurse_voice_requests_and_dispatch_match_without_reseeding_state() {
    #[derive(serde::Deserialize, Debug, PartialEq, Eq)]
    struct State {
        request: u8,
        priority: u8,
        pending: u16,
        delayed: u16,
        flags: u16,
    }
    #[derive(serde::Deserialize)]
    struct Actor {
        actor: usize,
        profile_base: u32,
        channel: u16,
        initial: State,
        playing: bool,
    }
    #[derive(serde::Deserialize)]
    struct Request {
        priority: u8,
        mode: u8,
        resolved_line: Option<u16>,
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "lowercase")]
    enum Call {
        Idle { channel: u16, result: u32 },
        Stop { channel: u16 },
        Play { channel: u16, line: u16 },
    }
    #[derive(serde::Deserialize)]
    struct Visit {
        index: usize,
        tick: u32,
        actor: usize,
        function: String,
        before: State,
        after: State,
        request: Option<Request>,
        calls: Vec<Call>,
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        actors: Vec<Actor>,
        observations: Vec<Visit>,
    }
    fn state(voice: &Voice) -> State {
        State {
            request: voice.mode | (voice.pending.map_or(0, |(_, priority)| priority) << 4),
            priority: voice.priority,
            pending: voice.pending.map_or(0, |(sound, _)| sound.index),
            delayed: 0,
            flags: u16::from(voice.latched) | (u16::from(voice.blocked) << 1),
        }
    }
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../tests/fixtures/nurse-voices.json")).unwrap();
    let mut next = 1;
    let mut voices: Vec<_> = fixture
        .actors
        .iter()
        .map(|actor| {
            let playing = actor.playing.then(|| {
                let id = VoiceId(next);
                next += 1;
                id
            });
            let initial = &actor.initial;
            Voice {
                pending: (initial.pending != 0)
                    .then(|| (sound(initial.pending), initial.request >> 4)),
                mode: initial.request & 15,
                priority: initial.priority,
                latched: initial.flags & 1 != 0,
                blocked: initial.flags & 2 != 0,
                playing,
                ..Default::default()
            }
        })
        .collect();
    let mut plays = 0;
    let mut stops = 0;
    let mut accepted = 0;
    for (index, row) in fixture.observations.iter().enumerate() {
        assert_eq!(row.index, index);
        let actor = &fixture.actors[row.actor];
        assert_eq!(actor.actor, row.actor);
        let voice = &mut voices[row.actor];
        assert_eq!(
            state(voice),
            row.before,
            "before {index} at tick {}",
            row.tick
        );
        let mut cues = Vec::new();
        if row.function == "dispatch" {
            // Only audio-idle observations are external replay input. Original
            // logical priority, pending requests and latch evolve uninterrupted.
            if actor.profile_base != 0 {
                let Call::Idle { channel, result } = row.calls[0] else {
                    panic!("missing idle query")
                };
                assert_eq!(channel, actor.channel);
                if result != 0 {
                    voice.playing = None;
                } else {
                    assert!(voice.playing.is_some());
                }
                voice
                    .step(
                        ActorId(row.actor as u8),
                        [0.; 3],
                        true,
                        &mut next,
                        &mut cues,
                    )
                    .unwrap();
            }
        } else {
            let request = row.request.as_ref().unwrap();
            assert_eq!(request.priority, request.mode); // This observation set uses equal nibbles.
            if let Some(line) = request.resolved_line {
                accepted += usize::from(voice.request(sound(line), request.priority));
            }
        }
        let mut emitted = cues.iter();
        for call in &row.calls {
            match call {
                Call::Idle { .. } => {}
                Call::Stop { channel } => {
                    assert_eq!(*channel, actor.channel);
                    assert!(matches!(emitted.next(), Some(Cue::VoiceStopped { .. })));
                    stops += 1;
                }
                Call::Play { channel, line } => {
                    assert_eq!(*channel, actor.channel);
                    assert!(
                        matches!(emitted.next(), Some(Cue::Voice { actor, sound, .. })
                        if actor.index() == row.actor && sound.index == *line)
                    );
                    plays += 1;
                }
            }
        }
        assert!(
            emitted.next().is_none(),
            "unexpected cue at tick {}",
            row.tick
        );
        assert_eq!(
            state(voice),
            row.after,
            "after {index} at tick {}",
            row.tick
        );
    }
    assert_eq!(
        (fixture.observations.len(), plays, stops, accepted),
        (1280, 7, 2, 7)
    );
}

#[test]
fn maintained_nurse_recovery_skips_caster_and_rejects_a_busy_higher_priority_line() {
    let source = include_str!("../../../../scripts/battle/nurse.sym")
        .replace("script battle;", "")
        .replace("use battle;", "")
        .replace("pub task run()", "task scene_models()");
    let source = format!("{source}\npub task run() {{ await recover(); }}");
    let prepared = crate::tests::prepared(&source, vec![crate::tests::actor(Side::Party); 3], 250);
    let mut battle = Battle::new(prepared);
    // The controlled original Nurse cast reaches recovery while Colette still
    // has a priority-two voice. Keep this external audio playback busy.
    battle.voices[1] = Voice {
        priority: 2,
        mode: 2,
        latched: true,
        playing: Some(VoiceId(1)),
        ..Default::default()
    };
    battle.next_voice = 2;
    battle
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(2),
                target: ActorId(2),
                action: 99,
            }],
            ..Default::default()
        })
        .unwrap();
    for _ in 0..121 {
        let frame = battle.step(BattleInput::default()).unwrap();
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Voice { .. }))
        );
    }
    assert!(battle.voices[0].pending.is_some());
    assert!(battle.voices[1].pending.is_none());
    assert!(battle.voices[2].pending.is_none());
    let frame = battle.step(BattleInput::default()).unwrap();
    assert!(matches!(
        frame.cues.as_slice(),
        [Cue::Voice {
            actor: ActorId(0),
            ..
        }]
    ));
    assert_eq!(battle.voices[1].playing, Some(VoiceId(1)));
}

#[test]
fn secondary_waits_for_playback_and_primary_request_clears_it() {
    let mut voice = Voice::default();
    let mut next = 1;
    voice.request(sound(1), 2);
    dispatch(&mut voice, true, &mut next);
    voice.enqueue(sound(47), 3);
    assert!(dispatch(&mut voice, true, &mut next).is_empty());
    voice.playing = None;
    let cues = dispatch(&mut voice, true, &mut next);
    assert!(matches!(
        cues.as_slice(),
        [Cue::Voice {
            sound: SoundBinding { index: 47, .. },
            ..
        }]
    ));
    assert_eq!(voice.priority, 3);
    voice.enqueue(sound(46), 3);
    assert!(!voice.request(sound(8), 2));
    assert!(voice.secondary.is_some());
    assert!(voice.request(sound(6), 3));
    assert!(voice.secondary.is_none());
}
