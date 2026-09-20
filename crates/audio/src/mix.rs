//! Convert instrument and group gains using cooked volume tables.
use anyhow::{Result, ensure};

pub const CENTER_PAN: u8 = 64;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Spatial {
    pub pan_scale: f32,
    #[serde(with = "crate::package::array")]
    pub left_delay: [u8; 128],
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tables {
    #[serde(with = "crate::package::array")]
    pub volume: [f32; 129],
    #[serde(with = "crate::package::array")]
    pub alternate_volume: [f32; 129],
    pub pan: [f32; 4],
    pub volume_16_scale: f32,
    pub controller_14_scale: f32,
    pub pan_16_scale: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<Spatial>,
}

pub struct Parameters {
    pub volume: u32,
    pub controller: u16,
    /// Stereo pan in 16.16 units, retaining authored ramp fractions.
    pub pan: u32,
    pub post: [u16; 2],
    pub scale: f32,
    pub group_volume: f32,
    pub aux_a: u8,
    pub alternate: bool,
    pub interaural_delay: bool,
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.volume
                .iter()
                .chain(&self.alternate_volume)
                .chain(&self.pan)
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid audio lookup table"
        );
        ensure!(
            [
                self.volume_16_scale,
                self.controller_14_scale,
                self.pan_16_scale
            ]
            .into_iter()
            .all(|v| v.is_finite() && v > 0.0 && v <= 1.0),
            "invalid audio normalization"
        );
        if let Some(spatial) = &self.spatial {
            ensure!(
                spatial.pan_scale.is_finite()
                    && (0.0..=1.0).contains(&spatial.pan_scale)
                    && spatial.left_delay.iter().all(|&delay| delay <= 32),
                "invalid spatial audio table"
            );
        }
        Ok(())
    }

    pub fn gains(&self, volume: u32, controller: u16, pan: u8, post: [u16; 2]) -> [[u16; 2]; 3] {
        self.gains_for(Parameters {
            volume,
            controller,
            pan: u32::from(pan) << 16,
            post,
            scale: 1.0,
            group_volume: 1.0,
            aux_a: 127,
            alternate: false,
            interaural_delay: false,
        })
    }

    pub fn gains_for(&self, parameters: Parameters) -> [[u16; 2]; 3] {
        let Parameters {
            volume,
            controller,
            pan,
            post,
            scale,
            group_volume,
            aux_a,
            alternate,
            interaural_delay,
        } = parameters;
        let volume = self.volume_16_scale * volume as f32;
        let volume = volume * scale;
        let direct =
            self.controller_14_scale * (volume * group_volume * f32::from(controller.min(16383)));
        let mut pan = self.pan_16_scale * pan.min(127 << 16).saturating_sub(65536) as f32;
        if interaural_delay {
            pan = 1.0
                + self
                    .spatial
                    .as_ref()
                    .expect("validated spatial audio")
                    .pan_scale
                    * (pan - 1.0);
        }
        let sides = [
            interpolate(&self.pan, 2.0 - pan),
            interpolate(&self.pan, pan),
        ];
        [
            direct,
            (1.0_f32 / 127.0)
                * (f32::from(aux_a)
                    * (self.controller_14_scale * (direct * f32::from(post[0].min(16383))))),
            self.controller_14_scale * (direct * f32::from(post[1].min(16383))),
        ]
        .map(|volume| {
            let curve = if alternate {
                &self.alternate_volume
            } else {
                &self.volume
            };
            let weight = interpolate(curve, 127.0 * volume);
            sides.map(|pan| (32767.0 * (weight * pan)) as u16)
        })
    }
}

/// A voice's post-envelope history, shared by its direct and auxiliary buses.
pub(crate) struct StereoDelay {
    history: [i16; 33],
    cursor: usize,
    pub shift: [u8; 2],
}
impl Default for StereoDelay {
    fn default() -> Self {
        Self {
            history: [0; 33],
            cursor: 0,
            shift: [16; 2],
        }
    }
}
impl StereoDelay {
    pub fn next(&mut self, sample: i16) -> [i16; 2] {
        self.history[self.cursor] = sample;
        let output = self
            .shift
            .map(|shift| self.history[(self.cursor + 33 - usize::from(shift)) % 33]);
        self.cursor = (self.cursor + 1) % 33;
        output
    }
}

fn interpolate(table: &[f32], value: f32) -> f32 {
    let value = value.clamp(0.0, (table.len() - 2) as f32);
    let index = value as usize;
    let fraction = value - index as f32;
    (1.0 - fraction) * table[index] + fraction * table[index + 1]
}

/// The linear ADSR gain is separate from the nonlinear volume lookup.
pub fn apply(sample: i16, envelope: u16, gain: u16) -> i16 {
    let enveloped = (i64::from(sample) * i64::from(envelope)) >> 15;
    ((enveloped * i64::from(gain)) >> 15).clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

/// Ramp new targets over 160 samples; unchanged targets correct rounding
/// residuals in whole 32-sample control intervals.
pub struct GainRamp {
    target: u16,
    value: i32,
    step: i32,
    remaining: u32,
}

impl GainRamp {
    pub fn new(value: u16) -> Self {
        Self {
            target: value,
            value: i32::from(value),
            step: 0,
            remaining: 0,
        }
    }
    pub fn set_target(&mut self, target: u16) {
        self.set_target_changed(target, target != self.target);
    }

    /// The hardware shares a change flag across the channels of each bus.
    pub(crate) fn set_target_changed(&mut self, target: u16, changed: bool) {
        let difference = i32::from(target) - self.value;
        if changed {
            self.step = difference / 160;
            self.remaining = 160;
        } else if (32..160).contains(&difference.abs()) {
            self.step = difference.signum();
            self.remaining = (difference.unsigned_abs() / 32) * 32;
        } else {
            self.step = 0;
            self.remaining = 0;
            if target == 0 && difference > -32 {
                self.value = 0;
            }
        }
        self.target = target;
    }
    pub fn next_gain(&mut self) -> u16 {
        let value = self.value as u16;
        if self.remaining != 0 {
            self.value += self.step;
            self.remaining -= 1;
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spatial_delay_keeps_independent_ear_offsets_across_history_wraps() {
        let mut delay = StereoDelay {
            shift: [0, 32],
            ..Default::default()
        };
        for frame in 0..100 {
            let sample = if frame % 33 == 0 { -16384 } else { 0 };
            assert_eq!(
                delay.next(sample),
                [sample, if frame % 33 == 32 { -16384 } else { 0 }]
            );
        }
        let mut delay = StereoDelay::default();
        for frame in 0..34 {
            assert_eq!(
                delay.next(if frame == 0 { 100 } else { 0 }),
                [if frame == 16 { 100 } else { 0 }; 2]
            );
        }
    }
    #[test]
    fn signed_pcm_rounds_down_at_each_gain_stage() {
        assert_eq!(apply(-1, 32767, 32767), -1);
        assert_eq!(apply(1, 32767, 32767), 0);
        assert_eq!(apply(16384, 16384, 16384), 4096);
    }

    #[test]
    fn gain_ramps_preserve_remainders_and_correct_only_whole_milliseconds() {
        let mut ramp = GainRamp::new(1000);
        ramp.set_target(801);
        assert_eq!(ramp.next_gain(), 1000);
        for _ in 1..160 {
            ramp.next_gain();
        }
        assert_eq!(ramp.next_gain(), 840);
        ramp.set_target(801);
        for _ in 0..32 {
            ramp.next_gain();
        }
        assert_eq!(ramp.next_gain(), 808);
        ramp.set_target(801);
        for _ in 0..160 {
            assert_eq!(ramp.next_gain(), 808);
        }
    }
}
