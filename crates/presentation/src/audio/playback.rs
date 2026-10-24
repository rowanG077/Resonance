//! One Bevy session source combines music and a persistent cue studio.
use super::{MusicFrames, TitleMusic};
use anyhow::{Context, Result};
use bevy::prelude::*;
use resonance_audio::{
    BLOCK_FRAMES,
    cue::{Bank, Studio},
    effect::Prepared as LoadedProgram,
};
use resonance_content::diagnostics::Diagnostics;
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
#[cfg(test)]
use std::{fs, path::Path};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct PlaybackAssets {
    music: Option<TitleMusic>,
    cues: Option<Arc<Bank>>,
    diagnostics: Diagnostics,
}

#[cfg(test)]
pub(super) fn test_assets(music: TitleMusic, cues: Bank) -> PlaybackAssets {
    PlaybackAssets {
        music: Some(music),
        cues: Some(Arc::new(cues)),
        diagnostics: Diagnostics::new(true),
    }
}

impl PlaybackAssets {
    #[cfg(test)]
    pub(crate) fn cue_only(&self) -> Self {
        Self {
            music: None,
            ..self.clone()
        }
    }

    #[cfg(test)]
    pub fn load(root: &Path, diagnostics: Diagnostics) -> Result<Self> {
        Self::load_in(
            &resonance_content::library::Library::open(root, &[])?,
            diagnostics,
        )
    }

    pub fn load_in(
        library: &resonance_content::library::Library,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut snapshot = library.snapshot();
        let definition: resonance_content::startup::Definition = snapshot.json(
            resonance_content::startup::PATH,
            resonance_content::preparation::JSON_LIMIT,
        )?;
        definition.art(snapshot.descriptor())?;
        Self::capture(&mut snapshot, &definition, diagnostics, &|| false)
    }

    pub(crate) fn capture(
        snapshot: &mut resonance_content::library::Snapshot,
        definition: &resonance_content::startup::Definition,
        diagnostics: Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        use anyhow::ensure;
        use resonance_content::{preparation::JSON_LIMIT, startup};
        let mut samples = resonance_audio::package::SampleCache::default();
        let result = super::Recording::capture(snapshot, &definition.music, cancelled);
        ensure!(!cancelled(), "title audio preparation cancelled");
        let music = diagnostics
            .attempt("title music preparation", result)?
            .map(|music| TitleMusic {
                music,
                master_lead_ms: 1185,
            });
        // Read cue structure independently so a bad optional music recording does
        // not hide healthy menu sounds under the existing tolerant policy.
        let result = (|| -> Result<startup::Cues<serde_json::Value>> {
            let cues: startup::Cues<serde_json::Value> =
                snapshot.json(&definition.cues, JSON_LIMIT)?;
            ensure!(
                cues.version == startup::VERSION
                    && !cues.cues.is_empty()
                    && cues.cues.len() <= 1024,
                "invalid title cue bank"
            );
            for parameters in cues.reverbs {
                resonance_audio::reverb::validate_parameters(parameters)?;
            }
            Ok(cues)
        })();
        ensure!(!cancelled(), "title audio preparation cancelled");
        let bank = diagnostics.attempt("title cue preparation", result)?;
        let cues = if let Some(bank) = bank {
            let mut cues = std::collections::BTreeMap::new();
            let mut names = std::collections::BTreeSet::new();
            for (name, path) in bank.cues {
                names.insert(name.clone());
                let result = (|| -> Result<_> {
                    ensure!(
                        !name.is_empty() && name.len() <= 128,
                        "invalid title cue name"
                    );
                    let path: String = serde_json::from_value(path)?;
                    let loaded =
                        capture_program(snapshot, &path, bank.reverbs, &mut samples, cancelled)?;
                    ensure!(
                        loaded.reverbs() == bank.reverbs,
                        "cue uses a different effects studio"
                    );
                    Ok(loaded)
                })();
                ensure!(!cancelled(), "title audio preparation cancelled");
                if let Some(cue) = diagnostics.attempt(&format!("title cue {name}"), result)? {
                    cues.insert(name, cue);
                }
            }
            for name in ["navigate", "confirm", "back", "error"] {
                if !names.contains(name) {
                    diagnostics.report(
                        &format!("title cue {name}"),
                        anyhow::anyhow!("missing required menu cue"),
                    )?;
                }
            }
            (!cues.is_empty()).then(|| {
                Arc::new(Bank {
                    sample_rate: resonance_audio::SOURCE_RATE,
                    reverbs: bank.reverbs,
                    cues,
                })
            })
        } else {
            None
        };
        ensure!(!cancelled(), "title audio preparation cancelled");
        Ok(Self {
            music,
            cues,
            diagnostics,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.music.is_none() && self.cues.is_none()
    }

    pub fn session(self, full_intro: bool) -> (GameAudio, SoundControl) {
        let (send, receive) = mpsc::sync_channel(64);
        let rendered = Arc::new(AtomicU64::new(0));
        // Publish music/effects levels and stereo together for each synthesis block.
        let settings = Arc::new(AtomicU32::new(0x1_7f7f));
        let error = Arc::new(Mutex::new(None));
        let control = SoundControl {
            cues: self.cues.clone(),
            send,
            rendered: rendered.clone(),
            settings: settings.clone(),
            diagnostics: self.diagnostics.clone(),
            error: error.clone(),
        };
        let audio = GameAudio {
            music: self.music.map(|music| music.entry(full_intro)),
            master_lead_ms: if full_intro { 1200 } else { 1185 },
            cues: self.cues,
            requests: Arc::new(Mutex::new(Some(receive))),
            rendered,
            settings,
            diagnostics: self.diagnostics,
            error,
        };
        (audio, control)
    }
}

fn capture_program(
    snapshot: &mut resonance_content::library::Snapshot,
    path: &str,
    reverbs: [[f32; 5]; 2],
    samples: &mut resonance_audio::package::SampleCache,
    cancelled: &impl Fn() -> bool,
) -> Result<Arc<LoadedProgram>> {
    anyhow::ensure!(!cancelled(), "title audio preparation cancelled");
    let definition: resonance_audio::effect::Definition =
        snapshot.json(path, resonance_content::preparation::JSON_LIMIT)?;
    let environment = definition.reverbs().is_none().then_some(reverbs);
    let loaded = definition
        .prepare_in(
            environment,
            &mut |path, limit| {
                anyhow::ensure!(!cancelled(), "title audio preparation cancelled");
                snapshot.read(path, limit)
            },
            samples,
        )
        .with_context(|| format!("prepare title program {path}"))?;
    Ok(Arc::new(loaded))
}

#[derive(Clone)]
pub(crate) struct SoundControl {
    cues: Option<Arc<Bank>>,
    send: SyncSender<Arc<LoadedProgram>>,
    rendered: Arc<AtomicU64>,
    settings: Arc<AtomicU32>,
    diagnostics: Diagnostics,
    error: Arc<Mutex<Option<String>>>,
}

impl SoundControl {
    pub fn settings(&self, music: u8, effects: u8, stereo: bool) {
        self.settings.store(
            u32::from(music.min(127))
                | (u32::from(effects.min(127)) << 8)
                | (u32::from(stereo) << 16),
            Ordering::Relaxed,
        );
    }

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
    master_lead_ms: u16,
    music: Option<TitleMusic>,
    cues: Option<Arc<Bank>>,
    requests: Arc<Mutex<Option<Receiver<Arc<LoadedProgram>>>>>,
    rendered: Arc<AtomicU64>,
    settings: Arc<AtomicU32>,
    diagnostics: Diagnostics,
    error: Arc<Mutex<Option<String>>>,
}

pub(crate) struct GameFrames {
    music: Option<MusicFrames>,
    studio: Option<Studio>,
    cue_fade: resonance_audio::volume::ControlFade,
    requests: Option<Receiver<Arc<LoadedProgram>>>,
    rendered: Arc<AtomicU64>,
    settings: Arc<AtomicU32>,
    stereo: bool,
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
            cue_fade: {
                let mut fade = resonance_audio::volume::ControlFade::new(0., 1., 2000).unwrap();
                for _ in 0..audio.master_lead_ms / 5 {
                    fade.advance_block();
                }
                fade
            },
            requests: None,
            rendered: audio.rendered.clone(),
            settings: audio.settings.clone(),
            stereo: true,
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
            let settings = self.settings.load(Ordering::Relaxed);
            let music_level = (settings & 0xff) as f32 / 127.;
            let effects_level = ((settings >> 8) & 0xff) as f32 / 127.;
            self.stereo = settings & 0x1_0000 != 0;
            if let Some(music) = &mut self.music {
                music.level = music_level;
                music.mono = !self.stereo;
            }
            if let Some(studio) = &mut self.studio {
                studio.set_group_volume(self.cue_fade.value() * effects_level)?;
            }
            self.cue_fade.advance_block();
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
        if !self.stereo {
            output = [(output[0] + output[1]) * 0.5; 2];
        }
        Ok(output)
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
    #[path = "named_tests.rs"]
    mod named;
    fn assets(diagnostics: Diagnostics) -> PlaybackAssets {
        let root = disk_assets();
        let mut assets = PlaybackAssets::load(root.path(), diagnostics).unwrap();
        assets.music = None;
        assets
    }

    fn disk_assets() -> tempfile::TempDir {
        use resonance_audio::data::{Command, Interpolation, ScoreOrigin};
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
            crate::field_audio::test_score_data(ScoreOrigin::SoundEffect);
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
        let program = resonance_audio::package::editable::Program {
            version: resonance_audio::package::editable::VERSION,
            programs: resources.programs,
            samples: [(
                1,
                resonance_audio::package::editable::Sample {
                    path: "sample.wav".into(),
                    key: 60,
                    loop_start: 0,
                    loop_length: 0,
                },
            )]
            .into(),
            score,
            tables,
            reverbs: Some([[0., 0., 1., 0., 0.]; 2]),
        };
        for path in ["cue.json", "back.json"] {
            fs::write(
                root.path().join(path),
                serde_json::to_vec(&resonance_audio::effect::Definition::Synthesized(
                    program.clone(),
                ))
                .unwrap(),
            )
            .unwrap();
        }
        let cues: resonance_content::startup::Cues = resonance_content::startup::Cues {
            version: resonance_content::startup::VERSION,
            reverbs: program.reverbs.unwrap(),
            cues: ["navigate", "confirm", "back", "error"]
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        if name == "back" {
                            "back.json"
                        } else {
                            "cue.json"
                        }
                        .into(),
                    )
                })
                .collect(),
        };
        fs::write(
            root.path().join("cues.json"),
            serde_json::to_vec(&cues).unwrap(),
        )
        .unwrap();
        named::write_startup(root.path()).unwrap();
        root
    }

    fn navigation() -> Vec<f32> {
        navigation_after(0)
    }

    fn navigation_after(frames: usize) -> Vec<f32> {
        let (audio, control) = assets(Diagnostics::new(true)).session(false);
        let mut source = audio.decoder();
        for _ in 0..frames * 2 {
            assert_eq!(source.next(), Some(0.));
        }
        control.play("navigate").unwrap();
        let samples: Vec<_> = source.take(640).collect();
        assert_eq!(samples.len(), 640);
        assert!(samples.iter().any(|sample| *sample > 0.));
        assert!(samples[600..].iter().all(|sample| *sample == 0.));
        samples
    }

    #[test]
    fn menu_cues_accept_stereo_wave_replacements_and_finish() -> Result<()> {
        let root = disk_assets();
        let mut wave = hound::WavWriter::create(
            root.path().join("replacement.wav"),
            hound::WavSpec {
                channels: 2,
                sample_rate: 48000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for _ in 0..480 {
            wave.write_sample(12000i16)?;
            wave.write_sample(-6000i16)?;
        }
        wave.finalize()?;
        fs::write(
            root.path().join("cue.json"),
            br#"{
            "kind":"wave", "version":1, "audio":"replacement.wav"
        }"#,
        )?;
        let mut assets = PlaybackAssets::load(root.path(), Diagnostics::new(true))?;
        assets.music = None;
        let (audio, control) = assets.session(false);
        control.settings(127, 127, true);
        let mut frames = audio.decoder();
        for _ in 0..74 {
            assert_eq!(frames.next(), Some(0.));
        }
        control.play("navigate")?;
        let samples = frames.take(1600).collect::<Vec<_>>();
        assert_eq!(samples.len(), 1600);
        assert!(
            samples
                .chunks_exact(2)
                .any(|frame| frame[0] > 0. && frame[1] < 0.)
        );
        assert!(samples[1400..].iter().all(|sample| *sample == 0.));
        Ok(())
    }

    #[test]
    fn broken_music_or_cue_keeps_healthy_cues_unless_paranoid() {
        for paranoid in [false, true] {
            for broken_music in [false, true] {
                // Match the session clock so the healthy cue has the same
                // startup gain when the retained music page is exhausted.
                let expected = navigation_after(if broken_music { 4000 } else { 0 });
                let diagnostics = Diagnostics::new(paranoid);
                let mut assets = assets(diagnostics.clone());
                if broken_music {
                    let (music, file) =
                        super::super::Recording::test(&vec![[0; 2]; 8000], None).unwrap();
                    // The first PCM page is retained. Truncation fails when the
                    // player later requests the next page from its open file.
                    file.set_len(44).unwrap();
                    assets.music = Some(TitleMusic {
                        music,
                        master_lead_ms: 1185,
                    });
                } else {
                    use resonance_audio::data::{Command, EventKind, ScoreOrigin, VoiceSource};
                    let (mut resources, mut score, tables) =
                        crate::field_audio::test_score_data(ScoreOrigin::SoundEffect);
                    for event in &mut score.first_events {
                        if let EventKind::Notes { source, .. } = &mut event.kind {
                            *source = VoiceSource::SoundEffect { id: 2 };
                        }
                    }
                    resources.programs.insert(
                        1,
                        vec![Command::Jump {
                            program: 1,
                            instruction: 0,
                        }],
                    );
                    let cue = Arc::new(
                        resonance_audio::package::Loaded::new(
                            resources,
                            score,
                            tables,
                            [[0., 0., 1., 0., 0.]; 2],
                        )
                        .unwrap()
                        .into(),
                    );
                    Arc::get_mut(assets.cues.as_mut().unwrap())
                        .unwrap()
                        .cues
                        .insert("broken".into(), cue);
                }
                let (audio, control) = assets.session(false);
                let mut source = audio.decoder();
                if broken_music {
                    for _ in 0..4000 * 2 {
                        assert_eq!(source.next(), Some(0.));
                    }
                } else {
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
                            .contains(if broken_music {
                                "music PCM page"
                            } else {
                                "instruction budget"
                            })
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
            let (audio, control) = assets(diagnostics.clone()).session(false);
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
        let (audio, control) = assets(Diagnostics::new(true)).session(false);
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
