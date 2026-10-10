//! PCM source advancement and fixed-point interpolation.
//! Filter data is an explicit decoder input; this module owns no audio device.
use crate::sample::Sample;
use anyhow::{Result, ensure};

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
    pub fn new(mode: Mode<'a>, ratio: u32) -> Self {
        Self {
            mode,
            ratio,
            fraction: 0,
            history: [0; 4],
        }
    }

    /// Any representable 16.16 increment is supported. Widening the addition
    /// preserves the phase even at the maximum increment.
    pub fn set_ratio(&mut self, ratio: u32) {
        self.ratio = ratio;
    }

    /// Whether exhausted input still has samples in the interpolation filter.
    pub fn has_pending(&self) -> bool {
        !matches!(self.mode, Mode::Direct) && self.history.iter().any(|&sample| sample != 0)
    }

    pub fn next_sample(&mut self, input: &mut SampleCursor<'_>) -> i16 {
        if matches!(self.mode, Mode::Direct) {
            let sample = input.next_sample();
            self.history.rotate_left(1);
            self.history[3] = sample;
            return sample;
        }
        // A zero playback rate can hold live input, but cannot hold an exhausted
        // one-shot forever. Flush its remaining filter history at one tap/frame.
        let ratio = if input.is_done() && self.ratio == 0 {
            65536
        } else {
            self.ratio
        };
        let position = u64::from(self.fraction) + u64::from(ratio);
        let advance = (position >> 16) as usize;
        self.fraction = (position & 0xffff) as u32;
        // Earlier input cannot affect a four-tap filter. Skip it arithmetically,
        // including loop boundaries, before reading the remaining history.
        input.skip(advance.saturating_sub(self.history.len()));
        for _ in 0..advance.min(self.history.len()) {
            self.history.rotate_left(1);
            self.history[3] = input.next_sample();
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
    sample: &'a Sample,
    position: usize,
    first_end: usize,
    loop_at: usize,
    #[cfg(test)]
    reads: usize,
}

impl<'a> SampleCursor<'a> {
    pub fn new(sample: &'a Sample) -> Result<Self> {
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
            #[cfg(test)]
            reads: 0,
        })
    }

    pub fn sample(&self) -> &Sample {
        self.sample
    }

    pub fn is_done(&self) -> bool {
        self.position >= self.first_end && self.sample.loop_pcm.is_empty()
    }

    fn skip(&mut self, frames: usize) {
        let first = frames.min(self.first_end - self.position);
        self.position += first;
        if !self.sample.loop_pcm.is_empty() {
            self.loop_at = (self.loop_at + (frames - first) % self.sample.loop_pcm.len())
                % self.sample.loop_pcm.len();
        }
    }

    pub fn next_sample(&mut self) -> i16 {
        #[cfg(test)]
        {
            self.reads += 1;
        }
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

    fn sample(pcm: Vec<i16>) -> Sample {
        Sample {
            key: 60,
            rate: crate::SOURCE_RATE as u16,
            loop_start: 0,
            loop_length: 0,
            pcm,
            loop_pcm: vec![],
        }
    }

    #[test]
    fn source_modes_keep_history_and_fraction_across_calls() {
        let sample = sample((1..=20).map(|value| value * 100).collect());
        let mut input = SampleCursor::new(&sample).unwrap();
        let mut linear = Resampler::new(Mode::Linear, 32768);
        let samples: Vec<_> = (0..10).map(|_| linear.next_sample(&mut input)).collect();
        assert_eq!(samples, [0, 0, 0, 0, 0, 0, 50, 100, 150, 200]);
        linear.set_ratio(65536);
        assert_eq!(linear.next_sample(&mut input), 300);
        let mut direct = Resampler::new(Mode::Direct, 0);
        assert_eq!(direct.next_sample(&mut input), 700);
        direct.set_ratio(u32::MAX);
        assert_eq!(direct.next_sample(&mut input), 800);
    }

    #[test]
    fn maximum_pitch_reads_only_filter_history_and_preserves_loop_phase() {
        let sample = Sample {
            loop_start: 1,
            loop_length: 2,
            loop_pcm: vec![4, 5],
            ..sample(vec![1, 2, 3, 99])
        };
        let mut input = SampleCursor::new(&sample).unwrap();
        let mut source = Resampler::new(Mode::Linear, u32::MAX);
        for _ in 0..2 {
            let before = input.reads;
            assert_eq!(source.next_sample(&mut input), 4);
            assert!(input.reads - before <= 4);
            assert_eq!(source.history, [4, 5, 4, 5]);
        }
        assert_eq!(source.fraction, 65534);
        assert!(!input.is_done());
    }

    #[test]
    fn skips_match_first_pass_restored_loops_and_exhausted_input() {
        for loop_pcm in [vec![], vec![4, 5]] {
            let sample = Sample {
                loop_start: 1,
                loop_length: loop_pcm.len() as u32,
                loop_pcm,
                ..sample(vec![1, 2, 3, 99])
            };
            let mut skipped = SampleCursor::new(&sample).unwrap();
            let mut read = SampleCursor::new(&sample).unwrap();
            for frames in [0, 1, 2, 3, 27, 65536, 1] {
                skipped.skip(frames);
                for _ in 0..frames {
                    read.next_sample();
                }
                assert_eq!(skipped.is_done(), read.is_done());
                for _ in 0..5 {
                    assert_eq!(skipped.next_sample(), read.next_sample());
                }
            }
        }
    }

    #[test]
    fn polyphase_selects_all_seven_fraction_bits_and_saturates() {
        let mut coefficients = [[0; 4]; 128];
        coefficients[64][3] = 16384;
        coefficients[0] = [32767; 4];
        let sample = sample(vec![10000; 8]);
        let mut input = SampleCursor::new(&sample).unwrap();
        let mut source = Resampler::new(Mode::Polyphase(&coefficients), 98304);
        assert_eq!(source.next_sample(&mut input), 5000);
        assert_eq!(source.next_sample(&mut input), 29999);
        assert_eq!(source.next_sample(&mut input), 5000);
        assert_eq!(source.next_sample(&mut input), 32767);
        assert!(Coefficients::from_be_bytes(&[0; 4095]).is_err());
    }

    #[test]
    fn final_input_sample_remains_audible_until_filter_history_drains() {
        let sample = sample(vec![12000]);
        let coefficients = [[16384; 4]; 128];
        for (mode, expected) in [
            (Mode::Linear, [0, 0, 0, 12000, 0]),
            (Mode::Polyphase(&coefficients), [6000, 6000, 6000, 6000, 0]),
        ] {
            let mut input = SampleCursor::new(&sample).unwrap();
            let mut source = Resampler::new(mode, 65536);
            for (frame, expected) in expected.into_iter().enumerate() {
                assert_eq!(source.next_sample(&mut input), expected);
                assert!(input.is_done());
                assert_eq!(source.has_pending(), frame < 4);
            }
        }
        let mut input = SampleCursor::new(&sample).unwrap();
        let mut direct = Resampler::new(Mode::Direct, 65536);
        assert_eq!(direct.next_sample(&mut input), 12000);
        assert!(!direct.has_pending());
    }

    #[test]
    fn zero_rate_holds_live_input_but_drains_exhausted_filter_history() {
        let sample = sample(vec![12000]);
        let mut input = SampleCursor::new(&sample).unwrap();
        let mut source = Resampler::new(Mode::Linear, 0);
        assert_eq!(source.next_sample(&mut input), 0);
        assert!(!input.is_done());
        source.set_ratio(65536);
        assert_eq!(source.next_sample(&mut input), 0);
        assert!(input.is_done() && source.has_pending());
        source.set_ratio(0);
        let tail: Vec<_> = (0..4).map(|_| source.next_sample(&mut input)).collect();
        assert_eq!(tail, [0, 0, 12000, 0]);
        assert!(!source.has_pending());
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
