//! Linear ADSR envelopes on the native source-frame clock.
use crate::volume::{Ramp, frames_from_millis};

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Parameters {
    pub attack_ms: u16,
    pub decay_ms: u16,
    /// Linear gain, after the bank's twelve-bit level conversion.
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
    ramp: Ramp,
}

fn ramp(initial: i32, target: i32, milliseconds: u16) -> Ramp {
    Ramp::new(
        initial,
        target,
        frames_from_millis(u64::from(milliseconds)).expect("u16 duration fits the source clock"),
    )
}

impl Envelope {
    pub fn new(parameters: Parameters) -> Self {
        let mut envelope = Self {
            parameters: Parameters {
                sustain: parameters.sustain.min(32767),
                ..parameters
            },
            phase: Phase::Attack,
            ramp: ramp(0, 32767, parameters.attack_ms),
        };
        envelope.settle();
        envelope
    }

    fn settle(&mut self) {
        while self.ramp.finished() {
            match self.phase {
                Phase::Attack => {
                    self.phase = Phase::Decay;
                    self.ramp = ramp(
                        32767,
                        i32::from(self.parameters.sustain),
                        self.parameters.decay_ms,
                    );
                }
                Phase::Decay => {
                    self.phase = if self.parameters.sustain == 0 {
                        Phase::Done
                    } else {
                        Phase::Sustain
                    };
                }
                Phase::Release => self.phase = Phase::Done,
                Phase::Sustain | Phase::Done => break,
            }
        }
    }

    /// Release from the level at the current source frame, including during attack.
    pub fn release(&mut self) {
        if matches!(self.phase, Phase::Release | Phase::Done) {
            return;
        }
        self.phase = Phase::Release;
        self.ramp = ramp(self.ramp.value(), 0, self.parameters.release_ms);
        self.settle();
    }

    pub fn next_gain(&mut self) -> u16 {
        let gain = self.ramp.value() as u16;
        self.ramp.advance(1);
        self.settle();
        gain
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_are_monotonic_and_reach_their_native_frame_endpoints() {
        let mut env = Envelope::new(Parameters {
            attack_ms: 60,
            decay_ms: 2,
            sustain: 10000,
            release_ms: 3,
        });
        let attack = frames_from_millis(60).unwrap();
        let decay = frames_from_millis(2).unwrap();
        let mut previous = 0;
        for _ in 0..attack {
            let gain = env.next_gain();
            assert!(gain >= previous);
            previous = gain;
        }
        assert_eq!(env.ramp.value(), 32767);
        previous = 32767;
        for _ in 0..decay {
            let gain = env.next_gain();
            assert!(gain <= previous);
            previous = gain;
        }
        for _ in 0..5 {
            assert_eq!(env.next_gain(), 10000);
        }
        env.release();
        for _ in 0..frames_from_millis(3).unwrap() {
            env.next_gain();
        }
        assert!(env.is_done());
        assert_eq!(env.next_gain(), 0);
    }

    #[test]
    fn release_preserves_the_current_gain_at_any_attack_frame() {
        for elapsed in [1, 31, 33, 80] {
            let mut env = Envelope::new(Parameters {
                attack_ms: 4,
                release_ms: 2,
                ..Default::default()
            });
            for _ in 0..elapsed {
                env.next_gain();
            }
            let current = env.ramp.value() as u16;
            env.release();
            let duration = frames_from_millis(2).unwrap();
            assert_eq!(env.next_gain(), current);
            let mut previous = current;
            for _ in 1..duration {
                let gain = env.next_gain();
                assert!(gain <= previous);
                previous = gain;
            }
            assert!(env.is_done());
            assert_eq!(env.next_gain(), 0);
        }
    }

    #[test]
    fn zero_phases_are_immediate_and_small_levels_still_finish() {
        let mut env = Envelope::new(Parameters::default());
        assert_eq!(env.next_gain(), 32767);
        env.release();
        assert!(env.is_done());
        assert_eq!(env.next_gain(), 0);
        let mut env = Envelope::new(Parameters {
            sustain: 1,
            release_ms: 100,
            ..Default::default()
        });
        env.release();
        for _ in 0..frames_from_millis(100).unwrap() {
            env.next_gain();
        }
        assert!(env.is_done());
        assert_eq!(env.next_gain(), 0);
    }
}
