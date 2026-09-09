//! Bounded decoded lookahead, distinct from the short committed device ring.
use anyhow::{Result, ensure};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
pub struct Pcm {
    chunks: Mutex<VecDeque<Vec<f32>>>,
    decoded: AtomicU64,
    consumed: AtomicU64,
    finished: AtomicBool,
    underruns: AtomicU64,
    capacity: u64,
}
impl Default for Pcm {
    fn default() -> Self {
        Self::new(crate::SOURCE_RATE)
    }
}
impl Pcm {
    pub fn new(rate: u32) -> Self {
        Self {
            chunks: Mutex::new(VecDeque::with_capacity(64)),
            decoded: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            capacity: u64::from(rate) / 2,
        }
    }
    pub fn buffered(&self) -> u64 {
        self.decoded
            .load(Ordering::Acquire)
            .saturating_sub(self.consumed.load(Ordering::Acquire))
    }
    pub fn needs_data(&self) -> bool {
        self.buffered() < self.capacity
    }
    pub fn push(&self, start: u64, samples: Vec<f32>) -> Result<()> {
        ensure!(
            start == self.decoded.load(Ordering::Acquire),
            "discontinuous decoded audio"
        );
        ensure!(
            samples.len().is_multiple_of(2) && !samples.is_empty() && samples.len() <= 131_072,
            "invalid decoded PCM block"
        );
        ensure!(self.needs_data(), "decoded audio lookahead exceeded");
        let frames = samples.len() as u64 / 2;
        self.chunks
            .lock()
            .expect("PCM queue poisoned")
            .push_back(samples);
        self.decoded.fetch_add(frames, Ordering::Release);
        Ok(())
    }
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Acquire)
    }
    pub fn source(self: &Arc<Self>, mono: bool) -> PcmSource {
        PcmSource {
            buffer: self.clone(),
            chunk: Vec::new().into_iter(),
            right: None,
            mono,
        }
    }
}
pub struct PcmSource {
    buffer: Arc<Pcm>,
    chunk: std::vec::IntoIter<f32>,
    right: Option<f32>,
    mono: bool,
}
impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            self.buffer.consumed.fetch_add(1, Ordering::Release);
            return Some(right);
        }
        if self.chunk.len() == 0 {
            if let Some(chunk) = self
                .buffer
                .chunks
                .lock()
                .expect("PCM queue poisoned")
                .pop_front()
            {
                self.chunk = chunk.into_iter();
            } else if self.buffer.finished() {
                return None;
            } else {
                // A source starvation is distinct from a device starvation.
                // The control thread reports it; never block the mixer worker.
                self.buffer.underruns.fetch_add(1, Ordering::Release);
                // Terminal failure keeps stereo pairing intact even if the
                // producer resumes between the left and right sample calls.
                self.buffer.finish();
                return None;
            }
        }
        let left = self.chunk.next()?;
        let right = self.chunk.next().expect("incomplete decoded stereo frame");
        let (left, right) = if self.mono {
            let value = (left + right) * 0.5;
            (value, value)
        } else {
            (left, right)
        };
        self.right = Some(right);
        Some(left)
    }
}
