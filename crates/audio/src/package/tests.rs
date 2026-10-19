use super::*;
use crate::{dls, mix, modulation, music_voice::Controls, resample};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

pub(crate) struct Fixture(pub(crate) PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn playback_data() -> (Resources, Score, Tables) {
    (
        Resources {
            programs: BTreeMap::from([(1, vec![Command::StartSample { sample: 2 }, Command::End])]),
            samples: BTreeMap::from([(
                2,
                std::sync::Arc::new(Sample {
                    key: 60,
                    rate: 32000,
                    pcm: vec![10, 20, 30, 40],
                    loop_pcm: vec![50, 60],
                    loop_start: 2,
                    loop_length: 2,
                }),
            )]),
        },
        Score {
            origin: crate::data::ScoreOrigin::Sequence,
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 0,
            end_tick: 100,
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
            dls: dls::Tables {
                attenuation: [0; 194],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        },
    )
}

fn prepared() -> Loaded {
    let (resources, score, tables) = playback_data();
    Loaded::new(resources, score, tables, [[0., 0., 1., 0., 0.]; 2]).unwrap()
}

pub(crate) fn fixture() -> (Fixture, serde_json::Value) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let root = Fixture(std::env::temp_dir().join(format!(
        "resonance-music-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&root.0).unwrap();
    let path = root.0.join("sample.wav");
    let loaded = prepared();
    let sample = &loaded.resources.samples[&2];
    let mut wave = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: u32::from(sample.rate),
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for &value in sample.pcm.iter().chain(&sample.loop_pcm) {
        wave.write_sample(value).unwrap();
    }
    wave.finalize().unwrap();
    let package = Package {
        version: VERSION,
        programs: loaded.resources.programs,
        samples: BTreeMap::from([(
            2,
            SampleAsset {
                path: "sample.wav".into(),
                sha256: format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                key: sample.key,
                rate: sample.rate,
                first_frames: sample.pcm.len() as u32,
                loop_start: sample.loop_start,
                loop_length: sample.loop_length,
            },
        )]),
        score: loaded.score,
        tables: loaded.tables,
        reverbs: loaded.reverbs,
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

fn control_deadline(milliseconds: u64) -> usize {
    crate::volume::frames_from_millis(milliseconds)
        .unwrap()
        .div_ceil(32) as usize
        * 32
}

#[test]
fn prepared_constructor_rejects_invalid_score_data() {
    let Loaded {
        resources,
        mut score,
        tables,
        reverbs,
    } = prepared();
    score.first_events.push(crate::data::Event {
        tick: 0,
        channel: 16,
        kind: crate::data::EventKind::Volume { value: 127 },
    });
    assert!(Loaded::new(resources, score, tables, reverbs).is_err());
}

#[test]
fn prepared_constructor_rejects_invalid_reverb() {
    let Loaded {
        resources,
        score,
        tables,
        mut reverbs,
    } = prepared();
    reverbs[0][0] = f32::NAN;
    assert!(Loaded::new(resources, score, tables, reverbs).is_err());
}

#[test]
fn package_boundary_rejects_every_unsupported_controller_destination() {
    use crate::data::{Arithmetic, Controller, Operand, Variable};
    let (root, mut value) = fixture();
    let destination = Variable::Controller(Controller::Paired(6));
    for command in [
        Command::SetVariable {
            destination,
            value: 0,
        },
        Command::Calculate {
            destination,
            operation: Arithmetic::Add,
            left: Variable::Local(0),
            right: Operand::Constant(0),
        },
        Command::VoiceHandle {
            destination,
            child: false,
        },
        Command::ReceiveMessage { destination },
    ] {
        value["programs"]["1"] = serde_json::to_value([command, Command::End]).unwrap();
        let error = load(&root, &value)
            .err()
            .expect("invalid destination accepted");
        assert!(
            format!("{error:#}").contains("RPN data-entry"),
            "{command:?}: {error}"
        );
    }
}

#[test]
fn pitch_envelope_has_one_validated_sustain() {
    let (root, mut value) = fixture();
    let envelope = dls::Timing {
        attack_timecents: 0,
        decay_timecents: 0,
        release_ms: 20,
        attack_velocity_scale: i32::MIN,
        decay_key_scale: i32::MIN,
    };
    for sustain in [0, 193, 194] {
        value["programs"]["1"] = serde_json::to_value([
            Command::PitchEnvelope {
                envelope,
                sustain,
                depth_8: 256,
            },
            Command::End,
        ])
        .unwrap();
        assert_eq!(load(&root, &value).is_ok(), sustain <= 193);
    }
}

#[test]
fn noops_preserve_playing_sample_phase_envelope_and_waits() {
    use crate::{
        data::{Interpolation, Note},
        music_voice::Voice,
    };
    let mut loaded = prepared();
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
            crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
            done.push(voice.is_done());
            if frame % 160 == 159 {
                let mut block = [[[0; 2]; 3]; 160];
                voice.mix_block(&mut block);
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
    let mut loaded = prepared();
    let note = Note {
        macro_id: 1,
        key: 60,
        velocity: 100,
        pan: 64,
        priority: 1,
        max_voices: 1,
    };
    for (count, release_at, finish) in [
        (2, None, 3 * control_deadline(10)),
        (u16::MAX, Some(480), 2 * control_deadline(10)),
    ] {
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
            crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
            assert_eq!(voice.is_done(), frame == finish, "frame {frame}");
            if frame % 160 == 159 {
                voice.mix_block(&mut [[[0; 2]; 3]; 160]);
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
        crate::music_voice::test_frame(&mut voice, Controls::default())
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
        crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
        if voice.is_done() || frame % 160 == 159 {
            let mut block = [[[0; 2]; 3]; 160];
            voice.mix_block(&mut block);
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
fn selectors_route_sends_without_overriding_dry_volume_or_stereo_pan() {
    use crate::data::{ControlTarget, Interpolation, Note, Operand};
    let mut loaded = prepared();
    loaded.tables.mix.volume = std::array::from_fn(|i| i as f32 / 128.);
    loaded.tables.mix.volume_16_scale = 1. / (127. * 65536.);
    loaded.tables.mix.controller_14_scale = 1. / 16383.;
    std::sync::Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap())
        .pcm
        .fill(20000);
    let mut render = |target, volume| {
        loaded.resources.programs.insert(
            1,
            vec![
                Command::SelectControl {
                    target,
                    source: Operand::Constant(0),
                    scale: 0,
                },
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::StartSample { sample: 2 },
                Command::Wait {
                    milliseconds: Some(10),
                    from_start: false,
                    key_off: false,
                    sample_end: false,
                },
                Command::End,
            ],
        );
        let mut controls = Controls::default();
        controls.paired[7] = volume;
        let mut voice = crate::music_voice::Voice::new(
            &loaded.resources,
            &loaded.tables,
            Note {
                macro_id: 1,
                key: 60,
                velocity: 100,
                pan: 64,
                priority: 1,
                max_voices: 1,
            },
        )
        .unwrap();
        for _ in 0..160 {
            crate::music_voice::test_frame(&mut voice, controls).unwrap();
        }
        let mut block = [[[0; 2]; 3]; 160];
        voice.mix_block(&mut block);
        block[0]
    };
    let normal = render(ControlTarget::SurroundPan, 127 << 7);
    let post_a = render(ControlTarget::PostAuxiliaryA, 127 << 7);
    assert!(normal[0][0] > 0);
    assert_eq!(normal[0], post_a[0]);
    assert_eq!(normal[1], [0; 2]);
    assert!(post_a[1][0] > 0);
    assert_eq!(post_a[2], [0; 2]);
    assert_eq!(render(ControlTarget::PostAuxiliaryA, 0), [[0; 2]; 3]);
    let pre_b = render(ControlTarget::PreAuxiliaryB, 0);
    assert_eq!(&pre_b[..2], &[[0; 2]; 2]);
    assert!(pre_b[2][0] > 0);
}

#[test]
fn identical_interpolation_preserves_pcm_for_live_and_finished_sources() {
    use crate::data::Interpolation;
    let mut loaded = prepared();
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
fn interpolation_changes_only_require_an_inactive_source() {
    use crate::{
        data::{Interpolation, Note},
        music_voice::Voice,
    };
    let mut loaded = prepared();
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
            for frame in 0..control_deadline(7) {
                crate::music_voice::test_frame(&mut voice, Controls::default()).unwrap();
                if frame % 160 == 159 {
                    voice.mix_block(&mut [[[0; 2]; 3]; 160]);
                }
            }
            assert_eq!(voice.source_active(), !finished);
            let result = crate::music_voice::test_frame(&mut voice, Controls::default());
            if finished {
                result.unwrap();
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("changing an active source mode")
                );
            }
        }
    }
}

#[test]
fn custom_volume_curves_and_overlapping_fades_reach_their_targets() {
    use crate::data::{Interpolation, VolumeCurve};
    let mut loaded = prepared();
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
fn pan_ramp_moves_continuously_and_surround_is_inert_in_stereo() {
    use crate::data::{Interpolation, PanAxis};
    let mut loaded = prepared();
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
        let ramp_start = crate::volume::frames_from_millis(u64::from(lead))
            .unwrap()
            .div_ceil(32)
            * 32;
        let frames = &actual.1[ramp_start as usize * 2..];
        let middle = crate::volume::frames_from_millis(45).unwrap() as usize;
        assert!(frames[middle * 2] != frames[0] || frames[middle * 2 + 1] != frames[1]);
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
            pre: [0; 2],
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

fn random_cue(before_ms: u16, wait: Command) -> Loaded {
    use crate::data::{Event, EventKind, Interpolation, Note};
    let mut loaded = prepared();
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

fn mixer_frame(
    synth: &crate::sequence::shared::Synthesizer,
    players: &[crate::sequence::stream::Stream],
) -> (Vec<bool>, crate::sequence::BusFrame) {
    let frames: Vec<_> = players.iter().map(|player| player.shared_frame()).collect();
    let live = frames.iter().map(Option::is_some).collect();
    let mut mixed = [[0; 2]; 3];
    // Read globals only after every player has marked its current frame read.
    for frame in frames.into_iter().flatten().chain([synth.unread_frame()]) {
        for (bus, source) in mixed.iter_mut().zip(frame) {
            for (value, source) in bus.iter_mut().zip(source) {
                *value += source;
            }
        }
    }
    (live, mixed)
}

#[test]
fn randomized_cues_replay_reproducibly_with_bounded_audible_completion() {
    use crate::sequence::{shared::Synthesizer, stream::Stream};
    use std::sync::Arc;
    let wait = |ms| Command::Wait {
        milliseconds: Some(ms),
        from_start: false,
        key_off: false,
        sample_end: false,
    };
    let render = || {
        let synth = Synthesizer::default();
        let players = [0, 4].map(|before| {
            let mut loaded = random_cue(0, Command::Noop);
            loaded.resources.programs.insert(
                1,
                vec![
                    Command::Interpolation {
                        mode: crate::data::Interpolation::Linear,
                        coefficients: 0,
                    },
                    wait(before),
                    Command::RandomNote {
                        low: 53,
                        high: 67,
                        cents: 7,
                        random_cents: true,
                        relative: false,
                    },
                    Command::RandomWait {
                        upper_ms: 17,
                        key_off: false,
                        sample_end: false,
                    },
                    Command::RandomBranch {
                        minimum: 128,
                        program: 1,
                        instruction: 6,
                    },
                    wait(1),
                    Command::StartSample { sample: 2 },
                    wait(1),
                    Command::RandomLoop {
                        instruction: 7,
                        count: 17,
                        key_off: false,
                        sample_end: false,
                    },
                    Command::End,
                ],
            );
            Stream::in_synthesizer(Arc::new(loaded), false, &synth).unwrap()
        });
        let bound = control_deadline(4) + control_deadline(17) + 18 * control_deadline(1) + 2 * 160;
        let mut output = Vec::new();
        for _ in 0..bound {
            synth.advance().unwrap();
            let (live, frame) = mixer_frame(&synth, &players);
            output.push(frame);
            if live.iter().all(|live| !live) {
                assert!(output.iter().flatten().flatten().any(|sample| *sample != 0));
                assert_eq!(frame, [[0; 2]; 3]);
                return output;
            }
        }
        panic!("random waits/loops exceeded their authored bounds");
    };
    assert_eq!(
        render(),
        render(),
        "a fresh session must replay reproducibly"
    );
}

mod shared_scheduler {
    use super::*;
    use crate::{
        data::{EventKind, Interpolation, ScoreOrigin},
        sequence::{shared::Synthesizer, stream::Stream},
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

    fn cue(programs: Vec<Vec<Command>>) -> Loaded {
        let mut loaded = random_cue(0, Command::Noop);
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

    fn compare_block(
        synth: &Synthesizer,
        players: &[Stream],
        reference_synth: &Synthesizer,
        references: &mut [Stream],
    ) {
        // Cue completion follows queued samples and release tails.
        // Compare the full six-bus mixer against independently authored fixed
        // commands; each global contribution is read exactly once per frame.
        for frame in 0..160 {
            synth.advance().unwrap();
            reference_synth.advance().unwrap();
            assert_eq!(
                mixer_frame(synth, players),
                mixer_frame(reference_synth, references),
                "mixed frame {frame}",
            );
        }
    }

    #[test]
    fn sample_end_wakes_random_waits_before_their_deadline() {
        for active in [false, true] {
            let synth = Synthesizer::default();
            let mut program = Vec::new();
            if active {
                program.push(Command::StartSample { sample: 3 });
            }
            program.extend(
                [Command::RandomWait {
                    upper_ms: 60_000,
                    key_off: false,
                    sample_end: true,
                }; 2],
            );
            let mut loaded = cue(vec![tone(program)]);
            Arc::make_mut(loaded.resources.samples.get_mut(&3).unwrap())
                .pcm
                .fill(-32000);
            let sample = Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
            sample.pcm.fill(32000);
            sample.loop_pcm.fill(32000);
            let player = Stream::in_synthesizer(Arc::new(loaded), false, &synth).unwrap();
            let mut first_tone = None;
            let bound = 2 * 32 + control_deadline(5) + 2 * 160;
            let mut completed = false;
            for index in 0..bound {
                synth.advance().unwrap();
                let (live, frame) = mixer_frame(&synth, std::slice::from_ref(&player));
                if frame[0][0] > 0 {
                    first_tone.get_or_insert(index);
                }
                if !live[0] {
                    completed = true;
                    break;
                }
            }
            assert_eq!(
                first_tone,
                Some(if active { 32 } else { 0 }),
                "sample end must wake at the next control boundary"
            );
            assert!(completed, "sample completion must wake the program");
        }
    }

    #[test]
    fn looping_music_and_random_effects_replay_then_release_independently() {
        let random = Command::RandomWait {
            upper_ms: 17,
            key_off: false,
            sample_end: false,
        };
        let mut music = cue(vec![tone(vec![random])]);
        music.score.origin = ScoreOrigin::Sequence;
        music.score.initial_bpm_1024 = 160_000;
        music.score.end_tick = 30;
        let EventKind::Notes { source, length, .. } = &mut music.score.first_events[0].kind else {
            unreachable!()
        };
        *source = crate::data::VoiceSource::Sequence {
            group: 0,
            program: 1,
            drums: false,
        };
        *length = 20;
        music.score.loop_events = music.score.first_events.clone();
        let music = Arc::new(music);
        let effect = Arc::new(cue(vec![tone(vec![random]), tone(vec![random])]));
        let render = || {
            let synth = Synthesizer::default();
            let song = Stream::in_synthesizer(music.clone(), true, &synth).unwrap();
            let effect = Stream::in_synthesizer(effect.clone(), false, &synth).unwrap();
            let mut output = Vec::new();
            let cycle = control_deadline(30);
            for _ in 0..cycle * 3 {
                synth.advance().unwrap();
                let frames = [song.shared_frame(), effect.shared_frame()];
                output.push(frames[0].unwrap_or([[0; 2]; 3]));
                let _ = synth.unread_frame();
            }
            for cycle in output.chunks(cycle) {
                assert!(cycle.iter().flatten().flatten().any(|sample| *sample != 0));
            }
            assert!(effect.shared_frame().is_none());
            drop(song);
            for _ in 0..2 * 160 {
                synth.advance().unwrap();
                let _ = synth.unread_frame();
            }
            assert_eq!(synth.unread_frame(), [[0; 2]; 3]);
            output
        };
        assert_eq!(render(), render());
    }

    #[test]
    fn cross_stream_groups_release_kill_and_clear_membership() {
        for kill in [false, true] {
            for clear in [false, true] {
                let synth = Synthesizer::default();
                let group = Command::ExclusiveGroup { group: 5, kill };
                let mut target = vec![group];
                if clear {
                    target.push(Command::ExclusiveGroup { group: 0, kill });
                }
                target.extend([
                    Command::StartSample { sample: 2 },
                    wait(None, true, false),
                    Command::End,
                ]);
                let caller = vec![wait(Some(5), false, false), group, Command::End];
                let players = [target, caller].map(|program| {
                    Stream::in_synthesizer(Arc::new(cue(vec![program])), false, &synth).unwrap()
                });
                let trigger = control_deadline(5);
                let mut heard_before = false;
                let mut heard_after = false;
                let mut final_live = Vec::new();
                for frame in 0..trigger + 4 * 160 {
                    synth.advance().unwrap();
                    let (live, mixed) = mixer_frame(&synth, &players);
                    let audible = mixed.iter().flatten().any(|sample| *sample != 0);
                    if frame < trigger {
                        heard_before |= audible;
                    }
                    if frame >= trigger + 2 * 160 {
                        heard_after |= audible;
                    }
                    final_live = live;
                }
                assert!(heard_before);
                assert_eq!(heard_after, clear);
                assert_eq!(final_live, [clear, false]);
            }
        }
    }

    #[test]
    fn macro_controller_writes_share_sequence_channels_but_not_sound_layers() {
        use crate::data::{Controller, Variable};
        for origin in [ScoreOrigin::Sequence, ScoreOrigin::SoundEffect] {
            let synth = Synthesizer::default();
            let reference_synth = Synthesizer::default();
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
            let mut actual = cue(vec![commands, tone(vec![])]);
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
            let mut expected = cue(vec![vec![Command::End], tone(vec![])]);
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
            let mut references =
                [Stream::in_synthesizer(Arc::new(expected), false, &reference_synth).unwrap()];
            for _ in 0..3 {
                compare_block(&synth, &players, &reference_synth, &mut references);
            }
        }
    }

    #[test]
    fn child_handle_messages_cross_cues_and_wake_after_parent_end() {
        use crate::data::{
            Comparison, MessageTarget,
            Variable::{Global, Local},
        };
        for broadcast in [false, true] {
            let synth = Synthesizer::default();
            let mut producer = cue(vec![vec![
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
            ]]);
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
                        destination: Local(0),
                    },
                    Command::SetVariable {
                        destination: Local(1),
                        value: 42,
                    },
                    Command::Branch {
                        comparison: Comparison::Equal,
                        left: Local(0),
                        right: Local(1),
                        invert: false,
                        instruction: 4,
                    },
                    wait(None, false, false), // A missing/wrong payload must remain live and fail below.
                    Command::Release,
                    Command::End,
                ],
            );
            let controller = cue(vec![vec![
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
            ]]);
            let players = [producer, controller]
                .map(|loaded| Stream::in_synthesizer(Arc::new(loaded), false, &synth).unwrap());
            let delivery = control_deadline(3);
            let mut heard = false;
            let mut completed = false;
            for frame in 0..delivery + 3 * 160 {
                synth.advance().unwrap();
                let (live, mixed) = mixer_frame(&synth, &players);
                if frame < delivery {
                    heard |= mixed.iter().flatten().any(|sample| *sample != 0);
                }
                if live.iter().all(|live| !live) {
                    completed = true;
                    break;
                }
            }
            assert!(
                heard,
                "child must outlive the parent and play before delivery"
            );
            assert!(completed, "child must receive the payload and release");
        }
    }

    #[test]
    fn macro_variables_follow_shared_order_and_children_start_with_fresh_locals() {
        use crate::data::{
            Arithmetic, Comparison, Operand,
            Variable::{Global, Local},
        };
        let synth = Synthesizer::default();
        let reference_synth = Synthesizer::default();
        let mut producer = cue(vec![vec![
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
        ]]);
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
            Arc::new(cue(vec![vec![
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
            ]]))
        };
        let players = [
            Stream::in_synthesizer(Arc::new(producer), false, &synth).unwrap(),
            Stream::in_synthesizer(consumer(2), false, &synth).unwrap(),
        ];
        let mut references = [
            Stream::in_synthesizer(
                Arc::new(cue(vec![vec![Command::End]])),
                false,
                &reference_synth,
            )
            .unwrap(),
            Stream::in_synthesizer(
                Arc::new(cue(vec![tone(vec![wait(Some(2), false, false)])])),
                false,
                &reference_synth,
            )
            .unwrap(),
        ];
        for _ in 0..2 {
            compare_block(&synth, &players, &reference_synth, &mut references);
        }
        drop(players);
        drop(references);
        // The bank belongs to the synthesizer, so destroying both cues leaves
        // the child's -14 available to a later cue.
        let later = consumer(0);
        assert!(Stream::new(later.clone(), false).is_ok());
        let players = [Stream::in_synthesizer(later, false, &synth).unwrap()];
        let mut references =
            [
                Stream::in_synthesizer(Arc::new(cue(vec![tone(vec![])])), false, &reference_synth)
                    .unwrap(),
            ];
        for _ in 0..2 {
            compare_block(&synth, &players, &reference_synth, &mut references);
        }
    }

    #[test]
    fn child_macros_inherit_gain_and_pan_and_outlive_the_parent() {
        let render = |level, pan, pan_delta| {
            let synth = Synthesizer::default();
            let mut loaded = cue(vec![vec![
                Command::SetVolume {
                    factor: 0,
                    offset: 0,
                    curve: Some(crate::data::VolumeCurve([level; 128])),
                    from_velocity: false,
                },
                Command::PanRamp {
                    axis: crate::data::PanAxis::Pan,
                    initial: pan,
                    delta: pan_delta,
                    milliseconds: 0,
                },
                Command::SpawnMacro {
                    program: 2,
                    instruction: 0,
                    key_offset: 0,
                    priority: 9,
                    max_voices: 255,
                },
                Command::End,
            ]]);
            loaded.resources.programs.insert(
                2,
                vec![
                    Command::Interpolation {
                        mode: Interpolation::Direct,
                        coefficients: 0,
                    },
                    Command::Envelope {
                        envelope: crate::data::Envelope::Dls(dls::Definition {
                            timing: dls::Timing {
                                attack_timecents: i32::MIN,
                                decay_timecents: i32::MIN,
                                release_ms: 0,
                                attack_velocity_scale: 0,
                                decay_key_scale: 0,
                            },
                            sustain: 193,
                        }),
                    },
                    Command::StartSample { sample: 2 },
                    wait(Some(5), false, false),
                    Command::End,
                ],
            );
            let sample = Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
            sample.pcm.fill(32000);
            sample.loop_pcm.fill(32000);
            loaded.tables.mix.volume = std::array::from_fn(|i| i as f32 / 128.);
            loaded.tables.mix.volume_16_scale = 1. / (255 << 16) as f32;
            loaded.tables.dls.attenuation =
                std::array::from_fn(|index| ((193 - index) * 32767 / 193) as u16);
            loaded.tables.mix.controller_14_scale = 1. / 16383.;
            loaded.tables.mix.pan = [0., 0.5, 1., 1.];
            loaded.tables.mix.pan_16_scale = 1. / (63 << 16) as f32;
            let player = Stream::in_synthesizer(Arc::new(loaded), false, &synth).unwrap();
            let mut output = Vec::new();
            for _ in 0..32 + control_deadline(5) + 3 * 160 {
                synth.advance().unwrap();
                let (live, frame) = mixer_frame(&synth, std::slice::from_ref(&player));
                output.push(frame[0]);
                if !live[0] {
                    return output;
                }
            }
            panic!("child did not complete after parent ended");
        };
        let full = render(127, 0, 0);
        let half = render(64, 0, 0);
        let right = render(127, 127, 0);
        let loud = render(200, 0, 0);
        assert_eq!(render(127, 0, -127), full);
        assert_eq!(render(127, 127, 127), right);
        assert_eq!(full.len(), half.len());
        assert!(full[..32].iter().all(|frame| *frame == [0; 2]));
        let onset = full.iter().position(|frame| frame[0] != 0).unwrap();
        assert!(onset >= 32);
        assert_eq!(full[onset][1], 0);
        assert!(
            loud[onset][0] > full[onset][0],
            "raw gain above127 must survive bounded envelope velocity"
        );
        assert!(half[onset][0] > 0 && half[onset][0] < full[onset][0]);
        assert_eq!(right[onset][0], 0);
        assert!(right[onset][1] > 0);
    }

    #[test]
    fn refused_child_spawns_leave_the_parent_playing() {
        let synth = Synthesizer::default();
        let reference_synth = Synthesizer::default();
        let mut loaded = cue(vec![tone(vec![
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
        ])]);
        loaded.resources.programs.insert(
            2,
            tone(vec![Command::RandomWait {
                upper_ms: 17,
                key_off: false,
                sample_end: false,
            }]),
        );
        let loaded = Arc::new(loaded);
        assert!(Stream::new(loaded.clone(), false).is_ok());
        let players = [Stream::in_synthesizer(loaded, false, &synth).unwrap()];
        let mut references =
            [
                Stream::in_synthesizer(Arc::new(cue(vec![tone(vec![])])), false, &reference_synth)
                    .unwrap(),
            ];
        for _ in 0..3 {
            compare_block(&synth, &players, &reference_synth, &mut references);
        }
    }
}

#[test]
fn finite_score_keeps_pending_notes_and_ends_when_the_last_macro_finishes() {
    use crate::sequence::{LiveControls, stream::Stream};
    use std::sync::Arc;
    let mut loaded = random_cue(
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
    let second_note = control_deadline(50);
    let onset = control_deadline(10);
    let stop = onset + control_deadline(5);
    let release_frames = crate::release::RELEASE_FRAMES as usize;
    let bound = (second_note + stop + release_frames).div_ceil(160) * 160;
    let mut stream = Stream::new(Arc::new(loaded), false).unwrap();
    let mut pcm = Vec::new();
    while let Some(block) = stream.block(LiveControls::default()).unwrap() {
        pcm.extend(block);
        assert!(
            pcm.len() <= bound,
            "completed macros retained a silent source handle"
        );
    }
    let audible =
        |frames: &[crate::sequence::BusFrame]| frames.iter().flatten().flatten().any(|&v| v != 0);
    assert!(!audible(&pcm[..onset]));
    assert!(audible(&pcm[onset..stop]));
    assert!(!audible(&pcm[stop + release_frames..second_note + onset]));
    assert!(audible(&pcm[second_note + onset..second_note + stop]));
    assert!(!audible(&pcm[second_note + stop + release_frames..]));
    assert!(stream.block(LiveControls::default()).unwrap().is_none());
}

#[test]
fn shared_random_cue_admission_release_and_isolated_rendering_are_explicit() {
    use crate::sequence::{LiveControls, shared::Synthesizer, stream::Stream};
    use std::sync::Arc;
    let random = Command::RandomWait {
        upper_ms: 65535,
        key_off: true,
        sample_end: false,
    };
    let synth = Synthesizer::default();
    // A command submitted inside a prepared audio block enters the next block,
    // without consuming any random values or claiming to have started early.
    for _ in 0..33 {
        synth.advance().unwrap();
    }
    let player = Stream::in_synthesizer(Arc::new(random_cue(10, random)), false, &synth).unwrap();
    for _ in 33..160 {
        synth.advance().unwrap();
        assert!(!player.started());
        assert_eq!(player.shared_frame(), Some([[0; 2]; 3]));
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
    let completion = (160
        + control_deadline(10)
        + control_deadline(5)
        + crate::release::RELEASE_FRAMES as usize)
        .div_ceil(160)
        * 160;
    for _ in 320..=completion {
        synth.advance().unwrap();
    }
    assert!(
        player.shared_frame().is_none(),
        "key-off must bypass the long random wait"
    );
    let loaded = Arc::new(random_cue(0, random));
    assert!(Stream::new(loaded.clone(), false).is_ok());
    assert!(Stream::in_synthesizer(loaded, true, &synth).is_err());
    let zero = Stream::in_synthesizer(
        Arc::new(random_cue(
            0,
            Command::RandomWait {
                upper_ms: 0,
                key_off: false,
                sample_end: false,
            },
        )),
        false,
        &synth,
    )
    .unwrap();
    for _ in 0..160 {
        synth.advance().unwrap();
        if zero.started() {
            break;
        }
    }
    assert!(zero.started());
    let mut completed = false;
    let mut heard = false;
    for _ in 0..control_deadline(5) + 2 * 160 {
        synth.advance().unwrap();
        let (live, frame) = mixer_frame(&synth, std::slice::from_ref(&zero));
        heard |= frame.iter().flatten().any(|sample| *sample != 0);
        if !live[0] {
            completed = true;
            break;
        }
    }
    assert!(
        heard && completed,
        "zero-bound wait must complete without delaying playback"
    );
}

#[test]
fn beat_waits_use_source_frames_sample_tempo_and_allow_key_off() {
    let mut loaded = prepared();
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
    let bpm = 120 * 1024 + 1023;
    let tick_frames =
        (60_u64 * 1024 * u64::from(crate::SOURCE_RATE)).div_ceil(u64::from(bpm) * 384);
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
        &[(0, bpm)],
        None,
    );
    assert_eq!(u64::from(looped.0), (4 * tick_frames).div_ceil(32) * 32);
    assert!(looped.1.iter().any(|&sample| sample != 0));
    let changed = render(
        &mut loaded,
        vec![beats(Some(384), false); 2],
        &[(3200, 60 * 1024)],
        None,
    );
    assert_eq!(
        u64::from(changed.0),
        crate::volume::frames_from_millis(1500)
            .unwrap()
            .div_ceil(32)
            * 32
    );
    assert!(changed.1.iter().any(|&sample| sample != 0));
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
fn pitch_sweep_slots_change_pitch_cancel_and_disable_independently() {
    use crate::data::{Interpolation, SweepSlot};
    let mut loaded = prepared();
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
fn modulators_preserve_vibrato_and_tremolo_input_endpoints() {
    let mut loaded = prepared();
    loaded.tables.mix.volume = std::array::from_fn(|index| (index as f32 / 127.).min(1.));
    loaded.tables.mix.volume_16_scale = 1. / (127 << 16) as f32;
    loaded.tables.mix.controller_14_scale = 1. / 16383.;
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
    assert!(
        plain != render(&mut loaded, vec![vibrato(100)]),
        "vibrato must change played PCM"
    );
    assert!(
        plain == render(&mut loaded, vec![vibrato(100), vibrato(0)]),
        "a disabled oscillator must leave playback unchanged"
    );
    use crate::data::TremoloInput;
    loaded.tables.modulation.tremolo = [1. / 8192., 1. / 4096., 1., 1. / 16384., 1.];
    let tremolo = vec![
        Command::Lfo { period_ms: 20 },
        Command::Tremolo {
            scale: 2048,
            modulation_scale: 4096,
        },
    ];
    let midpoint = render(&mut loaded, tremolo.clone());
    assert_ne!(plain.1, midpoint.1);
    let mut oscillator = tremolo;
    oscillator.push(Command::TremoloInput {
        input: TremoloInput::Lfo,
    });
    let modulated = render(&mut loaded, oscillator.clone());
    assert_ne!(modulated.1, plain.1);
    assert_ne!(modulated.1, midpoint.1);
    for (input, expected) in [
        (TremoloInput::Zero, &plain),
        (TremoloInput::Midpoint, &midpoint),
    ] {
        let mut commands = oscillator.clone();
        commands.push(Command::TremoloInput { input });
        assert_eq!(render(&mut loaded, commands), *expected);
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

    let oversized = Package::load_with(
        "bank.json",
        &mut |path, limit| {
            if path == "sample.wav" {
                Ok(vec![0; limit + 1])
            } else {
                Ok(fs::read(root.0.join(path))?)
            }
        },
        &mut cache,
    );
    assert!(oversized.is_err());

    let sample_path = root.0.join("sample.wav");
    let mut corrupt = fs::read(&sample_path).unwrap();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&sample_path, corrupt).unwrap();
    assert!(Package::load_with("bank.json", &mut read, &mut cache).is_err());
    fs::remove_file(sample_path).unwrap();
    assert!(Package::load_with("bank.json", &mut read, &mut cache).is_err());
}

#[test]
fn sample_cache_releases_pcm_after_live_owners_drop_and_reprepares() {
    use std::sync::Arc;
    let (root, value) = fixture();
    fs::write(
        root.0.join("bank.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let mut cache = SampleCache::default();
    let mut read = |path: &str, limit| read_bounded(&root.0.join(path), limit);
    let first = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    let second = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    let sample = Arc::downgrade(&first.resources.samples[&2]);
    let live_sample = first.resources.samples[&2].clone();
    drop((first, second));
    assert!(sample.upgrade().is_some());
    drop(live_sample);
    assert!(sample.upgrade().is_none());

    let mut replacement = value;
    replacement["samples"]["2"]["key"] = 61.into();
    fs::write(
        root.0.join("bank.json"),
        serde_json::to_vec(&replacement).unwrap(),
    )
    .unwrap();
    let loaded = Package::load_with("bank.json", &mut read, &mut cache).unwrap();
    assert_eq!(loaded.resources.samples[&2].pcm, [10, 20, 30, 40]);
    assert_eq!(loaded.resources.samples[&2].key, 61);
    assert_eq!(
        cache.0.len(),
        1,
        "preparation prunes expired sample entries"
    );
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
    let mut loaded = prepared();
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
    // A note sustains across several loop restarts. Compare it with the same
    // passage written out on a continuous score timeline.
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
            length: 26,
        },
    };
    // An explicitly written-out passage supplies an independent timing
    // expectation, without taking a loop or sharing its clock handoff.
    loaded.score.first_events = [1, 9, 17, 25, 33, 41, 49].map(event).into();
    let mut loaded = Arc::new(loaded);
    let expected = sequence::render_preview(loaded.clone(), loaded.reverbs, 1600).unwrap();
    assert!(expected.pcm.iter().any(|sample| *sample != 0));
    assert!(
        expected
            .voice_lifetimes
            .windows(2)
            .all(|notes| notes[0].start_frame < notes[1].start_frame)
    );
    assert!(
        expected
            .voice_lifetimes
            .iter()
            .any(|note| note.end_frame.is_some())
    );
    let score = &mut Arc::get_mut(&mut loaded).unwrap().score;
    score.end_tick = 8;
    score.first_events = vec![event(1)];
    score.loop_events = vec![event(1)];
    let mut studio = Studio::new(loaded.reverbs).unwrap();
    let mut stream = Stream::new(loaded, true).unwrap();
    let mut actual = Vec::new();
    for _ in 0..10 {
        for buses in stream.block(LiveControls::default()).unwrap().unwrap() {
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
fn existing_voices_follow_each_control_quantum_without_backdating_pcm() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note, VoiceSource},
        sequence::{LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let mut loaded = prepared();
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
    loaded.score.first_events = vec![Event {
        tick: 0,
        channel: 0,
        kind: EventKind::Notes {
            source: VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 1,
                max_voices: 1,
            }],
            length: 90,
        },
    }];
    let mut stream = Stream::new(Arc::new(loaded), false).unwrap();
    for _ in 0..10 {
        let controls = std::array::from_fn(|quantum| LiveControls {
            volume: quantum as f32 / 4.,
            ..Default::default()
        });
        let output = stream.block_envelope(controls).unwrap().unwrap();
        let levels: Vec<i64> = output
            .chunks_exact(32)
            .map(|quantum| {
                quantum
                    .iter()
                    .flat_map(|frame| frame[0])
                    .map(|sample| i64::from(sample).abs())
                    .sum()
            })
            .collect();
        assert_eq!(levels.len(), 5);
        assert_eq!(levels[0], 0, "later gain rewrote the silent first quantum");
        assert!(
            levels.windows(2).all(|pair| pair[0] < pair[1]),
            "the held voice must follow every increasing gain within the block: {levels:?}"
        );
    }
}

#[test]
fn mono_centers_live_voices_and_preserves_pan_changes_for_stereo() {
    use crate::{
        data::{
            ControlTarget, Controller, Event, EventKind, Interpolation, Note, Operand, ScoreOrigin,
            Variable, VoiceSource,
        },
        sequence::{LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let score = |centered, sound, selected| {
        let mut loaded = prepared();
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
        if sound {
            let hold = loaded.resources.programs[&1].clone();
            loaded.resources.programs.insert(2, hold);
            let program = loaded.resources.programs.get_mut(&1).unwrap();
            program.truncate(2);
            program.extend([
                Command::Wait {
                    milliseconds: Some(20),
                    from_start: false,
                    key_off: false,
                    sample_end: false,
                },
                Command::SetVariable {
                    destination: Variable::Controller(Controller::Paired(10)),
                    value: (if centered { 64 } else { 113 }) << 7,
                },
                Command::SpawnMacro {
                    program: 2,
                    instruction: 0,
                    key_offset: 0,
                    priority: 64,
                    max_voices: 255,
                },
                Command::End,
            ]);
            loaded.score.origin = ScoreOrigin::SoundEffect;
        }
        if selected {
            for program in loaded.resources.programs.values_mut() {
                program.insert(
                    0,
                    Command::SelectControl {
                        target: ControlTarget::Pan,
                        source: Operand::Constant(0),
                        scale: 0,
                    },
                );
            }
        }
        loaded.score.initial_bpm_1024 = 160_000; // One tick per millisecond.
        loaded.score.controls[0].paired[10] = (if centered { 64 } else { 17 }) << 7;
        loaded.score.first_events = vec![
            Event {
                tick: 0,
                channel: 0,
                kind: EventKind::Notes {
                    source: if sound {
                        VoiceSource::SoundEffect { id: 1 }
                    } else {
                        VoiceSource::Sequence {
                            group: 0,
                            program: 0,
                            drums: false,
                        }
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
        if sound {
            loaded.score.first_events.truncate(1);
        }
        Arc::new(loaded)
    };
    for (sound, selected) in [(false, false), (true, false), (false, true), (true, true)] {
        let panned = score(false, sound, selected);
        let mut stereo = Stream::new(panned.clone(), false).unwrap();
        let mut changing = Stream::new(panned.clone(), false).unwrap();
        let mut overridden = Stream::new(panned, false).unwrap();
        // An authored centered score supplies the expectation without using mono mode.
        let mut center = Stream::new(score(true, sound, selected), false).unwrap();
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
            let external = overridden
                .block(LiveControls {
                    pan: (3..10).contains(&block).then_some(32),
                    ..Default::default()
                })
                .unwrap()
                .unwrap();
            if !(3..12).contains(&block) {
                assert_eq!(external, stereo, "cleared override, block {block}");
            } else if (5..10).contains(&block) {
                assert_ne!(external, stereo, "override must remain audible");
                if !selected {
                    assert!(
                        external.iter().any(|buses| buses[0][1] > 0),
                        "authored pan change was overwritten"
                    );
                }
            }
            if block < 3 || (5..10).contains(&block) || block >= 12 {
                assert_ne!(stereo, center, "fixture must exercise audible panning");
                assert_eq!(actual, if mono { center } else { stereo }, "block {block}");
            }
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
    let mut loaded = prepared();
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
    let mut loaded = Arc::new(loaded);
    let preview = sequence::render_preview(loaded.clone(), loaded.reverbs, 2000).unwrap();
    assert_eq!(preview.voice_lifetimes.len(), 2);
    assert_eq!(
        preview.voice_lifetimes[0].end_frame,
        Some(preview.voice_lifetimes[1].start_frame)
    );
    assert!(preview.voice_lifetimes[1].end_frame.is_none());
    Arc::get_mut(&mut loaded)
        .unwrap()
        .score
        .first_events
        .truncate(1);
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
}

#[test]
fn spatial_voices_drain_the_last_sample_and_envelope_release_in_both_channels() {
    use crate::{
        data::{Envelope, Interpolation},
        envelope::Parameters,
        sequence::stream::Stream,
    };
    use std::sync::Arc;
    for envelope_end in [false, true] {
        for left_delay in [0, 16, 32] {
            let mut loaded = random_cue(0, Command::Noop);
            let sample = Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
            sample.rate = crate::SOURCE_RATE as u16;
            sample.loop_start = 0;
            if envelope_end {
                sample.pcm = vec![32000; 64];
                sample.loop_pcm = sample.pcm.clone();
                sample.loop_length = 64;
            } else {
                sample.pcm = vec![0; 64];
                *sample.pcm.last_mut().unwrap() = 32000;
                sample.loop_pcm.clear();
                sample.loop_length = 0;
            }
            loaded.tables.mix.spatial = Some(mix::Spatial {
                pan_scale: 1.,
                left_delay: [left_delay; 128],
            });
            let mut commands = vec![
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::VolumeCurve {
                    alternate: false,
                    interaural_delay: true,
                },
                Command::Envelope {
                    envelope: Envelope::Ordinary(Parameters {
                        release_ms: 1,
                        ..Default::default()
                    }),
                },
                Command::StartSample { sample: 2 },
            ];
            let last_input = if envelope_end {
                commands.extend([
                    Command::Wait {
                        milliseconds: Some(2),
                        from_start: false,
                        key_off: false,
                        sample_end: false,
                    },
                    Command::Release,
                ]);
                control_deadline(2) + crate::volume::frames_from_millis(1).unwrap() as usize - 1
            } else {
                63
            };
            commands.push(Command::End);
            loaded.resources.programs.insert(1, commands);
            let mut stream = Stream::new(Arc::new(loaded), false).unwrap();
            let mut pcm = Vec::new();
            let bound = (last_input + 33 + crate::RELEASE_FRAMES as usize).div_ceil(160) * 160;
            while let Some(block) = stream.block(Default::default()).unwrap() {
                pcm.extend(block.into_iter().map(|frame| frame[0]));
                assert!(pcm.len() <= bound, "spatial tail did not complete");
            }
            for (channel, delay) in [left_delay, 32 - left_delay].into_iter().enumerate() {
                let last = pcm.iter().rposition(|frame| frame[channel] != 0);
                assert_eq!(
                    last,
                    Some(last_input + usize::from(delay)),
                    "envelope_end={envelope_end}, channel={channel}"
                );
                assert!(pcm[last.unwrap()][channel] > 0);
            }
        }
    }
}

mod live_adsr {
    use super::*;
    use crate::{data::Envelope as Definition, envelope::Parameters};

    fn wait(milliseconds: u16) -> Command {
        Command::Wait {
            milliseconds: Some(milliseconds),
            from_start: false,
            key_off: false,
            sample_end: false,
        }
    }

    fn envelope(attack_ms: u16, sustain: u16) -> Command {
        Command::Envelope {
            envelope: Definition::Ordinary(Parameters {
                attack_ms,
                decay_ms: 0,
                sustain,
                release_ms: 4,
            }),
        }
    }

    fn render(commands: impl IntoIterator<Item = Command>) -> Vec<i32> {
        let mut loaded = prepared();
        let sample = std::sync::Arc::make_mut(loaded.resources.samples.get_mut(&2).unwrap());
        sample.pcm.fill(32000);
        sample.loop_pcm.fill(32000);
        let mut program = vec![Command::Interpolation {
            mode: crate::data::Interpolation::Direct,
            coefficients: 0,
        }];
        program.extend(commands);
        program.push(Command::End);
        let duration: u64 = program
            .iter()
            .filter_map(|command| match command {
                Command::Wait {
                    milliseconds: Some(ms),
                    ..
                } => Some(control_deadline(u64::from(*ms)) as u64),
                _ => None,
            })
            .sum();
        let mut score = random_cue(0, Command::Noop).score;
        score.end_tick = u32::MAX;
        loaded.score = score;
        loaded.resources.programs.insert(1, program);
        let mut stream =
            crate::sequence::stream::Stream::new(std::sync::Arc::new(loaded), false).unwrap();
        let bound = (duration as usize + control_deadline(4) + 160).div_ceil(160) * 160;
        let mut pcm = Vec::new();
        while let Some(block) = stream.block(Default::default()).unwrap() {
            assert!(block.iter().all(|frame| frame[0][0] == frame[0][1]));
            pcm.extend(block.iter().map(|frame| frame[0][0]));
            assert!(pcm.len() <= bound, "envelope voice did not complete");
        }
        assert_eq!(pcm.last(), Some(&0));
        pcm
    }

    #[test]
    fn envelope_changes_affect_only_subsequent_samples() {
        let full = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(15),
        ]);
        let changed = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(7),
            envelope(0, 8192),
            wait(8),
        ]);
        let change = control_deadline(7);
        assert_eq!(full[..change], changed[..change]);
        assert!(changed[change] > 0 && changed[change] < full[change]);
    }

    #[test]
    fn delayed_attack_begins_at_the_note_onset() {
        let pcm = render([
            wait(3),
            envelope(2, 32767),
            Command::StartSample { sample: 2 },
            wait(10),
        ]);
        let onset = control_deadline(3);
        let attack = crate::volume::frames_from_millis(2).unwrap() as usize;
        assert!(pcm[..=onset].iter().all(|&sample| sample == 0));
        assert!(pcm[onset + 1] > 0);
        assert!(
            pcm[onset..=onset + attack]
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );
        assert!(pcm[onset + attack] > 30_000);
    }

    #[test]
    fn retrigger_preserves_played_audio_and_starts_a_fresh_envelope() {
        let base = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(5),
            Command::Release,
            wait(10),
        ]);
        let retrigger = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(5),
            Command::Release,
            wait(2),
            Command::StartSample { sample: 2 },
            wait(8),
        ]);
        let onset = control_deadline(5) + control_deadline(2);
        assert_eq!(base[..onset], retrigger[..onset]);
        assert!(retrigger[onset] > base[onset]);
        assert!(retrigger[onset] > 30_000);
    }

    #[test]
    fn release_before_start_does_not_release_the_future_sample() {
        let baseline = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(8),
        ]);
        let released = render([
            envelope(0, 32767),
            Command::Release,
            Command::StartSample { sample: 2 },
            wait(8),
        ]);
        assert_eq!(released, baseline);
    }

    #[test]
    fn repeated_release_keeps_its_endpoint_and_replacement_can_release_again() {
        let baseline = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(3),
            Command::Release,
            wait(2),
            wait(8),
        ]);
        let repeated = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(3),
            Command::Release,
            Command::Release,
            wait(2),
            Command::Release,
            wait(8),
        ]);
        assert_eq!(repeated, baseline);
        let replaced = render([
            envelope(0, 32767),
            Command::StartSample { sample: 2 },
            wait(3),
            Command::Release,
            wait(2),
            envelope(0, 32767),
            Command::Release,
            wait(8),
        ]);
        let replacement = control_deadline(3) + control_deadline(2);
        assert_eq!(replaced[..replacement], baseline[..replacement]);
        assert!(replaced[replacement] > baseline[replacement]);
        let endpoint = replacement + crate::volume::frames_from_millis(4).unwrap() as usize;
        assert!(
            replaced[endpoint + crate::RELEASE_FRAMES as usize..]
                .iter()
                .all(|&sample| sample == 0)
        );
    }
}

#[test]
fn finite_preview_keeps_reverb_after_the_source_finishes() {
    use crate::{
        data::{Event, EventKind, Interpolation, Note, ScoreOrigin, VoiceSource},
        sequence::{self, LiveControls, stream::Stream},
    };
    use std::sync::Arc;
    let mut loaded = prepared();
    loaded.resources.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::VolumeControl { value: 16383 },
            Command::Auxiliary { bus: 0, value: 127 },
            Command::StartSample { sample: 2 },
            Command::End,
        ],
    );
    loaded.resources.samples.insert(
        2,
        Arc::new(Sample {
            key: 60,
            rate: crate::SOURCE_RATE as u16,
            pcm: vec![16000; 32],
            loop_start: 0,
            loop_length: 0,
            loop_pcm: vec![],
        }),
    );
    loaded.tables.mix.volume = std::array::from_fn(|index| (index as f32 / 127.).min(1.));
    loaded.tables.mix.volume_16_scale = 1. / (127 << 16) as f32;
    loaded.tables.mix.controller_14_scale = 1. / 16383.;
    loaded.score.origin = ScoreOrigin::SoundEffect;
    loaded.score.first_events = vec![Event {
        tick: 0,
        channel: 0,
        kind: EventKind::Notes {
            source: VoiceSource::SoundEffect { id: 1 },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity: 127,
                pan: 64,
                priority: 1,
                max_voices: 1,
            }],
            length: 99,
        },
    }];
    let reverbs = [[0.5, 0.5, 1., 0.5, 0.02]; 2];
    let loaded =
        Arc::new(Loaded::new(loaded.resources, loaded.score, loaded.tables, reverbs).unwrap());
    let mut source = Stream::new(loaded.clone(), false).unwrap();
    let mut source_frames = 0;
    while let Some(block) = source.block(LiveControls::default()).unwrap() {
        source_frames += block.len();
        assert!(source_frames < 1600, "finite source did not finish");
    }
    let preview = sequence::render_preview(loaded, reverbs, 4096).unwrap();
    assert_eq!(preview.pcm.len(), 4096 * 2);
    assert!(
        preview.pcm[(source_frames + 320) * 2..]
            .iter()
            .any(|sample| *sample != 0),
        "preview discarded the effect tail when its source completed"
    );
    assert_eq!(preview.notes, 1);
    assert_eq!(preview.maximum_voices, 1);
    assert_eq!(preview.voice_lifetimes.len(), 1);
    assert!(preview.voice_lifetimes[0].end_frame.is_some());
}
