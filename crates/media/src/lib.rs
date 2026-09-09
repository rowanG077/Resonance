//! Bounded movie decoding through FFmpeg, independent of Bevy.
mod decoder;
pub mod output;
mod stream;
pub use stream::MovieStream;

use anyhow::{Context, Result};
use resonance_content::MovieAsset;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
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

type DecodeResult = std::result::Result<MovieEvent, String>;

/// A local-file decoder with a bounded, interleaved output queue. Dropping it
/// closes the receiver, interrupts FFmpeg, and joins the worker even when the
/// producer is blocked on a full queue. No audio device is opened here.
pub struct MovieDecoder {
    receiver: Option<Mutex<Receiver<DecodeResult>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MovieDecoder {
    pub fn open(path: &Path, asset: MovieAsset) -> Result<Self> {
        asset.validate()?;
        let path = path
            .canonicalize()
            .context("cooked movie file is missing")?;
        let (sender, receiver) = mpsc::sync_channel(16);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let worker = thread::Builder::new()
            .name("movie-decoder".into())
            .spawn(move || {
                if let Err(error) = decoder::decode(&path, &asset, &stop, &sender)
                    && !stop.load(Ordering::Acquire)
                {
                    let _ = sender.send(Err(format!("{error:#}")));
                }
            })?;
        Ok(Self {
            receiver: Some(Mutex::new(receiver)),
            cancelled,
            worker: Some(worker),
        })
    }

    pub fn try_next(&self) -> Result<Option<MovieEvent>> {
        let receiver = self
            .receiver
            .as_ref()
            .context("movie decoder was closed")?
            .lock()
            .map_err(|_| anyhow::anyhow!("movie receiver poisoned"))?;
        match receiver.try_recv() {
            Ok(event) => event.map(Some).map_err(anyhow::Error::msg),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                anyhow::bail!("movie worker ended without a completion event")
            }
        }
    }

    /// Read a deterministic frame for capture without starting audio playback.
    pub fn frame(path: &Path, asset: MovieAsset, index: u32) -> Result<VideoFrame> {
        anyhow::ensure!(index < asset.frames, "movie capture frame is out of range");
        let decoder = Self::open(path, asset)?;
        loop {
            let event = decoder
                .receiver
                .as_ref()
                .context("movie receiver missing")?
                .lock()
                .map_err(|_| anyhow::anyhow!("movie receiver poisoned"))?
                .recv_timeout(Duration::from_secs(30))
                .context("movie frame decode timed out or stopped")?
                .map_err(anyhow::Error::msg)?;
            match event {
                MovieEvent::Video(frame) if frame.index == index => return Ok(frame),
                MovieEvent::End => anyhow::bail!("movie ended before capture frame {index}"),
                _ => {}
            }
        }
    }
}

impl Drop for MovieDecoder {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.receiver.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
