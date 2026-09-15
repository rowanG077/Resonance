//! Cooked cue PCM and controls mixed through live group gain and shared effects.
use crate::{mix, reverb};
use anyhow::{Result, ensure};
use std::sync::Arc;

pub mod package;

pub type Frame = [[i16; 2]; 3];

pub struct Cue {
    data: Data,
}

enum Data {
    Buses(Vec<Frame>),
    Program {
        package: Arc<crate::package::Loaded>,
        frames: usize,
    },
    Controlled {
        pcm: Vec<i16>,
        controls: Vec<Control>,
        tables: Arc<mix::Tables>,
    },
}

/// One five-millisecond voice-control target, before group volume conversion.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Control {
    pub volume: u32,
    pub controller: u16,
    pub pan: u8,
    pub post: [u16; 2],
}

impl Control {
    fn targets(self, tables: &mix::Tables, group_volume: f32) -> [[u16; 2]; 3] {
        tables.gains_for(mix::Parameters {
            pre: [0; 2],
            volume: self.volume,
            controller: self.controller,
            pan: u32::from(self.pan) << 16,
            post: self.post,
            scale: 1.0,
            group_volume,
            aux_a: 127,
            alternate: false,
            interaural_delay: false,
        })
    }
}

impl Cue {
    pub fn program(package: Arc<crate::package::Loaded>, frames: usize) -> Result<Self> {
        ensure!((1..=320_000).contains(&frames), "invalid cue duration");
        ensure!(
            !crate::sequence::shared::requires_shared(&package.resources),
            "menu cue requires shared synthesizer state"
        );
        Ok(Self {
            data: Data::Program { package, frames },
        })
    }
    pub fn new(frames: Vec<Frame>) -> Result<Self> {
        ensure!(
            !frames.is_empty() && frames.len() <= 320_000,
            "invalid cue duration"
        );
        Ok(Self {
            data: Data::Buses(frames),
        })
    }

    /// PCM already includes the linear envelope; retain nonlinear gain controls
    /// so category fades are applied before per-voice quantization and effects.
    pub fn controlled(
        pcm: Vec<i16>,
        controls: Vec<Control>,
        tables: Arc<mix::Tables>,
    ) -> Result<Self> {
        ensure!(
            !pcm.is_empty() && pcm.len() <= 320_000,
            "invalid cue duration"
        );
        ensure!(
            controls.len() == pcm.len().div_ceil(160),
            "cue control count differs from duration"
        );
        ensure!(
            controls.iter().all(|c| c.volume <= 127 << 16
                && c.controller <= 16383
                && c.pan <= 127
                && c.post.iter().all(|v| *v <= 16383)),
            "invalid cue controls"
        );
        tables.validate()?;
        Ok(Self {
            data: Data::Controlled {
                pcm,
                controls,
                tables,
            },
        })
    }
}

struct Voice {
    cue: Arc<Cue>,
    frame: usize,
    gains: Option<[[mix::GainRamp; 2]; 3]>,
    program: Option<Program>,
}
struct Program {
    stream: crate::sequence::stream::Stream,
    block: Box<[crate::sequence::BusFrame; 160]>,
    length: usize,
}

pub struct Studio {
    voices: Vec<Voice>,
    effects: reverb::Studio,
    group_volume: f32,
}

impl Studio {
    pub fn new(parameters: [[f32; 5]; 2]) -> Result<Self> {
        Ok(Self {
            voices: Vec::with_capacity(64),
            group_volume: 1.0,
            effects: reverb::Studio::new(parameters)?,
        })
    }

    pub fn play(&mut self, cue: Arc<Cue>) -> Result<()> {
        ensure!(self.voices.len() < 64, "cue voice budget exhausted");
        let program = match &cue.data {
            Data::Program { package, .. } => Some(Program {
                stream: crate::sequence::stream::Stream::new(package.clone(), false)?,
                block: Box::new([[[0; 2]; 3]; 160]),
                length: 0,
            }),
            _ => None,
        };
        self.voices.push(Voice {
            cue,
            frame: 0,
            gains: None,
            program,
        });
        Ok(())
    }

    pub fn set_group_volume(&mut self, volume: f32) -> Result<()> {
        ensure!(
            volume.is_finite() && (0.0..=1.0).contains(&volume),
            "invalid cue group volume"
        );
        self.group_volume = volume;
        Ok(())
    }

    /// Apply voice controls, then sum buses before filtering and quantizing
    /// the shared return. Effects retain their tails after the last voice ends.
    pub fn next_frame(&mut self) -> [i16; 2] {
        let mut buses = [[0i32; 2]; 3];
        self.voices.retain_mut(|voice| {
            let (input, length) = match &voice.cue.data {
                Data::Buses(frames) => (
                    frames[voice.frame].map(|bus| bus.map(i32::from)),
                    frames.len(),
                ),
                Data::Program { frames, .. } => {
                    let program = voice.program.as_mut().unwrap();
                    let index = voice.frame % 160;
                    if index == 0 {
                        let control = crate::sequence::LiveControls {
                            volume: self.group_volume,
                            pan: Some(64),
                            ..Default::default()
                        };
                        program.length = program
                            .stream
                            .render_block([control; 5], &mut program.block)
                            .expect("cooked cue program failed");
                    }
                    if index >= program.length {
                        return false;
                    }
                    (program.block[index], *frames)
                }
                Data::Controlled {
                    pcm,
                    controls,
                    tables,
                } => {
                    if voice.frame.is_multiple_of(160) {
                        let targets =
                            controls[voice.frame / 160].targets(tables, self.group_volume);
                        if let Some(gains) = &mut voice.gains {
                            for (bus, targets) in gains.iter_mut().zip(targets) {
                                for (gain, target) in bus.iter_mut().zip(targets) {
                                    gain.set_target(target);
                                }
                            }
                        } else {
                            voice.gains = Some(targets.map(|bus| bus.map(mix::GainRamp::new)));
                        }
                    }
                    let input = voice.gains.as_mut().unwrap().each_mut().map(|bus| {
                        bus.each_mut().map(|gain| {
                            ((i32::from(pcm[voice.frame]) * i32::from(gain.next_gain())) >> 15)
                                as i16
                        })
                    });
                    (input.map(|bus| bus.map(i32::from)), pcm.len())
                }
            };
            for (target, input) in buses.iter_mut().zip(input) {
                for (channel, sample) in target.iter_mut().zip(input) {
                    *channel += sample;
                }
            }
            voice.frame += 1;
            voice.frame < length
        });
        self.effects
            .process(buses)
            .map(|sample| sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_auxiliary_returns_quantize_after_voice_summing() {
        let cue = Arc::new(Cue::new(vec![[[0; 2], [0; 2], [3; 2]]]).unwrap());
        let mut studio = Studio::new([[1.0, 0.5, 1.0, 0.8, 0.01]; 2]).unwrap();
        studio.play(cue.clone()).unwrap();
        studio.play(cue).unwrap();
        for _ in 0..320 {
            assert_eq!(studio.next_frame(), [0; 2]);
        }
        // Each isolated return truncates 3 * 0.3 to zero. Shared return
        // truncates (3 + 3) * 0.3 to one after its two-block latency.
        assert_eq!(studio.next_frame(), [1; 2]);
        assert!(
            studio.voices.is_empty(),
            "completed cues retained their voices"
        );
        assert!(Cue::new(vec![]).is_err());
    }
}
