//! Periodic pitch and volume controls.
use anyhow::{Result, ensure};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tables {
    #[serde(with = "crate::package::array")]
    pub sine: [i16; 1024],
    /// Wave scale, depth scale, unity, modulation scale and gain slew per update.
    pub tremolo: [f32; 5],
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.sine.iter().all(|v| (0..=4096).contains(v))
                && self.tremolo.iter().all(|v| v.is_finite() && *v > 0.0),
            "invalid modulation tables"
        );
        Ok(())
    }

    pub fn sine(&self, angle: u32) -> i16 {
        let angle = angle & 4095;
        let index = if angle & 1024 == 0 {
            angle & 1023
        } else {
            1023 - (angle & 1023)
        };
        let value = self.sine[index as usize];
        if angle & 2048 != 0 { -value } else { value }
    }
}

#[derive(Default)]
pub struct Oscillator {
    period_frames: u64,
    phase_frames: u64,
    reverse: bool,
    pub value: i16,
}

impl Oscillator {
    pub fn set(&mut self, period_ms: u16, reverse: bool) {
        self.period_frames = crate::volume::frames_from_millis(u64::from(period_ms))
            .expect("u16 period fits the source clock");
        self.phase_frames = 0;
        self.reverse = reverse;
        self.value = 0;
    }

    pub fn advance(&mut self, frames: u64, tables: &Tables) {
        if self.period_frames != 0 {
            self.phase_frames = ((u128::from(self.phase_frames) + u128::from(frames))
                % u128::from(self.period_frames)) as u64;
            let phase = self.phase_frames * 4096 / self.period_frames;
            self.value = tables.sine(phase as u32);
            if self.reverse {
                self.value = -self.value;
            }
        }
    }
}

#[derive(Default)]
pub struct Vibrato {
    pub oscillator: Oscillator,
    pub depth_8: i32,
    pub modulation_depth_8: i16,
    pub scale_by_modulation: bool,
}

impl Vibrato {
    pub fn pitch_offset(&self, modulation: u16) -> i32 {
        let modulation = i32::from(modulation >> 7);
        let depth = self.depth_8 + ((i32::from(self.modulation_depth_8) * modulation) >> 7);
        let wave = if self.scale_by_modulation {
            (i32::from(self.oscillator.value) * modulation) >> 7
        } else {
            i32::from(self.oscillator.value)
        };
        (depth * wave) >> 4
    }
}

pub struct Tremolo {
    pub scale: u16,
    pub modulation_scale: u16,
    amount: f32,
}

impl Default for Tremolo {
    fn default() -> Self {
        Self {
            scale: 0,
            modulation_scale: 0,
            amount: 1.0,
        }
    }
}

impl Tremolo {
    pub fn new(scale: u16, modulation_scale: u16) -> Self {
        Self {
            scale,
            modulation_scale,
            ..Default::default()
        }
    }

    pub fn gain(&mut self, input: u16, modulation: u16, tables: &Tables) -> Result<f32> {
        if self.scale == 0 && self.modulation_scale == 0 {
            return Ok(1.0);
        }
        let [wave_scale, depth_scale, one, modulation_scale, slew] = tables.tremolo;
        let wave = wave_scale * f32::from(input / 2);
        let target = depth_scale
            * (f32::from(self.scale)
                * (one
                    - modulation_scale
                        * (f32::from(modulation)
                            * (4096 - i32::from(self.modulation_scale)) as f32)));
        if self.amount < target {
            self.amount = (self.amount + slew).min(target);
        } else if self.amount > target {
            self.amount = (self.amount - slew).max(target);
        }
        let gain = one - wave * (one - self.amount);
        ensure!(gain.is_finite(), "nonfinite tremolo gain");
        Ok(gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tables() -> Tables {
        Tables {
            sine: std::array::from_fn(|i| {
                (4096. * (i as f64 / 1023. * std::f64::consts::FRAC_PI_2).sin()) as i16
            }),
            tremolo: [1.; 5],
        }
    }

    #[test]
    fn oscillators_follow_native_periods_and_reverse_phase() {
        let tables = tables();
        let mut forward = Oscillator::default();
        let mut reversed = Oscillator::default();
        forward.set(100, false);
        reversed.set(100, true);
        let quarter = forward.period_frames / 4;
        forward.advance(quarter, &tables);
        reversed.advance(quarter, &tables);
        assert!(forward.value >= 4092);
        assert_eq!(forward.value, -reversed.value);
        forward.advance(2 * quarter, &tables);
        assert!(forward.value <= -4092);
        forward.advance(forward.period_frames - 3 * quarter, &tables);
        assert_eq!(forward.value, 0);
        forward.set(0, false);
        forward.advance(u64::MAX, &tables);
        assert_eq!(forward.value, 0);
    }

    #[test]
    fn long_running_phase_is_bounded_and_independent_of_update_chunks() {
        let tables = tables();
        let mut together = Oscillator::default();
        let mut split = Oscillator::default();
        together.set(17, false);
        split.set(17, false);
        together.advance(u64::MAX, &tables);
        split.advance(u64::MAX / 2, &tables);
        split.advance(u64::MAX - u64::MAX / 2, &tables);
        assert_eq!(together.phase_frames, split.phase_frames);
        assert_eq!(together.value, split.value);
        assert!(together.phase_frames < together.period_frames);
        let phase = together.phase_frames;
        let value = together.value;
        together.advance(together.period_frames, &tables);
        assert_eq!((together.phase_frames, together.value), (phase, value));
        assert!(together.value.abs() <= 4096);
    }

    #[test]
    fn vibrato_combines_fixed_and_modulation_depth_with_integer_rounding() {
        let mut vibrato = Vibrato {
            depth_8: 2,
            modulation_depth_8: 120,
            ..Default::default()
        };
        vibrato.oscillator.value = 4095;
        assert_eq!(vibrato.pitch_offset(0), 511);
        assert_eq!(vibrato.pitch_offset(64 << 7), 15868);
        vibrato.scale_by_modulation = true;
        assert_eq!(vibrato.pitch_offset(0), 0);
        assert_eq!(vibrato.pitch_offset(64 << 7), 7932);
    }

    #[test]
    fn restarting_a_modulator_starts_a_new_phase() {
        let tables = tables();
        let mut oscillator = Oscillator::default();
        oscillator.set(100, false);
        let quarter = oscillator.period_frames / 4;
        oscillator.advance(quarter, &tables);
        let previous = oscillator.value;
        assert!(previous > 0);
        oscillator.set(100, false);
        assert_eq!(oscillator.value, 0);
        oscillator.advance(quarter, &tables);
        assert_eq!(oscillator.value, previous);
    }
}
