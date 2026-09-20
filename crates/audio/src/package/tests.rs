use super::*;
use crate::{dls, mix, modulation, music_voice::Controls, pitch, resample};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (Fixture, serde_json::Value) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let root = Fixture(std::env::temp_dir().join(format!(
        "resonance-music-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&root.0).unwrap();
    let path = root.0.join("sample.wav");
    let mut wave = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 32000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for value in [10i16, 20, 30, 40, 50, 60] {
        wave.write_sample(value).unwrap();
    }
    wave.finalize().unwrap();
    let package = Package {
        version: VERSION,
        programs: BTreeMap::from([(1, vec![Command::StartSample { sample: 2 }, Command::End])]),
        samples: BTreeMap::from([(
            2,
            SampleAsset {
                path: "sample.wav".into(),
                sha256: format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                key: 60,
                rate: 32000,
                first_frames: 4,
                loop_start: 2,
                loop_length: 2,
            },
        )]),
        score: Score {
            origin: crate::data::ScoreOrigin::Sequence,
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 0,
            end_tick: 100,
            has_master_track: false,
            tempos: vec![],
            controls: [Controls::default(); 16],
            first_events: vec![],
            loop_events: vec![],
        },
        tables: Tables {
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
        reverbs: [[0., 0., 1., 0., 0.]; 2],
    };
    (root, serde_json::to_value(package).unwrap())
}

fn load(root: &Fixture, value: &serde_json::Value) -> Result<Loaded> {
    fs::write(
        root.0.join("music.json"),
        serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
    Package::load(&root.0, "music.json")
}

#[test]
fn noops_preserve_playing_sample_phase_envelope_and_waits() {
    use crate::{
        data::{Interpolation, Note},
        music_voice::Voice,
    };
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    let wait = Command::Wait {
        milliseconds: Some(7),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    let program = vec![
        Command::Noop,
        Command::Interpolation {
            mode: Interpolation::Direct,
            coefficients: 0,
        },
        Command::StartSample { sample: 2 },
        wait,
        Command::Noop,
        Command::Noop,
        wait,
        Command::StopSample,
        Command::End,
    ];
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 127,
        pan: 64,
        priority: 1,
        max_voices: 1,
    };
    let mut outputs = Vec::new();
    for keep_noops in [false, true] {
        loaded.resources.programs.insert(
            1,
            program
                .iter()
                .copied()
                .filter(|command| keep_noops || !matches!(command, Command::Noop))
                .collect(),
        );
        loaded.resources.validate().unwrap();
        let mut voice = Voice::new(&loaded.resources, &loaded.tables, note).unwrap();
        let mut pcm = Vec::new();
        let mut done = Vec::new();
        for frame in 0..800 {
            voice.prepare_frame(Controls::default()).unwrap();
            done.push(voice.is_done());
            if frame % 160 == 159 {
                let mut block = [[[0; 2]; 3]; 160];
                voice.mix_block(&mut block).unwrap();
                pcm.extend(block.into_iter().flatten().flatten());
            }
        }
        assert!(pcm.iter().any(|&sample| sample != 0));
        assert!(done.last().unwrap());
        outputs.push((pcm, done));
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn timed_pitch_steps_loop_and_key_off_without_losing_the_wait() {
    use crate::{data::Note, music_voice::Voice};
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 100,
        pan: 64,
        priority: 1,
        max_voices: 1,
    };
    for (count, release_at, finish) in [(2, None, 960), (u16::MAX, Some(480), 640)] {
        loaded.resources.programs.insert(
            1,
            vec![
                Command::PitchOffset {
                    from_original: true,
                    semitones: 0,
                    cents: 0,
                    wait_ms: 10,
                    from_start: false,
                },
                Command::Loop {
                    instruction: 0,
                    count,
                    key_off: true,
                    sample_end: false,
                },
                Command::End,
            ],
        );
        loaded.resources.validate().unwrap();
        let mut voice = Voice::new(&loaded.resources, &loaded.tables, note).unwrap();
        for frame in 0..=finish {
            if release_at == Some(frame) {
                voice.key_off().unwrap();
            }
            voice.prepare_frame(Controls::default()).unwrap();
            assert_eq!(voice.is_done(), frame == finish, "frame {frame}");
            if frame % 160 == 159 {
                voice.mix_block(&mut [[[0; 2]; 3]; 160]).unwrap();
            }
        }
    }
    loaded.resources.programs.insert(
        1,
        vec![Command::Loop {
            instruction: 0,
            count: u16::MAX,
            key_off: false,
            sample_end: false,
        }],
    );
    let mut voice = Voice::new(&loaded.resources, &loaded.tables, note).unwrap();
    assert!(
        voice
            .prepare_frame(Controls::default())
            .unwrap_err()
            .to_string()
            .contains("instruction budget")
    );
}

fn voice_pcm(
    loaded: &mut Loaded,
    commands: Vec<Command>,
    tempos: &[(u32, u32)],
    key_off: Option<u32>,
) -> (u32, Vec<i32>) {
    loaded.resources.programs.insert(1, commands);
    loaded.resources.validate().unwrap();
    let note = crate::data::Note {
        macro_id: 1,
        key: 60,
        velocity: 100,
        pan: 64,
        priority: 1,
        max_voices: 1,
    };
    let mut voice =
        crate::music_voice::Voice::new(&loaded.resources, &loaded.tables, note).unwrap();
    let mut pcm = Vec::new();
    for frame in 0..64000 {
        for &(_, bpm) in tempos.iter().filter(|(at, _)| *at == frame) {
            voice.set_tempo(bpm);
        }
        if key_off == Some(frame) {
            voice.key_off().unwrap();
        }
        voice.prepare_frame(Controls::default()).unwrap();
        if voice.is_done() || frame % 160 == 159 {
            let mut block = [[[0; 2]; 3]; 160];
            voice.mix_block(&mut block).unwrap();
            let count = if voice.is_done() { frame % 160 } else { 160 };
            pcm.extend(block[..count as usize].iter().flat_map(|frame| frame[0]));
        }
        if voice.is_done() {
            return (frame, pcm);
        }
    }
    panic!("offline voice exceeded its bounded duration");
}

#[test]
fn identical_interpolation_preserves_pcm_for_live_and_finished_sources() {
    use crate::data::Interpolation;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.tables.coefficients.0[2] =
        std::array::from_fn(|phase| [0, 0, 32767 - phase as i16 * 128, phase as i16 * 128]);
    let wait = |milliseconds| Command::Wait {
        milliseconds: Some(milliseconds),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    for finished in [false, true] {
        if finished {
            let sample = std::sync::Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
            sample.loop_length = 0;
            sample.loop_pcm.clear();
        }
        for mode in [
            Interpolation::Polyphase,
            Interpolation::Linear,
            Interpolation::Direct,
        ] {
            let select = Command::Interpolation {
                mode,
                coefficients: 2,
            };
            let render = |loaded: &mut Loaded, repeat| {
                voice_pcm(
                    loaded,
                    vec![
                        select,
                        Command::SetNote {
                            key: 60,
                            cents: 50,
                            wait_ms: 0,
                            from_start: false,
                        },
                        Command::StartSample { sample: 2 },
                        wait(7),
                        repeat,
                        wait(6),
                        repeat,
                        wait(7),
                        Command::End,
                    ],
                    &[],
                    None,
                )
            };
            let reference = render(&mut loaded, Command::Noop);
            let repeated = render(&mut loaded, select);
            assert!(reference.1.iter().any(|&sample| sample != 0));
            assert_eq!(repeated, reference, "{mode:?}, finished={finished}");
            if finished {
                assert!(repeated.1[7 * 32 * 2..].iter().all(|&sample| sample == 0));
            }
        }
    }
}

#[test]
fn changed_interpolation_is_rejected_for_retained_sources() {
    use crate::{
        data::{Interpolation, Note},
        music_voice::Voice,
    };
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 100,
        pan: 64,
        priority: 1,
        max_voices: 1,
    };
    for finished in [false, true] {
        if finished {
            let sample = std::sync::Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
            sample.loop_length = 0;
            sample.loop_pcm.clear();
        }
        for (mode, coefficients) in [(Interpolation::Linear, 2), (Interpolation::Polyphase, 1)] {
            loaded.resources.programs.insert(
                1,
                vec![
                    Command::Interpolation {
                        mode: Interpolation::Polyphase,
                        coefficients: 2,
                    },
                    Command::StartSample { sample: 2 },
                    Command::Wait {
                        milliseconds: Some(7),
                        from_start: false,
                        key_off: false,
                        sample_end: false,
                    },
                    Command::Interpolation { mode, coefficients },
                    Command::End,
                ],
            );
            let mut voice = Voice::new(&loaded.resources, &loaded.tables, note).unwrap();
            for frame in 0..7 * 32 {
                voice.prepare_frame(Controls::default()).unwrap();
                if frame % 160 == 159 {
                    voice.mix_block(&mut [[[0; 2]; 3]; 160]).unwrap();
                }
            }
            assert_eq!(voice.source_active(), !finished);
            let error = voice.prepare_frame(Controls::default()).unwrap_err();
            assert!(error.to_string().contains("changing an active source mode"));
        }
    }
}

#[test]
fn custom_volume_curves_and_overlapping_fades_match_native_targets() {
    use crate::data::{Interpolation, VolumeCurve};
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.tables.mix.volume = std::array::from_fn(|index| index as f32 / 128.0);
    loaded.tables.mix.volume_16_scale = 1.0 / (127 << 16) as f32;
    loaded.tables.mix.controller_14_scale = 1.0 / 16384.0;
    let sample = std::sync::Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
    sample.pcm.fill(16000);
    sample.loop_pcm.fill(16000);
    let mut curve = VolumeCurve([0; 128]);
    curve.0[63] = 100;
    curve.0[64] = 40;
    curve.0[100] = 180;
    let set = |offset| Command::SetVolume {
        factor: 0,
        offset,
        curve: None,
        from_velocity: false,
    };
    let mut cases = vec![
        (
            vec![Command::SetVolume {
                factor: 64,
                offset: 0,
                curve: Some(curve),
                from_velocity: false,
            }],
            vec![set(40)],
        ),
        (
            vec![
                Command::SetVolume {
                    factor: 127,
                    offset: 0,
                    curve: Some(curve),
                    from_velocity: true,
                },
                Command::VolumeControl { value: 4096 },
            ],
            vec![set(90), Command::VolumeControl { value: 8192 }],
        ),
    ];
    for milliseconds in [0, 20] {
        for from_silence in [false, true] {
            cases.push((
                vec![Command::FadeVolume {
                    factor: 64,
                    offset: 0,
                    curve: Some(curve),
                    milliseconds,
                    from_silence,
                }],
                vec![Command::FadeVolume {
                    factor: 0,
                    offset: 70,
                    curve: None,
                    milliseconds,
                    from_silence,
                }],
            ));
        }
    }
    // An instant setter while a fade is running must not reset its accumulator.
    let fade = Command::FadeVolume {
        factor: 64,
        offset: 0,
        curve: Some(curve),
        milliseconds: 20,
        from_silence: false,
    };
    let wait = Command::Wait {
        milliseconds: Some(5),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    cases.push((vec![fade, wait, set(110)], vec![fade, wait]));
    // A new fade does start from the current volume, including an intervening setter.
    cases.push((
        vec![fade, wait, set(110), fade],
        vec![
            fade,
            wait,
            set(110),
            Command::FadeVolume {
                factor: 0,
                offset: 0,
                curve: None,
                milliseconds: 20,
                from_silence: false,
            },
        ],
    ));
    for (actual, expected) in cases {
        let render = |loaded: &mut Loaded, commands: Vec<_>| {
            let mut program = vec![
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::StartSample { sample: 2 },
                set(127),
                Command::VolumeControl { value: 8192 },
            ];
            program.extend(commands);
            program.extend([
                Command::Wait {
                    milliseconds: Some(40),
                    from_start: false,
                    key_off: false,
                    sample_end: false,
                },
                Command::End,
            ]);
            voice_pcm(loaded, program, &[], None)
        };
        let actual = render(&mut loaded, actual);
        let expected = render(&mut loaded, expected);
        assert!(actual.1.iter().any(|&sample| sample != 0));
        assert_eq!(actual, expected);
    }
}

#[test]
fn pan_ramp_pcm_matches_native_control_steps_and_surround_is_inert_in_stereo() {
    use crate::data::{Interpolation, PanAxis};
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.tables.mix.pan = [0., 0.5, 1., 1.];
    loaded.tables.mix.pan_16_scale = 1. / (63 << 16) as f32;
    let sample = std::sync::Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
    sample.pcm.fill(12000);
    sample.loop_pcm.fill(12000);
    let wait = |milliseconds| Command::Wait {
        milliseconds: Some(milliseconds),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    let start = [
        Command::Interpolation {
            mode: Interpolation::Direct,
            coefficients: 0,
        },
        Command::StartSample { sample: 2 },
    ];
    let ramp = Command::PanRamp {
        axis: PanAxis::Pan,
        initial: 0,
        delta: 90,
        milliseconds: 90,
    };
    let surround = Command::PanRamp {
        axis: PanAxis::Surround,
        initial: 127,
        delta: -127,
        milliseconds: 30,
    };
    for lead in [0, 5] {
        let start = start.into_iter().chain([wait(lead)]).collect::<Vec<_>>();
        let actual = voice_pcm(
            &mut loaded,
            start
                .iter()
                .copied()
                .chain([ramp, wait(105), Command::End])
                .collect(),
            &[],
            None,
        );
        // Waking at 5ms retains the previous pitch timestamp: the new ramp
        // immediately consumes those 5ms, then advances every 15ms.
        let reference = start
            .iter()
            .copied()
            .chain((0..=6).flat_map(|step| {
                [
                    Command::PanRamp {
                        axis: PanAxis::Pan,
                        initial: (lead as u8 + step * 15).min(90),
                        delta: 0,
                        milliseconds: 0,
                    },
                    wait(15),
                ]
            }))
            .chain([Command::End])
            .collect();
        assert_eq!(
            actual,
            voice_pcm(&mut loaded, reference, &[], None),
            "lead {lead}"
        );
        assert!(actual.1.chunks_exact(2).any(|frame| frame[0] != frame[1]));
        assert_eq!(
            actual,
            voice_pcm(
                &mut loaded,
                start
                    .into_iter()
                    .chain([ramp, surround, wait(105), Command::End])
                    .collect(),
                &[],
                None
            )
        );
    }
    // A half-unit must reach gain calculation rather than rounding to either MIDI value.
    let gains = |pan| {
        loaded.tables.mix.gains_for(mix::Parameters {
            volume: 100 << 16,
            controller: 16383,
            pan,
            post: [0; 2],
            scale: 1.,
            group_volume: 1.,
            aux_a: 127,
            alternate: false,
            interaural_delay: false,
        })
    };
    let middle = gains((64 << 16) + 32768);
    assert_ne!(middle, gains(64 << 16));
    assert_ne!(middle, gains(65 << 16));
}

fn random_cue(root: &Fixture, value: &serde_json::Value, before_ms: u16, wait: Command) -> Loaded {
    use crate::data::{Event, EventKind, Interpolation, Note};
    let mut loaded = load(root, value).unwrap();
    loaded.score.origin = crate::data::ScoreOrigin::SoundEffect;
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::Wait {
                milliseconds: Some(before_ms),
                from_start: false,
                key_off: false,
                sample_end: false,
            },
            wait,
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: Some(5),
                from_start: false,
                key_off: false,
                sample_end: false,
            },
            Command::StopSample,
            Command::End,
        ],
    );
    loaded.score.first_events.push(Event {
        tick: 0,
        channel: 0,
        kind: EventKind::Notes {
            source: crate::data::VoiceSource::SoundEffect { id: 0 },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 100,
                pan: 64,
                priority: 1,
                max_voices: 255,
            }],
            length: 99,
        },
    });
    loaded
}

#[test]
fn random_notes_branches_and_loops_preserve_shared_draws_and_pcm() {
    use crate::{
        data::Interpolation,
        sequence::{LiveControls, shared::Synthesizer, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let note = |key, cents| Command::SetNote {
        key,
        cents,
        wait_ms: 0,
        from_start: false,
    };
    let mut cases = vec![
        (
            Command::RandomNote {
                low: 67,
                high: 60,
                cents: -7,
                random_cents: false,
                relative: false,
            },
            note(65, -7),
            1,
        ),
        (
            Command::RandomNote {
                low: 53,
                high: 66,
                cents: -7,
                random_cents: true,
                relative: false,
            },
            note(60, 19),
            2,
        ),
        (
            Command::RandomNote {
                low: 100,
                high: 100,
                cents: 0,
                random_cents: false,
                relative: true,
            },
            note(117, 0),
            1,
        ),
        (
            Command::RandomNote {
                low: 200,
                high: 210,
                cents: 0,
                random_cents: false,
                relative: false,
            },
            note(77, 0),
            1,
        ),
        (
            Command::RandomNote {
                low: 60,
                high: 60,
                cents: 0,
                random_cents: false,
                relative: false,
            },
            note(60, 0),
            1,
        ),
        (
            Command::RandomBranch {
                minimum: 117,
                program: 1,
                instruction: 4,
            },
            Command::Jump {
                program: 1,
                instruction: 4,
            },
            1,
        ),
        (
            Command::RandomBranch {
                minimum: 118,
                program: 1,
                instruction: 4,
            },
            Command::Noop,
            1,
        ),
        (
            Command::RandomBranch {
                minimum: 0,
                program: 99,
                instruction: 100,
            },
            Command::Noop,
            1,
        ),
    ];
    for bound in [1, 17] {
        cases.push((
            Command::RandomLoop {
                instruction: 1,
                count: bound,
                key_off: false,
                sample_end: false,
            },
            Command::Wait {
                milliseconds: Some(54389 % bound + 1),
                from_start: false,
                key_off: false,
                sample_end: false,
            },
            1,
        ));
    }
    for (command, reference, draws) in cases {
        let make = |command| {
            let before = u16::from(matches!(command, Command::RandomLoop { .. }));
            let mut loaded = random_cue(&root, &value, before, command);
            loaded.resources.programs.get_mut(&1).unwrap()[0] = Command::Interpolation {
                mode: Interpolation::Linear,
                coefficients: 0,
            };
            loaded.tables.pitch.up = std::array::from_fn(|i| 2f32.powf(i as f32 / 12.));
            loaded.tables.pitch.down = std::array::from_fn(|i| 2f32.powf(-(i as f32) / 12.));
            loaded
        };
        let synth = Synthesizer::default();
        let actual = Stream::in_synthesizer(Arc::new(make(command)), false, &synth).unwrap();
        let mut expected = Stream::new(Arc::new(make(reference)), false).unwrap();
        for _ in 0..4 {
            let block = expected.block(LiveControls::default()).unwrap();
            for frame in 0..160 {
                synth.advance().unwrap();
                assert_eq!(
                    actual.shared_frame().unwrap(),
                    block.as_ref().map(|b| b[frame]),
                    "{command:?}"
                );
            }
        }
        assert_eq!(synth.random_state().1, draws, "{command:?}");
        assert_eq!(
            synth.random_state().0,
            if draws == 1 { 0xa8351d63 } else { 509449289 }
        );
    }
}

#[test]
fn shared_random_cues_order_waits_across_streams_and_preserve_production_pcm() {
    use crate::sequence::{LiveControls, shared::Synthesizer, stream::Stream};
    use std::sync::Arc;
    let (root, value) = fixture();
    let random = Command::RandomWait {
        upper_ms: 17,
        key_off: false,
    };
    let fixed = |duration| Command::Wait {
        milliseconds: Some(duration),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    // The source seed1 yields54389,30289: modulo17 gives6,12. The earlier
    // deadline must draw first even when its stream was registered last.
    // Equal deadlines preserve the order in which waits were scheduled.
    for before in [[4, 1], [1, 1]] {
        let synth = Synthesizer::default();
        let players = before.map(|ms| {
            Stream::in_synthesizer(
                Arc::new(random_cue(&root, &value, ms, random)),
                false,
                &synth,
            )
            .unwrap()
        });
        let mut expected = [(before[0], 12), (before[1], 6)].map(|(ms, delay)| {
            Stream::new(Arc::new(random_cue(&root, &value, ms, fixed(delay))), false).unwrap()
        });
        let mut actual = [Vec::new(), Vec::new()];
        let mut reference = [Vec::new(), Vec::new()];
        for block in 0..8 {
            let mut active = [false; 2];
            for i in 0..2 {
                let samples = expected[i].block(LiveControls::default()).unwrap();
                active[i] = samples.is_some();
                reference[i].extend(samples.unwrap_or_else(|| vec![[[0; 2]; 3]; 160]));
            }
            for _ in 0..160 {
                synth.advance().unwrap();
                for i in 0..2 {
                    let sample = players[i].shared_frame().unwrap();
                    assert_eq!(
                        sample.is_some(),
                        active[i],
                        "cue {i}, block {block}: lifetime"
                    );
                    actual[i].push(sample.unwrap_or([[0; 2]; 3]));
                }
            }
            if block == 0 {
                assert_eq!(synth.random_state(), (509449289, 2));
            }
        }
        assert!(
            actual
                .iter()
                .all(|pcm| pcm.iter().flatten().flatten().any(|&v| v != 0))
        );
        assert_eq!(actual, reference);
        assert_eq!(synth.random_state(), (509449289, 2));
        drop(players);
        let next = Stream::in_synthesizer(
            Arc::new(random_cue(&root, &value, 0, random)),
            false,
            &synth,
        )
        .unwrap();
        synth.advance().unwrap();
        assert!(next.started());
        assert_eq!(
            synth.random_state(),
            (4078542139, 3),
            "cue turnover must not reseed the synthesizer"
        );
    }
}

mod shared_scheduler {
    use super::*;
    use crate::{
        data::{Event, EventKind, Interpolation, ScoreOrigin},
        sequence::{LiveControls, shared::Synthesizer, stream::Stream},
    };
    use std::sync::Arc;

    fn wait(ms: Option<u16>, key_off: bool, sample_end: bool) -> Command {
        Command::Wait {
            milliseconds: ms,
            from_start: false,
            key_off,
            sample_end,
        }
    }

    fn cue(root: &Fixture, value: &serde_json::Value, programs: Vec<Vec<Command>>) -> Loaded {
        let mut loaded = random_cue(root, value, 0, Command::Noop);
        let EventKind::Notes { voices, length, .. } = &mut loaded.score.first_events[0].kind else {
            unreachable!()
        };
        let note = voices[0];
        *length = u16::MAX;
        *voices = (1..=programs.len())
            .map(|id| crate::data::Note {
                macro_id: id as u16,
                ..note
            })
            .collect();
        loaded.resources.programs = programs
            .into_iter()
            .enumerate()
            .map(|(i, mut program)| {
                program.insert(
                    0,
                    Command::Interpolation {
                        mode: Interpolation::Direct,
                        coefficients: 0,
                    },
                );
                (i as u16 + 1, program)
            })
            .collect();
        let mut short = (*loaded.resources.samples[&2]).clone();
        short.loop_length = 0;
        short.loop_pcm.clear();
        loaded.resources.samples.insert(3, Arc::new(short));
        loaded
    }

    fn tone(mut prefix: Vec<Command>) -> Vec<Command> {
        prefix.extend([
            Command::StartSample { sample: 2 },
            wait(Some(5), false, false),
            Command::End,
        ]);
        prefix
    }

    fn compare_block(synth: &Synthesizer, players: &[Stream], references: &mut [Stream]) {
        let blocks: Vec<_> = references
            .iter_mut()
            .map(|stream| stream.block(LiveControls::default()).unwrap())
            .collect();
        for frame in 0..160 {
            synth.advance().unwrap();
            for (index, player) in players.iter().enumerate() {
                assert_eq!(
                    player.shared_frame().unwrap(),
                    blocks[index].as_ref().map(|block| block[frame]),
                    "stream {index}, frame {frame}"
                );
            }
        }
    }

    #[test]
    fn timed_and_looping_sequences_share_native_rng_order_with_sound_effects() {
        let (root, value) = fixture();
        let random = Command::RandomWait {
            upper_ms: 17,
            key_off: false,
        };
        let sequence = |delays: &[Command], looping: bool| {
            let mut loaded = cue(
                &root,
                &value,
                delays.iter().map(|&delay| tone(vec![delay])).collect(),
            );
            let EventKind::Notes { voices, .. } = &loaded.score.first_events[0].kind else {
                unreachable!()
            };
            let note = voices[0];
            loaded.score.origin = ScoreOrigin::Sequence;
            loaded.score.initial_bpm_1024 = 160_000; // One tick per millisecond.
            loaded.score.end_tick = if looping { 20 } else { 60 };
            loaded.score.first_events = (0..if looping { 1 } else { 3 })
                .flat_map(|cycle| {
                    let tick = cycle * 20;
                    [
                        Event {
                            tick,
                            channel: 0,
                            kind: EventKind::Volume { value: 32 },
                        },
                        Event {
                            tick,
                            channel: 0,
                            kind: EventKind::Notes {
                                source: crate::data::VoiceSource::Sequence {
                                    group: 0,
                                    program: 0,
                                    drums: false,
                                },
                                voices: vec![crate::data::Note {
                                    macro_id: if delays.len() == 1 {
                                        1
                                    } else {
                                        cycle as u16 + 1
                                    },
                                    ..note
                                }],
                                length: 25,
                            },
                        },
                        Event {
                            tick: tick + 10,
                            channel: 0,
                            kind: EventKind::Volume { value: 96 },
                        },
                    ]
                })
                .collect();
            loaded.score.loop_events = if looping {
                loaded.score.first_events.clone()
            } else {
                Vec::new()
            };
            loaded.tables.mix.volume = std::array::from_fn(|i| i as f32 / 128.);
            loaded
        };
        for looping in [false, true] {
            for order in [[0, 1, 2], [2, 0, 1], [0, 2, 1]] {
                let synth = Synthesizer::default();
                let mut players = Vec::new();
                let mut references = Vec::new();
                for index in order {
                    let (actual, reference) = if index == 2 {
                        (
                            cue(&root, &value, vec![tone(vec![random])]),
                            cue(
                                &root,
                                &value,
                                vec![tone(vec![wait(Some(14), false, false)])],
                            ),
                        )
                    } else {
                        // Sequence B is visited before A, so prepending their
                        // notes dispatches A then B, ahead of the admitted SFX.
                        // Seed1 modulo17: 6,12,14,3,15,12,14 across three passes.
                        let delays = if index == 0 { [6, 3, 12] } else { [12, 15, 14] }
                            .map(|ms| wait(Some(ms), false, false));
                        (sequence(&[random], looping), sequence(&delays, false))
                    };
                    players.push(
                        Stream::in_synthesizer(Arc::new(actual), looping && index != 2, &synth)
                            .unwrap(),
                    );
                    references.push(Stream::new(Arc::new(reference), false).unwrap());
                }
                // Compare independent, explicitly written notes and waits with
                // actual timed/loop dispatch, including intervening volume events.
                for _ in 0..12 {
                    compare_block(&synth, &players, &mut references);
                }
                assert_eq!(synth.random_state(), (2800480555, 7));
            }
        }
    }

    #[test]
    fn layered_start_timer_and_release_draws_follow_global_voice_order() {
        let (root, value) = fixture();
        for (before, release) in [(0, false), (1, false), (100, true)] {
            let synth = Synthesizer::default();
            let actual = || {
                tone(vec![
                    wait(Some(before), release, false),
                    Command::RandomWait {
                        upper_ms: 17,
                        key_off: false,
                    },
                ])
            };
            let players = [
                Stream::in_synthesizer(
                    Arc::new(cue(&root, &value, vec![actual(), actual()])),
                    false,
                    &synth,
                )
                .unwrap(),
                Stream::in_synthesizer(Arc::new(cue(&root, &value, vec![actual()])), false, &synth)
                    .unwrap(),
            ];
            // Newest stream, then reverse layer order: draws54389,30289,26228.
            let expected = |delay| {
                tone(vec![
                    wait(Some(if release { 5 } else { before }), false, false),
                    wait(Some(delay), false, false),
                ])
            };
            let mut references = [
                Stream::new(
                    Arc::new(cue(&root, &value, vec![expected(14), expected(12)])),
                    false,
                )
                .unwrap(),
                Stream::new(Arc::new(cue(&root, &value, vec![expected(6)])), false).unwrap(),
            ];
            for block in 0..8 {
                if release && block == 1 {
                    assert_eq!(synth.random_state().1, 0);
                    for player in &players {
                        player
                            .set_shared_controls(
                                [LiveControls {
                                    release: true,
                                    ..Default::default()
                                }; 5],
                            )
                            .unwrap();
                    }
                }
                compare_block(&synth, &players, &mut references);
            }
            assert_eq!(synth.random_state(), (4078542139, 3));
        }
    }

    #[test]
    fn ordinary_sources_participate_in_global_sample_completion_ties() {
        let (root, value) = fixture();
        for ordinary in [false, true] {
            let synth = Synthesizer::default();
            let mut players = Vec::new();
            let mut references = Vec::new();
            if ordinary {
                let program = vec![Command::StartSample { sample: 2 }, wait(None, false, false)];
                players.push(
                    Stream::in_synthesizer(
                        Arc::new(cue(&root, &value, vec![program.clone()])),
                        false,
                        &synth,
                    )
                    .unwrap(),
                );
                references
                    .push(Stream::new(Arc::new(cue(&root, &value, vec![program])), false).unwrap());
            }
            for delay in if ordinary { [6, 12] } else { [12, 6] } {
                let actual = tone(vec![
                    Command::StartSample { sample: 3 },
                    wait(None, false, true),
                    Command::RandomWait {
                        upper_ms: 17,
                        key_off: false,
                    },
                ]);
                let expected = tone(vec![
                    Command::StartSample { sample: 3 },
                    wait(Some(5), false, false),
                    wait(Some(delay), false, false),
                ]);
                players.push(
                    Stream::in_synthesizer(
                        Arc::new(cue(&root, &value, vec![actual])),
                        false,
                        &synth,
                    )
                    .unwrap(),
                );
                references.push(
                    Stream::new(Arc::new(cue(&root, &value, vec![expected])), false).unwrap(),
                );
            }
            // With the still-playing ordinary source, newest-first[B,A,C]
            // partitions as[A,B,C]; without it[B,A] stays[B,A].
            for _ in 0..6 {
                compare_block(&synth, &players, &mut references);
            }
            assert_eq!(synth.random_state(), (509449289, 2));
        }
    }

    #[test]
    fn cross_stream_groups_preserve_random_draws_release_kill_and_clear() {
        let (root, value) = fixture();
        let random = Command::RandomWait {
            upper_ms: 17,
            key_off: false,
        };
        for (kill, clear) in [(false, false), (true, false), (true, true)] {
            let synth = Synthesizer::default();
            let group = Command::ExclusiveGroup { group: 5, kill };
            let mut target = vec![group];
            if clear {
                target.push(Command::ExclusiveGroup { group: 0, kill });
            }
            target.extend([
                Command::StartSample { sample: 2 },
                wait(None, true, false),
                random,
                Command::End,
            ]);
            let caller = tone(vec![wait(Some(5), false, false), group, random]);
            let players = [target, caller].map(|program| {
                Stream::in_synthesizer(Arc::new(cue(&root, &value, vec![program])), false, &synth)
                    .unwrap()
            });
            // At 5ms the caller consumes draw1 (6ms). A released target runs
            // next pass at 6ms and consumes draw2 (12ms), ending at 18ms.
            // Killing ends the source at 5ms; group0 leaves it untouched.
            let mut references = [
                vec![
                    Command::StartSample { sample: 2 },
                    wait((!clear).then_some(if kill { 5 } else { 18 }), false, false),
                    Command::End,
                ],
                tone(vec![wait(Some(11), false, false)]),
            ]
            .map(|program| {
                Stream::new(Arc::new(cue(&root, &value, vec![program])), false).unwrap()
            });
            for _ in 0..6 {
                compare_block(&synth, &players, &mut references);
            }
            assert_eq!(
                synth.random_state(),
                if kill {
                    (0xa8351d63, 1)
                } else {
                    (509449289, 2)
                }
            );
        }
    }

    #[test]
    fn macro_controller_writes_share_sequence_channels_but_not_sound_layers() {
        use crate::data::{Controller, Variable};
        let (root, value) = fixture();
        for origin in [ScoreOrigin::Sequence, ScoreOrigin::SoundEffect] {
            let synth = Synthesizer::default();
            let writes = [(7, 7001), (10, 6555), (11, 12001)];
            let mut commands: Vec<_> = writes
                .into_iter()
                .map(|(index, value)| Command::SetVariable {
                    destination: Variable::Controller(Controller::Paired(index)),
                    value,
                })
                .collect();
            commands.extend([
                Command::SetVariable {
                    destination: Variable::Controller(Controller::PitchBend),
                    value: 8401,
                },
                Command::End,
            ]);
            let mut actual = cue(&root, &value, vec![commands, tone(vec![])]);
            actual.score.origin = origin;
            let EventKind::Notes { source, .. } = &mut actual.score.first_events[0].kind else {
                unreachable!()
            };
            *source = match origin {
                ScoreOrigin::Sequence => crate::data::VoiceSource::Sequence {
                    group: 7,
                    program: 1,
                    drums: false,
                },
                ScoreOrigin::SoundEffect => crate::data::VoiceSource::SoundEffect { id: 7 },
            };
            let mut expected = cue(&root, &value, vec![vec![Command::End], tone(vec![])]);
            if origin == ScoreOrigin::Sequence {
                for (index, value) in writes {
                    expected.score.controls[0].paired[index as usize] = value as u16;
                }
                expected.score.controls[0].pitch_bend = 8401;
            }
            for cue in [&mut actual, &mut expected] {
                cue.tables.mix.volume = std::array::from_fn(|index| index as f32 / 128.);
                cue.tables.mix.volume_16_scale = 1. / (127 << 16) as f32;
                cue.tables.mix.controller_14_scale = 1. / 16383.;
                cue.tables.mix.pan = [0., 0.5, 1., 1.];
                cue.tables.mix.pan_16_scale = 1. / (63 << 16) as f32;
            }
            let players = [Stream::in_synthesizer(Arc::new(actual), false, &synth).unwrap()];
            let mut references = [Stream::new(Arc::new(expected), false).unwrap()];
            for _ in 0..3 {
                compare_block(&synth, &players, &mut references);
            }
        }
    }

    #[test]
    fn child_handle_messages_cross_cues_and_wake_after_parent_end() {
        use crate::data::{
            MessageTarget,
            Variable::{Global, Local},
        };
        let (root, value) = fixture();
        for broadcast in [false, true] {
            let synth = Synthesizer::default();
            let mut producer = cue(
                &root,
                &value,
                vec![vec![
                    Command::SpawnMacro {
                        program: 2,
                        instruction: 0,
                        key_offset: 0,
                        priority: 9,
                        max_voices: 255,
                    },
                    Command::VoiceHandle {
                        destination: Global(0),
                        child: true,
                    },
                    Command::End,
                ]],
            );
            producer.resources.programs.insert(
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
                    Command::StartSample { sample: 2 },
                    wait(None, false, false),
                ],
            );
            producer.resources.programs.insert(
                3,
                vec![
                    Command::ReceiveMessage {
                        destination: Global(1),
                    },
                    Command::Release,
                    Command::End,
                ],
            );
            let controller = cue(
                &root,
                &value,
                vec![vec![
                    wait(Some(3), false, false),
                    Command::SetVariable {
                        destination: Local(0),
                        value: 42,
                    },
                    Command::SendMessage {
                        target: if broadcast {
                            MessageTarget::Macro(2)
                        } else {
                            MessageTarget::Handle(Global(0))
                        },
                        value: Local(0),
                    },
                    Command::End,
                ]],
            );
            let players = [producer, controller]
                .map(|cue| Stream::in_synthesizer(Arc::new(cue), false, &synth).unwrap());
            // Child starts at 1ms. Delivery at 3ms wakes its trap in the next
            // scheduler pass, independently of the parent that ended at 0ms.
            let mut references = [
                vec![
                    wait(Some(1), false, false),
                    Command::StartSample { sample: 2 },
                    wait(Some(3), false, false),
                    Command::End,
                ],
                vec![Command::End],
            ]
            .map(|program| {
                Stream::new(Arc::new(cue(&root, &value, vec![program])), false).unwrap()
            });
            for _ in 0..3 {
                compare_block(&synth, &players, &mut references);
            }
        }
    }

    #[test]
    fn macro_variables_follow_shared_order_and_children_start_with_fresh_locals() {
        use crate::data::{
            Arithmetic, Comparison, Operand,
            Variable::{Global, Local},
        };
        let (root, value) = fixture();
        let synth = Synthesizer::default();
        let mut producer = cue(
            &root,
            &value,
            vec![vec![
                Command::SetVariable {
                    destination: Local(0),
                    value: 99,
                },
                Command::SetVariable {
                    destination: Global(0),
                    value: 7,
                },
                Command::SpawnMacro {
                    program: 2,
                    instruction: 0,
                    key_offset: 0,
                    priority: 9,
                    max_voices: 255,
                },
                Command::End,
            ]],
        );
        producer.resources.programs.insert(
            2,
            vec![
                Command::Branch {
                    comparison: Comparison::Equal,
                    left: Local(0),
                    right: Local(1),
                    invert: false,
                    instruction: 2,
                },
                Command::End,
                Command::Calculate {
                    destination: Global(0),
                    operation: Arithmetic::Multiply,
                    left: Global(0),
                    right: Operand::Constant(-2),
                },
                Command::End,
            ],
        );
        let consumer = |delay| {
            Arc::new(cue(
                &root,
                &value,
                vec![vec![
                    wait(Some(delay), false, false),
                    Command::SetVariable {
                        destination: Local(0),
                        value: -14,
                    },
                    Command::Branch {
                        comparison: Comparison::Equal,
                        left: Global(0),
                        right: Local(0),
                        invert: false,
                        instruction: 5,
                    },
                    Command::End,
                    Command::StartSample { sample: 2 },
                    wait(Some(5), false, false),
                    Command::End,
                ]],
            ))
        };
        let players = [
            Stream::in_synthesizer(Arc::new(producer), false, &synth).unwrap(),
            Stream::in_synthesizer(consumer(2), false, &synth).unwrap(),
        ];
        let mut references = [
            Stream::new(
                Arc::new(cue(&root, &value, vec![vec![Command::End]])),
                false,
            )
            .unwrap(),
            Stream::new(
                Arc::new(cue(
                    &root,
                    &value,
                    vec![tone(vec![wait(Some(2), false, false)])],
                )),
                false,
            )
            .unwrap(),
        ];
        for _ in 0..2 {
            compare_block(&synth, &players, &mut references);
        }
        drop(players);
        // The bank belongs to the synthesizer, so destroying both cues leaves
        // the child's -14 available to a later cue.
        let later = consumer(0);
        assert!(Stream::new(later.clone(), false).is_err());
        let players = [Stream::in_synthesizer(later, false, &synth).unwrap()];
        let mut references =
            [Stream::new(Arc::new(cue(&root, &value, vec![tone(vec![])])), false).unwrap()];
        for _ in 0..2 {
            compare_block(&synth, &players, &mut references);
        }
        assert_eq!(synth.random_state(), (1, 0));
    }

    #[test]
    fn child_macros_start_next_pass_inherit_controls_and_outlive_the_parent() {
        let (root, value) = fixture();
        for origin in [ScoreOrigin::SoundEffect, ScoreOrigin::Sequence] {
            for (offset, key, level, pan) in
                [(-100, 0, 88, 17), (-7, 53, 88, 17), (100, 127, 200, 255)]
            {
                let synth = Synthesizer::default();
                let volume = Command::SetVolume {
                    factor: 0,
                    offset: 0,
                    curve: Some(crate::data::VolumeCurve([level; 128])),
                    from_velocity: false,
                };
                let panning = Command::PanRamp {
                    axis: crate::data::PanAxis::Pan,
                    initial: pan,
                    delta: 0,
                    milliseconds: 0,
                };
                let spatial = Command::VolumeCurve {
                    alternate: false,
                    interaural_delay: true,
                };
                let first_wait = if offset == 100 {
                    Command::BeatWait {
                        ticks: Some(1),
                        key_off: false,
                        sample_end: false,
                    }
                } else {
                    Command::Wait {
                        milliseconds: Some(2),
                        from_start: true,
                        key_off: false,
                        sample_end: false,
                    }
                };
                let onset = if offset == 100 && origin == ScoreOrigin::Sequence {
                    2
                } else {
                    3
                };
                let mut loaded = cue(
                    &root,
                    &value,
                    vec![vec![
                        Command::SetNote {
                            key: 100,
                            cents: 0,
                            wait_ms: 0,
                            from_start: false,
                        },
                        volume,
                        panning,
                        spatial,
                        Command::SpawnMacro {
                            program: 2,
                            instruction: 1,
                            key_offset: offset,
                            priority: 9,
                            max_voices: 255,
                        },
                        Command::RandomWait {
                            upper_ms: 1,
                            key_off: false,
                        },
                        Command::End,
                    ]],
                );
                loaded.resources.programs.insert(
                    2,
                    vec![
                        Command::End,
                        Command::Interpolation {
                            mode: Interpolation::Direct,
                            coefficients: 0,
                        },
                        first_wait,
                        Command::RandomWait {
                            upper_ms: 1,
                            key_off: false,
                        },
                        Command::StartSample { sample: 2 },
                        wait(None, true, false),
                        Command::End,
                    ],
                );
                if origin == ScoreOrigin::Sequence {
                    loaded.score.origin = origin;
                    loaded.score.initial_bpm_1024 = 160_000;
                    let EventKind::Notes { source, length, .. } =
                        &mut loaded.score.first_events[0].kind
                    else {
                        unreachable!()
                    };
                    *source = crate::data::VoiceSource::Sequence {
                        group: 1,
                        program: 2,
                        drums: false,
                    };
                    *length = 10;
                }
                let mut reference = cue(
                    &root,
                    &value,
                    vec![vec![
                        volume,
                        panning,
                        spatial,
                        wait(Some(onset), false, false),
                        Command::StartSample { sample: 2 },
                        wait(Some(10 - onset), false, false),
                        Command::End,
                    ]],
                );
                let EventKind::Notes { voices, .. } = &mut reference.score.first_events[0].kind
                else {
                    unreachable!()
                };
                voices[0].key = key;
                for package in [&mut loaded, &mut reference] {
                    package.tables.mix.spatial = Some(mix::Spatial {
                        pan_scale: 1.,
                        left_delay: std::array::from_fn(|index| (index / 4) as u8),
                    });
                }
                let players = [Stream::in_synthesizer(Arc::new(loaded), false, &synth).unwrap()];
                let mut references = [Stream::new(Arc::new(reference), false).unwrap()];
                for block in 0..4 {
                    if block == 2 && origin == ScoreOrigin::SoundEffect {
                        players[0]
                            .set_shared_controls(
                                [LiveControls {
                                    release: true,
                                    ..Default::default()
                                }; 5],
                            )
                            .unwrap();
                    }
                    compare_block(&synth, &players, &mut references);
                }
                assert_eq!(synth.random_state(), (509449289, 2));
            }
        }
    }

    #[test]
    fn failed_child_spawns_continue_parent_without_allocating_or_drawing_randomness() {
        let (root, value) = fixture();
        let synth = Synthesizer::default();
        let mut loaded = cue(
            &root,
            &value,
            vec![tone(vec![
                Command::SpawnMacro {
                    program: 999,
                    instruction: 65535,
                    key_offset: 0,
                    priority: 255,
                    max_voices: 255,
                },
                Command::SpawnMacro {
                    program: 2,
                    instruction: 0,
                    key_offset: 0,
                    priority: 255,
                    max_voices: 1,
                },
            ])],
        );
        loaded.resources.programs.insert(
            2,
            tone(vec![Command::RandomWait {
                upper_ms: 17,
                key_off: false,
            }]),
        );
        let loaded = Arc::new(loaded);
        assert!(Stream::new(loaded.clone(), false).is_err());
        let players = [Stream::in_synthesizer(loaded, false, &synth).unwrap()];
        let mut references =
            [Stream::new(Arc::new(cue(&root, &value, vec![tone(vec![])])), false).unwrap()];
        for _ in 0..3 {
            compare_block(&synth, &players, &mut references);
        }
        assert_eq!(synth.random_state(), (1, 0));
    }

    #[test]
    fn shared_voice_budget_and_dropped_streams_use_one_pool() {
        let (root, value) = fixture();
        let synth = Synthesizer::default();
        let held = |count, origin, priority, last_ends| {
            let mut programs =
                vec![vec![Command::StartSample { sample: 2 }, wait(None, false, false)]; count];
            if last_ends {
                *programs.last_mut().unwrap() = vec![
                    Command::StartSample { sample: 2 },
                    wait(Some(5), false, false),
                    Command::End,
                ];
            }
            let mut loaded = cue(&root, &value, programs);
            loaded.score.origin = origin;
            let EventKind::Notes { source, voices, .. } = &mut loaded.score.first_events[0].kind
            else {
                unreachable!()
            };
            *source = match origin {
                ScoreOrigin::Sequence => crate::data::VoiceSource::Sequence {
                    group: 0,
                    program: 0,
                    drums: false,
                },
                ScoreOrigin::SoundEffect => crate::data::VoiceSource::SoundEffect { id: 0 },
            };
            for note in voices {
                note.priority = priority;
            }
            Arc::new(loaded)
        };
        let sequence = held(42, ScoreOrigin::Sequence, 1, false);
        let mut players = vec![
            Stream::in_synthesizer(sequence.clone(), false, &synth).unwrap(),
            Stream::in_synthesizer(held(22, ScoreOrigin::SoundEffect, 9, false), false, &synth)
                .unwrap(),
        ];
        let mut references = vec![
            Stream::new(sequence, false).unwrap(),
            Stream::new(held(22, ScoreOrigin::SoundEffect, 9, true), false).unwrap(),
        ];
        compare_block(&synth, &players, &mut references);
        // A lower-priority SFX cannot steal cheaper sequence voices once its
        // own 22 slots are occupied, and refusal must not execute its RNG.
        let mut refused = cue(
            &root,
            &value,
            vec![tone(vec![Command::RandomWait {
                upper_ms: 17,
                key_off: false,
            }])],
        );
        let EventKind::Notes { voices, .. } = &mut refused.score.first_events[0].kind else {
            unreachable!()
        };
        voices[0].priority = 8;
        players.push(Stream::in_synthesizer(Arc::new(refused), false, &synth).unwrap());
        references.push(
            Stream::new(
                Arc::new(cue(&root, &value, vec![vec![Command::End]])),
                false,
            )
            .unwrap(),
        );
        let replacement = held(1, ScoreOrigin::SoundEffect, 9, false);
        players.push(Stream::in_synthesizer(replacement.clone(), false, &synth).unwrap());
        references.push(Stream::new(replacement, false).unwrap());
        compare_block(&synth, &players, &mut references);
        assert_eq!(synth.random_state(), (1, 0));
        drop(players.remove(1));
        drop(references.remove(1));
        let refill = held(21, ScoreOrigin::SoundEffect, 9, false);
        players.push(Stream::in_synthesizer(refill.clone(), false, &synth).unwrap());
        references.push(Stream::new(refill, false).unwrap());
        for _ in 0..3 {
            compare_block(&synth, &players, &mut references);
        }
    }
}

#[test]
fn finite_score_keeps_pending_notes_and_ends_when_the_last_macro_finishes() {
    use crate::sequence::{LiveControls, stream::Stream};
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = random_cue(
        &root,
        &value,
        0,
        Command::Wait {
            milliseconds: Some(10),
            from_start: false,
            key_off: false,
            sample_end: false,
        },
    );
    loaded.score.initial_bpm_1024 = 160_000; // One tick per millisecond.
    loaded.score.origin = crate::data::ScoreOrigin::Sequence;
    let crate::data::EventKind::Notes { source, .. } = &mut loaded.score.first_events[0].kind
    else {
        unreachable!()
    };
    *source = crate::data::VoiceSource::Sequence {
        group: 0,
        program: 0,
        drums: false,
    };
    let mut second = loaded.score.first_events[0].clone();
    second.tick = 50;
    loaded.score.first_events.push(second);
    let mut stream = Stream::new(Arc::new(loaded), false).unwrap();
    let mut pcm = Vec::new();
    while let Some(block) = stream.block(LiveControls::default()).unwrap() {
        pcm.extend(block);
        assert!(
            pcm.len() <= 2240,
            "completed macros retained a silent source handle"
        );
    }
    // Each note waits 10ms, sounds for 5ms and ends. The 50ms score gap
    // remains alive, but completion has no four-second effect-tail padding.
    assert!((2080..=2240).contains(&pcm.len()));
    let audible =
        |frames: &[crate::sequence::BusFrame]| frames.iter().flatten().flatten().any(|&v| v != 0);
    assert!(audible(&pcm[..640]) && audible(&pcm[1600..]));
    assert!(!audible(&pcm[640..1600]));
}

#[test]
fn shared_random_cue_admission_release_and_isolated_rendering_are_explicit() {
    use crate::sequence::{LiveControls, shared::Synthesizer, stream::Stream};
    use std::sync::Arc;
    let (root, value) = fixture();
    let random = Command::RandomWait {
        upper_ms: 65535,
        key_off: true,
    };
    let synth = Synthesizer::default();
    // A command submitted inside a prepared DSP block enters the next block,
    // without consuming any random values or claiming to have started early.
    for _ in 0..33 {
        synth.advance().unwrap();
    }
    let player = Stream::in_synthesizer(
        Arc::new(random_cue(&root, &value, 10, random)),
        false,
        &synth,
    )
    .unwrap();
    for _ in 33..160 {
        synth.advance().unwrap();
        assert!(!player.started());
        assert_eq!(player.shared_frame().unwrap(), Some([[0; 2]; 3]));
    }
    synth.advance().unwrap();
    assert!(player.started());
    for _ in 161..320 {
        synth.advance().unwrap();
    }
    player
        .set_shared_controls(
            [LiveControls {
                release: true,
                ..Default::default()
            }; 5],
        )
        .unwrap();
    for _ in 320..800 {
        synth.advance().unwrap();
    }
    assert_eq!(
        synth.random_state(),
        (1, 0),
        "key-off bypasses the random draw"
    );
    let loaded = Arc::new(random_cue(&root, &value, 0, random));
    assert!(Stream::new(loaded.clone(), false).is_err());
    assert!(Stream::in_synthesizer(loaded, true, &synth).is_err());
    let zero = Stream::in_synthesizer(
        Arc::new(random_cue(
            &root,
            &value,
            0,
            Command::RandomWait {
                upper_ms: 0,
                key_off: false,
            },
        )),
        false,
        &synth,
    )
    .unwrap();
    synth.advance().unwrap();
    assert!(zero.started());
    assert_eq!(
        synth.random_state(),
        (1, 0),
        "zero upper bound consumes no randomness"
    );
}

#[test]
fn beat_waits_preserve_fractional_deadlines_tempo_sampling_and_key_off_pcm() {
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    let start = [
        Command::Interpolation {
            mode: crate::data::Interpolation::Direct,
            coefficients: 0,
        },
        Command::StartSample { sample: 2 },
    ];
    let beats = |ticks, key_off| Command::BeatWait {
        ticks,
        key_off,
        sample_end: false,
    };
    let ms = |milliseconds| Command::Wait {
        milliseconds,
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    let render = |loaded: &mut Loaded, tail: Vec<_>, tempos: &[_], key_off| {
        voice_pcm(
            loaded,
            start
                .into_iter()
                .chain(tail)
                .chain([Command::End])
                .collect(),
            tempos,
            key_off,
        )
    };
    // One native tick at120 BPM is312/256 ms after integer division. Four waits
    // finish at5 ms, retaining each scheduled deadline rather than rounding four times.
    let looped = render(
        &mut loaded,
        vec![
            beats(Some(1), false),
            Command::Loop {
                instruction: 2,
                count: 3,
                key_off: false,
                sample_end: false,
            },
        ],
        &[(0, 120 * 1024 + 1023)],
        None,
    );
    assert_eq!(looped.0, 160);
    assert!(looped.1.iter().any(|&sample| sample != 0));
    assert_eq!(looped, render(&mut loaded, vec![ms(Some(5))], &[], None));
    let changed = render(
        &mut loaded,
        vec![beats(Some(384), false); 2],
        &[(3200, 60 * 1024)],
        None,
    );
    assert_eq!(changed.0, 48000);
    assert_eq!(
        changed,
        render(&mut loaded, vec![ms(Some(500)), ms(Some(1000))], &[], None)
    );
    let interrupted = render(&mut loaded, vec![beats(None, true)], &[], Some(64));
    assert_eq!(interrupted.0, 64);
    assert_eq!(
        interrupted,
        render(
            &mut loaded,
            vec![Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            }],
            &[],
            Some(64)
        )
    );
}

#[test]
fn both_pitch_sweep_slots_mix_additively_and_cancel_independently() {
    use crate::data::{Interpolation, SweepSlot};
    let legacy: Command = serde_json::from_str(
        r#"{"operation":"pitch_sweep","step_hz":1000,"period":2,"wait_ms":0}"#,
    )
    .unwrap();
    assert!(matches!(
        legacy,
        Command::PitchSweep {
            slot: SweepSlot::First,
            ..
        }
    ));
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    let sweep = |slot, step_hz, period| Command::PitchSweep {
        slot,
        step_hz,
        period,
        wait_ms: 0,
    };
    let wait = |milliseconds| Command::Wait {
        milliseconds: Some(milliseconds),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    let render = |loaded: &mut Loaded, commands: Vec<_>| {
        voice_pcm(
            loaded,
            [
                Command::Interpolation {
                    mode: Interpolation::Linear,
                    coefficients: 0,
                },
                Command::StartSample { sample: 2 },
            ]
            .into_iter()
            .chain(commands)
            .chain([Command::End])
            .collect(),
            &[],
            None,
        )
    };
    let plain = render(&mut loaded, vec![wait(100)]);
    let first = render(
        &mut loaded,
        vec![sweep(SweepSlot::First, 1000, 8), wait(100)],
    );
    assert_ne!(first.1, plain.1, "sweep must change rendered PCM");
    assert_eq!(
        first,
        render(
            &mut loaded,
            vec![sweep(SweepSlot::Second, 1000, 8), wait(100)]
        )
    );
    assert_eq!(
        plain,
        render(
            &mut loaded,
            vec![
                sweep(SweepSlot::First, 1000, 8),
                sweep(SweepSlot::Second, -1000, 8),
                wait(100)
            ]
        )
    );
    let doubled = render(
        &mut loaded,
        vec![sweep(SweepSlot::First, 2000, 8), wait(100)],
    );
    assert_eq!(
        doubled,
        render(
            &mut loaded,
            vec![
                sweep(SweepSlot::First, 1000, 8),
                sweep(SweepSlot::Second, 1000, 8),
                wait(100)
            ]
        )
    );
    let retained = render(
        &mut loaded,
        vec![sweep(SweepSlot::First, 1000, 8), wait(20), wait(80)],
    );
    assert_eq!(
        retained,
        render(
            &mut loaded,
            vec![
                sweep(SweepSlot::First, 1000, 8),
                wait(20),
                sweep(SweepSlot::Second, 0, 0),
                wait(80)
            ]
        )
    );
}

#[test]
fn vibrato_disable_and_phase_match_unmodulated_notes() {
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.tables.modulation.sine = std::array::from_fn(|i| i as i16 * 4);
    let vibrato = |period_ms| Command::Vibrato {
        period_ms,
        depth_8: 256,
        reverse: false,
        scale_by_modulation: false,
    };
    let render = |loaded: &mut Loaded, modulation: Vec<_>| {
        voice_pcm(
            loaded,
            [
                Command::Interpolation {
                    mode: crate::data::Interpolation::Linear,
                    coefficients: 0,
                },
                Command::StartSample { sample: 2 },
            ]
            .into_iter()
            .chain(modulation)
            .chain([
                Command::Wait {
                    milliseconds: Some(100),
                    from_start: false,
                    key_off: false,
                    sample_end: false,
                },
                Command::End,
            ])
            .collect(),
            &[],
            None,
        )
    };
    let plain = render(&mut loaded, vec![]);
    assert!(plain.1.iter().any(|&sample| sample != 0));
    assert_ne!(plain, render(&mut loaded, vec![vibrato(100)]));
    assert_eq!(plain, render(&mut loaded, vec![vibrato(100), vibrato(0)]));
    // A constant quarter-wave holds the first half-period at exactly +/- one
    // semitone, giving an independent reference through ordinary note commands.
    loaded.tables.modulation.sine.fill(4096);
    loaded.tables.pitch.up = std::array::from_fn(|i| 2f32.powf(i as f32 / 12.0));
    loaded.tables.pitch.down = std::array::from_fn(|i| 2f32.powf(-(i as f32) / 12.0));
    let plain = render(&mut loaded, vec![]);
    for (reverse, key) in [(false, 61), (true, 59)] {
        let actual = render(
            &mut loaded,
            vec![Command::Vibrato {
                period_ms: 200,
                depth_8: 256,
                reverse,
                scale_by_modulation: false,
            }],
        );
        let expected = render(
            &mut loaded,
            vec![Command::SetNote {
                key,
                cents: 0,
                wait_ms: 0,
                from_start: false,
            }],
        );
        assert_ne!(actual, plain);
        assert_eq!(actual, expected);
    }
}

#[test]
fn spatial_instruments_require_their_tables_during_preparation() {
    let (root, mut value) = fixture();
    value["programs"]["1"][0] = serde_json::to_value(Command::VolumeCurve {
        alternate: true,
        interaural_delay: true,
    })
    .unwrap();
    assert!(
        load(&root, &value)
            .err()
            .unwrap()
            .to_string()
            .contains("spatial audio tables")
    );
}

#[test]
fn banks_share_samples_only_when_tuning_and_loop_metadata_match() {
    let (root, value) = fixture();
    fs::write(
        root.0.join("bank.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let mut cache = SampleCache::default();
    let mut read = |path: &str, _: usize| Ok(fs::read(root.0.join(path))?);
    let a = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    let b = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &a.resources.samples[&2],
        &b.resources.samples[&2]
    ));
    let mut changed = value;
    changed["samples"]["2"]["key"] = 61.into();
    fs::write(
        root.0.join("bank.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let c = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    assert!(!std::sync::Arc::ptr_eq(
        &a.resources.samples[&2],
        &c.resources.samples[&2]
    ));
    assert_eq!(a.resources.samples[&2].pcm, c.resources.samples[&2].pcm);
}

#[test]
fn package_preserves_independent_loop_pcm_and_rejects_corruption() {
    let (root, value) = fixture();
    let loaded = load(&root, &value).unwrap();
    let sample = &loaded.resources.samples[&2];
    assert_eq!(sample.pcm, [10, 20, 30, 40]);
    assert_eq!(sample.loop_pcm, [50, 60]);
    let cases = [
        ("/version", serde_json::json!(1)),
        ("/samples/2/sha256", serde_json::json!("incorrect")),
        ("/samples/2/first_frames", serde_json::json!(5)),
        ("/samples/2/loop_start", serde_json::json!(4)),
        ("/samples/2/path", serde_json::json!("../sample.wav")),
        ("/programs/1/0/sample", serde_json::json!(3)),
        ("/tables/pitch/up", serde_json::json!([1.])),
        ("/tables/coefficients", serde_json::json!([0])),
        ("/score/controls/0/paired/10", serde_json::json!(16384)),
        ("/score/end_tick", serde_json::json!(0)),
    ];
    for (pointer, replacement) in cases {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        assert!(load(&root, &invalid).is_err(), "accepted invalid {pointer}");
    }
    assert!(Package::load(&root.0, "../music.json").is_err());
}

#[test]
fn field_loops_keep_note_onsets_and_held_note_releases_on_time() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        reverb::Studio,
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::End,
        ],
    );
    // Exactly one score tick per millisecond. Each ten-tick note spans the
    // eight-tick loop, exercising both the new and the retiring clock.
    loaded.score.initial_bpm_1024 = 160_000;
    let event = |tick| Event {
        tick,
        channel: 0,
        kind: EventKind::Notes {
            source: crate::data::VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 64,
                max_voices: 255,
            }],
            length: 10,
        },
    };
    // An explicitly written-out passage supplies an independent timing
    // expectation, without taking a loop or sharing its clock handoff.
    loaded.score.first_events = [1, 9, 17].map(event).into();
    let expected = sequence::render_preview(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        800,
    )
    .unwrap();
    assert!(expected.pcm.iter().any(|sample| *sample != 0));
    assert_eq!(
        expected
            .voice_lifetimes
            .iter()
            .map(|voice| voice.start_frame)
            .collect::<Vec<_>>(),
        [32, 288, 544]
    );
    loaded.score.end_tick = 8;
    loaded.score.first_events = vec![event(1)];
    loaded.score.loop_events = vec![event(1)];
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    let mut stream = Stream::new(Arc::new(loaded), true).unwrap();
    let mut actual = Vec::new();
    for _ in 0..5 {
        for buses in stream.block(LiveControls::default()).unwrap().unwrap() {
            actual.extend(
                studio
                    .process(buses)
                    .map(|sample| sample.clamp(-32768, 32767) as i16),
            );
        }
    }
    assert_eq!(actual, expected.pcm);
    stream.stop().unwrap();
}

#[test]
fn worker_preserves_millisecond_controls_from_the_offline_renderer() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        reverb::Studio,
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.tables.mix.volume = std::array::from_fn(|i| i as f32 / 128.);
    loaded.tables.mix.volume_16_scale = 1. / (127. * 65536.);
    loaded.tables.mix.controller_14_scale = 1. / 16383.;
    let sample = Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
    sample.pcm.fill(12000);
    sample.loop_pcm.fill(16000);
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::End,
        ],
    );
    loaded.score.first_events.push(Event {
        // Birth inside the block observes the current group value even
        // though existing voices normally update on five-ms boundaries.
        tick: 1,
        channel: 0,
        kind: EventKind::Notes {
            source: crate::data::VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 64,
                max_voices: 255,
            }],
            length: 90,
        },
    });
    let volume = |frame: u32| (frame / 32 % 5) as f32 / 4.;
    let expected = sequence::render_preview_with_volume(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        1600,
        volume,
    )
    .unwrap();
    assert!(
        expected.pcm.iter().any(|sample| *sample != 0),
        "silent fixture cannot test gain changes"
    );
    let held = sequence::render_preview_with_volume(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        1600,
        |frame| volume(frame / 160 * 160),
    )
    .unwrap();
    assert_ne!(
        expected.pcm, held.pcm,
        "fixture must detect the previous block-wide gain hold"
    );
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    let mut stream = Stream::new(Arc::new(loaded), false).unwrap();
    let mut actual = Vec::new();
    for block in 0..10 {
        let input = std::array::from_fn(|i| LiveControls {
            volume: volume(block * 160 + i as u32 * 32),
            ..Default::default()
        });
        for buses in stream.block_envelope(input).unwrap().unwrap() {
            actual.extend(
                studio
                    .process(buses)
                    .map(|sample| sample.clamp(-32768, 32767) as i16),
            );
        }
    }
    assert_eq!(actual, expected.pcm);
}

#[test]
fn mono_centers_live_voices_and_preserves_pan_changes_for_stereo() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        sequence::{LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let score = |centered| {
        let mut loaded = load(&root, &value).unwrap();
        loaded.tables.mix.pan = [0., std::f32::consts::FRAC_1_SQRT_2, 1., 1.];
        loaded.tables.mix.pan_16_scale = 1. / (63. * 65536.);
        loaded.resources.programs.insert(
            1,
            vec![
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::StartSample { sample: 2 },
                Command::Wait {
                    milliseconds: None,
                    from_start: false,
                    key_off: true,
                    sample_end: false,
                },
                Command::End,
            ],
        );
        loaded.score.initial_bpm_1024 = 160_000; // One tick per millisecond.
        loaded.score.controls[0].paired[10] = (if centered { 64 } else { 17 }) << 7;
        loaded.score.first_events = vec![
            Event {
                tick: 0,
                channel: 0,
                kind: EventKind::Notes {
                    source: crate::data::VoiceSource::Sequence {
                        group: 0,
                        program: 0,
                        drums: false,
                    },
                    voices: vec![Note {
                        macro_id: 1,
                        key: 60,
                        velocity: 127,
                        pan: if centered { 64 } else { 48 },
                        priority: 64,
                        max_voices: 255,
                    }],
                    length: 90,
                },
            },
            Event {
                tick: 20,
                channel: 0,
                kind: EventKind::Pan {
                    value: if centered { 64 } else { 113 },
                },
            },
        ];
        Arc::new(loaded)
    };
    let panned = score(false);
    let mut stereo = Stream::new(panned.clone(), false).unwrap();
    let mut changing = Stream::new(panned, false).unwrap();
    // An authored centered score supplies the expectation without using mono mode.
    let mut center = Stream::new(score(true), false).unwrap();
    for block in 0..16 {
        let stereo = stereo.block(LiveControls::default()).unwrap().unwrap();
        let center = center.block(LiveControls::default()).unwrap().unwrap();
        let mono = (3..10).contains(&block);
        let actual = changing
            .block(LiveControls {
                mono,
                ..Default::default()
            })
            .unwrap()
            .unwrap();
        if block < 3 || (5..10).contains(&block) || block >= 12 {
            assert_ne!(stereo, center, "fixture must exercise audible panning");
            assert_eq!(actual, if mono { center } else { stereo }, "block {block}");
        }
    }
}

#[test]
fn exclusive_group_ends_the_previous_voice_and_cue_release_finishes() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note},
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let (root, value) = fixture();
    let mut loaded = load(&root, &value).unwrap();
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::ExclusiveGroup {
                group: 5,
                kill: true,
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: None,
                from_start: false,
                key_off: true,
                sample_end: false,
            },
            Command::StopSample,
            Command::End,
        ],
    );
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 127,
        pan: 64,
        priority: 64,
        max_voices: 255,
    };
    loaded.score.first_events = [0, 20]
        .map(|tick| Event {
            tick,
            channel: 0,
            kind: EventKind::Notes {
                source: crate::data::VoiceSource::Sequence {
                    group: 0,
                    program: 0,
                    drums: false,
                },
                voices: vec![note],
                length: 90,
            },
        })
        .to_vec();
    let preview = sequence::render_preview(
        &loaded.resources,
        &loaded.score,
        &loaded.tables,
        loaded.reverbs,
        2000,
    )
    .unwrap();
    assert_eq!(preview.voice_lifetimes.len(), 2);
    assert_eq!(
        preview.voice_lifetimes[0].end_frame,
        Some(preview.voice_lifetimes[1].start_frame)
    );
    assert!(preview.voice_lifetimes[1].end_frame.is_none());
    loaded.score.first_events.truncate(1);
    let loaded = Arc::new(loaded);
    let mut stream = Stream::new(loaded.clone(), false).unwrap();
    assert_eq!(
        stream
            .block(LiveControls::default())
            .unwrap()
            .unwrap()
            .len(),
        160 // Unclipped stereo bus frames, before shared effects and output conversion.
    );
    assert!(
        stream
            .block(LiveControls {
                release: true,
                ..Default::default()
            })
            .unwrap()
            .is_some()
    );
    let mut completed = false;
    for _ in 0..850 {
        if stream.block(LiveControls::default()).unwrap().is_none() {
            completed = true;
            break;
        }
    }
    assert!(completed, "released cue did not finish its macros");
    // A worker waiting for its first control request must also cancel cleanly.
    let mut paused = Stream::new(loaded, true).unwrap();
    paused.stop().unwrap();
    assert!(paused.block(LiveControls::default()).unwrap().is_none());
}
