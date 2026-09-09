//! DLS volume envelopes using cooked logarithmic conversion tables.
use anyhow::{Result, ensure};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tables {
    #[serde(with = "crate::package::array")]
    pub attenuation: [u16; 194],
    #[serde(with = "crate::package::array")]
    pub inverse: [u8; 1024],
    #[serde(with = "crate::package::array")]
    pub sustain: [f32; 128],
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.attenuation.iter().all(|v| *v <= 32767)
                && self.inverse.iter().all(|v| *v <= 193)
                && self
                    .sustain
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid DLS envelope tables"
        );
        Ok(())
    }

    fn level(&self, logarithmic: i32) -> i32 {
        let index = (193 - ((logarithmic + 32768) >> 16)).clamp(0, 193);
        i32::from(self.attenuation[index as usize]) << 16
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub attack_ms: u16,
    pub decay_ms: u16,
    /// Logarithmic level, 0 (silent) through 193 (full scale).
    pub sustain: u16,
    pub release_ms: u16,
}

/// Typed instrument envelope, before note-dependent time scaling.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Definition {
    pub attack_timecents: i32,
    pub decay_timecents: i32,
    pub sustain_index: u16,
    pub release_ms: u16,
    pub attack_velocity_scale: i32,
    pub decay_key_scale: i32,
}

impl Definition {
    pub fn resolve(&self, tables: &Tables, velocity: u8, key: u8) -> Result<Parameters> {
        ensure!(velocity < 128 && key < 128, "invalid DLS note parameters");
        let scaled_time = |time: i32, scale: i32, factor: f32| {
            let time = if scale == i32::MIN {
                time
            } else {
                time.wrapping_add((factor * scale as f32) as i32)
            };
            // Round to single precision around the double-precision power calculation.
            let exponent = 1.271_565_8e-8_f32 * time as f32;
            (1000.0_f32 * (2.0_f64.powf(f64::from(exponent)) as f32)) as u32 as u16
        };
        let index = usize::from(self.sustain_index);
        // The sustain curve has 128 points plus an implicit unity endpoint.
        ensure!(index <= tables.sustain.len(), "invalid DLS sustain index");
        let value = tables.sustain.get(index).copied().unwrap_or(1.0);
        let linear = (4096.0 * value) as u16;
        let sustain = 193 - u16::from(tables.inverse[usize::from((linear >> 2).min(1023))]);
        Ok(Parameters {
            attack_ms: scaled_time(
                self.attack_timecents,
                self.attack_velocity_scale,
                f32::from(velocity) / 128.0,
            ),
            decay_ms: scaled_time(
                self.decay_timecents,
                self.decay_key_scale,
                f32::from(key) / 128.0,
            ),
            sustain,
            release_ms: self.release_ms,
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
    remaining: u32,
    value: i32,
    logarithmic: i32,
    step: i32,
    sample: u32,
    gain: i32,
    delta: i32,
}

impl<'a> Envelope<'a> {
    const MAXIMUM: i32 = 0x7fff0000;

    pub fn new(parameters: Parameters, tables: &'a Tables) -> Result<Self> {
        ensure!(parameters.sustain <= 193, "invalid DLS logarithmic sustain");
        let mut result = Self {
            tables,
            parameters,
            phase: Phase::Attack,
            remaining: u32::from(parameters.attack_ms),
            value: 0,
            logarithmic: 0,
            step: 0,
            sample: 0,
            gain: 0,
            delta: 0,
        };
        if result.remaining != 0 {
            result.step = Self::MAXIMUM / result.remaining as i32;
        } else {
            result.advance_phase();
        }
        Ok(result)
    }

    fn advance_phase(&mut self) {
        if self.phase == Phase::Attack {
            self.phase = Phase::Decay;
            let distance = (193 - u32::from(self.parameters.sustain)) << 16;
            self.remaining = (u32::from(self.parameters.decay_ms) * (distance / 193)) >> 16;
            if let Some(step) = distance.checked_div(self.remaining) {
                self.value = Self::MAXIMUM;
                self.logarithmic = 193 << 16;
                self.step = -(step as i32);
                return;
            }
        }
        if self.phase == Phase::Decay && self.parameters.sustain != 0 {
            self.phase = Phase::Sustain;
            self.logarithmic = i32::from(self.parameters.sustain) << 16;
            self.value = self.tables.level(self.logarithmic);
        } else {
            self.phase = Phase::Done;
            self.value = 0;
        }
        self.step = 0;
    }

    pub fn release(&mut self) {
        if self.phase == Phase::Done {
            return;
        }
        if self.phase == Phase::Attack {
            self.logarithmic =
                (193 - i32::from(self.tables.inverse[(self.value >> 21) as usize])) << 16;
        }
        self.remaining = ((0.000_323_834_2_f32 * self.logarithmic as f32)
            * f32::from(self.parameters.release_ms)) as u32
            >> 12;
        self.phase = Phase::Release;
        if self.remaining == 0 {
            self.phase = Phase::Done;
            self.value = 0;
            self.step = 0;
            self.gain = 0;
            self.delta = 0;
        } else {
            self.step = -self.logarithmic / self.remaining as i32;
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    pub fn next_gain(&mut self) -> u16 {
        self.next_gain_at(self.sample.is_multiple_of(160))
    }

    pub(crate) fn next_gain_at(&mut self, block_start: bool) -> u16 {
        if self.sample.is_multiple_of(32) {
            let old = self.value;
            let step = self.step;
            if !matches!(self.phase, Phase::Sustain | Phase::Done) {
                if self.phase == Phase::Attack {
                    self.value += step;
                } else {
                    self.logarithmic += step;
                    self.value = self.tables.level(self.logarithmic);
                }
            }
            let delta = (self.value - old) / (1 << 21);
            if block_start || self.sample == 0 || step != 0 || self.delta != delta || self.is_done()
            {
                self.gain = old >> 16;
                self.delta = delta;
            }
            if !matches!(self.phase, Phase::Sustain | Phase::Done) {
                self.remaining -= 1;
                if self.remaining == 0 {
                    self.advance_phase();
                }
            }
        }
        let gain = self.gain.clamp(0, 32767) as u16;
        self.gain += self.delta;
        self.sample += 1;
        gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> Tables {
        Tables {
            attenuation: std::array::from_fn(|i| ((193 - i) * 32767 / 193) as u16),
            inverse: std::array::from_fn(|i| (193 - i * 193 / 1023) as u8),
            sustain: std::array::from_fn(|i| i as f32 / 127.0),
        }
    }

    #[test]
    fn converts_timecents_and_note_dependent_scales() {
        let tables = tables();
        tables.validate().unwrap();
        let definition = Definition {
            attack_timecents: 0,
            decay_timecents: 0,
            sustain_index: 127,
            release_ms: 493,
            attack_velocity_scale: -1200 * 65536,
            decay_key_scale: i32::MIN,
        };
        let p = definition.resolve(&tables, 64, 99).unwrap();
        assert_eq!(
            (p.attack_ms, p.decay_ms, p.sustain, p.release_ms),
            (707, 1000, 193, 493)
        );
        assert!(definition.resolve(&tables, 128, 99).is_err());
        let full = Definition {
            sustain_index: 128,
            ..definition
        };
        assert_eq!(full.resolve(&tables, 64, 99).unwrap().sustain, 193);
        assert!(
            Definition {
                sustain_index: 129,
                ..definition
            }
            .resolve(&tables, 64, 99)
            .is_err()
        );
    }

    #[test]
    fn decay_uses_logarithmic_distance_and_release_stops() {
        let tables = tables();
        let mut env = Envelope::new(
            Parameters {
                attack_ms: 0,
                decay_ms: 4,
                sustain: 96,
                release_ms: 4,
            },
            &tables,
        )
        .unwrap();
        assert_eq!(env.next_gain(), 32767);
        for _ in 1..64 {
            env.next_gain();
        }
        assert_eq!(env.next_gain(), tables.attenuation[97]);
        for _ in 1..32 {
            env.next_gain();
        }
        env.release();
        assert!(!env.is_done());
        for _ in 0..64 {
            env.next_gain();
        }
        assert!(env.is_done());
        assert_eq!(env.next_gain(), 0);
    }

    #[test]
    fn keyoff_during_linear_attack_converts_current_level() {
        let tables = tables();
        let mut env = Envelope::new(
            Parameters {
                attack_ms: 4,
                decay_ms: 0,
                sustain: 193,
                release_ms: 8,
            },
            &tables,
        )
        .unwrap();
        for _ in 0..64 {
            env.next_gain();
        }
        env.release();
        assert_eq!(env.next_gain(), 16383);
        for _ in 1..256 {
            env.next_gain();
        }
        assert!(env.is_done());
        assert_eq!(env.next_gain(), 0);
    }
}
