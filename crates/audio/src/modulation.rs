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
    period_ms: u32,
    counter_ticks: u32,
    pub value: i16,
}

impl Oscillator {
    pub fn set(&mut self, period_ms: u16, reverse: bool) {
        self.period_ms = u32::from(period_ms);
        self.counter_ticks = if reverse { self.period_ms * 128 } else { 0 };
        self.value = 0;
    }

    /// Voice allocation resets the LFO period while retaining phase.
    /// Re-enabling an already active LFO resets its phase.
    pub(crate) fn set_lfo(&mut self, period_ms: u16) {
        if self.period_ms != 0 {
            self.counter_ticks = 0;
        }
        self.period_ms = u32::from(period_ms);
    }

    pub(crate) fn counter(&self) -> u32 {
        self.counter_ticks
    }

    pub(crate) fn restore_counter(&mut self, counter: u32) {
        self.counter_ticks = counter;
    }

    pub fn advance(&mut self, delta_ms: u32, tables: &Tables) {
        if let Some(period) = std::num::NonZeroU32::new(self.period_ms) {
            self.counter_ticks = self.counter_ticks.wrapping_add(delta_ms.wrapping_mul(256));
            let phase = ((self.counter_ticks % (period.get() * 256)) * 16) / period.get();
            self.value = tables.sine(phase);
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

    pub fn gain(&mut self, lfo: i16, modulation: u16, tables: &Tables) -> f32 {
        if self.scale == 0 && self.modulation_scale == 0 {
            return 1.0;
        }
        let [wave_scale, depth_scale, one, modulation_scale, slew] = tables.tremolo;
        let wave = wave_scale * (8192 - ((8192 - i32::from(lfo) * 2) >> 1)) as f32;
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
        one - wave * (one - self.amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quadrant_reflection_and_periodic_phase_preserve_endpoints() {
        let tables = Tables {
            sine: std::array::from_fn(|i| i as i16 * 4),
            tremolo: [1.; 5],
        };
        assert_eq!(
            [0, 1023, 1024, 2047, 2048, 3071, 3072, 4095, 4096].map(|a| tables.sine(a)),
            [0, 4092, 4092, 0, 0, -4092, -4092, 0, 0]
        );
        let mut oscillator = Oscillator::default();
        oscillator.set(100, false);
        oscillator.advance(25, &tables);
        assert_eq!(oscillator.value, 4092);
        oscillator.advance(50, &tables);
        assert_eq!(oscillator.value, -4092);
        oscillator.advance(25, &tables);
        assert_eq!(oscillator.value, 0);
        oscillator.advance(25, &tables);
        assert_eq!(oscillator.value, 4092);
        oscillator.set(0, false);
        oscillator.advance(100, &tables);
        assert_eq!(
            oscillator.value, 0,
            "disabled vibrato must discard its audible value"
        );
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
    fn lfo_activation_retains_full_phase_but_reconfiguration_resets_it() {
        let tables = Tables {
            sine: std::array::from_fn(|i| i as i16 * 4),
            tremolo: [1.; 5],
        };
        let mut oscillator = Oscillator::default();
        oscillator.restore_counter(200 * 256);
        oscillator.set_lfo(172);
        oscillator.advance(10, &tables);
        assert_eq!(oscillator.counter(), 210 * 256);
        assert_eq!(oscillator.value, tables.sine(38 * 4096 / 172));
        oscillator.set_lfo(166);
        oscillator.advance(10, &tables);
        assert_eq!(oscillator.counter(), 10 * 256);
        assert_eq!(oscillator.value, tables.sine(10 * 4096 / 166));
    }
}
