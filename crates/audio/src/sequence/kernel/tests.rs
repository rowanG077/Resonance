use super::*;
use crate::{
    data::{Command, Note},
    dls, mix, modulation, pitch, resample,
};
use std::collections::BTreeMap;

fn fixture() -> (Resources, Score, Tables) {
    (
        Resources {
            programs: BTreeMap::from([(1, vec![Command::End])]),
            samples: BTreeMap::new(),
        },
        Score {
            origin: crate::data::ScoreOrigin::Sequence,
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 2,
            end_tick: 10,
            has_master_track: false,
            tempos: vec![],
            controls: [Controls::default(); 16],
            first_events: vec![],
            loop_events: vec![],
        },
        Tables {
            mix: mix::Tables {
                volume: [1.; 129],
                alternate_volume: [1.; 129],
                pan: [1.; 4],
                volume_16_scale: 1.,
                controller_14_scale: 1.,
                pan_16_scale: 1.,
                spatial: None,
            },
            pitch: pitch::Tables {
                up: [1.; 128],
                down: [1.; 128],
                semitone: 1.05946,
            },
            dls: dls::Tables {
                attenuation: [0; 194],
                inverse: [0; 1024],
                sustain: [0.; 128],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        },
    )
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
    voice.set_random(control.clone());
    let mut channel = Controls::default();
    voice.prepare_commands(&mut channel).unwrap();
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
fn macro_mailboxes_preserve_handles_fifo_capacity_and_one_shot_traps() {
    use crate::data::{Arithmetic, Comparison, Operand, Variable::Global};
    let (mut bank, _, tables) = fixture();
    let wait = Command::Wait {
        milliseconds: None,
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    bank.programs.insert(
        1,
        vec![
            Command::VoiceHandle {
                destination: Global(0),
                child: false,
            },
            Command::SpawnMacro {
                program: 99,
                instruction: 0,
                key_offset: 0,
                priority: 9,
                max_voices: 255,
            },
            Command::VoiceHandle {
                destination: Global(1),
                child: true,
            },
            Command::Calculate {
                destination: Global(2),
                operation: Arithmetic::Add,
                left: Global(0),
                right: Operand::Constant(0),
            },
            Command::Branch {
                comparison: Comparison::Equal,
                left: Global(0),
                right: Global(2),
                invert: false,
                instruction: 7,
            },
            Command::SetVariable {
                destination: Global(3),
                value: 1,
            },
            Command::MessageTrap {
                program: 2,
                instruction: 0,
            },
            Command::Wait {
                milliseconds: Some(1),
                from_start: false,
                key_off: false,
                sample_end: false,
            },
            Command::ReceiveMessage {
                destination: Global(4),
            },
            Command::ReceiveMessage {
                destination: Global(5),
            },
            Command::ReceiveMessage {
                destination: Global(6),
            },
            Command::ReceiveMessage {
                destination: Global(7),
            },
            Command::ReceiveMessage {
                destination: Global(8),
            },
            wait,
        ],
    );
    bank.programs.insert(
        2,
        vec![
            Command::ReceiveMessage {
                destination: Global(10),
            },
            Command::SetVariable {
                destination: Global(9),
                value: 7,
            },
            wait,
        ],
    );
    let control = shared::Control::default();
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
    voice.handle = 0x8000_8001;
    voice.last_child = 123;
    voice.set_random(control.clone());
    // Delivering before startup retains the queue. Full delivery must neither
    // replace its oldest entry nor fire an installed message trap.
    for value in [0x1234_5678, -1, i32::MIN, 42] {
        voice.send_message(value);
    }
    voice.prepare_commands(&mut Controls::default()).unwrap();
    assert_eq!(
        (control.variable(0), control.variable(1)),
        (0x8000_8001u32 as i32, -1)
    );
    assert_eq!((control.variable(2), control.variable(3)), (-32767, 1));
    voice.send_message(99);
    voice.prepare_commands(&mut Controls::default()).unwrap();
    assert_eq!(control.variable(9), 0);
    for _ in 0..32 {
        voice.prepare_frame(Controls::default()).unwrap();
    }
    voice.prepare_commands(&mut Controls::default()).unwrap();
    assert_eq!(
        (4..9)
            .map(|index| control.variable(index))
            .collect::<Vec<_>>(),
        [0x1234_5678, -1, i32::MIN, 42, 0]
    );
    voice.send_message(0x7654_3210);
    voice.prepare_commands(&mut Controls::default()).unwrap();
    assert_eq!(control.variable(10), 0x7654_3210);
    assert_eq!(control.variable(9), 7);
    control.set_variable(9, 0);
    voice.send_message(55);
    voice.prepare_commands(&mut Controls::default()).unwrap();
    assert_eq!(control.variable(9), 0);
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
fn allocation_age_keeps_fractional_decay_and_add_age_restarts_from_integer_age() {
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
            Command::SetAge { value: 1 },
            Command::AgePeriod { milliseconds: 30 },
            wait(Some(10)),
            Command::AddAge { value: 2 },
            Command::Priority { value: 9 },
            Command::Priority { value: 3 },
            Command::Priority { value: 9 },
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
    for frame in 0..=320 {
        voice.prepare_frame(Controls::default()).unwrap();
        if frame == 160 {
            assert_eq!(voice.allocation_priority(), (9, 27648, 0));
            assert_eq!(voice.priority(), 9 << 24);
        }
        if frame % 160 == 159 {
            voice.mix_block(&mut [[[0; 2]; 3]; 160]).unwrap();
        }
    }
    assert_eq!(voice.allocation_priority(), (9, 60416, 2));
    assert_eq!(voice.priority(), (9 << 24) | 1);
}

#[test]
fn crossed_pre_end_and_terminal_events_run_before_same_callback_restart() {
    let (bank, mut song, tables) = fixture();
    song.initial_bpm_1024 = 1000 * 1024;
    song.controls[1].pitch_bend = 1000;
    song.first_events = vec![
        event(9, 0, EventKind::Volume { value: 31 }),
        event(10, 1, EventKind::PitchBend { value: 8192 }),
        event(10, 2, EventKind::Volume { value: 40 }),
    ];
    song.loop_events = vec![event(2, 2, EventKind::Volume { value: 70 })];
    let mut kernel =
        Kernel::new(&bank, &song, &tables, Some(320), true, ClockStart::Running).unwrap();
    kernel.time[0] = (8 << 16) | 49152;
    kernel.next_millisecond(LiveControls::default()).unwrap();
    assert_eq!(kernel.controls[0].paired[7], 127 << 7);
    assert!(kernel.time[0] >> 16 > u64::from(song.end_tick));
    kernel.next_millisecond(LiveControls::default()).unwrap();
    assert_eq!(kernel.controls[0].paired[7], 31 << 7);
    assert_eq!(kernel.controls[1].pitch_bend, 8192);
    assert_eq!(kernel.controls[2].paired[7], 70 << 7);
    assert_eq!(kernel.result.as_ref().unwrap().loop_starts, [32]);
}

#[test]
fn terminal_note_keeps_its_outgoing_clock_until_authored_note_off() {
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
    song.end_tick = 18;
    song.first_events = vec![event(
        18,
        0,
        EventKind::Notes {
            source: crate::data::VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            voices: vec![Note {
                macro_id: 1,
                key: 83,
                velocity: 90,
                pan: 64,
                priority: 1,
                max_voices: 64,
            }],
            length: 4,
        },
    )];
    let mut kernel =
        Kernel::new(&bank, &song, &tables, Some(320), true, ClockStart::Running).unwrap();
    kernel.time[0] = (18 << 16) | 16384;
    kernel.next_millisecond(LiveControls::default()).unwrap();
    assert_eq!(kernel.clock, 1);
    assert_eq!(kernel.voices[0].clock, 0);
    assert_eq!(kernel.voices[0].end_tick, Some(22));
    for _ in 0..4 {
        kernel.next_millisecond(LiveControls::default()).unwrap();
        assert_eq!(kernel.voices[0].end_tick, Some(22));
    }
    assert!(kernel.time[0] >> 16 >= 22);
    assert!(kernel.time[1] >> 16 < 22);
    kernel.next_millisecond(LiveControls::default()).unwrap();
    let result = kernel.result.as_ref().unwrap();
    assert_eq!(result.notes, 1);
    assert_eq!(result.voice_lifetimes[0].start_frame, 0);
    assert_eq!(result.voice_lifetimes[0].end_frame, Some(160));
}

#[test]
fn loop_carries_fraction_and_preserves_each_clocks_tempo_and_startup_seed() {
    for (master, startup) in [
        (true, ClockStart::Running),
        (false, ClockStart::Cold),
        (false, ClockStart::Running),
    ] {
        let (bank, mut song, tables) = fixture();
        song.initial_bpm_1024 = 100 * 1024;
        song.has_master_track = master;
        if master {
            song.tempos = vec![
                Tempo {
                    tick: 2,
                    bpm_1024: 100 * 1024,
                },
                Tempo {
                    tick: 9,
                    bpm_1024: 200 * 1024,
                },
            ];
        }
        let mut kernel = Kernel::new(&bank, &song, &tables, Some(320), true, startup).unwrap();
        let outgoing = (12 << 16) | 12345;
        let incoming = (2 << 16) | 12345;
        kernel.time[0] = outgoing;
        kernel.next_millisecond(LiveControls::default()).unwrap();
        let old_delta = tick_delta(if master { 200 * 1024 } else { 100 * 1024 });
        let new_delta = if matches!(startup, ClockStart::Cold) {
            0
        } else {
            tick_delta(100 * 1024)
        };
        assert_eq!(kernel.clock, 1);
        assert_eq!(kernel.bpm, 100 * 1024);
        assert_eq!(kernel.increments, [old_delta, new_delta]);
        assert_eq!(kernel.time, [outgoing + old_delta, incoming + new_delta]);
    }
}
