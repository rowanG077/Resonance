//! One Bevy session source combines music and a persistent cue studio.
use super::{MusicFrames, TitleMusic};
use anyhow::{Context, Result};
use bevy::prelude::*;
use resonance_audio::cue::{
    Cue, Studio,
    package::{Loaded, Manifest},
};
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
use std::{
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

pub(crate) struct PlaybackAssets {
    music: Option<TitleMusic>,
    cues: Option<Arc<Loaded>>,
}

impl PlaybackAssets {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("title-sounds.json");
        let cues = if path.is_file() {
            let metadata: resonance_content::TitleSounds = serde_json::from_slice(&fs::read(path)?)
                .context("invalid title sound manifest; run resonance-import cook-title-sounds")?;
            metadata.validate()?;
            let cues = Manifest::load(root, &metadata.path, &metadata.sha256)?;
            anyhow::ensure!(
                ["navigate", "confirm", "back", "error"]
                    .iter()
                    .all(|key| cues.cues.contains_key(*key)),
                "missing required menu cues; run resonance-import cook-title-sounds"
            );
            Some(Arc::new(cues))
        } else {
            None
        };
        Ok(Self {
            music: TitleMusic::load(root)?,
            cues,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.music.is_none() && self.cues.is_none()
    }
    pub fn has_music(&self) -> bool {
        self.music.is_some()
    }

    pub fn session(self, full_intro: bool) -> (GameAudio, SoundControl) {
        let (send, receive) = mpsc::sync_channel(64);
        let rendered = Arc::new(AtomicU64::new(0));
        let control = SoundControl {
            cues: self.cues.clone(),
            send,
            rendered: rendered.clone(),
        };
        let audio = GameAudio {
            music: self.music.map(|music| music.entry(full_intro)),
            cues: self.cues,
            requests: Arc::new(Mutex::new(Some(receive))),
            rendered,
            master_lead_ms: if full_intro { 1200 } else { 1185 },
        };
        (audio, control)
    }
}

#[derive(Clone)]
pub(crate) struct SoundControl {
    cues: Option<Arc<Loaded>>,
    send: SyncSender<Arc<Cue>>,
    rendered: Arc<AtomicU64>,
}

impl SoundControl {
    pub fn play(&self, name: &str) -> Result<()> {
        let bank = self.cues.as_ref().context("missing cooked cue package")?;
        let cue = bank
            .cues
            .get(name)
            .with_context(|| format!("unknown sound cue {name}"))?;
        self.send
            .try_send(cue.clone())
            .context("cue request queue is full or stopped")
    }
    pub fn rendered_frames(&self) -> u64 {
        self.rendered.load(Ordering::Relaxed)
    }
}

/// A session owns one consumer. Start a fresh session when replaying assets.
#[derive(Asset, TypePath, Clone)]
pub(crate) struct GameAudio {
    music: Option<TitleMusic>,
    cues: Option<Arc<Loaded>>,
    requests: Arc<Mutex<Option<Receiver<Arc<Cue>>>>>,
    rendered: Arc<AtomicU64>,
    master_lead_ms: u16,
}

pub(crate) struct GameFrames {
    music: Option<MusicFrames>,
    studio: Option<Studio>,
    cue_fade: Option<resonance_audio::volume::Fade>,
    requests: Option<Receiver<Arc<Cue>>>,
    rendered: Arc<AtomicU64>,
    frame: u64,
    channel: usize,
    samples: [f32; 2],
    active: bool,
}

impl GameFrames {
    fn start(audio: &GameAudio) -> Result<Self> {
        let requests = audio
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("cue consumer lock poisoned"))?
            .take()
            .context("audio session already has a consumer")?;
        // Music and effects share master fade age; effects have no sequence fade.
        let mut cue_fade = resonance_audio::volume::Fade::new(0.0, 1.0, 2000)?;
        for _ in 0..audio.master_lead_ms / 5 {
            cue_fade.advance_block();
        }
        Ok(Self {
            cue_fade: Some(cue_fade),
            music: audio.music.as_ref().map(MusicFrames::start).transpose()?,
            studio: audio
                .cues
                .as_ref()
                .map(|bank| Studio::new(bank.reverbs))
                .transpose()?,
            requests: Some(requests),
            rendered: audio.rendered.clone(),
            frame: 0,
            channel: 0,
            samples: [0.; 2],
            active: true,
        })
    }

    pub fn stop(&mut self) -> Result<()> {
        self.active = false;
        self.requests.take();
        if let Some(music) = &mut self.music {
            music.stop()?;
        }
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<[f32; 2]>> {
        if self.frame.is_multiple_of(160) {
            if let Some(fade) = &mut self.cue_fade {
                // Schedule cue controls at block start, then advance the group fade.
                if let Some(studio) = &mut self.studio {
                    studio.set_group_volume(fade.value())?;
                }
                fade.advance_block();
            }
            for cue in self.requests.iter().flat_map(|r| r.try_iter()) {
                self.studio
                    .as_mut()
                    .context("cue request without a studio")?
                    .play(cue)?;
            }
        }
        let mut output = [0.; 2];
        if let Some(music) = &mut self.music {
            for sample in &mut output {
                let Some(value) = music.next() else {
                    return Ok(None);
                };
                *sample = value;
            }
        }
        if let Some(studio) = &mut self.studio {
            for (output, value) in output.iter_mut().zip(studio.next_frame()) {
                *output += f32::from(value) / 32768.0;
            }
        }
        Ok(Some(output))
    }
}

impl Iterator for GameFrames {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if !self.active {
            return None;
        }
        if self.channel == 0 {
            self.samples = match self.next_frame() {
                Ok(Some(samples)) => samples,
                Ok(None) => {
                    self.active = false;
                    return None;
                }
                Err(error) => {
                    panic!("audio session failed: {error:#}");
                }
            };
        }
        let sample = self.samples[self.channel];
        self.channel = (self.channel + 1) % 2;
        if self.channel == 0 {
            self.frame += 1;
            self.rendered.store(self.frame, Ordering::Relaxed);
        }
        Some(sample)
    }
}

impl Source for GameFrames {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(32028).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for GameAudio {
    type Decoder = GameFrames;
    fn decoder(&self) -> Self::Decoder {
        GameFrames::start(self).expect("could not initialize audio session")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_requests_follow_source_blocks_and_stop_with_the_consumer() {
        let bank = Loaded {
            sample_rate: 32028,
            reverbs: [[0., 0., 1., 0., 0.]; 2],
            cues: std::collections::BTreeMap::from([(
                "navigate".into(),
                Arc::new(Cue::new(vec![[[100, -100], [0; 2], [0; 2]]; 2]).unwrap()),
            )]),
        };
        let (audio, control) = PlaybackAssets {
            music: None,
            cues: Some(Arc::new(bank)),
        }
        .session(false);
        let mut source = audio.decoder();
        control.play("navigate").unwrap();
        for expected in [100, -100, 100, -100, 0, 0] {
            assert_eq!((source.next().unwrap() * 32768.) as i16, expected);
        }
        assert_eq!(control.rendered_frames(), 3);
        control.play("navigate").unwrap();
        for _ in 3..160 {
            assert_eq!(source.next(), Some(0.));
            assert_eq!(source.next(), Some(0.));
        }
        assert_eq!(source.next(), Some(100. / 32768.));
        assert_eq!(source.next(), Some(-100. / 32768.));
        assert_eq!(control.rendered_frames(), 161);
        assert!(control.play("missing").is_err());
        source.stop().unwrap();
        assert!(source.next().is_none());
        assert!(control.play("navigate").is_err());
    }
}
