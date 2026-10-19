//! A short native fade prevents discontinuities when a source stops.
use crate::sequence::BusFrame;

/// One source transport block provides a short, bounded release.
pub const RELEASE_FRAMES: u32 = crate::BLOCK_FRAMES as u32;

#[derive(Default)]
pub(crate) struct Release {
    samples: BusFrame,
    queued_frames: u64,
    remaining: u32,
}

impl Release {
    pub(crate) fn new(samples: BusFrame, queued_frames: u64) -> Self {
        Self {
            samples,
            queued_frames,
            remaining: RELEASE_FRAMES,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.remaining > 0
    }

    pub(crate) fn mix(&mut self, frame: &mut BusFrame) {
        if self.queued_frames > 0 {
            self.queued_frames -= 1;
            return;
        }
        for (bus, samples) in frame.iter_mut().zip(self.samples) {
            for (sample, tail) in bus.iter_mut().zip(samples) {
                *sample += (i64::from(tail) * i64::from(self.remaining) / i64::from(RELEASE_FRAMES))
                    as i32;
            }
        }
        self.remaining = self.remaining.saturating_sub(1);
    }
}
