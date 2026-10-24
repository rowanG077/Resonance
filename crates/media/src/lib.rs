//! Bounded Rust movie codecs and device-rate conversion, independent of Bevy.
mod container;
mod decoder;
pub mod encode;
mod source;
pub use container::VideoReader;
pub mod output;
mod stream;
pub use stream::MovieStream;

use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread,
    time::Duration,
};

#[derive(Debug)]
pub struct VideoFrame {
    pub index: u32,
    pub timestamp: Duration,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub struct AudioChunk {
    pub start_frame: u64,
    pub timestamp: Duration,
    pub samples: Vec<f32>,
}

#[derive(Debug)]
pub enum MovieEvent {
    Video(VideoFrame),
    Audio(AudioChunk),
    End,
}

type DecodeResult<T> = std::result::Result<Option<T>, String>;

/// Decoded video frames buffered before playback and in each decoder queue.
pub const VIDEO_LOOKAHEAD: usize = 10;

/// Facts from the current media headers. Unknown stream length is supported;
/// playback and preparation use EOF instead of a manually maintained count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub width: u32,
    pub height: u32,
    pub frame_duration: Duration,
    pub sample_rate: u32,
    pub audio_frames: Option<u64>,
}

struct Track<T> {
    receiver: Mutex<Option<Receiver<DecodeResult<T>>>>,
    cancelled: Arc<AtomicBool>,
}
impl<T: Send + 'static> Track<T> {
    fn start(
        name: &str,
        capacity: usize,
        decode: impl FnOnce(&AtomicBool, &SyncSender<DecodeResult<T>>) -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        thread::Builder::new().name(name.into()).spawn(move || {
            let result = decode(&stop, &sender);
            if !stop.load(Ordering::Acquire) {
                let _ = sender.send(result.map(|()| None).map_err(|error| format!("{error:#}")));
            }
        })?;
        Ok(Self {
            receiver: Mutex::new(Some(receiver)),
            cancelled,
        })
    }
    fn complete(&self) -> bool {
        self.receiver
            .lock()
            .expect("movie receiver poisoned")
            .is_none()
    }
    fn try_next(&self) -> Result<Option<T>> {
        let mut receiver = self.receiver.lock().expect("movie receiver poisoned");
        let Some(queue) = receiver.as_ref() else {
            return Ok(None);
        };
        match queue.try_recv() {
            Ok(Ok(Some(frame))) => Ok(Some(frame)),
            Ok(Ok(None)) => {
                receiver.take();
                Ok(None)
            }
            Ok(Err(error)) => Err(anyhow::Error::msg(error)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                anyhow::bail!("movie worker ended without a completion event")
            }
        }
    }
}
impl<T> Drop for Track<T> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        // Closing the receiver releases a worker blocked on its full queue.
        self.receiver
            .get_mut()
            .expect("movie receiver poisoned")
            .take();
    }
}

/// Independent local-file audio/video workers with bounded queues. Video runs
/// ten decoded frames ahead; neither decoding nor video backpressure holds up
/// audio. Dropping a track cancels its worker without waiting for a decode or read.
pub struct MovieDecoder {
    video: Track<VideoFrame>,
    audio: Track<AudioChunk>,
    format: Format,
    source: source::Source,
}

impl MovieDecoder {
    pub fn open(path: &Path, audio_track: u16) -> Result<Self> {
        Self::from_file(
            File::open(path).with_context(|| format!("open movie {}", path.display()))?,
            audio_track,
        )
    }

    pub fn load(snapshot: &mut resonance_content::library::Snapshot, id: u32) -> Result<Self> {
        let selected = resonance_content::movie::capture(snapshot, id)?;
        Self::from_file(selected.file, selected.definition.audio_track)
            .with_context(|| format!("movie {id}: {}", selected.definition.path))
    }

    pub fn from_file(file: File, audio_track: u16) -> Result<Self> {
        let source = source::Source::new(file)?;
        let reader = VideoReader::from_input(container::from_source(source.clone())?)?;
        let audio =
            decoder::AudioReader::new(container::from_source(source.clone())?, audio_track)?;
        let (width, height) = reader.dimensions();
        let frame_duration = reader
            .frame_duration()
            .context("movie video requires a default frame duration")?;
        ensure!(
            (Duration::from_millis(10)..=Duration::from_millis(100)).contains(&frame_duration),
            "movie frame duration must be between 10 and 100 milliseconds"
        );
        let format = Format {
            width,
            height,
            frame_duration,
            sample_rate: audio.sample_rate,
            audio_frames: audio.frames,
        };
        let completion = decoder::Completion::default();
        let video_completion = completion.clone();
        let video = Track::start(
            "movie-video-decoder",
            VIDEO_LOOKAHEAD,
            move |stop, sender| decoder::video(reader, format, stop, sender, video_completion),
        )?;
        let audio = Track::start("movie-audio-decoder", 16, move |stop, sender| {
            audio.run(stop, sender, completion)
        })?;
        Ok(Self {
            video,
            audio,
            format,
            source,
        })
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// Offline capture planning only. Scan timestamps from the pinned stream,
    /// without decoding pixels or requiring a stored frame-count inventory.
    pub fn video_timestamps(&self) -> Result<Vec<Duration>> {
        let mut input = container::from_source(self.source.clone())?;
        let id = container::track(&input, matroska_demuxer::TrackType::Video)?
            .track_number()
            .get();
        let scale = input.info().timestamp_scale().get();
        let mut packet = matroska_demuxer::Frame::default();
        let mut times = Vec::new();
        while input.next_frame(&mut packet)? {
            if packet.track == id {
                let time = container::timestamp(packet.timestamp, scale)?;
                ensure!(
                    times.len() < 100_000
                        && time < Duration::from_secs(3600)
                        && times.last().is_none_or(|&previous| time > previous),
                    "invalid movie video timeline"
                );
                times.push(time);
            }
        }
        ensure!(
            times.first() == Some(&Duration::ZERO),
            "movie video must start at time zero"
        );
        Ok(times)
    }

    pub fn try_video(&self) -> Result<Option<VideoFrame>> {
        self.video.try_next()
    }
    pub fn try_audio(&self) -> Result<Option<AudioChunk>> {
        self.audio.try_next()
    }
    pub fn video_complete(&self) -> bool {
        self.video.complete()
    }
    pub fn audio_complete(&self) -> bool {
        self.audio.complete()
    }
    /// Drain either track for file-only inspection. Track order is preserved;
    /// the independent workers do not impose cross-track timestamp ordering.
    pub fn try_next(&self) -> Result<Option<MovieEvent>> {
        if let Some(frame) = self.try_video()? {
            return Ok(Some(MovieEvent::Video(frame)));
        }
        if let Some(chunk) = self.try_audio()? {
            return Ok(Some(MovieEvent::Audio(chunk)));
        }
        Ok((self.video_complete() && self.audio_complete()).then_some(MovieEvent::End))
    }

    /// Read a deterministic frame for capture without starting audio playback.
    pub fn frame(path: &Path, index: u32) -> Result<VideoFrame> {
        ensure!(index < 100_000, "movie capture frame is out of range");
        let mut reader = VideoReader::open(path)?;
        loop {
            let frame = reader
                .next_frame()?
                .context("movie ended before capture frame")?;
            if frame.index == index {
                return Ok(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_a_busy_decoder_returns_before_the_decode_finishes() {
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (finished, finished_rx) = mpsc::channel();
        let track = Track::<()>::start("cancel-test", 1, move |cancelled, _| {
            started.send(())?;
            release_rx.recv()?;
            finished.send(cancelled.load(Ordering::Acquire))?;
            Ok(())
        })
        .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (dropped, dropped_rx) = mpsc::channel();
        let caller = thread::spawn(move || {
            drop(track);
            dropped.send(()).unwrap();
        });
        let returned = dropped_rx.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        caller.join().unwrap();
        assert!(returned.is_ok(), "cancellation waited for the decoder");
    }
}
