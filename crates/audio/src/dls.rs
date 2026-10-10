//! DLS volume envelopes using cooked logarithmic conversion tables.
use crate::volume::{Ramp, frames_from_millis};
use anyhow::{Result, ensure};

const LOG_MAXIMUM: i32 = 193 << 16;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tables {
    #[serde(with = "crate::package::array")]
    pub attenuation: [u16; 194],
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.attenuation.iter().all(|v| *v <= 32767),
            "invalid DLS envelope tables"
        );
        Ok(())
    }

    fn level(&self, logarithmic: i32) -> i32 {
        let position = LOG_MAXIMUM - logarithmic.clamp(0, LOG_MAXIMUM);
        let index = (position >> 16) as usize;
        let initial = i32::from(self.attenuation[index]);
        let target = i32::from(
            *self
                .attenuation
                .get(index + 1)
                .unwrap_or(&self.attenuation[index]),
        );
        (initial << 16) + (target - initial) * (position & 65535)
    }

    fn logarithmic(&self, value: i32) -> i32 {
        for (index, pair) in self.attenuation.windows(2).enumerate() {
            let initial = i32::from(pair[0]) << 16;
            let target = i32::from(pair[1]) << 16;
            if initial > target && (target..=initial).contains(&value) {
                let fraction = (u64::from((initial - value) as u32) * 65536)
                    .div_ceil((initial - target) as u64);
                return LOG_MAXIMUM - ((index as i32) << 16) - fraction as i32;
            }
        }
        if value >= i32::from(self.attenuation[0]) << 16 {
            LOG_MAXIMUM
        } else {
            0
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub attack_frames: u64,
    pub decay_frames: u64,
    /// Logarithmic level, 0 (silent) through 193 (full scale).
    pub sustain: u16,
    pub release_frames: u64,
}

/// Typed instrument envelope, before note-dependent time scaling.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Definition {
    #[serde(flatten)]
    pub timing: Timing,
    /// Cooked logarithmic level, independent of note timing.
    pub sustain: u16,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Timing {
    pub attack_timecents: i32,
    pub decay_timecents: i32,
    pub release_ms: u16,
    pub attack_velocity_scale: i32,
    pub decay_key_scale: i32,
}

impl Definition {
    pub fn resolve(&self, velocity: u8, key: u8) -> Result<Parameters> {
        self.timing.resolve(self.sustain, velocity, key)
    }
}

impl Timing {
    pub fn validate(&self) -> Result<()> {
        // Each phase is monotonic in its note-dependent scale, so both ends
        // cover every valid velocity and key without enumerating all notes.
        for endpoint in [0, 127] {
            self.resolve(0, endpoint, endpoint)?;
        }
        Ok(())
    }

    pub fn resolve(&self, sustain: u16, velocity: u8, key: u8) -> Result<Parameters> {
        ensure!(velocity < 128 && key < 128, "invalid DLS note parameters");
        let scaled_frames = |time: i32, scale: i32, factor: u8| -> Result<u64> {
            // The minimum authored time denotes an immediate phase.
            if time == i32::MIN {
                return Ok(0);
            }
            let adjustment = if scale == i32::MIN {
                0.
            } else {
                f64::from(factor) * f64::from(scale) / 128.
            };
            let timecents = (f64::from(time) + adjustment) / 65536.;
            let frames = (timecents / 1200.).exp2() * f64::from(crate::SOURCE_RATE);
            ensure!(
                frames.is_finite() && frames < u64::MAX as f64,
                "DLS duration exceeds the source clock"
            );
            Ok(frames.ceil() as u64)
        };
        Ok(Parameters {
            attack_frames: scaled_frames(
                self.attack_timecents,
                self.attack_velocity_scale,
                velocity,
            )?,
            decay_frames: scaled_frames(self.decay_timecents, self.decay_key_scale, key)?,
            sustain,
            release_frames: frames_from_millis(u64::from(self.release_ms))?,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

pub struct Envelope<'a> {
    tables: &'a Tables,
    parameters: Parameters,
    phase: Phase,
    ramp: Ramp,
    value: i32,
}

fn scaled_duration(frames: u64, distance: i32) -> u64 {
    (u128::from(frames) * distance as u128).div_ceil(LOG_MAXIMUM as u128) as u64
}

impl<'a> Envelope<'a> {
    const MAXIMUM: i32 = 32767 << 16;

    pub fn new(parameters: Parameters, tables: &'a Tables) -> Result<Self> {
        ensure!(parameters.sustain <= 193, "invalid DLS logarithmic sustain");
        let mut envelope = Self {
            tables,
            parameters,
            phase: Phase::Attack,
            ramp: Ramp::new(0, Self::MAXIMUM, parameters.attack_frames),
            value: 0,
        };
        envelope.settle();
        Ok(envelope)
    }

    fn settle(&mut self) {
        while self.ramp.finished() {
            match self.phase {
                Phase::Attack => {
                    self.phase = Phase::Decay;
                    let target = i32::from(self.parameters.sustain) << 16;
                    self.ramp = Ramp::new(
                        LOG_MAXIMUM,
                        target,
                        scaled_duration(self.parameters.decay_frames, LOG_MAXIMUM - target),
                    );
                    self.value = Self::MAXIMUM;
                }
                Phase::Decay => {
                    self.phase = if self.parameters.sustain == 0 {
                        Phase::Done
                    } else {
                        Phase::Sustain
                    };
                    self.value = if self.parameters.sustain == 0 {
                        0
                    } else {
                        self.tables.level(self.ramp.value())
                    };
                }
                Phase::Release => {
                    self.phase = Phase::Done;
                    self.value = 0;
                }
                Phase::Sustain | Phase::Done => break,
            }
        }
    }

    pub fn release(&mut self) {
        if matches!(self.phase, Phase::Release | Phase::Done) {
            return;
        }
        let logarithmic = self.tables.logarithmic(self.value);
        self.phase = Phase::Release;
        self.ramp = Ramp::new(
            logarithmic,
            0,
            scaled_duration(self.parameters.release_frames, logarithmic),
        );
        self.settle();
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    pub fn next_gain(&mut self) -> u16 {
        let gain = (self.value >> 16) as u16;
        self.ramp.advance(1);
        self.value = match self.phase {
            Phase::Attack => self.ramp.value(),
            Phase::Decay | Phase::Release => self.tables.level(self.ramp.value()),
            Phase::Sustain | Phase::Done => self.value,
        };
        self.settle();
        gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> Tables {
        Tables {
            attenuation: std::array::from_fn(|i| ((193 - i) * 32767 / 193) as u16),
        }
    }

    #[test]
    fn note_scaling_uses_wide_frame_durations_and_rejects_overflow() {
        let timing = Timing {
            attack_timecents: 8 * 1200 * 65536,
            decay_timecents: 0,
            release_ms: 493,
            attack_velocity_scale: -1200 * 65536,
            decay_key_scale: i32::MIN,
        };
        let slow = timing.resolve(193, 0, 99).unwrap();
        let faster = timing.resolve(193, 64, 99).unwrap();
        assert_eq!(slow.attack_frames, u64::from(crate::SOURCE_RATE) * 256);
        assert!(faster.attack_frames < slow.attack_frames);
        assert_eq!(slow.decay_frames, u64::from(crate::SOURCE_RATE));
        assert_eq!(slow.release_frames, frames_from_millis(493).unwrap());
        assert!(timing.resolve(193, 128, 99).is_err());
        let wide = Timing {
            attack_timecents: 1_600_000_000,
            attack_velocity_scale: 1_600_000_000,
            ..timing
        };
        assert!(
            wide.resolve(193, 127, 0).unwrap().attack_frames
                > wide.resolve(193, 0, 0).unwrap().attack_frames
        );
        assert!(
            Timing {
                attack_timecents: i32::MAX,
                attack_velocity_scale: i32::MAX,
                ..timing
            }
            .resolve(193, 127, 0)
            .is_err()
        );
        assert_eq!(
            Timing {
                attack_timecents: i32::MIN,
                ..timing
            }
            .resolve(193, 127, 0)
            .unwrap()
            .attack_frames,
            0
        );
    }

    #[test]
    fn logarithmic_decay_and_release_follow_the_authored_curve() {
        let mut tables = tables();
        tables.attenuation =
            std::array::from_fn(|i| ((193 - i).pow(2) * 32767 / 193usize.pow(2)) as u16);
        tables.validate().unwrap();
        let mut env = Envelope::new(
            Parameters {
                attack_frames: 0,
                decay_frames: 386,
                sustain: 96,
                release_frames: 386,
            },
            &tables,
        )
        .unwrap();
        let mut previous = 32767;
        for frame in 0..194 {
            let gain = env.next_gain();
            assert!(gain <= previous);
            if frame == 97 {
                let midpoint = 32767. * ((193. + 96.) / 2. / 193_f64).powi(2);
                assert!((f64::from(gain) - midpoint).abs() < 2.);
            }
            previous = gain;
        }
        assert_eq!(env.next_gain(), tables.attenuation[97]);
        env.release();
        for _ in 0..191 {
            env.next_gain();
        }
        assert!(!env.is_done());
        env.next_gain();
        assert!(env.is_done());
        assert_eq!(env.next_gain(), 0);
    }

    #[test]
    fn release_during_attack_preserves_current_gain_between_control_boundaries() {
        let tables = tables();
        for elapsed in [1, 31, 33, 80] {
            let mut env = Envelope::new(
                Parameters {
                    attack_frames: frames_from_millis(4).unwrap(),
                    decay_frames: 0,
                    sustain: 193,
                    release_frames: frames_from_millis(8).unwrap(),
                },
                &tables,
            )
            .unwrap();
            for _ in 0..elapsed {
                env.next_gain();
            }
            let current = (env.value >> 16) as u16;
            env.release();
            assert_eq!(env.next_gain(), current);
            let mut previous = current;
            for _ in 1..frames_from_millis(8).unwrap() {
                let gain = env.next_gain();
                assert!(gain <= previous);
                previous = gain;
            }
            assert!(env.is_done());
            assert_eq!(env.next_gain(), 0);
        }
    }
}
