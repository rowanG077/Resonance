//! Audio delivery never waits for the presentation thread to drain video.
use crate::{MovieDecoder, MovieEvent, VideoFrame};
use anyhow::{Result, ensure};
use resonance_playback::Pcm;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Shared {
    audio: Arc<Pcm>,
    video: Mutex<VecDeque<VideoFrame>>,
    cancelled: AtomicBool,
    complete: AtomicBool,
    error: Mutex<Option<String>>,
}
pub struct MovieStream {
    shared: Arc<Shared>,
}
impl MovieStream {
    pub fn start(decoder: MovieDecoder, prepared: VecDeque<MovieEvent>, rate: u32) -> Result<Self> {
        let mut audio = VecDeque::new();
        let mut video = VecDeque::new();
        for event in prepared {
            match event {
                MovieEvent::Audio(chunk) => audio.push_back(chunk),
                MovieEvent::Video(frame) => video.push_back(frame),
                MovieEvent::End => anyhow::bail!("movie ended during preparation"),
            }
        }
        let shared = Arc::new(Shared {
            audio: Arc::new(Pcm::new(rate)),
            video: Mutex::new(VecDeque::with_capacity(crate::VIDEO_LOOKAHEAD)),
            cancelled: AtomicBool::new(false),
            complete: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        let state = shared.clone();
        thread::Builder::new()
            .name("resonance-movie-feed".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    while !state.cancelled.load(Ordering::Acquire) {
                        let mut progressed = false;
                        if state.audio.needs_data() && !state.audio.finished() {
                            let chunk = match audio.pop_front() {
                                Some(chunk) => Some(chunk),
                                None => decoder.try_audio()?,
                            };
                            if let Some(chunk) = chunk {
                                state.audio.push(chunk.start_frame, chunk.samples)?;
                                progressed = true;
                            } else if decoder.audio_complete() {
                                state.audio.finish();
                            }
                        }
                        // Backpressure only video. Never discard future frames or
                        // make the audio producer wait for video decoding/draining.
                        {
                            let mut frames =
                                state.video.lock().expect("movie frame queue poisoned");
                            if frames.len() < crate::VIDEO_LOOKAHEAD {
                                let frame = match video.pop_front() {
                                    Some(frame) => Some(frame),
                                    None => decoder.try_video()?,
                                };
                                if let Some(frame) = frame {
                                    frames.push_back(frame);
                                    progressed = true;
                                }
                            }
                        }
                        if state.audio.finished() && decoder.video_complete() {
                            state.complete.store(true, Ordering::Release);
                            return Ok(());
                        }
                        if !progressed {
                            thread::sleep(Duration::from_millis(1));
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    *state.error.lock().expect("movie fault queue poisoned") =
                        Some(format!("{error:#}"));
                }
                // The decoder's workers are joined here when it drops.
            })?;
        Ok(Self { shared })
    }
    pub fn audio(&self) -> Arc<Pcm> {
        self.shared.audio.clone()
    }
    pub fn try_video(&self) -> Option<VideoFrame> {
        self.shared
            .video
            .lock()
            .expect("movie frame queue poisoned")
            .pop_front()
    }
    pub fn complete(&self) -> bool {
        self.shared.complete.load(Ordering::Acquire)
    }
    /// Offline consumers can outrun decoding after loading or GPU stalls. Wait
    /// before pulling PCM; live mixer and device callbacks must never call this.
    pub fn wait_for_audio(&self, frames: u64, timeout: Duration) -> Result<()> {
        let started = Instant::now();
        loop {
            self.check()?;
            if self.shared.audio.finished() || self.shared.audio.buffered() >= frames {
                return Ok(());
            }
            ensure!(
                started.elapsed() < timeout,
                "movie audio preparation timed out: {frames} frames requested, {} buffered",
                self.shared.audio.buffered()
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
    pub fn check(&self) -> Result<()> {
        if let Some(error) = self
            .shared
            .error
            .lock()
            .expect("movie fault queue poisoned")
            .as_ref()
        {
            anyhow::bail!("movie feed failed: {error}");
        }
        Ok(())
    }
}
impl Drop for MovieStream {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
    }
}
