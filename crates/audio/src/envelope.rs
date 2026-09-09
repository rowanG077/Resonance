//! ADSR envelopes with millisecond controls and a linear per-sample gain ramp.

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Parameters {
    pub attack_ms: u16,
    pub decay_ms: u16,
    /// Linear hardware gain, after the bank's twelve-bit level conversion.
    pub sustain: u16,
    pub release_ms: u16,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            attack_ms: 0,
            decay_ms: 0,
            sustain: 32767,
            release_ms: 0,
        }
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

pub struct Envelope {
    parameters: Parameters,
    phase: Phase,
    remaining_ms: u32,
    value: i32,
    step: i32,
    sample: u32,
    gain: i32,
    delta: i32,
}

impl Envelope {
    const MAXIMUM: i32 = 0x7fff0000;

    pub fn new(parameters: Parameters) -> Self {
        let mut result = Self {
            parameters: Parameters {
                sustain: parameters.sustain.min(32767),
                ..parameters
            },
            phase: Phase::Attack,
            remaining_ms: u32::from(parameters.attack_ms),
            value: 0,
            step: 0,
            sample: 0,
            gain: 0,
            delta: 0,
        };
        if result.remaining_ms != 0 {
            result.step = Self::MAXIMUM / result.remaining_ms as i32;
        } else {
            result.advance_phase();
        }
        result
    }

    fn advance_phase(&mut self) {
        if self.phase == Phase::Attack {
            self.phase = Phase::Decay;
            self.remaining_ms = u32::from(self.parameters.decay_ms);
            if self.remaining_ms != 0 {
                self.value = Self::MAXIMUM;
                self.step = -(Self::MAXIMUM - (i32::from(self.parameters.sustain) << 16))
                    / self.remaining_ms as i32;
                return;
            }
        }
        if self.phase == Phase::Decay && self.parameters.sustain != 0 {
            self.phase = Phase::Sustain;
            self.value = i32::from(self.parameters.sustain) << 16;
        } else {
            self.phase = Phase::Done;
            self.value = 0;
        }
        self.step = 0;
    }

    /// Begin release at a millisecond boundary, using the current level.
    pub fn release(&mut self) {
        if self.phase == Phase::Done {
            return;
        }
        self.phase = Phase::Release;
        self.remaining_ms = u32::from(self.parameters.release_ms);
        if self.remaining_ms == 0 {
            self.phase = Phase::Done;
            self.value = 0;
            self.step = 0;
            self.gain = 0;
            self.delta = 0;
        } else {
            self.step = -self.value / self.remaining_ms as i32;
        }
    }

    pub fn next_gain(&mut self) -> u16 {
        self.next_gain_at(self.sample.is_multiple_of(160))
    }

    /// Music voices share DSP block boundaries even when notes start between them.
    pub(crate) fn next_gain_at(&mut self, block_start: bool) -> u16 {
        if self.sample.is_multiple_of(32) {
            let start = self.value >> 16;
            let step = self.step;
            // Negative deltas truncate toward zero, unlike an arithmetic shift.
            let delta = step / (1 << 21);
            if block_start
                || self.sample == 0
                || step != 0
                || self.delta != delta
                || self.phase == Phase::Done
            {
                self.gain = start;
                self.delta = delta;
            }
            if !matches!(self.phase, Phase::Sustain | Phase::Done) {
                self.value += step;
                self.remaining_ms -= 1;
                if self.remaining_ms == 0 {
                    self.advance_phase();
                }
            }
        }
        let result = self.gain.clamp(0, 32767) as u16;
        self.gain += self.delta;
        self.sample += 1;
        result
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_corrects_integer_remainder_each_millisecond() {
        let mut env = Envelope::new(Parameters {
            attack_ms: 60,
            sustain: 24576,
            ..Default::default()
        });
        assert_eq!(env.next_gain(), 0);
        assert_eq!(env.next_gain(), 17);
        for _ in 2..32 {
            env.next_gain();
        }
        assert_eq!(env.next_gain(), 546);
        for _ in 33..1920 {
            env.next_gain();
        }
        for _ in 0..320 {
            assert_eq!(env.next_gain(), 24576);
        }
    }

    #[test]
    fn zero_phases_and_release_from_current_attack_level() {
        let mut env = Envelope::new(Parameters::default());
        for _ in 0..320 {
            assert_eq!(env.next_gain(), 32767);
        }
        env.release();
        assert_eq!(env.next_gain(), 0);
        let mut env = Envelope::new(Parameters {
            attack_ms: 4,
            release_ms: 2,
            ..Default::default()
        });
        for _ in 0..64 {
            env.next_gain();
        }
        env.release();
        assert_eq!(env.next_gain(), 16383);
        assert_eq!(env.next_gain(), 16128);
        for _ in 2..64 {
            env.next_gain();
        }
        assert_eq!(env.next_gain(), 0);
        let mut zero = Envelope::new(Parameters {
            sustain: 0,
            ..Default::default()
        });
        assert_eq!(zero.next_gain(), 0);
    }

    #[test]
    fn decay_starts_at_full_scale_and_negative_delta_truncates_toward_zero() {
        let mut env = Envelope::new(Parameters {
            attack_ms: 1,
            decay_ms: 2,
            sustain: 10000,
            ..Default::default()
        });
        for _ in 0..32 {
            env.next_gain();
        }
        assert_eq!(env.next_gain(), 32767);
        assert_eq!(env.next_gain(), 32412);
        for _ in 2..32 {
            env.next_gain();
        }
        assert_eq!(env.next_gain(), 21383);
        for _ in 1..32 {
            env.next_gain();
        }
        assert_eq!(env.next_gain(), 10000);
    }
}
