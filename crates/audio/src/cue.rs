//! Cooked cue programs mixed through shared effects.
use crate::{
    package::Loaded,
    reverb,
    sequence::{BusFrame, LiveControls, shared::Synthesizer, stream::Stream},
};
use anyhow::{Result, ensure};
use std::sync::Arc;

pub mod package;

pub struct Studio {
    voices: Vec<Stream>,
    synthesizer: Synthesizer,
    effects: reverb::Studio,
    control: LiveControls,
}

impl Studio {
    pub fn new(parameters: [[f32; 5]; 2]) -> Result<Self> {
        Ok(Self {
            voices: Vec::new(),
            synthesizer: Synthesizer::default(),
            control: LiveControls {
                pan: Some(64),
                ..Default::default()
            },
            effects: reverb::Studio::new(parameters)?,
        })
    }

    pub fn play(&mut self, cue: Arc<Loaded>) -> Result<()> {
        let stream = Stream::in_synthesizer(cue, false, &self.synthesizer)?;
        stream.set_shared_controls([self.control; crate::CONTROLS_PER_BLOCK])?;
        self.voices.push(stream);
        Ok(())
    }

    pub fn set_group_volume(&mut self, volume: f32) -> Result<()> {
        ensure!(
            volume.is_finite() && (0.0..=1.0).contains(&volume),
            "invalid cue group volume"
        );
        self.control.volume = volume;
        for voice in &self.voices {
            voice.set_shared_controls([self.control; crate::CONTROLS_PER_BLOCK])?;
        }
        Ok(())
    }

    /// A failed program is retired before reporting its error. Returning Ok from
    /// the callback keeps the remaining voices and shared effect tail audible.
    pub fn next_frame(
        &mut self,
        mut on_error: impl FnMut(anyhow::Error) -> Result<()>,
    ) -> Result<[i16; 2]> {
        let error = self.synthesizer.advance().err();
        let mut buses = [[0i32; 2]; 3];
        let mut mix = |input: BusFrame| {
            for (target, input) in buses.iter_mut().zip(input) {
                for (channel, sample) in target.iter_mut().zip(input) {
                    *channel += sample;
                }
            }
        };
        self.voices.retain(|voice| match voice.shared_frame() {
            Some(input) => {
                mix(input);
                true
            }
            None => false,
        });
        mix(self.synthesizer.unread_frame());
        if let Some(error) = error {
            on_error(error.context("cue program rendering failed"))?;
        }
        Ok(self
            .effects
            .process(buses)
            .map(|sample| sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture(
        commands: Vec<crate::data::Command>,
    ) -> (crate::package::tests::Fixture, crate::package::Package) {
        let (root, value) = crate::package::tests::fixture();
        let mut package: crate::package::Package = serde_json::from_value(value).unwrap();
        package.programs.insert(1, commands);
        package.score = sound_score(package.score);
        (root, package)
    }

    fn sound_score(mut score: crate::data::Score) -> crate::data::Score {
        use crate::data::{Event, EventKind, Note, ScoreOrigin, VoiceSource};
        score.origin = ScoreOrigin::SoundEffect;
        score.first_events = vec![Event {
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
        score
    }

    fn signal_cue(
        mode: crate::data::Interpolation,
        key: u8,
        pcm: Vec<i16>,
        automation: Vec<crate::data::Command>,
    ) -> Arc<Loaded> {
        use crate::data::{Command, EventKind};
        let commands = [
            Command::Interpolation {
                mode,
                coefficients: 0,
            },
            Command::VolumeControl { value: 16383 },
            Command::StartSample { sample: 2 },
        ]
        .into_iter()
        .chain(automation)
        .chain([Command::End])
        .collect();
        let (mut resources, score, mut tables) = crate::package::tests::playback_data();
        let mut score = sound_score(score);
        let EventKind::Notes { voices, .. } = &mut score.first_events[0].kind else {
            unreachable!()
        };
        voices[0].key = key;
        voices[0].max_voices = crate::sequence::VOICE_BUDGET as u8;
        tables.mix.volume = std::array::from_fn(|index| (index as f32 / 127.).min(1.));
        tables.mix.volume_16_scale = 1. / (127 << 16) as f32;
        tables.mix.controller_14_scale = 1. / 16383.;
        // A normalized two-tap fractional-delay filter is enough to exercise
        // actual polyphase selection without relying on an imported sound bank.
        tables.coefficients.0[0] =
            std::array::from_fn(|phase| [0, 0, 32767 - phase as i16 * 256, phase as i16 * 256]);
        resources.programs.insert(1, commands);
        resources.samples = std::collections::BTreeMap::from([(
            2,
            Arc::new(crate::sample::Sample {
                key: 60,
                rate: (crate::SOURCE_RATE / 2) as u16,
                pcm,
                loop_pcm: vec![],
                loop_start: 0,
                loop_length: 0,
            }),
        )]);
        let loaded =
            crate::package::Loaded::new(resources, score, tables, [[0., 0., 1., 0., 0.]; 2])
                .unwrap();
        Arc::new(loaded)
    }

    fn render_signal(cue: Arc<Loaded>) -> Vec<[i16; 2]> {
        let mut studio = Studio::new([[0., 0., 1., 0., 0.]; 2]).unwrap();
        studio.play(cue).unwrap();
        let output = (0..crate::SOURCE_RATE)
            .map(|_| studio.next_frame(Err).unwrap())
            .collect();
        assert!(
            studio.voices.is_empty(),
            "finite analytic cue did not complete"
        );
        output
    }

    #[test]
    fn cue_resampling_preserves_tone_frequency_and_amplitude_across_octaves() {
        use crate::data::Interpolation;
        const PERIOD: usize = 64;
        const AMPLITUDE: f64 = 8192.;
        let pcm: Vec<_> = (0..4096)
            .map(|frame| {
                (AMPLITUDE * (std::f64::consts::TAU * frame as f64 / PERIOD as f64).sin()).round()
                    as i16
            })
            .collect();
        // Linear interpolation's sine curvature bound, plus integer PCM/gain rounding.
        let error_bound = AMPLITUDE * (std::f64::consts::TAU / PERIOD as f64).powi(2) / 8. + 4.;
        for mode in [Interpolation::Linear, Interpolation::Polyphase] {
            for (key, output_period) in [(48, 256), (60, 128), (72, 64)] {
                let output = render_signal(signal_cue(mode, key, pcm.clone(), vec![]));
                let filter_span = 4 * output_period / PERIOD;
                let source_end = pcm.len() * output_period / PERIOD;
                for window in output[filter_span..source_end - filter_span].chunks_exact(1024) {
                    let (mut energy, mut sine, mut cosine) = (0., 0., 0.);
                    for (frame, sample) in window.iter().enumerate() {
                        assert_eq!(sample[0], sample[1]);
                        let sample = f64::from(sample[0]);
                        let phase = std::f64::consts::TAU * frame as f64 / output_period as f64;
                        energy += sample * sample;
                        sine += sample * phase.sin();
                        cosine += sample * phase.cos();
                    }
                    let frames = window.len() as f64;
                    let rms = (energy / frames).sqrt();
                    assert!(
                        (rms - AMPLITUDE / 2_f64.sqrt()).abs() <= error_bound,
                        "key {key}: RMS {rms}"
                    );
                    let fundamental = 2. * (sine * sine + cosine * cosine) / frames;
                    let residual = ((energy - fundamental).max(0.) / frames).sqrt();
                    assert!(
                        residual <= error_bound,
                        "key {key}: wrong frequency or distorted PCM ({residual})"
                    );
                }
                assert!(
                    output[source_end + filter_span + crate::RELEASE_FRAMES as usize..]
                        .iter()
                        .all(|s| *s == [0; 2])
                );
            }
        }
    }

    #[test]
    fn finite_cues_render_the_last_sample_through_both_filters() {
        use crate::data::Interpolation;
        let mut pcm = vec![0; 64];
        *pcm.last_mut().unwrap() = 16000;
        for mode in [Interpolation::Linear, Interpolation::Polyphase] {
            let output = render_signal(signal_cue(mode, 60, pcm.clone(), vec![]));
            assert!(output[..128].iter().all(|sample| *sample == [0; 2]));
            assert!(
                output[128..136].iter().any(|sample| sample[0] > 1000),
                "last sample was discarded with filter history"
            );
            assert!(
                output[136 + crate::RELEASE_FRAMES as usize..]
                    .iter()
                    .all(|sample| *sample == [0; 2])
            );
        }
    }

    #[test]
    fn cue_automation_is_causal_reaches_its_endpoint_and_can_be_interrupted() {
        use crate::data::{Command, Interpolation};
        let wait = |milliseconds| Command::Wait {
            milliseconds: Some(milliseconds),
            from_start: false,
            key_off: false,
            sample_end: false,
        };
        let start = crate::volume::frames_from_millis(50).unwrap().div_ceil(32) * 32;
        let duration = crate::volume::frames_from_millis(100).unwrap();
        let restore = start + crate::volume::frames_from_millis(50).unwrap().div_ceil(32) * 32;
        for mode in [Interpolation::Linear, Interpolation::Polyphase] {
            let baseline = render_signal(signal_cue(mode, 60, vec![8192; 4096], vec![]));
            for interrupted in [false, true] {
                let mut commands = vec![
                    wait(50),
                    Command::FadeVolume {
                        factor: 0,
                        offset: 0,
                        curve: None,
                        milliseconds: 100,
                        from_silence: false,
                    },
                ];
                if interrupted {
                    commands.extend([
                        wait(50),
                        Command::SetVolume {
                            factor: 0,
                            offset: 127,
                            curve: None,
                            from_velocity: false,
                        },
                    ]);
                }
                let output = render_signal(signal_cue(mode, 60, vec![8192; 4096], commands));
                for frame in 0..(start + duration + 160) as usize {
                    let gain = if frame < start as usize || interrupted && frame >= restore as usize
                    {
                        1.
                    } else {
                        1. - ((frame as u64 - start) as f64 / duration as f64).min(1.)
                    };
                    for channel in 0..2 {
                        let expected = f64::from(baseline[frame][channel]) * gain;
                        assert!(
                            (f64::from(output[frame][channel]) - expected).abs() <= 2.,
                            "frame {frame}, interrupted {interrupted}: expected {expected}, got {}",
                            output[frame][channel]
                        );
                    }
                }
                if !interrupted {
                    assert!(
                        output[(start + duration) as usize..]
                            .iter()
                            .all(|sample| *sample == [0; 2])
                    );
                }
            }
        }
    }

    #[test]
    fn failed_program_is_reported_once_while_healthy_cues_continue() {
        let broken = signal_cue(
            crate::data::Interpolation::Direct,
            60,
            vec![0; 16],
            vec![crate::data::Command::Jump {
                program: 1,
                instruction: 3,
            }],
        );
        let healthy = signal_cue(
            crate::data::Interpolation::Direct,
            60,
            vec![123; 16],
            vec![],
        );
        let expected = render_signal(healthy.clone());
        assert!(expected.iter().any(|frame| *frame != [0; 2]));
        for paranoid in [false, true] {
            let mut studio = Studio::new([[0., 0., 1., 0., 0.]; 2]).unwrap();
            studio.play(broken.clone()).unwrap();
            studio.play(healthy.clone()).unwrap();
            let mut errors = 0;
            let first = studio.next_frame(|error| {
                errors += 1;
                assert!(format!("{error:#}").contains("instruction budget"));
                if paranoid { Err(error) } else { Ok(()) }
            });
            assert_eq!(errors, 1);
            assert_eq!(first.is_err(), paranoid);
            let mut output: Vec<_> = first.into_iter().collect();
            let skipped = usize::from(paranoid);
            while !studio.voices.is_empty() && skipped + output.len() < expected.len() {
                output.push(studio.next_frame(Err).unwrap());
            }
            assert!(studio.voices.is_empty(), "healthy cue did not complete");
            assert_eq!(output, expected[skipped..skipped + output.len()]);
        }
    }

    #[test]
    fn cue_programs_share_global_controls() {
        use crate::data::{Command, ControlTarget, Interpolation, Operand, Variable};
        let writer = signal_cue(
            Interpolation::Direct,
            60,
            vec![0; 16],
            vec![Command::SetVariable {
                destination: Variable::Global(0),
                value: -8192,
            }],
        );
        let reader = signal_cue(
            Interpolation::Direct,
            60,
            vec![8192; 64],
            vec![Command::SelectControl {
                target: ControlTarget::Volume,
                source: Operand::Variable(Variable::Global(0)),
                scale: 65536,
            }],
        );
        let isolated = render_signal(reader.clone());
        assert!(isolated.iter().any(|frame| *frame != [0; 2]));
        let mut studio = Studio::new([[0., 0., 1., 0., 0.]; 2]).unwrap();
        studio.play(writer).unwrap();
        studio.play(reader).unwrap();
        for _ in isolated {
            assert_eq!(studio.next_frame(Err).unwrap(), [0; 2]);
        }
        assert!(studio.voices.is_empty());
    }

    #[test]
    fn overlapping_cues_share_the_native_voice_budget() {
        let cue = signal_cue(
            crate::data::Interpolation::Direct,
            60,
            vec![128; 64],
            vec![],
        );
        let expected = render_signal(cue.clone());
        let mut studio = Studio::new([[0., 0., 1., 0., 0.]; 2]).unwrap();
        for _ in 0..=crate::sequence::VOICE_BUDGET {
            studio.play(cue.clone()).unwrap();
        }
        for frame in expected {
            assert_eq!(
                studio.next_frame(Err).unwrap(),
                frame.map(|sample| sample * crate::sequence::VOICE_BUDGET as i16)
            );
        }
        assert!(studio.voices.is_empty());
    }

    #[test]
    fn audible_program_failure_keeps_its_release_and_the_healthy_cue() {
        use crate::data::{Command, Interpolation};
        let healthy = signal_cue(Interpolation::Direct, 60, vec![1000; 4096], vec![]);
        let expected = render_signal(healthy.clone());
        let broken = signal_cue(
            Interpolation::Direct,
            60,
            vec![4000; 4096],
            vec![
                Command::Wait {
                    milliseconds: Some(10),
                    from_start: false,
                    key_off: false,
                    sample_end: false,
                },
                Command::Jump {
                    program: 1,
                    instruction: 4,
                },
            ],
        );
        let mut studio = Studio::new([[0., 0., 1., 0., 0.]; 2]).unwrap();
        studio.play(broken).unwrap();
        studio.play(healthy).unwrap();
        let mut errors = Vec::new();
        let output: Vec<_> = (0..expected.len())
            .map(|frame| {
                studio
                    .next_frame(|error| {
                        assert!(format!("{error:#}").contains("instruction budget"));
                        errors.push(frame);
                        Ok(())
                    })
                    .unwrap()
            })
            .collect();
        assert_eq!(errors.len(), 1);
        let failed_at = errors[0];
        assert!(failed_at > 0 && output[failed_at - 1][0] > expected[failed_at - 1][0]);
        assert!(
            output[failed_at][0] > expected[failed_at][0],
            "audible failure lost its release"
        );
        let released_at = failed_at + crate::RELEASE_FRAMES as usize;
        assert_eq!(output[released_at..], expected[released_at..]);
        assert!(studio.voices.is_empty());
    }

    #[test]
    fn simultaneous_auxiliary_returns_quantize_after_voice_summing() {
        let pcm = vec![4; 16];
        let completion_limit =
            (pcm.len() * 2 + crate::RELEASE_FRAMES as usize).div_ceil(160) * 160 + 160;
        let cue = signal_cue(
            crate::data::Interpolation::Direct,
            60,
            pcm,
            vec![crate::data::Command::Auxiliary { bus: 1, value: 127 }],
        );
        let parameters = [[1.0, 0.5, 1.0, 0.8, 0.01]; 2];
        let mut single = Studio::new(parameters).unwrap();
        let mut overlap = Studio::new(parameters).unwrap();
        single.play(cue.clone()).unwrap();
        overlap.play(cue.clone()).unwrap();
        overlap.play(cue).unwrap();
        let mut rounded_after_summing = false;
        for _ in 0..completion_limit {
            let one = single.next_frame(Err).unwrap();
            let two = overlap.next_frame(Err).unwrap();
            rounded_after_summing |= (0..2).any(|channel| two[channel] > one[channel] * 2);
            if single.voices.is_empty() && overlap.voices.is_empty() {
                break;
            }
        }
        assert!(
            rounded_after_summing,
            "auxiliary returns were rounded per voice"
        );
        assert!(single.voices.is_empty() && overlap.voices.is_empty());
    }
}
