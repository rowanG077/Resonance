//! PCM source advancement and fixed-point interpolation.
//! Filter data is an explicit decoder input; this module owns no audio device.
use crate::sample::Sample;
use anyhow::{Result, ensure};
use std::borrow::Cow;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Coefficients(#[serde(with = "crate::package::coefficients")] pub [[[i16; 4]; 128]; 4]);

impl Coefficients {
    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == 4096,
            "expected 4096 bytes of DSP coefficient data"
        );
        let mut tables = [[[0; 4]; 128]; 4];
        for (value, bytes) in tables
            .iter_mut()
            .flatten()
            .flatten()
            .zip(bytes.chunks_exact(2))
        {
            *value = i16::from_be_bytes(bytes.try_into()?);
        }
        Ok(Self(tables))
    }
}

#[derive(Clone, Copy)]
pub enum Mode<'a> {
    Polyphase(&'a [[i16; 4]; 128]),
    Linear,
    Direct,
}

pub struct Resampler<'a> {
    mode: Mode<'a>,
    ratio: u32,
    fraction: u32,
    history: [i16; 4],
}

impl<'a> Resampler<'a> {
    pub fn new(mode: Mode<'a>, ratio: u32) -> Result<Self> {
        let mut result = Self {
            mode,
            ratio: 0,
            fraction: 0,
            history: [0; 4],
        };
        result.set_ratio(ratio)?;
        Ok(result)
    }

    /// The original pitch control has twelve fractional bits and clamps at
    /// 0x3fff before shifting four places into the source's 16.16 ratio.
    pub fn set_ratio(&mut self, ratio: u32) -> Result<()> {
        ensure!(
            ratio <= 0x3fff0,
            "source ratio exceeds the original pitch range"
        );
        self.ratio = ratio;
        Ok(())
    }

    pub fn next_sample(&mut self, mut input: impl FnMut() -> i16) -> i16 {
        if matches!(self.mode, Mode::Direct) {
            let sample = input();
            self.history.rotate_left(1);
            self.history[3] = sample;
            return sample;
        }
        self.fraction += self.ratio;
        let advance = self.fraction >> 16;
        self.fraction &= 0xffff;
        for _ in 0..advance {
            self.history.rotate_left(1);
            self.history[3] = input();
        }
        let value = match self.mode {
            Mode::Polyphase(coefficients) => {
                let phase = (self.fraction >> 9) as usize;
                self.history
                    .iter()
                    .zip(coefficients[phase])
                    .map(|(&sample, coefficient)| i64::from(sample) * i64::from(coefficient))
                    .sum::<i64>()
                    >> 15
            }
            Mode::Linear => {
                let fraction = i64::from(self.fraction);
                (i64::from(self.history[0]) * (65536 - fraction)
                    + i64::from(self.history[1]) * fraction)
                    >> 16
            }
            Mode::Direct => unreachable!(),
        };
        value.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
    }
}

/// First-pass PCM and separately restored ADPCM loop PCM have different
/// predictor histories. Repeating a slice of the first pass loses that state.
pub struct SampleCursor<'a> {
    sample: Cow<'a, Sample>,
    position: usize,
    first_end: usize,
    loop_at: usize,
}

impl<'a> SampleCursor<'a> {
    pub fn new(sample: &'a Sample) -> Result<Self> {
        Self::with_sample(Cow::Borrowed(sample))
    }

    pub fn from_owned(sample: Sample) -> Result<Self> {
        Self::with_sample(Cow::Owned(sample))
    }

    fn with_sample(sample: Cow<'a, Sample>) -> Result<Self> {
        let end = u64::from(sample.loop_start) + u64::from(sample.loop_length);
        ensure!(
            end <= sample.pcm.len() as u64 && sample.loop_pcm.len() == sample.loop_length as usize,
            "invalid decoded sample loop"
        );
        let first_end = if sample.loop_length == 0 {
            sample.pcm.len()
        } else {
            end as usize
        };
        Ok(Self {
            sample,
            position: 0,
            loop_at: 0,
            first_end,
        })
    }

    pub fn sample(&self) -> &Sample {
        &self.sample
    }

    pub fn is_done(&self) -> bool {
        self.position >= self.first_end && self.sample.loop_pcm.is_empty()
    }

    pub fn next_sample(&mut self) -> i16 {
        if self.position < self.first_end {
            let sample = self.sample.pcm[self.position];
            self.position += 1;
            sample
        } else if self.sample.loop_pcm.is_empty() {
            0
        } else {
            let sample = self.sample.loop_pcm[self.loop_at];
            self.loop_at = (self.loop_at + 1) % self.sample.loop_pcm.len();
            sample
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_modes_keep_history_and_fraction_across_calls() {
        let mut value = 0i16;
        let mut input = || {
            value += 100;
            value
        };
        let mut linear = Resampler::new(Mode::Linear, 32768).unwrap();
        let samples: Vec<_> = (0..10).map(|_| linear.next_sample(&mut input)).collect();
        assert_eq!(samples, [0, 0, 0, 0, 0, 0, 50, 100, 150, 200]);
        linear.set_ratio(65536).unwrap();
        assert_eq!(linear.next_sample(&mut input), 300);
        let mut direct = Resampler::new(Mode::Direct, 0).unwrap();
        assert_eq!(direct.next_sample(|| -123), -123);
        assert!(direct.set_ratio(0x40000).is_err());
    }

    #[test]
    fn polyphase_selects_all_seven_fraction_bits_and_saturates() {
        let mut coefficients = [[0; 4]; 128];
        coefficients[64][3] = 16384;
        coefficients[0] = [32767; 4];
        let mut source = Resampler::new(Mode::Polyphase(&coefficients), 98304).unwrap();
        assert_eq!(source.next_sample(|| 10000), 5000);
        assert_eq!(source.next_sample(|| 10000), 29999);
        assert_eq!(source.next_sample(|| 10000), 5000);
        assert_eq!(source.next_sample(|| 10000), 32767);
        assert!(Coefficients::from_be_bytes(&[0; 4095]).is_err());
    }

    #[test]
    fn repeats_restored_pcm_and_stops_at_loop_end_before_unused_tail() {
        let mut sample = Sample {
            key: 60,
            rate: 32000,
            loop_start: 1,
            loop_length: 2,
            pcm: vec![1, 2, 3, 99],
            loop_pcm: vec![4, 5],
        };
        let mut cursor = SampleCursor::new(&sample).unwrap();
        assert_eq!(
            (0..8).map(|_| cursor.next_sample()).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 4, 5, 4]
        );
        sample.loop_length = 0;
        sample.loop_pcm.clear();
        let mut cursor = SampleCursor::new(&sample).unwrap();
        assert_eq!(
            (0..6).map(|_| cursor.next_sample()).collect::<Vec<_>>(),
            [1, 2, 3, 99, 0, 0]
        );
    }
}
