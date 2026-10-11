//! Device-rate conversion is separate from the oracle-compatible native DSP.
use anyhow::{Result, ensure};
use resonance_playback::{Converter, SOURCE_BLOCK, SOURCE_RATE};
use rubato::{
    Resampler as _, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

pub struct Resampler {
    context: SincFixedIn<f32>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    rate: u32,
    input_frames: u64,
    output_frames: u64,
    finished: bool,
    pending: usize,
}
impl Resampler {
    pub fn new(rate: u32) -> Result<Self> {
        ensure!(
            (8000..=192000).contains(&rate),
            "invalid output sample rate"
        );
        let context = SincFixedIn::new(
            f64::from(rate) / f64::from(SOURCE_RATE),
            1.0,
            SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Cubic,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            },
            SOURCE_BLOCK,
            2,
        )?;
        Ok(Self {
            input: context.input_buffer_allocate(true),
            output: context.output_buffer_allocate(true),
            context,
            rate,
            input_frames: 0,
            output_frames: 0,
            finished: false,
            pending: 0,
        })
    }
    fn target_frames(&self) -> u64 {
        (self.input_frames * u64::from(self.rate) + u64::from(SOURCE_RATE) / 2)
            / u64::from(SOURCE_RATE)
    }
    pub fn delay_frames(&self) -> i64 {
        self.target_frames().saturating_sub(self.output_frames) as i64
    }
    fn process(&mut self, output: &mut Vec<[f32; 2]>, limit: u64) -> Result<()> {
        let (_, written) = self
            .context
            .process_into_buffer(&self.input, &mut self.output, None)?;
        let count = written.min(limit.saturating_sub(self.output_frames) as usize);
        output.extend((0..count).map(|i| [self.output[0][i], self.output[1][i]]));
        self.output_frames += count as u64;
        Ok(())
    }
    /// Drain the filter and emit exactly the rounded source duration. Startup
    /// latency stays in the input lookahead; it is not a prefix of silent PCM.
    pub fn finish(&mut self, output: &mut Vec<[f32; 2]>) -> Result<()> {
        self.finished = true;
        if self.pending != 0 {
            for channel in &mut self.input {
                channel[self.pending..].fill(0.0);
            }
            self.process(output, self.target_frames())?;
            self.pending = 0;
        }
        self.input.iter_mut().for_each(|channel| channel.fill(0.0));
        let target = self.target_frames();
        for _ in 0..16 {
            if self.output_frames == target {
                return Ok(());
            }
            self.process(output, target)?;
        }
        anyhow::bail!("output resampler failed to drain")
    }
}
impl Converter for Resampler {
    fn convert(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) -> Result<()> {
        ensure!(!self.finished, "output resampler is already finished");
        ensure!(
            !input.is_empty() && input.len() <= SOURCE_BLOCK,
            "invalid native output block"
        );
        self.input_frames += input.len() as u64;
        for frame in input {
            self.input[0][self.pending] = frame[0];
            self.input[1][self.pending] = frame[1];
            self.pending += 1;
            if self.pending == SOURCE_BLOCK {
                self.process(output, u64::MAX)?;
                self.pending = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn convert(input: &[[f32; 2]], rate: u32, block: usize) -> Vec<[f32; 2]> {
        let mut resampler = Resampler::new(rate).unwrap();
        let mut output = Vec::new();
        for chunk in input.chunks(block) {
            resampler.convert(chunk, &mut output).unwrap();
        }
        resampler.finish(&mut output).unwrap();
        assert_eq!(
            output.len() as u64,
            (input.len() as u64 * u64::from(rate) + u64::from(SOURCE_RATE) / 2)
                / u64::from(SOURCE_RATE)
        );
        let len = output.len();
        resampler.finish(&mut output).unwrap();
        assert_eq!(len, output.len());
        assert!(resampler.convert(&[[0.; 2]], &mut output).is_err());
        output
    }
    #[test]
    fn preserves_duration_channels_and_origin_at_device_rates() {
        let mut input = vec![[0.; 2]; 6413];
        input[1000][0] = 0.5;
        input[3000][1] = -0.5;
        for rate in [8000, 32028, 44100, 48000, 192000] {
            let output = convert(&input, rate, SOURCE_BLOCK);
            // Rubato's sinc sampling origin is one output period minus one
            // native period; the phase offset stays below one native sample.
            for (channel, source) in [(0, 1000), (1, 3000)] {
                let peak = output
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a[channel].abs().total_cmp(&b[channel].abs()))
                    .unwrap()
                    .0;
                assert!(
                    peak.abs_diff(
                        ((source + 1) as f64 * f64::from(rate) / f64::from(SOURCE_RATE) - 1.0)
                            .round() as usize
                    ) <= 1,
                    "rate {rate}: source {source}, peak {peak}"
                );
            }
            assert!(output.iter().flatten().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn partial_blocks_do_not_insert_silence() {
        let input: Vec<_> = (0..4001)
            .map(|i| [((i as f32) * 0.13).sin() * 0.2, 0.0])
            .collect();
        let a = convert(&input, 48000, 160);
        let b = convert(&input, 48000, 37);
        assert!(
            a.iter()
                .zip(b)
                .all(|(a, b)| (a[0] - b[0]).abs() < 0.00001 && b[1] == 0.0)
        );
    }
    #[test]
    fn downsampling_rejects_out_of_band_energy() {
        let input: Vec<_> = (0..SOURCE_RATE)
            .map(|i| {
                let t = i as f32 / SOURCE_RATE as f32;
                [
                    (std::f32::consts::TAU * 1000.0 * t).sin(),
                    (std::f32::consts::TAU * 10000.0 * t).sin(),
                ]
            })
            .collect();
        let output = convert(&input, 8000, SOURCE_BLOCK);
        let energy = |ch| {
            output[100..7900]
                .iter()
                .map(|v| f64::from(v[ch]).powi(2))
                .sum::<f64>()
        };
        assert!(10.0 * (energy(0) / energy(1)).log10() > 60.0);
    }
}
