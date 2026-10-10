//! One Bevy session source combines music and a persistent cue studio.
use super::{MusicFrames, TitleMusic};
use anyhow::{Context, Result};
use bevy::prelude::*;
use resonance_audio::{
    BLOCK_FRAMES,
    cue::{
        Studio,
        package::{Loaded, Manifest},
    },
    package::Loaded as LoadedProgram,
};
use resonance_content::diagnostics::Diagnostics;
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

#[derive(Clone)]
pub(crate) struct PlaybackAssets {
    music: Option<TitleMusic>,
    cues: Option<Arc<Loaded>>,
    diagnostics: Diagnostics,
}

#[cfg(test)]
pub(super) fn test_assets(music: TitleMusic, cues: Loaded) -> PlaybackAssets {
    PlaybackAssets {
        music: Some(music),
        cues: Some(Arc::new(cues)),
        diagnostics: Diagnostics::new(true),
    }
}

impl PlaybackAssets {
    pub fn load(root: &Path, diagnostics: Diagnostics) -> Result<Self> {
        let music = diagnostics.attempt("title music preparation", TitleMusic::load(root))?;
        let mut rejected = Vec::new();
        let cues = diagnostics.attempt(
            "title cue preparation",
            (|| -> Result<_> {
                let path = root.join("title-sounds.json");
                let metadata: resonance_content::TitleAudio = serde_json::from_slice(
                    &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
                )?;
                metadata.validate()?;
                Manifest::load(root, &metadata.path, &metadata.sha256, |name, error| {
                    rejected.push((name.to_owned(), error));
                    Ok(())
                })
            })(),
        )?;
        if let Some(bank) = &cues {
            for name in ["navigate", "confirm", "back", "error"] {
                if !bank.cues.contains_key(name)
                    && !rejected.iter().any(|(rejected, _)| rejected == name)
                {
                    rejected.push((
                        name.to_owned(),
                        anyhow::anyhow!("missing required menu cue"),
                    ));
                }
            }
        }
        for (name, error) in rejected {
            diagnostics.report(&format!("title cue {name}"), error)?;
        }
        Ok(Self {
            music,
            cues: cues.filter(|bank| !bank.cues.is_empty()).map(Arc::new),
            diagnostics,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.music.is_none() && self.cues.is_none()
    }

    pub fn session(self) -> (GameAudio, SoundControl) {
        let (send, receive) = mpsc::sync_channel(64);
        let rendered = Arc::new(AtomicU64::new(0));
        let error = Arc::new(Mutex::new(None));
        let control = SoundControl {
            cues: self.cues.clone(),
            send,
            rendered: rendered.clone(),
            diagnostics: self.diagnostics.clone(),
            error: error.clone(),
        };
        let audio = GameAudio {
            music: self.music,
            cues: self.cues,
            requests: Arc::new(Mutex::new(Some(receive))),
            rendered,
            diagnostics: self.diagnostics,
            error,
        };
        (audio, control)
    }
}

#[derive(Clone)]
pub(crate) struct SoundControl {
    cues: Option<Arc<Loaded>>,
    send: SyncSender<Arc<LoadedProgram>>,
    rendered: Arc<AtomicU64>,
    diagnostics: Diagnostics,
    error: Arc<Mutex<Option<String>>>,
}

impl SoundControl {
    pub fn play(&self, name: &str) -> Result<()> {
        let result = (|| {
            let bank = self.cues.as_ref().context("missing cooked cue package")?;
            let cue = bank
                .cues
                .get(name)
                .with_context(|| format!("unknown sound cue {name}"))?;
            self.send
                .try_send(cue.clone())
                .context("cue request queue is full or stopped")
        })();
        if let Err(error) = result {
            self.diagnostics
                .report("title cue request", error)
                .inspect_err(|error| {
                    store_error(&self.error, error);
                })?;
        }
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        if let Some(error) = self
            .error
            .lock()
            .map_err(|_| anyhow::anyhow!("title audio error lock poisoned"))?
            .take()
        {
            anyhow::bail!("title audio failed: {error}");
        }
        Ok(())
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
    requests: Arc<Mutex<Option<Receiver<Arc<LoadedProgram>>>>>,
    rendered: Arc<AtomicU64>,
    diagnostics: Diagnostics,
    error: Arc<Mutex<Option<String>>>,
}

pub(crate) struct GameFrames {
    music: Option<MusicFrames>,
    studio: Option<Studio>,
    fade: resonance_audio::volume::Fade,
    requests: Option<Receiver<Arc<LoadedProgram>>>,
    rendered: Arc<AtomicU64>,
    frame: u64,
    channel: usize,
    samples: [f32; 2],
    active: bool,
    diagnostics: Diagnostics,
    error: Arc<Mutex<Option<String>>>,
}

fn store_error(slot: &Mutex<Option<String>>, error: &anyhow::Error) {
    if let Ok(mut slot) = slot.lock() {
        slot.get_or_insert_with(|| format!("{error:#}"));
    }
}

impl GameFrames {
    fn start(audio: &GameAudio) -> Self {
        let mut frames = Self {
            music: None,
            studio: None,
            fade: resonance_audio::volume::Fade::new(
                0.,
                1.,
                resonance_audio::volume::TITLE_STARTUP_MS,
            )
            .expect("valid native title fade"),
            requests: None,
            rendered: audio.rendered.clone(),
            frame: 0,
            channel: 0,
            samples: [0.; 2],
            active: false,
            diagnostics: audio.diagnostics.clone(),
            error: audio.error.clone(),
        };
        if let Err(error) = frames.initialize(audio) {
            store_error(&frames.error, &error);
        }
        frames
    }

    fn initialize(&mut self, audio: &GameAudio) -> Result<()> {
        let requests = audio
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("cue consumer lock poisoned"))
            .and_then(|mut requests| {
                requests
                    .take()
                    .context("audio session already has a consumer")
            });
        let Some(requests) = self.diagnostics.attempt("title audio session", requests)? else {
            return Ok(());
        };
        self.requests = Some(requests);
        if let Some(music) = &audio.music {
            self.music = self
                .diagnostics
                .attempt("title music initialization", MusicFrames::start(music))?;
        }
        if let Some(bank) = &audio.cues {
            self.studio = self
                .diagnostics
                .attempt("title cue initialization", Studio::new(bank.reverbs))?;
        }
        self.active = true;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.requests.take();
        self.music.take();
        self.studio.take();
    }

    fn next_frame(&mut self) -> Result<[f32; 2]> {
        if self.frame.is_multiple_of(BLOCK_FRAMES as u64) {
            for cue in self.requests.iter().flat_map(|r| r.try_iter()) {
                let result = self
                    .studio
                    .as_mut()
                    .context("cue request without a studio")
                    .and_then(|studio| studio.play(cue));
                if let Err(error) = result {
                    self.diagnostics.report("title cue playback", error)?;
                }
            }
        }
        let mut output = [0.; 2];
        if let Some(music) = &mut self.music {
            match music.next_frame() {
                Ok(frame) => output = frame,
                Err(error) => {
                    self.music = None;
                    self.diagnostics.report("title music playback", error)?;
                }
            }
        }
        if let Some(studio) = &mut self.studio {
            let frame =
                studio.next_frame(|error| self.diagnostics.report("title cue playback", error))?;
            for (output, value) in output.iter_mut().zip(frame) {
                *output += f32::from(value) / 32768.0;
            }
        }
        Ok(output.map(|sample| sample * self.fade.value_at(self.frame)))
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
                Ok(samples) => samples,
                Err(error) => {
                    store_error(&self.error, &error);
                    self.stop();
                    return None;
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
        SampleRate::new(resonance_audio::SOURCE_RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for GameAudio {
    type Decoder = GameFrames;
    fn decoder(&self) -> Self::Decoder {
        GameFrames::start(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assets(diagnostics: Diagnostics) -> PlaybackAssets {
        let root = disk_assets();
        let mut assets = PlaybackAssets::load(root.path(), diagnostics).unwrap();
        assets.music = None;
        assets
    }

    fn disk_assets() -> tempfile::TempDir {
        use resonance_audio::{
            data::{Command, Interpolation, ScoreOrigin},
            package::SampleAsset,
        };
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let mut wave = hound::WavWriter::create(
            root.path().join("sample.wav"),
            hound::WavSpec {
                channels: 1,
                sample_rate: resonance_audio::SOURCE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16 {
            wave.write_sample(1000i16).unwrap();
        }
        wave.finalize().unwrap();
        let (mut resources, score, tables) =
            crate::field_audio::test_score_data(ScoreOrigin::Sequence);
        resources.programs.insert(
            1,
            vec![
                Command::Interpolation {
                    mode: Interpolation::Direct,
                    coefficients: 0,
                },
                Command::StartSample { sample: 1 },
                Command::End,
            ],
        );
        let mut package = resonance_audio::package::Package {
            version: resonance_audio::package::VERSION,
            programs: resources.programs,
            samples: [(
                1,
                SampleAsset {
                    path: "sample.wav".into(),
                    sha256: format!(
                        "{:x}",
                        Sha256::digest(fs::read(root.path().join("sample.wav")).unwrap())
                    ),
                    key: 60,
                    rate: resonance_audio::SOURCE_RATE as u16,
                    first_frames: 16,
                    loop_start: 0,
                    loop_length: 0,
                },
            )]
            .into(),
            score,
            tables,
            reverbs: [[0., 0., 1., 0., 0.]; 2],
        };
        fs::write(
            root.path().join("music.json"),
            serde_json::to_vec(&package).unwrap(),
        )
        .unwrap();
        package.score = crate::field_audio::test_score_data(ScoreOrigin::SoundEffect).1;
        let bytes = serde_json::to_vec(&package).unwrap();
        for path in ["cue.json", "back.json"] {
            fs::write(root.path().join(path), &bytes).unwrap();
        }
        let manifest = Manifest {
            version: resonance_audio::cue::package::VERSION,
            sample_rate: resonance_audio::SOURCE_RATE,
            reverbs: package.reverbs,
            cues: ["navigate", "confirm", "back", "error"]
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        resonance_audio::cue::package::Asset {
                            program: resonance_audio::cue::package::Program {
                                path: if name == "back" {
                                    "back.json"
                                } else {
                                    "cue.json"
                                }
                                .into(),
                                sha256: format!("{:x}", Sha256::digest(&bytes)),
                            },
                        },
                    )
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        fs::write(root.path().join("cues.json"), &bytes).unwrap();
        fs::write(
            root.path().join("title-sounds.json"),
            serde_json::to_vec(&resonance_content::TitleAudio {
                version: 3,
                path: "cues.json".into(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.path().join("title-audio.json"),
            serde_json::to_vec(&resonance_content::TitleAudio {
                version: 3,
                path: "music.json".into(),
                sha256: format!(
                    "{:x}",
                    Sha256::digest(fs::read(root.path().join("music.json")).unwrap())
                ),
            })
            .unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn title_loading_reports_bad_descriptors_and_isolates_rejected_cues() {
        let root = disk_assets();
        for (path, scope) in [
            ("title-audio.json", "title music preparation"),
            ("music.json", "title music preparation"),
            ("title-sounds.json", "title cue preparation"),
            ("back.json", "title cue back"),
        ] {
            let original = fs::read(root.path().join(path)).unwrap();
            for missing in [false, true] {
                if missing {
                    fs::remove_file(root.path().join(path)).unwrap();
                } else if matches!(path, "title-audio.json" | "music.json") {
                    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
                    if path == "title-audio.json" {
                        value.as_object_mut().unwrap().remove("sha256");
                    } else {
                        value["score"]["initial_bpm_1024"] = serde_json::json!(240 * 1024);
                    }
                    fs::write(root.path().join(path), serde_json::to_vec(&value).unwrap()).unwrap();
                } else {
                    fs::write(root.path().join(path), b"not JSON").unwrap();
                }
                for paranoid in [false, true] {
                    let diagnostics = Diagnostics::new(paranoid);
                    let loaded = PlaybackAssets::load(root.path(), diagnostics.clone());
                    assert_eq!(loaded.is_err(), paranoid, "{path}, missing={missing}");
                    let entries = diagnostics.entries();
                    assert_eq!(entries.len(), 1);
                    assert_eq!(
                        (entries[0].scope.as_str(), entries[0].occurrences),
                        (scope, 1)
                    );
                    if let Ok(mut assets) = loaded {
                        assert_eq!(
                            assets.music.is_none(),
                            matches!(path, "title-audio.json" | "music.json")
                        );
                        assert_eq!(assets.cues.is_none(), path == "title-sounds.json");
                        if let Some(bank) = &assets.cues {
                            assert!(bank.cues.contains_key("navigate"));
                            assert_eq!(bank.cues.contains_key("back"), path != "back.json");
                            // Listen to the admitted cue without music masking a failure.
                            assets.music = None;
                        }
                        let (source, control) = assets.session();
                        if path != "title-sounds.json" {
                            control.play("navigate").unwrap();
                        }
                        assert!(source.decoder().take(320).any(|sample| sample != 0.));
                        control.check().unwrap();
                    }
                }
                fs::write(root.path().join(path), &original).unwrap();
                let diagnostics = Diagnostics::new(true);
                assert!(PlaybackAssets::load(root.path(), diagnostics.clone()).is_ok());
                assert!(
                    !diagnostics.has_errors(),
                    "repaired descriptor stayed rejected"
                );
            }
        }
    }

    fn navigation() -> Vec<f32> {
        let (audio, control) = assets(Diagnostics::new(true)).session();
        control.play("navigate").unwrap();
        let samples: Vec<_> = audio.decoder().take(640).collect();
        assert_eq!(samples.len(), 640);
        assert!(samples.iter().any(|sample| *sample > 0.));
        assert!(samples[600..].iter().all(|sample| *sample == 0.));
        samples
    }

    #[test]
    fn broken_music_or_cue_keeps_healthy_cues_unless_paranoid() {
        let expected = navigation();
        for paranoid in [false, true] {
            for broken_music in [false, true] {
                let diagnostics = Diagnostics::new(paranoid);
                let mut assets = assets(diagnostics.clone());
                let origin = if broken_music {
                    resonance_audio::data::ScoreOrigin::Sequence
                } else {
                    resonance_audio::data::ScoreOrigin::SoundEffect
                };
                let (mut resources, mut score, tables) =
                    crate::field_audio::test_score_data(origin);
                for event in &mut score.first_events {
                    if !broken_music
                        && let resonance_audio::data::EventKind::Notes { source, .. } =
                            &mut event.kind
                    {
                        *source = resonance_audio::data::VoiceSource::SoundEffect { id: 2 };
                    }
                }
                resources.programs.insert(
                    1,
                    vec![resonance_audio::data::Command::Jump {
                        program: 1,
                        instruction: 0,
                    }],
                );
                let score = Arc::new(
                    resonance_audio::package::Loaded::new(
                        resources,
                        score,
                        tables,
                        [[0., 0., 1., 0., 0.]; 2],
                    )
                    .unwrap(),
                );
                if broken_music {
                    assets.music = Some(TitleMusic { music: score });
                } else {
                    Arc::get_mut(assets.cues.as_mut().unwrap())
                        .unwrap()
                        .cues
                        .insert("broken".into(), score);
                }
                let (audio, control) = assets.session();
                let mut source = audio.decoder();
                if !broken_music {
                    control.play("broken").unwrap();
                }
                control.play("navigate").unwrap();
                if paranoid {
                    assert_eq!(source.next(), None);
                    assert!(
                        control
                            .check()
                            .unwrap_err()
                            .to_string()
                            .contains("instruction budget")
                    );
                    assert!(control.check().is_ok(), "failure was delivered twice");
                } else {
                    assert_eq!(
                        source.by_ref().take(expected.len()).collect::<Vec<_>>(),
                        expected
                    );
                    assert!(control.check().is_ok());
                }
                assert_eq!(diagnostics.entries().len(), 1);
                assert_eq!(diagnostics.entries()[0].occurrences, 1);
            }
        }
    }

    #[test]
    fn request_and_duplicate_consumer_failures_follow_the_session_policy() {
        let expected = navigation();
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let (audio, control) = assets(diagnostics.clone()).session();
            let mut source = audio.decoder();
            assert_eq!(control.play("missing").is_err(), paranoid);
            assert_eq!(control.check().is_err(), paranoid);
            control.play("navigate").unwrap();
            assert_eq!(
                source.by_ref().take(expected.len()).collect::<Vec<_>>(),
                expected
            );
            assert!(control.check().is_ok());
            assert!(diagnostics.has_errors());
            let mut duplicate = audio.decoder();
            assert_eq!(duplicate.next(), None);
            assert_eq!(control.check().is_err(), paranoid);
        }
    }

    #[test]
    fn cue_requests_follow_source_blocks_and_stop_with_the_consumer() {
        let expected = navigation();
        let (audio, control) = assets(Diagnostics::new(true)).session();
        let mut source = audio.decoder();
        control.play("navigate").unwrap();
        assert_eq!(source.by_ref().take(6).collect::<Vec<_>>(), expected[..6]);
        assert_eq!(control.rendered_frames(), 3);
        control.play("navigate").unwrap();
        assert_eq!(
            source.by_ref().take(314).collect::<Vec<_>>(),
            expected[6..320]
        );
        assert_eq!(control.rendered_frames(), 160);
        let second_block: Vec<_> = source.by_ref().take(320).collect();
        assert!(
            second_block
                .iter()
                .zip(&expected[320..])
                .any(|(actual, baseline)| actual > baseline),
            "queued cue did not start at the next source block"
        );
        assert_eq!(control.rendered_frames(), 320);
        assert!(control.play("missing").is_err());
        source.stop();
        assert!(source.next().is_none());
        assert!(control.play("navigate").is_err());
    }
}
