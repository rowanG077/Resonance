use super::*;
use crate::{
    data::{Command, Envelope, Event, EventKind, Interpolation, Resources, Score, ScoreOrigin},
    envelope,
    music_voice::{Controls, Tables},
    sample::Sample,
};
use std::collections::BTreeMap;

fn wait(milliseconds: Option<u16>, sample_end: bool) -> Command {
    Command::Wait {
        milliseconds,
        from_start: false,
        key_off: false,
        sample_end,
    }
}

fn fixture(commands: Vec<Command>, looping_sample: bool) -> Arc<Loaded> {
    configured_fixture(commands, looping_sample, |_, _, _| {})
}

fn configured_fixture(
    commands: Vec<Command>,
    looping_sample: bool,
    configure: impl FnOnce(&mut Resources, &mut Score, &mut Tables),
) -> Arc<Loaded> {
    let mut program = vec![
        Command::Interpolation {
            mode: Interpolation::Direct,
            coefficients: 0,
        },
        Command::Envelope {
            envelope: Envelope::Ordinary(envelope::Parameters::default()),
        },
        Command::StartSample { sample: 1 },
    ];
    program.extend(commands);
    let mut resources = Resources {
        programs: BTreeMap::from([(1, program)]),
        samples: BTreeMap::from([(
            1,
            Arc::new(Sample {
                key: 60,
                rate: 32000,
                loop_start: 0,
                loop_length: if looping_sample { 160 } else { 0 },
                pcm: vec![12000; 160],
                loop_pcm: if looping_sample {
                    vec![12000; 160]
                } else {
                    vec![]
                },
            }),
        )]),
    };
    let mut score = Score {
        origin: ScoreOrigin::Sequence,
        initial_bpm_1024: 120 * 1024,
        loop_start_tick: 0,
        end_tick: u32::from(u16::MAX),
        tempos: vec![],
        controls: [Controls::default(); 16],
        first_events: vec![Event {
            tick: 0,
            channel: 0,
            kind: EventKind::Notes {
                source: VoiceSource::Sequence {
                    group: 1,
                    program: 1,
                    drums: false,
                },
                voices: vec![Note {
                    macro_id: 1,
                    key: 60,
                    velocity: 127,
                    pan: 64,
                    priority: 9,
                    max_voices: 1,
                }],
                length: u16::MAX,
            },
        }],
        loop_events: vec![],
    };
    let mut tables = crate::package::tests::playback_data().2;
    tables.mix.volume = std::array::from_fn(|i| if i == 0 { 0. } else { 1. });
    tables.mix.alternate_volume = tables.mix.volume;
    configure(&mut resources, &mut score, &mut tables);
    Arc::new(Loaded::new(resources, score, tables, [[0., 0., 1., 0., 0.]; 2]).unwrap())
}

fn render_block(synth: &Synthesizer, stream: &Stream) -> Result<Vec<BusFrame>> {
    (0..160)
        .map(|_| {
            synth.advance()?;
            Ok(stream.frame().unwrap_or([[0; 2]; 3]))
        })
        .collect()
}

#[test]
fn replacement_voice_starts_its_own_lfo_phase() -> Result<()> {
    let loaded = configured_fixture(
        vec![
            Command::Lfo { period_ms: 100 },
            wait(Some(10), false),
            Command::Calculate {
                destination: crate::data::Variable::Global(0),
                operation: crate::data::Arithmetic::Add,
                left: crate::data::Variable::Controller(crate::data::Controller::Lfo),
                right: crate::data::Operand::Constant(0),
            },
            wait(None, false),
        ],
        true,
        |_, _, tables| {
            tables.modulation.sine = std::array::from_fn(|i| i as i16 * 4);
        },
    );
    let fresh = Synthesizer::default();
    let expected = fresh.start(loaded.clone(), false)?;
    let used = Synthesizer::default();
    let previous = used.start(loaded.clone(), false)?;
    for _ in 0..7 {
        render_block(&used, &previous)?;
    }
    // The authored per-source limit forces reuse of the old voice's slot.
    let replacement = used.start(loaded, false)?;
    for _ in 0..3 {
        render_block(&fresh, &expected)?;
        render_block(&used, &replacement)?;
    }
    let expected = fresh.0.lock().unwrap().random.variable(0);
    assert_ne!(expected, 0, "fixture did not sample the oscillator");
    assert_eq!(used.0.lock().unwrap().random.variable(0), expected);
    Ok(())
}

#[test]
fn dropped_player_drains_submitted_audio_then_fades_to_silence() -> Result<()> {
    let synth = Synthesizer::default();
    let stream = synth.start(fixture(vec![wait(None, false)], true), false)?;
    synth.advance()?;
    let heard = stream.frame().unwrap();
    assert!(heard[0][0] > 0);
    drop(stream);
    assert_eq!(
        synth.unread_frame(),
        [[0; 2]; 3],
        "already read PCM is not mixed twice"
    );
    for _ in 1..160 {
        synth.advance()?;
        assert_eq!(
            synth.unread_frame(),
            heard,
            "submitted block survives handle drop"
        );
    }
    let mut previous = heard[0][0];
    for _ in 0..160 {
        synth.advance()?;
        let tail = synth.unread_frame()[0][0];
        assert!(tail >= 0 && tail <= previous);
        previous = tail;
    }
    for _ in 0..320 {
        synth.advance()?;
        assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
    }
    Ok(())
}

#[test]
fn pause_drains_audio_without_ending_the_score() -> Result<()> {
    let synth = Synthesizer::default();
    let stream = synth.start(fixture(vec![wait(None, false)], true), false)?;
    assert!(render_block(&synth, &stream)?[159][0][0] > 0);
    stream.pause(true)?;
    let fade = render_block(&synth, &stream)?;
    assert!(fade[0][0][0] > fade[159][0][0]);
    assert!(
        render_block(&synth, &stream)?
            .iter()
            .all(|frame| *frame == [[0; 2]; 3])
    );
    assert!(stream.frame().is_some(), "paused score remains resumable");
    stream.pause(false)?;
    synth.advance()?;
    assert!(
        stream.frame().is_none(),
        "finished score has no further notes after resume"
    );
    Ok(())
}

#[test]
fn macro_end_waits_for_sample_and_its_release_before_completion() -> Result<()> {
    let synth = Synthesizer::default();
    let stream = synth.start(fixture(vec![Command::End], false), false)?;
    let audio = render_block(&synth, &stream)?;
    assert!(audio.iter().any(|frame| frame[0][0] > 0));
    let tail = render_block(&synth, &stream)?;
    assert!(tail[0][0][0] > 0);
    assert!(stream.frame().is_some());
    synth.advance()?;
    assert!(stream.frame().is_none());
    assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
    Ok(())
}

#[test]
fn replacing_a_voice_does_not_discard_its_queued_samples_or_adopt_its_controls() -> Result<()> {
    let synth = Synthesizer::default();
    let old = synth.start(fixture(vec![wait(None, false)], true), false)?;
    let heard = render_block(&synth, &old)?[159];
    let new = synth.start(fixture(vec![wait(None, false)], true), false)?;
    let quiet = LiveControls {
        volume: 0.,
        ..Default::default()
    };
    new.controls([quiet; 5])?;
    let old_tail = render_block(&synth, &old)?;
    assert_eq!(old_tail[0], heard);
    assert!(old_tail[159][0][0] < heard[0][0]);
    assert_eq!(new.frame(), Some([[0; 2]; 3]));
    synth.advance()?;
    assert_eq!(old.frame(), None);
    assert_eq!(new.frame(), Some([[0; 2]; 3]));
    Ok(())
}

#[test]
fn external_release_is_finite_and_preserves_tiny_stereo_values() -> Result<()> {
    let synth = Synthesizer::default();
    let samples = [[159, -159], [31, -47], [1, -1]];
    synth.release(samples, 0);
    synth.advance()?;
    assert_eq!(synth.unread_frame(), samples);
    for _ in 1..160 {
        synth.advance()?;
    }
    synth.advance()?;
    assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
    Ok(())
}

#[test]
fn runtime_priority_covers_notes_children_macro_updates_and_restoring_authored_priority()
-> Result<()> {
    for child in [false, true] {
        let configure_priority = |_: &mut Resources, score: &mut Score, _: &mut Tables| {
            let EventKind::Notes { voices, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            voices[0].priority = 255;
        };
        let quiet = configured_fixture(vec![wait(None, false)], true, configure_priority);
        let retry = configured_fixture(vec![wait(None, false)], true, |bank, score, tables| {
            configure_priority(bank, score, tables);
            // One score tick per control quantum: retry before the child starts,
            // then again when the later-created stream clears its override.
            score.initial_bpm_1024 = crate::SOURCE_RATE * 60 * 1024 / (384 * 32);
            for tick in [1, 11] {
                let mut event = score.first_events[0].clone();
                event.tick = tick;
                score.first_events.push(event);
            }
        });
        let important = configured_fixture(
            vec![Command::Priority { value: 0 }, wait(None, false)],
            true,
            |bank, score, tables| {
                configure_priority(bank, score, tables);
                let EventKind::Notes { voices, .. } = &mut score.first_events[0].kind else {
                    unreachable!()
                };
                voices[0].priority = 0;
                if child {
                    bank.programs.insert(2, bank.programs[&1].clone());
                    bank.programs.insert(
                        1,
                        vec![
                            Command::SpawnMacro {
                                program: 2,
                                instruction: 0,
                                key_offset: 0,
                                priority: 0,
                                max_voices: 2,
                            },
                            Command::End,
                        ],
                    );
                }
            },
        );
        let synth = Synthesizer::default();
        let earlier = synth.start(retry, false)?;
        let important = synth.start(important, false)?;
        let elevated = LiveControls {
            priority: Some(256),
            ..Default::default()
        };
        important.controls([elevated; 5])?;
        assert!(render_block(&synth, &important)?[159][0][0] > 0);
        assert_eq!(earlier.frame(), Some([[0; 2]; 3]));

        let rejected = synth.start(quiet, false)?;
        assert!(render_block(&synth, &important)?[159][0][0] > 0);
        assert_eq!(rejected.frame(), Some([[0; 2]; 3]));
        let mut controls = [LiveControls::default(); 5];
        controls[0] = elevated;
        important.controls(controls)?;
        assert!(render_block(&synth, &earlier)?[159][0][0] > 0);
        assert_eq!(rejected.frame(), None, "rejected stream must complete");
        let tail = render_block(&synth, &important)?;
        assert!(tail[31][0][0] > 0);
        assert!(tail[32..].iter().all(|frame| *frame == [[0; 2]; 3]));
        // Replacement at frame 352 releases through 511; its final transport
        // block remains readable through 639, including the silent remainder.
        assert_eq!(important.submitted_until(), 640);
        synth.advance()?;
        assert_eq!(important.frame(), None, "replaced stream must complete");
        drop((earlier, important, rejected));
        for _ in 0..3 {
            for _ in 0..160 {
                synth.advance()?;
            }
        }
        assert!(synth.0.lock().unwrap().entries.is_empty());
    }
    Ok(())
}

#[test]
fn stealing_uses_ages_at_the_previous_quantum_end_in_either_entry_order() -> Result<()> {
    for reverse in [false, true] {
        let loaded = |age, period, tick| {
            configured_fixture(
                vec![
                    Command::SetAge { value: age },
                    Command::AgePeriod {
                        milliseconds: period,
                    },
                    wait(None, false),
                ],
                true,
                |_, score, _| {
                    score.initial_bpm_1024 = crate::SOURCE_RATE * 60 * 1024 / (384 * 32);
                    score.first_events[0].tick = tick;
                    let EventKind::Notes { voices, .. } = &mut score.first_events[0].kind else {
                        unreachable!()
                    };
                    voices[0].max_voices = 2;
                },
            )
        };
        // The initially preferred voice ages below the held voice within one
        // quantum. A third note must steal it when the next quantum admits notes.
        let mut entries = [loaded(1000, 1, 0), loaded(500, 0, 0), loaded(1000, 0, 1)];
        if reverse {
            entries.reverse();
        }
        let synth = Synthesizer::default();
        let mut streams = entries
            .into_iter()
            .map(|loaded| synth.start_recorded(loaded, false, true))
            .collect::<Result<Vec<_>>>()?;
        if reverse {
            streams.reverse();
        }
        assert!(render_block(&synth, &streams[2])?[159][0][0] > 0);
        let decayed = synth.take_preview(&streams[0]).unwrap();
        let held = synth.take_preview(&streams[1]).unwrap();
        assert_eq!(decayed.voice_lifetimes[0].end_frame, Some(32));
        assert_eq!(held.voice_lifetimes[0].end_frame, None);
    }
    Ok(())
}

#[test]
fn sound_effects_sustain_until_release_without_an_arrangement_deadline() -> Result<()> {
    let loaded = configured_fixture(
        vec![
            Command::Envelope {
                envelope: Envelope::Ordinary(envelope::Parameters {
                    release_ms: 2,
                    ..Default::default()
                }),
            },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::Release,
            Command::End,
        ],
        true,
        |_, score, _| {
            score.origin = ScoreOrigin::SoundEffect;
            score.end_tick = 0;
            let EventKind::Notes { source, length, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id: 1 };
            *length = 1; // Incidental arrangement length must not release a sound effect.
        },
    );
    let synth = Synthesizer::default();
    let stream = synth.start(loaded, false)?;
    for _ in 0..3 {
        assert!(
            render_block(&synth, &stream)?
                .iter()
                .all(|frame| frame[0][0] > 0)
        );
    }
    stream.controls(
        [LiveControls {
            release: true,
            ..Default::default()
        }; 5],
    )?;
    let mut tail = render_block(&synth, &stream)?;
    assert!(tail[0][0][0] > 0 && tail[159][0][0] < tail[0][0][0]);
    tail.extend(render_block(&synth, &stream)?);
    let release_end =
        crate::volume::frames_from_millis(2)? as usize + crate::RELEASE_FRAMES as usize;
    assert!(
        tail[160][0][0] > 0,
        "the last envelope sample must finish its transport release"
    );
    assert!(
        tail[release_end..]
            .iter()
            .all(|frame| *frame == [[0; 2]; 3])
    );
    synth.advance()?;
    assert_eq!(stream.frame(), None);
    Ok(())
}

#[test]
fn finite_cues_release_their_priority_slots_before_later_quiet_cues() -> Result<()> {
    let synth = Synthesizer::default();
    for priority in std::iter::repeat_n(9, 3).chain([0]) {
        let loaded = configured_fixture(vec![Command::End], false, |_, score, _| {
            score.origin = ScoreOrigin::SoundEffect;
            let EventKind::Notes { source, voices, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id: 1 };
            voices[0].priority = priority;
        });
        let stream = synth.start(loaded, false)?;
        assert!(
            render_block(&synth, &stream)?
                .iter()
                .any(|frame| frame[0][0] > 0),
            "completed sounds must not occupy voice priority slots"
        );
        render_block(&synth, &stream)?;
        synth.advance()?;
        assert_eq!(stream.frame(), None);
        for _ in 1..160 {
            synth.advance()?;
        }
    }
    Ok(())
}

#[test]
fn admitting_a_cue_preserves_a_dropped_cues_final_submitted_block() -> Result<()> {
    let synth = Synthesizer::default();
    let old = synth.start(fixture(vec![Command::End], false), false)?;
    render_block(&synth, &old)?;
    synth.advance()?;
    let last = old.frame().unwrap();
    assert!(last[0][0] > 0);
    drop(old);
    let silent = configured_fixture(vec![], false, |resources, _, _| {
        resources.programs.insert(1, vec![Command::End]);
    });
    let new = synth.start(silent, false)?;
    for _ in 1..160 {
        synth.advance()?;
        assert!(
            synth.unread_frame()[0][0] > 0,
            "new admissions must preserve final queued PCM"
        );
    }
    synth.advance()?;
    let _ = new.frame();
    assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
    Ok(())
}

#[test]
fn ended_looping_sample_releases_its_envelope_and_completes() -> Result<()> {
    let synth = Synthesizer::default();
    let stream = synth.start(
        fixture(vec![wait(Some(5), false), Command::End], true),
        false,
    )?;
    assert!(
        render_block(&synth, &stream)?
            .iter()
            .any(|frame| frame[0][0] > 0)
    );
    let release = render_block(&synth, &stream)?;
    assert!(release[0][0][0] > release[159][0][0]);
    for _ in 0..2 {
        render_block(&synth, &stream)?;
    }
    assert_eq!(stream.frame(), None);
    assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
    Ok(())
}

#[test]
fn messages_are_delivered_after_the_current_quantum_in_either_voice_order() -> Result<()> {
    use crate::data::{
        MessageTarget,
        Variable::{Global, Local},
    };
    for receiver_first in [false, true] {
        let loaded = configured_fixture(vec![], false, |resources, score, _| {
            resources.programs = BTreeMap::from([
                (
                    1,
                    vec![
                        Command::SetVariable {
                            destination: Local(0),
                            value: 42,
                        },
                        Command::SendMessage {
                            target: MessageTarget::Macro(2),
                            value: Local(0),
                        },
                        Command::End,
                    ],
                ),
                (
                    2,
                    vec![
                        Command::Interpolation {
                            mode: Interpolation::Direct,
                            coefficients: 0,
                        },
                        Command::MessageTrap {
                            program: 3,
                            instruction: 0,
                        },
                        wait(None, false),
                    ],
                ),
                (
                    3,
                    vec![
                        Command::ReceiveMessage {
                            destination: Global(0),
                        },
                        Command::StartSample { sample: 1 },
                        Command::End,
                    ],
                ),
            ]);
            let EventKind::Notes { voices, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            let note = Note {
                max_voices: 2,
                ..voices[0]
            };
            *voices = if receiver_first { [2, 1] } else { [1, 2] }
                .map(|macro_id| Note { macro_id, ..note })
                .to_vec();
        });
        let synth = Synthesizer::default();
        let stream = synth.start(loaded, false)?;
        let output = render_block(&synth, &stream)?;
        assert!(
            output[..32]
                .iter()
                .flatten()
                .flatten()
                .all(|sample| *sample == 0)
        );
        assert!(output[32][0][0] > 0);
        assert_eq!(synth.0.lock().unwrap().random.variable(0), 42);
    }
    Ok(())
}

#[test]
fn every_exclusive_group_command_runs_in_standalone_and_shared_playback() -> Result<()> {
    let loaded = configured_fixture(vec![], true, |resources, score, _| {
        let member = |group| {
            vec![
                Command::ExclusiveGroup { group, kill: false },
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::StartSample { sample: 1 },
                wait(None, false),
            ]
        };
        resources.programs = BTreeMap::from([
            (1, member(1)),
            (2, member(2)),
            (
                3,
                vec![
                    wait(Some(2), false),
                    Command::ExclusiveGroup {
                        group: 1,
                        kill: true,
                    },
                    Command::ExclusiveGroup {
                        group: 2,
                        kill: true,
                    },
                    Command::End,
                ],
            ),
        ]);
        let EventKind::Notes { voices, .. } = &mut score.first_events[0].kind else {
            unreachable!()
        };
        let note = voices[0];
        *voices = (1..=3)
            .map(|macro_id| Note {
                macro_id,
                max_voices: 3,
                ..note
            })
            .collect();
    });
    for shared_clock in [false, true] {
        let synth = Synthesizer::default();
        let mut stream = if shared_clock {
            super::super::stream::Stream::in_synthesizer(loaded.clone(), false, &synth)?
        } else {
            super::super::stream::Stream::new(loaded.clone(), false)?
        };
        let mut heard = false;
        let mut completed = false;
        for _ in 0..8 {
            if shared_clock {
                for _ in 0..160 {
                    synth.advance()?;
                    if let Some(frame) = stream.shared_frame() {
                        heard |= frame.iter().flatten().any(|sample| *sample != 0);
                    } else {
                        completed = true;
                    }
                }
            } else if let Some(block) = stream.block(LiveControls::default())? {
                heard |= block.iter().flatten().flatten().any(|sample| *sample != 0);
            } else {
                completed = true;
            }
            if completed {
                break;
            }
        }
        assert!(heard, "group members must play before being stopped");
        assert!(
            completed,
            "both group kills must execute before the caller ends"
        );
    }
    Ok(())
}

#[test]
fn instruction_fuel_spans_host_yields_and_a_failed_entry_does_not_stop_healthy_audio() -> Result<()>
{
    let budget = crate::music_voice::INSTRUCTION_BUDGET;
    enum Fault {
        Instructions,
        HostYield,
        DeliveryBatch,
        PendingMessages,
        Tremolo,
    }
    let program = |fault: &Fault| {
        let commands = match fault {
            Fault::Instructions | Fault::HostYield => {
                let mut commands = vec![Command::Noop; budget - 2];
                commands.push(if matches!(fault, Fault::HostYield) {
                    Command::SendMessage {
                        target: crate::data::MessageTarget::Macro(1),
                        value: crate::data::Variable::Local(0),
                    }
                } else {
                    Command::Noop
                });
                commands.push(Command::Jump {
                    program: 1,
                    instruction: 0,
                });
                commands
            }
            Fault::DeliveryBatch | Fault::PendingMessages => {
                let count = if matches!(fault, Fault::DeliveryBatch) {
                    super::super::MESSAGE_BUDGET / 2 + 1
                } else {
                    super::super::MESSAGE_BUDGET / 2
                };
                let mut commands = vec![
                    Command::SendMessage {
                        target: crate::data::MessageTarget::Macro(1),
                        value: crate::data::Variable::Local(0),
                    };
                    count
                ];
                // The second case fills retained FIFOs across separate quanta,
                // while every individual delivery batch remains within budget.
                commands.extend([
                    wait(Some(1), false),
                    Command::Jump {
                        program: 1,
                        instruction: 0,
                    },
                ]);
                commands
            }
            Fault::Tremolo => vec![
                Command::Tremolo {
                    scale: 1,
                    modulation_scale: 0,
                },
                wait(None, false),
            ],
        };
        configured_fixture(vec![], false, |resources, score, tables| {
            resources.programs.insert(1, commands);
            if matches!(fault, Fault::Tremolo) {
                tables.modulation.tremolo = [f32::MAX; 5];
            }
            score.origin = ScoreOrigin::SoundEffect;
            let EventKind::Notes { source, voices, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id: 9 };
            if matches!(fault, Fault::DeliveryBatch) {
                // Sibling voices share their entry's delivery budget.
                voices[0].max_voices = 2;
                voices.push(voices[0]);
            }
        })
    };
    for fault in [
        Fault::Instructions,
        Fault::HostYield,
        Fault::DeliveryBatch,
        Fault::Tremolo,
    ] {
        let synth = Synthesizer::default();
        let reference = Synthesizer::default();
        let healthy = synth.start(fixture(vec![wait(None, false)], true), false)?;
        let expected = reference.start(fixture(vec![wait(None, false)], true), false)?;
        let failed = synth.start(program(&fault), false)?;
        let error = synth.advance().unwrap_err().to_string();
        let expected_error = match fault {
            Fault::Instructions | Fault::HostYield => {
                "music program 1 instruction budget exhausted"
            }
            Fault::DeliveryBatch => "audio message-delivery budget exceeded",
            Fault::PendingMessages => "audio pending-message budget exceeded",
            Fault::Tremolo => "nonfinite tremolo gain",
        };
        assert!(error.contains(expected_error), "{error}");
        reference.advance()?;
        assert_eq!(failed.frame(), None);
        assert_eq!(healthy.frame(), expected.frame());
        assert!(healthy.frame().unwrap()[0][0] > 0);
        for _ in 1..160 {
            synth.advance()?;
            reference.advance()?;
            assert_eq!(healthy.frame(), expected.frame());
            assert_eq!(failed.frame(), None);
        }
        // The failed source's one-voice limit must be available to a later cue.
        let replacement = configured_fixture(vec![Command::End], false, |_, score, _| {
            score.origin = ScoreOrigin::SoundEffect;
            let EventKind::Notes { source, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id: 9 };
        });
        let replacement = synth.start(replacement, false)?;
        synth.advance()?;
        assert!(replacement.frame().unwrap()[0][0] > 0);
    }
    // A full recipient stays bounded without poisoning later senders or
    // preventing delivery to another recipient of the same macro.
    let synth = Synthesizer::default();
    let held = synth.start(fixture(vec![wait(None, false)], true), false)?;
    let flooding = synth.start(program(&Fault::PendingMessages), false)?;
    let error = synth.advance().unwrap_err().to_string();
    assert!(error.contains("audio pending-message budget exceeded"));
    assert!(held.frame().unwrap()[0][0] > 0);
    assert!(flooding.frame().is_some());
    drop(flooding);
    for _ in 1..160 {
        synth.advance()?;
    }
    let sound = |commands, id, macro_id, receiver| {
        configured_fixture(commands, true, |resources, score, _| {
            let entry = resources.programs.remove(&1).unwrap();
            resources.programs.insert(macro_id, entry);
            if receiver {
                resources.programs.insert(
                    2,
                    vec![
                        Command::ReceiveMessage {
                            destination: crate::data::Variable::Global(1),
                        },
                        wait(None, false),
                    ],
                );
            }
            score.origin = ScoreOrigin::SoundEffect;
            let EventKind::Notes { source, voices, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id };
            voices[0].macro_id = macro_id;
        })
    };
    let receiver = synth.start(
        sound(
            vec![
                Command::MessageTrap {
                    program: 2,
                    instruction: 0,
                },
                wait(None, false),
            ],
            10,
            1,
            true,
        ),
        false,
    )?;
    let sender = synth.start(
        sound(
            vec![
                Command::SetVariable {
                    destination: crate::data::Variable::Local(0),
                    value: 77,
                },
                Command::SendMessage {
                    target: crate::data::MessageTarget::Macro(1),
                    value: crate::data::Variable::Local(0),
                },
                wait(None, false),
            ],
            11,
            3,
            false,
        ),
        false,
    )?;
    let error = synth.advance().unwrap_err().to_string();
    assert!(error.contains("audio pending-message budget exceeded"));
    assert_eq!(synth.0.lock().unwrap().random.variable(1), 77);
    for stream in [&held, &sender, &receiver] {
        assert!(stream.frame().unwrap()[0][0] > 0);
    }
    for _ in 1..320 {
        synth.advance()?;
        assert!(sender.frame().unwrap()[0][0] > 0);
        assert!(receiver.frame().unwrap()[0][0] > 0);
    }

    // One entry can use its entire allowance without charging the next sender.
    let synth = Synthesizer::default();
    let message = Command::SendMessage {
        target: crate::data::MessageTarget::Macro(99),
        value: crate::data::Variable::Local(0),
    };
    let mut full = vec![message; super::super::MESSAGE_BUDGET];
    full.push(wait(None, false));
    let full = synth.start(sound(full, 12, 4, false), false)?;
    let later = synth.start(
        sound(
            vec![
                message,
                Command::SetVariable {
                    destination: crate::data::Variable::Global(2),
                    value: 77,
                },
                wait(None, false),
            ],
            13,
            5,
            false,
        ),
        false,
    )?;
    for _ in 0..320 {
        synth.advance()?;
        assert!(full.frame().unwrap()[0][0] > 0);
        assert!(later.frame().unwrap()[0][0] > 0);
    }
    assert_eq!(synth.0.lock().unwrap().random.variable(2), 77);

    // A full budget of finite authored work remains valid.
    let valid = configured_fixture(vec![], false, |resources, _, _| {
        let mut commands = vec![Command::Noop; budget - 1];
        commands.push(Command::End);
        resources.programs.insert(1, commands);
    });
    let synth = Synthesizer::default();
    let _valid = synth.start(valid, false)?;
    synth.advance()?;
    Ok(())
}
