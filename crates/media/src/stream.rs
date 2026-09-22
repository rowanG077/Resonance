//! Audio delivery never waits for the presentation thread to drain video.
use crate::{MovieDecoder, MovieEvent, VideoFrame};
use anyhow::{Result, ensure};
use resonance_playback::Pcm;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Shared {
    audio: Arc<Pcm>,
    video: Mutex<VecDeque<VideoFrame>>,
    cancelled: AtomicBool,
    complete: AtomicBool,
    retired: AtomicBool,
    dropped: AtomicU64,
    error: Mutex<Option<String>>,
}
pub struct MovieStream {
    shared: Arc<Shared>,
}
impl MovieStream {
    pub fn start(
        decoder: MovieDecoder,
        mut prepared: VecDeque<MovieEvent>,
        rate: u32,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            audio: Arc::new(Pcm::new(rate)),
            video: Mutex::new(VecDeque::with_capacity(32)),
            cancelled: AtomicBool::new(false),
            complete: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            error: Mutex::new(None),
        });
        let state = shared.clone();
        thread::Builder::new()
            .name("resonance-movie-feed".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    while !state.cancelled.load(Ordering::Acquire) {
                        if !state.audio.needs_data() {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        let Some(event) = (if let Some(event) = prepared.pop_front() {
                            Some(event)
                        } else {
                            decoder.try_next()?
                        }) else {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        };
                        match event {
                            MovieEvent::Video(frame) => {
                                let mut frames =
                                    state.video.lock().expect("movie frame queue poisoned");
                                // The decoder is paced by audio. Old video can be
                                // discarded after a long render stall; PCM cannot.
                                if frames.len() == 32 {
                                    frames.pop_front();
                                    state.dropped.fetch_add(1, Ordering::Relaxed);
                                }
                                frames.push_back(frame);
                            }
                            MovieEvent::Audio(chunk) => {
                                state.audio.push(chunk.start_frame, chunk.samples)?
                            }
                            MovieEvent::End => {
                                state.audio.finish();
                                state.complete.store(true, Ordering::Release);
                                return Ok(());
                            }
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    *state.error.lock().expect("movie fault queue poisoned") =
                        Some(format!("{error:#}"));
                }
                // Closing and joining the decoder happens on this worker, never on a
                // skip, field transition, or device callback.
                drop(decoder);
                state.retired.store(true, Ordering::Release);
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
    pub fn dropped_frames(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
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
        ensure!(
            self.shared.audio.underruns() == 0,
            "movie decoded audio underrun"
        );
        Ok(())
    }
}
impl Drop for MovieStream {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn offline_audio_waits_for_decoding_and_preserves_eof_errors_and_timeout() -> Result<()> {
        let shared = Arc::new(Shared {
            audio: Arc::new(Pcm::new(32028)),
            video: Mutex::new(VecDeque::new()),
            cancelled: AtomicBool::new(false),
            complete: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            error: Mutex::new(None),
        });
        let stream = MovieStream {
            shared: shared.clone(),
        };
        let (send, receive) = mpsc::channel();
        let waiter = thread::spawn(move || {
            send.send(stream.wait_for_audio(2, Duration::from_secs(5)))
                .unwrap();
            stream
        });
        assert!(matches!(
            receive.recv_timeout(Duration::from_millis(10)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        shared.audio.push(0, vec![1., 2., 3., 4.])?;
        receive.recv_timeout(Duration::from_secs(5))??;
        let stream = waiter.join().unwrap();
        assert_eq!(
            shared.audio.source(false).take(4).collect::<Vec<_>>(),
            [1., 2., 3., 4.]
        );
        assert_eq!(shared.audio.underruns(), 0);
        assert!(
            stream
                .wait_for_audio(1, Duration::ZERO)
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        shared.audio.finish();
        stream.wait_for_audio(1, Duration::ZERO)?;
        *shared.error.lock().unwrap() = Some("decoder failed".into());
        assert!(
            stream
                .wait_for_audio(1, Duration::ZERO)
                .unwrap_err()
                .to_string()
                .contains("decoder failed")
        );
        Ok(())
    }
}
