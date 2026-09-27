//! Bounded Rust movie codecs and device-rate conversion, independent of Bevy.
mod container;
mod decoder;
pub mod encode;
pub use container::VideoReader;
pub mod output;
mod stream;
pub use stream::MovieStream;

use anyhow::{Context, Result, ensure};
use resonance_content::MovieAsset;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
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

struct Track<T> {
    receiver: Mutex<Option<Receiver<DecodeResult<T>>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
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
        let worker = thread::Builder::new().name(name.into()).spawn(move || {
            let result = decode(&stop, &sender);
            if !stop.load(Ordering::Acquire) {
                let _ = sender.send(result.map(|()| None).map_err(|error| format!("{error:#}")));
            }
        })?;
        Ok(Self {
            receiver: Mutex::new(Some(receiver)),
            cancelled,
            worker: Some(worker),
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
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Independent local-file audio/video workers with bounded queues. Video runs
/// ten decoded frames ahead; neither decoding nor video backpressure holds up
/// audio. Each track cancels and joins its worker when dropped.
pub struct MovieDecoder {
    video: Track<VideoFrame>,
    audio: Track<AudioChunk>,
}

impl MovieDecoder {
    pub fn open(path: &Path, asset: MovieAsset) -> Result<Self> {
        asset.validate()?;
        let path = path
            .canonicalize()
            .context("cooked movie file is missing")?;
        let video_path = path.clone();
        let video_asset = asset.clone();
        let video = Track::start(
            "movie-video-decoder",
            VIDEO_LOOKAHEAD,
            move |stop, sender| decoder::video(&video_path, &video_asset, stop, sender),
        )?;
        let audio = Track::start("movie-audio-decoder", 16, move |stop, sender| {
            decoder::audio(&path, &asset, stop, sender)
        })?;
        Ok(Self { video, audio })
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
    pub fn frame(path: &Path, asset: MovieAsset, index: u32) -> Result<VideoFrame> {
        asset.validate()?;
        ensure!(index < asset.frames, "movie capture frame is out of range");
        let mut reader = VideoReader::open(path)?;
        ensure!(
            reader.dimensions() == (asset.width, asset.height),
            "movie video format disagrees with the cooked manifest"
        );
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
