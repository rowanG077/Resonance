use super::Voice;
use crate::{
    ActionDefinition, ActionPhase, ActionRequest, Activity, ActorId, Battle, BattleInput, Cue,
    PreparedBattle, ResourceBinding, Side, SoundBinding, VoiceLine,
};
use anyhow::Result;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    character: u8,
    technique: u16,
    start_remaining: u16,
    start_tick: u32,
    release_tick: u32,
    chant_remaining: u16,
    chant_line: u16,
    release_line: u16,
    requests: Vec<Visit>,
    dispatches: Vec<Visit>,
    checkpoints: Vec<Visit>,
    idle_changes: Vec<IdleChange>,
}

#[derive(Deserialize)]
struct IdleChange {
    tick: u32,
    idle: bool,
}

#[derive(Deserialize)]
struct Countdown {
    remaining: i16,
    last_voice_remaining: i16,
}

#[derive(Deserialize)]
struct Visit {
    combat_tick: u32,
    before: String,
    after: String,
    casting: Countdown,
    calls: Vec<Call>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Call {
    Idle { result: u32 },
    Stop,
    Play { line: u16 },
}

// Only the original voice control bytes are compared. The captured volume,
// pan and audio tail remain separate presentation evidence.
fn control(raw: &str) -> (u8, u8, u16, u16, u16) {
    let word = u64::from_str_radix(&raw[..16], 16).unwrap();
    (
        (word >> 56) as u8,
        (word >> 48) as u8,
        (word >> 32) as u16,
        (word >> 16) as u16,
        word as u16,
    )
}

fn voice_control(voice: &Voice) -> (u8, u8, u16, u16, u16) {
    (
        voice.mode | (voice.pending.map_or(0, |(_, priority)| priority) << 4),
        voice.priority,
        voice.pending.map_or(0, |(sound, _)| sound.index),
        0,
        u16::from(voice.latched) | (u16::from(voice.blocked) << 1),
    )
}

fn battle(case: &Case) -> Result<Battle> {
    // This harness supplies an uninterrupted original countdown. Chant control
    // flow stays in the maintained module; models and stored-scene transitions
    // are exercised by their own integration tests.
    let source = format!(
        r#"
        script battle;
        use battle;
        use battle::casting;
        asset chant: battle::Voice = "test/chant";
        asset fallback: battle::Voice = "test/fallback";
        asset release: battle::Voice = "test/release";
        pub task run() {{
            let voices = casting::Voices {{ chant: chant, fallback: fallback, release: release }};
            battle::set_cast_remaining(ticks({}));
            await battle::next_update();
            let mut last = ticks(0);
            while true {{
                let remaining = battle::cast_remaining();
                last = casting::chant_voice(voices, remaining, last);
                if remaining == ticks(0) {{
                    battle::voice(battle::owner(), voices.release, 2);
                    await battle::next_update();
                    return;
                }}
                battle::set_cast_remaining(remaining - ticks(1));
                await battle::next_update();
            }}
        }}
        "#,
        case.start_remaining
    );
    let compiled = symphonia_script_compiler::compile(
        "test",
        &BTreeMap::from([
            ("test".into(), source),
            (
                "battle::casting".into(),
                include_str!("../../../../scripts/battle/casting.sym").into(),
            ),
        ]),
        &crate::native_declarations(),
    )?;
    // BTLusual member 12 durations, independently recovered from the request
    // observations. 3898C uses an unflagged absolute generic fallback.
    let (chant_duration, fallback, fallback_duration, release_duration) = match case.character {
        3 => (37, 248, 37, 45),
        4 => (62, 369, 41, 37),
        _ => panic!("unexpected caster"),
    };
    let resources = compiled
        .assets
        .iter()
        .map(|asset| {
            let (index, duration) = match asset.path.as_str() {
                "test/chant" => (case.chant_line, chant_duration),
                "test/fallback" => (fallback, fallback_duration),
                "test/release" => (case.release_line, release_duration),
                _ => panic!("unexpected voice binding"),
            };
            ResourceBinding::Voice(vec![Some(VoiceLine {
                sound: SoundBinding { resource: 1, index },
                duration,
            })])
        })
        .collect();
    let entry = compiled
        .program
        .authored()
        .unwrap()
        .functions
        .iter()
        .find(|function| function.name == "test::run")
        .unwrap()
        .entry;
    Ok(Battle::new(Arc::new(PreparedBattle::new(
        vec![crate::tests::actor(Side::Party)],
        vec![ActionDefinition {
            id: 99,
            phase: ActionPhase::Casting,
            program: Arc::new(compiled.program),
            entry,
            duration: 1000,
            tp_cost: 0,
            resources,
        }],
        1,
        vec![],
        vec![],
    )?)))
}

#[test]
fn maintained_chant_requests_and_dispatch_match_original_nurse_and_lightning() -> Result<()> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../tests/fixtures/casting-voices.json"))?;
    assert_eq!(fixture.cases.len(), 2);
    for case in fixture.cases {
        let expected = match case.character {
            4 => (237, 234, 316, 82),
            3 => (216, 382, 439, 57),
            _ => panic!("unexpected caster"),
        };
        assert_eq!(
            (
                case.technique,
                case.requests[0].combat_tick,
                case.release_tick,
                case.chant_remaining
            ),
            expected
        );
        assert_eq!(case.requests.len(), 2);
        assert_eq!(case.dispatches.len(), 2);
        assert_eq!(case.requests[1].combat_tick, case.release_tick);
        assert_eq!(case.requests[0].casting.remaining, expected.3 as i16);
        assert_eq!(case.requests[0].casting.last_voice_remaining, 0);
        assert_eq!(case.requests[1].casting.remaining, 0);
        assert_eq!(
            case.requests[1].casting.last_voice_remaining,
            expected.3 as i16
        );
        let mut battle = battle(&case)?;
        let initial = case
            .checkpoints
            .iter()
            .find(|visit| visit.combat_tick == case.start_tick)
            .unwrap();
        let (request, priority, pending, delayed, flags) = control(&initial.before);
        assert_eq!((pending, delayed), (0, 0));
        assert!(case.idle_changes[0].idle);
        // Seed logical voice state once. Later state comes only from the
        // authored helper, common dispatch and external audio completion.
        battle.voices[0] = Voice {
            mode: request & 15,
            priority,
            latched: flags & 1 != 0,
            blocked: flags & 2 != 0,
            ..Default::default()
        };
        battle.step(BattleInput {
            actions: vec![ActionRequest {
                actor: ActorId(0),
                action: 99,
                target: ActorId(0),
            }],
            ..Default::default()
        })?;
        let mut previous_playback = None;
        let mut plays = 0;
        let mut stops = 0;
        for tick in case.start_tick..=case.release_tick {
            if let Some(request) = case.requests.iter().find(|visit| visit.combat_tick == tick) {
                assert_eq!(
                    voice_control(&battle.voices[0]),
                    control(&request.before),
                    "{} before request at {tick}",
                    case.name
                );
            }
            let mut input = BattleInput::default();
            if case
                .idle_changes
                .iter()
                .any(|change| change.tick == tick && change.idle)
                && let Some(playback) = battle.voices[0].playing
            {
                input.voices_finished.push(playback);
                previous_playback = None;
            }
            let frame = battle.step(input)?;
            let actual: Vec<_> = frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Voice { .. } | Cue::VoiceStopped { .. }))
                .collect();
            if let Some(observed) = case
                .dispatches
                .iter()
                .find(|visit| visit.combat_tick == tick)
            {
                let mut actual = actual.into_iter();
                for call in &observed.calls {
                    match call {
                        Call::Idle { result } => {
                            assert_eq!(*result != 0, previous_playback.is_none());
                        }
                        Call::Stop => {
                            let Cue::VoiceStopped { playback } = actual.next().unwrap() else {
                                panic!("missing voice stop at {tick}");
                            };
                            assert_eq!(Some(*playback), previous_playback);
                            stops += 1;
                        }
                        Call::Play { line } => {
                            let Cue::Voice {
                                actor,
                                playback,
                                sound,
                                ..
                            } = actual.next().unwrap()
                            else {
                                panic!("missing voice at {tick}");
                            };
                            assert_eq!(*actor, ActorId(0));
                            assert_eq!(sound.index, *line);
                            assert_ne!(Some(*playback), previous_playback);
                            previous_playback = Some(*playback);
                            plays += 1;
                        }
                    }
                }
                assert!(actual.next().is_none(), "extra voice cue at {tick}");
                assert_eq!(voice_control(&battle.voices[0]), control(&observed.after));
            } else {
                assert!(
                    actual.is_empty(),
                    "unexpected {} voice at {tick}",
                    case.name
                );
            }
            if let Some(observed) = case
                .checkpoints
                .iter()
                .find(|visit| visit.combat_tick == tick)
            {
                assert_eq!(
                    voice_control(&battle.voices[0]),
                    control(&observed.after),
                    "{} voice state at {tick}",
                    case.name
                );
                let Activity::Casting { clock, .. } = battle.actors[0].activity else {
                    panic!("caster left countdown at {tick}");
                };
                assert_eq!(
                    clock, observed.casting.remaining,
                    "{} countdown at {tick}",
                    case.name
                );
            }
        }
        assert_eq!((plays, stops), (2, 1));
    }
    Ok(())
}
