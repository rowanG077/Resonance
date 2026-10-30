//! Offline music rendering. Musical boundaries come from the live sequencer;
//! PCM is written incrementally without collecting a soundtrack in memory.
use super::{LiveControls, stream::Stream};
use crate::{BLOCK_FRAMES, CONTROLS_PER_BLOCK, SOURCE_RATE, package::Loaded, reverb::Studio};
use anyhow::{Result, ensure};
use std::{
    collections::VecDeque,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    ops::Range,
    sync::Arc,
};

pub const MAX_FRAMES: u64 = SOURCE_RATE as u64 * 60 * 20;
const MAX_RECORDING_FRAMES: u64 = SOURCE_RATE as u64 * 60 * 60;
const WINDOW_FRAMES: usize = 2 * BLOCK_FRAMES;
const MAX_BOUNDARIES: usize = 128;

struct Boundary {
    frame: u64,
    samples: Vec<[i32; 8]>,
}

#[derive(Debug)]
pub struct Rendered {
    pub frames: u64,
    pub repeat: Option<Range<u64>>,
    /// Native bus interval reused when baking a periodic effects state. The
    /// original PCM prefix ends at this interval's end; no join is blended.
    pub source_repeat: Option<Range<u64>>,
}

/// Render the original introduction and a settled repeat, or a complete one-shot
/// including its release/reverb. The writer must be stereo PCM16 at SOURCE_RATE.
/// A caller publishes only after successful rendering and WAV finalization.
pub fn render<W: Write + Seek, B: Read + Write + Seek>(
    loaded: Arc<Loaded>,
    reverbs: [[f32; 5]; 2],
    writer: &mut hound::WavWriter<W>,
    scratch: &mut B,
    cancelled: impl Fn() -> bool,
) -> Result<Rendered> {
    render_bounded(
        loaded,
        reverbs,
        writer,
        scratch,
        MAX_FRAMES,
        MAX_RECORDING_FRAMES,
        cancelled,
    )
}

fn render_bounded<W: Write + Seek, B: Read + Write + Seek>(
    loaded: Arc<Loaded>,
    reverbs: [[f32; 5]; 2],
    writer: &mut hound::WavWriter<W>,
    scratch: &mut B,
    mut limit: u64,
    recording_limit: u64,
    cancelled: impl Fn() -> bool,
) -> Result<Rendered> {
    ensure!(
        loaded.score().origin == crate::data::ScoreOrigin::Sequence,
        "music recording requires a sequence"
    );
    let spec = writer.spec();
    ensure!(
        spec.channels == 2
            && spec.sample_rate == SOURCE_RATE
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "music recording requires native-rate stereo PCM16 output"
    );
    let looping = !loaded.score().loop_events.is_empty();
    let mut stream = Stream::new(loaded, looping)?;
    let mut studio = Studio::new(reverbs)?;
    let mut frames = 0;
    let mut written = 0;
    let mut block = [[[0; 2]; 3]; BLOCK_FRAMES];
    let mut history = VecDeque::new();
    // Hold only the lookahead window. Once a recurring boundary is verified,
    // its following frames have not yet been committed to the WAV.
    let mut pending = VecDeque::<[i16; 2]>::new();
    let mut measuring = Vec::<Boundary>::new();
    let mut boundaries = Vec::<Boundary>::new();
    scratch.seek(SeekFrom::Start(0))?;
    let mut scratch = BufWriter::with_capacity(64 * 1024, scratch);
    let mut replay = None::<Replay>;
    let mut source_repeat = None;
    loop {
        ensure!(!cancelled(), "music rendering cancelled");
        let length = if let Some(replay) = &mut replay {
            for buses in &mut block {
                *buses = replay.next(scratch.get_mut())?;
            }
            BLOCK_FRAMES
        } else {
            stream.render_block([LiveControls::default(); CONTROLS_PER_BLOCK], &mut block)?
        };
        let boundary = stream.last_loop_frame();
        for (index, buses) in block.iter().enumerate() {
            if !looping && index >= length && studio.is_silent() {
                ensure!(frames != 0, "music recording is empty");
                for frame in pending.drain(..) {
                    for sample in frame {
                        writer.write_sample(sample)?;
                    }
                }
                return Ok(Rendered {
                    frames,
                    repeat: None,
                    source_repeat: None,
                });
            }
            ensure!(
                frames < limit,
                "music exceeds bounded rendering duration at {frames} source frames; selected dry passage: {source_repeat:?}"
            );
            let at_boundary = replay
                .as_ref()
                .map_or(boundary == Some(frames), |r| r.next_boundary == frames);
            if at_boundary {
                if let Some(replay) = &mut replay {
                    replay.next_boundary += replay.source.end - replay.source.start;
                }
                // Recurrence itself verifies the cut. An extra traversal for
                // an arbitrary warm-up time would only lengthen the recording.
                if history.len() == WINDOW_FRAMES {
                    ensure!(
                        boundaries.len() + measuring.len() < MAX_BOUNDARIES,
                        "music has no recurring rendered boundary within {MAX_BOUNDARIES} musical passes"
                    );
                    measuring.push(Boundary {
                        frame: frames,
                        samples: history.iter().copied().collect(),
                    });
                }
            }
            if looping && replay.is_none() {
                for sample in buses.iter().flatten() {
                    scratch.write_all(&sample.to_le_bytes())?;
                }
            }
            let pcm = studio.process(*buses);
            let state = [
                buses[0][0],
                buses[0][1],
                buses[1][0],
                buses[1][1],
                buses[2][0],
                buses[2][1],
                pcm[0],
                pcm[1],
            ];
            for boundary in &mut measuring {
                boundary.samples.push(state);
            }
            history.push_back(state);
            if history.len() > WINDOW_FRAMES {
                history.pop_front();
            }
            pending.push_back(pcm.map(|s| s.clamp(i16::MIN as i32, i16::MAX as i32) as i16));
            if pending.len() > WINDOW_FRAMES {
                for sample in pending.pop_front().unwrap() {
                    writer.write_sample(sample)?;
                }
                written += 1;
            }
            frames += 1;
            while measuring
                .first()
                .is_some_and(|b| b.samples.len() == WINDOW_FRAMES * 2)
            {
                let current = measuring.remove(0);
                // The native clock can need several musical passes to return
                // to the same event/sample phase. Match both sides of the cut,
                // including every dry/aux bus and the final studio output.
                // No crossfade, rounding of the repeat length or PCM rewriting.
                if let Some(previous) = boundaries.iter().find(|b| b.samples == current.samples) {
                    ensure!(
                        written == current.frame,
                        "music lookahead was committed before its boundary"
                    );
                    return Ok(Rendered {
                        frames: written,
                        repeat: Some(previous.frame..current.frame),
                        source_repeat,
                    });
                }
                if replay.is_none()
                    && let Some(previous) = boundaries.iter().find(|b| {
                        b.samples
                            .iter()
                            .zip(&current.samples)
                            .all(|(a, b)| a[..6] == b[..6])
                    })
                {
                    // The chosen dry performance is already a valid loop. Its
                    // effects must be baked against that repeated input, not
                    // against a different later synthesizer traversal. Replay
                    // the exact saved buses through the continuing studio until
                    // its state also recurs. No sample is crossfaded or retimed.
                    let source = previous.frame..current.frame;
                    // A valid source passage found near the search limit must
                    // still be allowed to repeat through the studio. Bound this
                    // separate phase by two periods plus the effect tail, within
                    // the same one-hour ceiling accepted by the WAV reader.
                    let tail = ((2. * reverbs[0][2].max(reverbs[1][2]) + 0.2) * SOURCE_RATE as f32)
                        .ceil() as u64;
                    limit = limit
                        .max(
                            source.end
                                + 2 * (source.end - source.start)
                                + tail
                                + (2 * WINDOW_FRAMES) as u64,
                        )
                        .min(recording_limit);
                    scratch.flush()?;
                    replay = Some(Replay::new(source.clone(), frames));
                    source_repeat = Some(source);
                    boundaries.clear();
                    measuring.clear();
                    boundaries.push(current);
                    break;
                }
                boundaries.push(current);
            }
            if replay.as_ref().is_some_and(|r| r.started_at == frames) {
                break;
            }
        }
    }
}

struct Replay {
    source: Range<u64>,
    position: u64,
    next_boundary: u64,
    started_at: u64,
    buffer: Vec<super::BusFrame>,
    cursor: usize,
}

impl Replay {
    fn new(source: Range<u64>, frame: u64) -> Self {
        let length = source.end - source.start;
        Self {
            position: source.start + (frame - source.end) % length,
            next_boundary: source.end + ((frame - source.end) / length + 1) * length,
            source,
            started_at: frame,
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    fn next(&mut self, scratch: &mut (impl Read + Seek)) -> Result<super::BusFrame> {
        if self.cursor == self.buffer.len() {
            if self.position == self.source.end {
                self.position = self.source.start;
            }
            let count = (self.source.end - self.position).min(4096) as usize;
            scratch.seek(SeekFrom::Start(self.position * 24))?;
            let mut bytes = vec![0u8; count * 24];
            scratch.read_exact(&mut bytes)?;
            self.buffer = bytes
                .chunks_exact(24)
                .map(|frame| {
                    std::array::from_fn(|bus| {
                        std::array::from_fn(|channel| {
                            let at = (bus * 2 + channel) * 4;
                            i32::from_le_bytes(frame[at..at + 4].try_into().unwrap())
                        })
                    })
                })
                .collect();
            self.cursor = 0;
        }
        let frame = self.buffer[self.cursor];
        self.cursor += 1;
        self.position += 1;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests;
