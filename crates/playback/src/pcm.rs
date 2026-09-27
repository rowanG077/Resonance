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
    rate: crate::SampleRate,
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
            rate: crate::SampleRate::new(rate).expect("nonzero PCM sample rate"),
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
        let mut chunks = self.chunks.lock().expect("PCM queue poisoned");
        chunks.push_back(samples);
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
            buffering: false,
        }
    }
}
pub struct PcmSource {
    buffer: Arc<Pcm>,
    chunk: std::vec::IntoIter<f32>,
    right: Option<f32>,
    mono: bool,
    buffering: bool,
}
impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            self.buffer.consumed.fetch_add(1, Ordering::Release);
            return Some(right);
        }
        // Refill the lookahead before resuming, so repeated mixer polls or
        // tiny producer chunks do not turn a single stall into rapid gaps.
        // EOF releases a short final tail without requiring a full buffer.
        if self.buffering && self.buffer.needs_data() && !self.buffer.finished() {
            return None;
        }
        if self.chunk.len() == 0 {
            let mut chunks = self.buffer.chunks.lock().expect("PCM queue poisoned");
            if let Some(chunk) = chunks.pop_front() {
                self.chunk = chunk.into_iter();
            } else if self.buffer.finished() {
                self.buffering = false;
                return None;
            } else {
                if !self.buffering {
                    self.buffer.underruns.fetch_add(1, Ordering::Release);
                }
                self.buffering = true;
                return None;
            }
        }
        self.buffering = false;
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
impl crate::Source for PcmSource {
    fn channels(&self) -> crate::ChannelCount {
        crate::ChannelCount::new(2).expect("stereo")
    }
    fn sample_rate(&self) -> crate::SampleRate {
        self.buffer.rate
    }
    fn is_pending(&self) -> bool {
        self.buffering
    }
}
