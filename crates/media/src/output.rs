//! Device-rate conversion is separate from the oracle-compatible native DSP.
use anyhow::{Result, ensure};
use ffmpeg_next as ffmpeg;
use resonance_playback::{Converter, SOURCE_BLOCK, SOURCE_RATE};

pub struct Resampler {
    context: ffmpeg::software::resampling::Context,
    input: ffmpeg::frame::Audio,
    output: ffmpeg::frame::Audio,
}
impl Resampler {
    pub fn new(rate: u32) -> Result<Self> {
        ensure!(
            (8000..=192000).contains(&rate),
            "invalid output sample rate"
        );
        ffmpeg::init()?;
        let format = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);
        let mut options = ffmpeg::Dictionary::new();
        options.set("filter_size", "64");
        options.set("phase_shift", "10");
        options.set("cutoff", "0.97");
        let context = ffmpeg::software::resampling::Context::get_with(
            format,
            ffmpeg::ChannelLayout::STEREO,
            SOURCE_RATE,
            format,
            ffmpeg::ChannelLayout::STEREO,
            rate,
            options,
        )?;
        let mut input =
            ffmpeg::frame::Audio::new(format, SOURCE_BLOCK, ffmpeg::ChannelLayout::STEREO);
        input.set_rate(SOURCE_RATE);
        let mut output = ffmpeg::frame::Audio::new(format, 2048, ffmpeg::ChannelLayout::STEREO);
        output.set_rate(rate);
        Ok(Self {
            context,
            input,
            output,
        })
    }
    pub fn delay_frames(&self) -> i64 {
        self.context.delay().map_or(0, |d| d.output)
    }
    /// File taps may finish the converter. Live output continues with silence
    /// so filter tails and the device clock keep running after a source ends.
    pub fn finish(&mut self, output: &mut Vec<[f32; 2]>) -> Result<()> {
        for _ in 0..16 {
            self.output.set_samples(2048);
            self.context.flush(&mut self.output)?;
            output.extend(
                self.output
                    .plane::<(f32, f32)>(0)
                    .iter()
                    .map(|&(l, r)| [l, r]),
            );
            if self.output.samples() == 0 {
                return Ok(());
            }
        }
        anyhow::bail!("output resampler failed to drain")
    }
}
impl Converter for Resampler {
    fn convert(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) -> Result<()> {
        ensure!(
            !input.is_empty() && input.len() <= SOURCE_BLOCK,
            "invalid native output block"
        );
        self.input.set_samples(input.len());
        for (destination, source) in self.input.plane_mut::<(f32, f32)>(0).iter_mut().zip(input) {
            *destination = (source[0], source[1]);
        }
        self.output.set_samples(2048);
        self.context.run(&self.input, &mut self.output)?;
        output.extend(
            self.output
                .plane::<(f32, f32)>(0)
                .iter()
                .map(|&(l, r)| [l, r]),
        );
        Ok(())
    }
}
