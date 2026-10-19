use super::*;
use crate::data::{Command, Note};
use std::collections::BTreeMap;

fn fixture() -> (Resources, Score, Tables) {
    let (mut resources, mut score, tables) = crate::package::tests::playback_data();
    resources.programs = BTreeMap::from([(1, vec![Command::End])]);
    resources.samples.clear();
    score.loop_start_tick = 2;
    score.end_tick = 10;
    (resources, score, tables)
}

fn event(tick: u32, channel: u8, kind: EventKind) -> Event {
    Event {
        tick,
        channel,
        kind,
    }
}

#[test]
fn controller_operands_preserve_fraction_clamp_and_lfo_read_only_semantics() {
    use crate::data::{
        Arithmetic, Controller, Operand,
        Variable::{Controller as Register, Global},
    };
    let (mut bank, _, tables) = fixture();
    let write = |controller, value| Command::SetVariable {
        destination: Register(controller),
        value,
    };
    let read = |controller, destination| Command::Calculate {
        destination: Global(destination),
        operation: Arithmetic::Add,
        left: Register(controller),
        right: Operand::Constant(0),
    };
    bank.programs.insert(
        1,
        vec![
            write(Controller::Paired(7), 7001),
            write(Controller::Paired(10), 8193),
            write(Controller::Paired(11), -1),
            write(Controller::PitchBend, 32767),
            write(Controller::Surround, 12345),
            write(Controller::Lfo, 1),
            read(Controller::Paired(7), 0),
            read(Controller::Paired(10), 1),
            read(Controller::Paired(11), 2),
            read(Controller::PitchBend, 3),
            read(Controller::Surround, 4),
            read(Controller::Lfo, 5),
            Command::End,
        ],
    );
    let control = shared::Control::default();
    let mut voice = Voice::new_at(
        &bank,
        &tables,
        Note {
            macro_id: 1,
            key: 60,
            velocity: 100,
            pan: 64,
            priority: 9,
            max_voices: 255,
        },
        0,
        control.clone(),
    )
    .unwrap();
    let mut channel = Controls::default();
    voice
        .prepare_commands(&mut channel, &mut {
            crate::music_voice::INSTRUCTION_BUDGET
        })
        .unwrap();
    assert_eq!(
        (0..6)
            .map(|index| control.variable(index))
            .collect::<Vec<_>>(),
        [7001, 8193, 0, 16383, 12345, 8192]
    );
    channel.set_coarse(7, 40);
    assert_eq!(channel.paired[7], (40 << 7) | (7001 & 127));
    channel.paired[1] = 255;
    channel.paired[11] = 1234;
    channel.post = [77, 88];
    let child = channel.child();
    assert_eq!(
        (
            child.paired[7],
            child.paired[10],
            child.pitch_bend,
            child.surround
        ),
        (channel.paired[7], 8193, 16383, 12345)
    );
    assert_eq!(
        (child.paired[1], child.paired[11], child.post),
        (0, 127 << 7, [77, 0])
    );
}

#[test]
fn macro_mailboxes_deliver_every_message_in_order_and_wake_a_trap_once() {
    use crate::data::Variable::Global;
    let (mut bank, _, tables) = fixture();
    let wait = Command::Wait {
        milliseconds: None,
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    let messages = [11, -7, 23, 5, 91, 42];
    let mut receive: Vec<_> = (0..messages.len() as u8)
        .map(|index| Command::ReceiveMessage {
            destination: Global(index),
        })
        .collect();
    receive.extend([
        Command::MessageTrap {
            program: 2,
            instruction: 0,
        },
        wait,
    ]);
    bank.programs.insert(1, receive);
    bank.programs.insert(
        2,
        vec![
            Command::ReceiveMessage {
                destination: Global(6),
            },
            Command::SetVariable {
                destination: Global(7),
                value: 1,
            },
            wait,
        ],
    );
    let control = shared::Control::default();
    let mut voice = Voice::new_at(
        &bank,
        &tables,
        Note {
            macro_id: 1,
            key: 60,
            velocity: 100,
            pan: 64,
            priority: 9,
            max_voices: 1,
        },
        0,
        control.clone(),
    )
    .unwrap();
    for value in messages {
        voice.send_message(value).unwrap();
    }
    voice
        .prepare_commands(&mut Controls::default(), &mut {
            crate::music_voice::INSTRUCTION_BUDGET
        })
        .unwrap();
    for (index, value) in messages.into_iter().enumerate() {
        assert_eq!(control.variable(index as u8), value);
    }
    voice.send_message(99).unwrap();
    assert_eq!(control.variable(6), 0);
    voice
        .prepare_commands(&mut Controls::default(), &mut {
            crate::music_voice::INSTRUCTION_BUDGET
        })
        .unwrap();
    assert_eq!((control.variable(6), control.variable(7)), (99, 1));
    control.set_variable(7, 0);
    voice.send_message(55).unwrap();
    voice
        .prepare_commands(&mut Controls::default(), &mut {
            crate::music_voice::INSTRUCTION_BUDGET
        })
        .unwrap();
    assert_eq!((control.variable(6), control.variable(7)), (99, 0));
}

#[test]
fn macro_arithmetic_saturates_signed_values_and_validates_registers_and_branches() {
    use crate::data::{Arithmetic::*, Comparison, Operand, Variable};
    for (operation, left, right, expected) in [
        (Add, 32767, 1, 32767),
        (Add, -32768, -1, -32768),
        (Subtract, -32768, 1, -32768),
        (Subtract, -5, -3, -2),
        (Multiply, -200, 200, -32768),
        (Multiply, -200, -200, 32767),
        (Divide, -32768, -1, 32767),
        (Divide, -7, 2, -3),
        (Divide, 12, 0, 0),
    ] {
        assert_eq!(operation.evaluate(left, right), expected);
    }
    let (mut resources, _, _) = fixture();
    for command in [
        Command::SetVariable {
            destination: Variable::Global(16),
            value: 1,
        },
        Command::Calculate {
            destination: Variable::Local(0),
            operation: Add,
            left: Variable::Local(16),
            right: Operand::Constant(0),
        },
        Command::Branch {
            comparison: Comparison::Less,
            left: Variable::Local(0),
            right: Variable::Local(1),
            invert: true,
            instruction: 1,
        },
    ] {
        resources.programs.insert(1, vec![command]);
        assert!(resources.validate().is_err());
    }
}

#[test]
fn allocation_priority_ages_and_authored_updates_take_effect() {
    let (mut bank, _, tables) = fixture();
    let wait = |milliseconds| Command::Wait {
        milliseconds,
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    bank.programs.insert(
        1,
        vec![
            Command::SetAge { value: 1000 },
            Command::AgePeriod { milliseconds: 30 },
            wait(Some(10)),
            Command::AddAge { value: 2000 },
            Command::Priority { value: 3 },
            wait(None),
        ],
    );
    let mut voice = Voice::new(
        &bank,
        &tables,
        Note {
            macro_id: 1,
            key: 60,
            velocity: 100,
            pan: 64,
            priority: 9,
            max_voices: 255,
        },
    )
    .unwrap();
    let deadline = crate::volume::frames_from_millis(10).unwrap().div_ceil(32) * 32;
    let initial_age = voice.allocation_priority().1;
    for frame in 0..=deadline {
        crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
        if frame == 160 {
            assert!(voice.allocation_priority().1 < initial_age);
        }
        if frame % 160 == 159 {
            voice.mix_block(&mut [[[0; 2]; 3]; 160]);
        }
    }
    assert_eq!(voice.allocation_priority().0, 3);
    assert!(voice.allocation_priority().1 > 1000);
}

#[test]
fn age_periods_decay_monotonically_to_their_native_frame_endpoints() {
    for milliseconds in [0, 117, 118] {
        let (mut bank, _, tables) = fixture();
        bank.programs.insert(
            1,
            vec![
                Command::SetAge { value: 60_000 },
                Command::AgePeriod { milliseconds },
                Command::Wait {
                    milliseconds: None,
                    from_start: false,
                    key_off: false,
                    sample_end: false,
                },
            ],
        );
        let mut voice = Voice::new(
            &bank,
            &tables,
            Note {
                macro_id: 1,
                key: 60,
                velocity: 100,
                pan: 64,
                priority: 9,
                max_voices: 1,
            },
        )
        .unwrap();
        voice
            .prepare_commands(&mut Controls::default(), &mut {
                crate::music_voice::INSTRUCTION_BUDGET
            })
            .unwrap();
        let frames = crate::volume::frames_from_millis(u64::from(milliseconds)).unwrap();
        let mut previous = voice.allocation_priority().1;
        for frame in 0..frames.max(160) {
            crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
            let age = voice.allocation_priority().1;
            assert!(age <= previous);
            if milliseconds == 0 {
                assert_eq!(age, 60_000);
            } else if frame + 1 < frames {
                assert!(age > 0);
            } else {
                assert_eq!(age, 0);
            }
            previous = age;
        }
    }
}

#[test]
fn smallest_tempos_eventually_dispatch_the_next_tick() -> Result<()> {
    for bpm in [1, 2] {
        let (bank, mut song, tables) = fixture();
        song.initial_bpm_1024 = bpm;
        song.first_events = vec![event(1, 0, EventKind::Volume { value: 31 })];
        song.validate(&bank)?;
        let mut kernel = Kernel::new(
            &bank,
            &song,
            &tables,
            false,
            shared::Control::default(),
            false,
        )?;
        let frames = TICK_DENOMINATOR.div_ceil(u128::from(bpm) * TICKS_PER_BEAT) as u64;
        kernel.clock.advance(frames - 1, bpm);
        kernel.prepare_controls(LiveControls::default())?;
        assert_eq!(kernel.controls[0].paired[7], 127 << 7);
        kernel.clock.advance(1, bpm);
        kernel.prepare_controls(LiveControls::default())?;
        assert_eq!(kernel.controls[0].paired[7], 31 << 7);
    }
    Ok(())
}

#[test]
fn terminal_events_precede_loop_entry_and_tempo_restarts_from_the_loop_position() -> Result<()> {
    let (bank, mut song, tables) = fixture();
    song.initial_bpm_1024 = 120 * 1024;
    song.end_tick = 8;
    song.first_events = vec![
        event(7, 0, EventKind::Volume { value: 31 }),
        event(8, 1, EventKind::PitchBend { value: 8192 }),
        event(8, 2, EventKind::Volume { value: 40 }),
    ];
    song.loop_events = vec![event(2, 2, EventKind::Volume { value: 70 })];
    song.tempos = vec![Tempo {
        tick: 4,
        bpm_1024: 240 * 1024,
    }];
    let mut kernel = Kernel::new(
        &bank,
        &song,
        &tables,
        true,
        shared::Control::default(),
        true,
    )?;
    let mut changed_tempo = false;
    for _ in 0..20 {
        kernel.prepare_controls(LiveControls::default())?;
        kernel.next_quantum()?;
        changed_tempo |= kernel.bpm == 240 * 1024;
        if !kernel.result.as_ref().unwrap().loop_starts.is_empty() {
            break;
        }
    }
    assert!(changed_tempo);
    assert!(!kernel.result.as_ref().unwrap().loop_starts.is_empty());
    assert_eq!(kernel.controls[0].paired[7], 31 << 7);
    assert_eq!(kernel.controls[1].pitch_bend, 8192);
    assert_eq!(kernel.controls[2].paired[7], 70 << 7);
    assert_eq!(kernel.bpm, 120 * 1024);
    Ok(())
}

#[test]
fn held_notes_keep_absolute_deadlines_across_multiple_loops() -> Result<()> {
    let (mut bank, mut song, tables) = fixture();
    bank.programs.insert(
        1,
        vec![
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::End,
        ],
    );
    song.initial_bpm_1024 = 160_000;
    song.loop_start_tick = 0;
    song.end_tick = 4;
    song.first_events = vec![event(
        0,
        0,
        EventKind::Notes {
            source: crate::data::VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 90,
                pan: 64,
                priority: 1,
                max_voices: 64,
            }],
            length: 20,
        },
    )];
    let reverbs = [[0., 0., 1., 0., 0.]; 2];
    let package = Arc::new(crate::package::Loaded::new(bank, song, tables, reverbs)?);
    let result = render_preview(package, reverbs, 1600)?;
    assert_eq!(result.notes, 1);
    assert!(result.loop_starts.len() >= 5);
    let note = &result.voice_lifetimes[0];
    assert_eq!(note.start_frame, 0);
    let end = note
        .end_frame
        .expect("held note never received its note-off");
    let deadline = (20 * crate::SOURCE_RATE).div_ceil(1000);
    assert!((deadline..=deadline + 32).contains(&end));
    Ok(())
}

#[test]
fn sequence_pause_retires_notes_preserves_cursor_and_resumes_on_the_shared_clock() -> Result<()> {
    let (mut bank, mut song, tables) = fixture();
    bank.programs.insert(
        1,
        vec![Command::Wait {
            milliseconds: None,
            from_start: false,
            key_off: false,
            sample_end: false,
        }],
    );
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 100,
        pan: 64,
        priority: 9,
        max_voices: 255,
    };
    song.end_tick = 100;
    song.first_events = vec![
        event(
            0,
            0,
            EventKind::Notes {
                source: crate::data::VoiceSource::Sequence {
                    group: 0,
                    program: 0,
                    drums: false,
                },
                voices: vec![note],
                length: 50,
            },
        ),
        event(
            10,
            0,
            EventKind::Notes {
                source: crate::data::VoiceSource::Sequence {
                    group: 0,
                    program: 0,
                    drums: false,
                },
                voices: vec![note],
                length: 50,
            },
        ),
    ];
    let mut kernel = Kernel::new(
        &bank,
        &song,
        &tables,
        false,
        shared::Control::default(),
        false,
    )?;
    kernel.prepare_controls(LiveControls::default())?;
    assert_eq!(kernel.voices.len(), 1);
    let time = kernel.clock.0;
    let next = kernel.events.peek().unwrap().tick;
    kernel.pause(true);
    assert!(kernel.voices.is_empty());
    for _ in 0..50 {
        kernel.prepare_controls(LiveControls::default())?;
        kernel.next_quantum()?;
        if kernel.frame.is_multiple_of(160) {
            kernel.finish_block(&mut [[[0; 2]; 3]; 160]);
        }
    }
    assert_eq!(kernel.clock.0, time);
    assert_eq!(kernel.events.peek().unwrap().tick, next);
    assert!(!kernel.ended);
    kernel.pause(false);
    for _ in 0..20 {
        kernel.prepare_controls(LiveControls::default())?;
        kernel.next_quantum()?;
        // A shared-clock caller mixes each block before preparing the next.
        if kernel.frame.is_multiple_of(160) {
            kernel.finish_block(&mut [[[0; 2]; 3]; 160]);
        }
    }
    assert_eq!(
        kernel.voices.len(),
        1,
        "held notes are not restarted; the next authored note is issued"
    );
    assert!(kernel.clock.0 > time);
    Ok(())
}
