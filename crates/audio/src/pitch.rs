//! Table-based pitch conversion from musical notes to playback increments.
use crate::sample::Sample;
use anyhow::{Result, ensure};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Tables {
    #[serde(with = "crate::package::array")]
    pub up: [f32; 128],
    #[serde(with = "crate::package::array")]
    pub down: [f32; 128],
    pub semitone: f32,
}

impl Tables {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.up
                .iter()
                .chain(&self.down)
                .all(|v| v.is_finite() && *v > 0.)
                && self.semitone.is_finite()
                && (1.0..2.0).contains(&self.semitone),
            "invalid pitch conversion tables"
        );
        Ok(())
    }

    /// Pitch has sixteen fractional bits. Effects can exceed the MIDI range:
    /// the integer note wraps to eight bits before looking up its tuning.
    /// Fractions interpolate already-quantized rates of adjacent semitones.
    pub fn ratio(&self, pitch: i32, sample: &Sample) -> Result<u32> {
        ensure!(sample.key < 128 && sample.rate != 0, "invalid sample pitch");
        let key = i32::from((pitch >> 16) as u8);
        let difference = key - i32::from(sample.key);
        let factor = match difference {
            ..0 => self.down[(-difference) as usize],
            0..128 => self.up[difference as usize],
            // Large upward bends continue into the adjacent downward curve.
            // Some sound effects rely on this discontinuity in the tuning bank.
            _ => self.down[difference as usize - 128],
        };
        let rate = f32::from(sample.rate) * factor;
        let integer = ((4096. * rate) / 32000.) as u32 as u16;
        let next = (self.semitone * f32::from(integer)) as u32 as u16;
        let fraction = pitch & 65535;
        let converted = (i64::from(integer) << 16)
            + i64::from(fraction) * (i64::from(next) - i64::from(integer));
        Ok(((converted >> 16) as u16).min(0x3fff) as u32 * 16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_original_integer_rate_quantization() {
        let tables = Tables {
            up: std::array::from_fn(|i| 2f32.powf(i as f32 / 12.)),
            down: std::array::from_fn(|i| 2f32.powf(-(i as f32) / 12.)),
            semitone: 2f32.powf(1. / 12.),
        };
        tables.validate().unwrap();
        let mut sample = Sample {
            key: 60,
            rate: 32000,
            loop_start: 0,
            loop_length: 0,
            pcm: vec![],
            loop_pcm: vec![],
        };
        for key in 0..128 {
            sample.key = key;
            for (fraction, ratio) in [(0, 65536), (32768, 67472), (65535, 69408)] {
                assert_eq!(
                    tables
                        .ratio((i32::from(key) << 16) + fraction, &sample)
                        .unwrap(),
                    ratio
                );
            }
        }
        sample.key = 60;
        assert_eq!(tables.ratio(72 << 16, &sample).unwrap(), 131072);
        assert_eq!(tables.ratio((60 << 16) + 32768, &sample).unwrap(), 67472);
        sample.rate = 22050;
        assert_eq!(tables.ratio(60 << 16, &sample).unwrap(), 45152);
        sample.key = 128;
        assert!(tables.ratio(60 << 16, &sample).is_err());
    }

    #[test]
    fn wide_effect_bends_wrap_notes_and_preserve_the_tuning_bank_boundary() {
        let mut tables = Tables {
            up: [1.; 128],
            down: [1.; 128],
            semitone: 1.059_463_1,
        };
        tables.up[68] = 2.;
        tables.down[9] = 0.594_603_54;
        tables.down[60] = 0.03125;
        tables.down[67] = 0.020_856_857;
        let sample = Sample {
            key: 60,
            rate: 32000,
            loop_start: 0,
            loop_length: 0,
            pcm: vec![],
            loop_pcm: vec![],
        };
        for (pitch, ratio) in [
            (128 << 16, 131072),
            (188 << 16, 65536),
            ((188 << 16) + 32768, 67472),
            (197 << 16, 38960),
            (256 << 16, 2048),
            (-1, 1424),
        ] {
            assert_eq!(tables.ratio(pitch, &sample).unwrap(), ratio, "{pitch}");
        }
    }
}
