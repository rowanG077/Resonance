//! Native playback of prepared mono or stereo PCM.
use super::*;

const RELEASE_MILLISECONDS: u64 = 5;
const RELEASE_FRAMES: u64 = RATE as u64 * RELEASE_MILLISECONDS / 1000;

pub(super) struct Sampler {
    clip: Arc<Clip>,
    frame: u64,
    duration: u64,
    last: [f32; 2],
}
impl Sampler {
    pub(super) fn new(clip: Arc<Clip>) -> Result<Self> {
        ensure!(
            clip.rate > 0
                && (1..=2).contains(&clip.channels)
                && clip.sample_count() > 0
                && clip.sample_count().is_multiple_of(clip.channels),
            "invalid battle stream dimensions"
        );
        let duration = ((clip.sample_count() / clip.channels) as u64 * u64::from(RATE))
            .div_ceil(u64::from(clip.rate));
        Ok(Self {
            clip,
            frame: 0,
            duration,
            last: [0.; 2],
        })
    }

    pub(super) fn finished(&self) -> bool {
        self.frame >= self.duration + RELEASE_FRAMES
    }

    pub(super) fn sample(&mut self) -> Option<[f32; 2]> {
        if self.finished() {
            return None;
        }
        let sample = if self.frame < self.duration {
            self.last = self
                .clip
                .sample(self.frame * u64::from(self.clip.rate), RATE, [1.; 2])
                .expect("validated PCM cursor");
            self.last
        } else {
            // Finish smoothly even when a prepared clip ends on a nonzero sample.
            let gain = 1. - (self.frame - self.duration + 1) as f32 / RELEASE_FRAMES as f32;
            self.last.map(|value| value * gain)
        };
        self.frame += 1;
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_and_stereo_play_at_the_declared_rate_and_release_to_silence() -> Result<()> {
        for rate in [8000, 22050, 48000, 96000] {
            for channels in [1, 2] {
                let mut sampler = Sampler::new(Arc::new(test_clip(
                    (0..rate / 10).flat_map(|_| [8192, -8192].into_iter().take(channels)),
                    rate,
                    channels,
                )))?;
                let output: Vec<_> = std::iter::from_fn(|| sampler.sample()).collect();
                assert!((0.1..0.11).contains(&(output.len() as f32 / RATE as f32)));
                assert_eq!(
                    output[0],
                    if channels == 1 {
                        [0.25; 2]
                    } else {
                        [0.25, -0.25]
                    }
                );
                assert_eq!(output.last(), Some(&[0.; 2]));
                assert!(sampler.finished());
            }
        }
        Ok(())
    }

    #[test]
    fn interpolates_between_frames_without_mixing_stereo_channels() -> Result<()> {
        let mut sampler = Sampler::new(Arc::new(test_clip(vec![0, 16384, 16384, 0], RATE / 2, 2)))?;
        assert_eq!(sampler.sample(), Some([0., 0.5]));
        assert_eq!(sampler.sample(), Some([0.25; 2]));
        assert_eq!(sampler.sample(), Some([0.5, 0.]));
        Ok(())
    }

    #[test]
    fn rejects_invalid_prepared_clips() {
        for (rate, channels, pcm) in [
            (0, 1, vec![1]),
            (32000, 0, vec![1]),
            (32000, 3, vec![1, 1, 1]),
            (32000, 2, vec![1]),
            (32000, 1, vec![]),
        ] {
            assert!(Sampler::new(Arc::new(test_clip(pcm, rate, channels))).is_err());
        }
    }
}
