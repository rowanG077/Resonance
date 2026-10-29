//! Bounded PCM music readers over retained file handles. Each playback has its
//! own cursor, so title/field/battle instances never seek one another's stream.
use anyhow::{Context, Result, ensure};
use resonance_content::library::StreamReader;
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    ops::Range,
    sync::Arc,
};

const PAGE_FRAMES: u32 = 4096;
const MAX_SECONDS: u32 = 60 * 60;

#[derive(Clone)]
struct Page {
    start: u32,
    frames: Arc<Vec<[i16; 2]>>,
}

pub struct Prepared {
    source: StreamReader,
    data: u64,
    frames: u32,
    rate: u32,
    channels: u16,
    repeat: Option<Range<u32>>,
    pages: VecDeque<Page>,
}

impl Prepared {
    pub fn new(file: File) -> Result<Arc<Self>> {
        let metadata = file.metadata()?;
        let source = StreamReader::new(file)?;
        let wave = hound::WavReader::new(BufReader::new(source.clone()))?;
        let spec = wave.spec();
        ensure!(
            spec.sample_format == hound::SampleFormat::Int
                && spec.bits_per_sample == 16
                && (spec.channels == 1 || spec.channels == 2),
            "music requires mono or stereo PCM16 WAV"
        );
        ensure!(
            (8000..=192000).contains(&spec.sample_rate),
            "unsupported music sample rate"
        );
        let frames = wave.duration();
        ensure!(
            frames > 0 && frames <= spec.sample_rate * MAX_SECONDS,
            "music must contain between one frame and one hour of PCM"
        );
        ensure!(
            wave.len() == frames * u32::from(spec.channels),
            "incomplete music PCM frame"
        );
        let data = wave.into_inner().stream_position()?;
        ensure!(
            data + u64::from(frames) * u64::from(spec.channels) * 2 <= metadata.len(),
            "truncated music WAV"
        );
        let repeat = resonance_asset_writer::wav::read_loop(&mut source.clone())?;
        if let Some(repeat) = &repeat {
            ensure!(
                repeat.start < repeat.end && repeat.end <= frames,
                "music loop is outside current WAV frames"
            );
        }
        let mut prepared = Self {
            source,
            data,
            frames,
            rate: spec.sample_rate,
            channels: spec.channels,
            repeat,
            pages: VecDeque::new(),
        };
        prepared.pages.push_back(prepared.page(0)?);
        if let Some(repeat) = &prepared.repeat {
            let page = repeat.start / PAGE_FRAMES * PAGE_FRAMES;
            if page != 0 {
                prepared.pages.push_back(prepared.page(page)?);
            }
        }
        Ok(Arc::new(prepared))
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    pub fn repeat(&self) -> Option<Range<u32>> {
        self.repeat.clone()
    }

    fn page(&self, start: u32) -> Result<Page> {
        ensure!(start < self.frames, "music page is outside current WAV");
        let count = PAGE_FRAMES.min(self.frames - start);
        let mut source = self.source.clone();
        source.seek(SeekFrom::Start(
            self.data + u64::from(start) * u64::from(self.channels) * 2,
        ))?;
        let mut bytes = vec![0; count as usize * usize::from(self.channels) * 2];
        source
            .read_exact(&mut bytes)
            .context("read music PCM page")?;
        let frames = bytes
            .chunks_exact(usize::from(self.channels) * 2)
            .map(|frame| {
                let left = i16::from_le_bytes([frame[0], frame[1]]);
                let right = if self.channels == 1 {
                    left
                } else {
                    i16::from_le_bytes([frame[2], frame[3]])
                };
                [left, right]
            })
            .collect();
        Ok(Page {
            start,
            frames: Arc::new(frames),
        })
    }

    pub fn start(self: &Arc<Self>, output_rate: u32) -> Result<Player> {
        ensure!(
            (8000..=192000).contains(&output_rate),
            "unsupported music output rate"
        );
        Ok(Player {
            source: self.clone(),
            pages: self.pages.clone(),
            position: 0,
            output_rate,
            paused: false,
        })
    }
}

pub struct Player {
    source: Arc<Prepared>,
    pages: VecDeque<Page>,
    // Rational source position; preserve resampling overshoot when wrapping.
    position: u64,
    output_rate: u32,
    paused: bool,
}

impl Player {
    pub fn pause(&mut self, paused: bool) {
        self.paused = paused;
    }
    pub fn position(&self) -> u64 {
        self.position / u64::from(self.output_rate)
    }

    fn sample(&mut self, index: u32) -> Result<[i16; 2]> {
        let start = index / PAGE_FRAMES * PAGE_FRAMES;
        if let Some(slot) = self.pages.iter().position(|page| page.start == start) {
            let page = self.pages.remove(slot).unwrap();
            let result = page.frames[(index - start) as usize];
            self.pages.push_front(page);
            return Ok(result);
        }
        let page = self.source.page(start)?;
        let result = page.frames[(index - start) as usize];
        self.pages.truncate(1);
        self.pages.push_front(page);
        Ok(result)
    }

    /// PCM units before volume controls and final mix/clipping. None means a
    /// completed one-shot. Pausing emits silence without consuming the recording.
    pub fn next_frame(&mut self) -> Result<Option<[f32; 2]>> {
        if self.paused {
            return Ok(Some([0.; 2]));
        }
        let denominator = u64::from(self.output_rate);
        let end = self
            .source
            .repeat
            .as_ref()
            .map_or(self.source.frames, |repeat| repeat.end);
        if self.position >= u64::from(end) * denominator {
            let Some(repeat) = &self.source.repeat else {
                return Ok(None);
            };
            let start = u64::from(repeat.start) * denominator;
            let length = u64::from(repeat.end - repeat.start) * denominator;
            self.position = start + (self.position - start) % length;
        }
        let index = (self.position / denominator) as u32;
        let next = if index + 1 == end {
            self.source
                .repeat
                .as_ref()
                .map_or(index, |repeat| repeat.start)
        } else {
            index + 1
        };
        let fraction = (self.position % denominator) as f32 / self.output_rate as f32;
        let a = self.sample(index)?;
        let b = self.sample(next)?;
        let gain = if self.source.channels == 1 {
            std::f32::consts::FRAC_1_SQRT_2
        } else {
            1.
        };
        let result = std::array::from_fn(|channel| {
            let a = f32::from(a[channel]);
            (a + (f32::from(b[channel]) - a) * fraction) * gain
        });
        self.position += u64::from(self.source.rate);
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests;
