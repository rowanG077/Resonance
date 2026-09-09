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

    /// Input is MIDI pitch with sixteen fractional bits. Fractional pitch
    /// interpolates the already-quantized rate of this and the next semitone.
    pub fn ratio(&self, pitch: i32, sample: &Sample) -> Result<u32> {
        ensure!(
            (0..128 << 16).contains(&pitch) && sample.key < 128 && sample.rate != 0,
            "invalid note or sample pitch"
        );
        let key = pitch >> 16;
        let difference = key - i32::from(sample.key);
        let factor = if difference >= 0 {
            self.up[difference as usize]
        } else {
            self.down[(-difference) as usize]
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
        assert_eq!(tables.ratio(60 << 16, &sample).unwrap(), 65536);
        assert_eq!(tables.ratio(72 << 16, &sample).unwrap(), 131072);
        assert_eq!(tables.ratio((60 << 16) + 32768, &sample).unwrap(), 67472);
        sample.rate = 22050;
        assert_eq!(tables.ratio(60 << 16, &sample).unwrap(), 45152);
        assert!(tables.ratio(-1, &sample).is_err());
    }
}
