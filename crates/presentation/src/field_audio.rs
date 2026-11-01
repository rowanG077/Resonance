//! Field audio adapter: recorded music, live cue lifetimes and spoken lines.
//! The same source feeds Bevy and the device-free recorder.
use crate::audio_output::{PlaybackSettings, Player as AudioPlayer};
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
#[path = "battle_audio.rs"]
pub(crate) mod battle;
mod music;
#[cfg(test)]
mod named_tests;
use music::MusicPlayer;
#[path = "field_audio_record.rs"]
mod record;
#[path = "field_audio_validation.rs"]
pub(super) mod validation;
#[cfg(test)]
#[path = "field_voice_tests.rs"]
mod voice_tests;
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::prelude::*;
pub use record::record_field_audio;
#[cfg(test)]
use resonance_audio::package::Loaded;
use resonance_audio::{
    BLOCK_FRAMES, CONTROL_FRAMES, CONTROLS_PER_BLOCK,
    effect::{Definition as EffectDefinition, Prepared as PreparedEffect, Stream},
    reverb::Studio,
    sequence::{BusFrame, LiveControls, shared::Synthesizer},
    volume::Fade,
};
use resonance_content::{diagnostics::Diagnostics, field_audio::MusicReverbs};
use resonance_events::{AudioCommand, MusicCommand};
use resonance_game::clock::{UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR};
#[cfg(test)]
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs;
use std::{
    collections::{BTreeMap, VecDeque},
    io::Cursor,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

const RATE: u32 = resonance_audio::SOURCE_RATE;
pub(super) const PCM_SPEC: hound::WavSpec = hound::WavSpec {
    channels: 2,
    sample_rate: RATE,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
};
pub(super) struct Clip {
    bytes: Arc<[u8]>,
    data: std::ops::Range<usize>,
    rate: u32,
    channels: usize,
}
impl Clip {
    fn capture(
        voice: &resonance_content::field_audio::Voice,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
    ) -> Result<Arc<Self>> {
        voice.validate()?;
        let count = voice.frames as usize * usize::from(voice.channels);
        let bytes = read(&voice.path, count * 2 + 1024 * 1024)?;
        Ok(Arc::new(Self::prepare(bytes, voice)?))
    }

    fn silence(
        clock: resonance_content::field_audio::speech::Clock,
        rate: Option<u32>,
    ) -> Result<Self> {
        clock.validate()?;
        let rate = rate.unwrap_or(clock.sample_rate);
        ensure!(
            (8000..=96000).contains(&rate),
            "invalid silence playback rate"
        );
        Ok(Self {
            bytes: Arc::from([]),
            data: 0..clock.frames as usize * usize::from(clock.channels) * 2,
            rate,
            channels: usize::from(clock.channels),
        })
    }
    pub(super) fn prepare(
        bytes: Arc<[u8]>,
        voice: &resonance_content::field_audio::Voice,
    ) -> Result<Self> {
        let mut wave = hound::WavReader::new(Cursor::new(bytes.as_ref()))?;
        let spec = wave.spec();
        ensure!(
            spec.channels == voice.channels
                && spec.sample_rate == voice.source_sample_rate
                && spec.bits_per_sample == 16
                && spec.sample_format == hound::SampleFormat::Int
                && wave.duration() == voice.frames,
            "PCM clip format differs from metadata"
        );
        let samples = wave.len() as usize;
        // Reading one value also rejects a wider PCM container. The remaining
        // PCM16 samples are viewed directly in the verified resident WAV bytes.
        wave.samples::<i16>()
            .next()
            .context("empty spoken line")??;
        let start = wave.into_inner().position() as usize - 2;
        let end = start
            .checked_add(samples * 2)
            .context("voice size overflow")?;
        ensure!(
            samples == voice.frames as usize * usize::from(voice.channels) && end <= bytes.len(),
            "truncated PCM clip"
        );
        Ok(Self {
            bytes,
            data: start..end,
            rate: voice.sample_rate,
            channels: usize::from(voice.channels),
        })
    }

    fn sample_count(&self) -> usize {
        self.data.len() / 2
    }

    fn value(&self, sample: usize) -> i16 {
        if self.bytes.is_empty() {
            return 0;
        }
        let index = self.data.start + sample * 2;
        i16::from_le_bytes([self.bytes[index], self.bytes[index + 1]])
    }

    pub(super) fn sample(
        &self,
        position: u64,
        denominator: u32,
        pan: [f32; 2],
    ) -> Option<[f32; 2]> {
        let index = (position / u64::from(denominator)) as usize;
        let count = self.sample_count() / self.channels;
        if index >= count {
            return None;
        }
        let fraction = (position % u64::from(denominator)) as f32 / denominator as f32;
        Some(std::array::from_fn(|channel| {
            let source = channel.min(self.channels - 1);
            let a = f32::from(self.value(index * self.channels + source));
            let b = f32::from(self.value((index + 1).min(count - 1) * self.channels + source));
            (a + (b - a) * fraction) / 32768.0 * pan[channel]
        }))
    }
}
#[cfg(test)]
fn test_clip(samples: impl IntoIterator<Item = i16>, rate: u32, channels: usize) -> Clip {
    let bytes: Arc<[u8]> = samples.into_iter().flat_map(i16::to_le_bytes).collect();
    Clip {
        data: 0..bytes.len(),
        bytes,
        rate,
        channels,
    }
}
#[cfg(test)]
pub(crate) fn test_score() -> Arc<PreparedEffect> {
    test_score_kind(resonance_audio::data::ScoreOrigin::SoundEffect)
}
#[cfg(test)]
pub(crate) fn test_music() -> Arc<crate::audio::Recording> {
    crate::audio::Recording::test(&[[12000; 2]; 160], Some(0..160))
        .unwrap()
        .0
}
#[cfg(test)]
fn test_score_kind(origin: resonance_audio::data::ScoreOrigin) -> Arc<PreparedEffect> {
    let (resources, score, tables) = test_score_data(origin);
    Arc::new(
        Loaded::new(resources, score, tables, [[0.5, 0.5, 1., 0.5, 0.]; 2])
            .unwrap()
            .into(),
    )
}
#[cfg(test)]
pub(crate) fn test_score_data(
    origin: resonance_audio::data::ScoreOrigin,
) -> (
    resonance_audio::data::Resources,
    resonance_audio::data::Score,
    resonance_audio::music_voice::Tables,
) {
    use resonance_audio::{data::*, envelope, music_voice::Controls, sample::Sample};
    let volume: Vec<_> = (0..129)
        .map(|index| (index as f32 / 127.).min(1.))
        .collect();
    (
 Resources {
            programs: [(1, vec![
                Command::Interpolation { mode: Interpolation::Direct, coefficients: 0 },
                Command::VolumeControl { value: 16383 },
                Command::Envelope { envelope: Envelope::Ordinary(envelope::Parameters::default()) },
                Command::StartSample { sample: 1 },
                Command::Wait { milliseconds: None, from_start: false, key_off: false, sample_end: false },
            ])].into(),
            samples: [(1, Arc::new(Sample {
                key: 60, rate: 32000, pcm: vec![12000; 160],
                loop_start: 0, loop_length: 160, loop_pcm: vec![12000; 160],
            }))].into(),
        },
        Score {
            origin,
            initial_bpm_1024: 120 * 1024, loop_start_tick: 0, end_tick: u32::from(u16::MAX),
            tempos: vec![], controls: [Controls::default(); 16],
            first_events: vec![Event {
                tick: 0, channel: 0,
                kind: EventKind::Notes {
                    source: match origin {
                        ScoreOrigin::Sequence => VoiceSource::Sequence { group: 1, program: 1, drums: false },
                        ScoreOrigin::SoundEffect => VoiceSource::SoundEffect { id: 1 },
                    },
                    voices: vec![Note { macro_id: 1, key: 60, velocity: 127, pan: 64, priority: 9, max_voices: 1 }],
                    length: u16::MAX,
                },
            }],
            loop_events: vec![],
        },
        serde_json::from_value(serde_json::json!({
            "mix": {"volume": volume, "alternate_volume": volume,
                "pan": vec![1.; 4], "volume_16_scale": 1. / (127 << 16) as f32,
                "controller_14_scale": 1. / 16383., "pan_16_scale": 1. / (63 << 16) as f32},
            "dls": {"attenuation": vec![0; 194], "inverse": vec![0; 1024], "sustain": vec![0.; 128]},
            "modulation": {"sine": vec![0; 1024], "tremolo": vec![1.; 5]},
            "coefficients": vec![0; 2048],
        })).unwrap(),
    )
}
#[derive(Clone)]
pub(super) struct Assets {
    diagnostics: Diagnostics,
    recordings: BTreeMap<i16, Arc<crate::audio::Recording>>,
    sounds: BTreeMap<i16, Arc<PreparedEffect>>,
    voices: BTreeMap<u32, Arc<Clip>>,
    voice_gains: [f32; 128],
    reverbs: [[f32; 5]; 2],
    music_reverbs: Option<MusicReverbs>,
}

#[derive(Default)]
pub(super) struct Cache {
    samples: resonance_audio::package::SampleCache,
}

impl Cache {
    /// Prepare an editable bank from the same snapshot as its field. All nested
    /// program/sample paths are discovered here; no persisted inventory or
    /// declared digest is consulted.
    pub(crate) fn capture_named(
        &mut self,
        definition: &resonance_content::field_audio::document::Definition,
        snapshot: &mut resonance_content::library::Snapshot,
        diagnostics: Diagnostics,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<Assets>> {
        ensure!(!cancelled(), "audio preparation cancelled");
        let environment = definition.environment(snapshot)?;
        let resolved = definition.resolve(snapshot).context("audio choices")?;
        self.capture_resolved(&environment, resolved, snapshot, diagnostics, &cancelled)
    }

    pub(crate) fn capture_resolved(
        &mut self,
        definition: &resonance_content::field_audio::mix::Environment,
        resolved: resonance_content::field_audio::document::Resolved,
        snapshot: &mut resonance_content::library::Snapshot,
        diagnostics: Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Assets>> {
        ensure!(!cancelled(), "audio preparation cancelled");
        self.samples.prune();
        let mut assets = Assets {
            voice_gains: definition.voice_gains,
            reverbs: definition.reverbs,
            music_reverbs: Some(definition.music_reverbs.clone()),
            ..Assets::silent(diagnostics.clone())
        };
        for (id, path) in resolved.music {
            ensure!(!cancelled(), "audio preparation cancelled");
            let prepared = crate::audio::Recording::capture(snapshot, &path, cancelled)
                .with_context(|| format!("music resource {id}: {path}"));
            ensure!(!cancelled(), "audio preparation cancelled");
            if let Some(recording) = diagnostics.attempt("music resource", prepared)? {
                assets.recordings.insert(id, recording);
            }
        }
        let mut effects_by_path = BTreeMap::<String, Arc<PreparedEffect>>::new();
        for (id, path) in resolved.effects {
            if let Some(effect) = effects_by_path.get(&path) {
                assets.sounds.insert(id, effect.clone());
                continue;
            }
            ensure!(!cancelled(), "audio preparation cancelled");
            let prepared = (|| -> Result<_> {
                let program =
                    EffectDefinition::read(&path, &mut |path, limit| snapshot.read(path, limit))?;
                Ok(Arc::new(program.prepare_in(
                    Some(definition.reverbs),
                    &mut |path, limit| {
                        ensure!(!cancelled(), "audio preparation cancelled");
                        snapshot.read(path, limit)
                    },
                    &mut self.samples,
                )?))
            })()
            .with_context(|| format!("audio resource {id}: {path}"));
            ensure!(!cancelled(), "audio preparation cancelled");
            if let Some(package) = diagnostics.attempt("audio resource", prepared)? {
                effects_by_path.insert(path, package.clone());
                assets.sounds.insert(id, package);
            }
        }
        let mut voices_by_path = BTreeMap::<(String, Option<u32>), Arc<Clip>>::new();
        for (id, voice) in resolved.voices {
            use resonance_content::field_audio::speech::Source;
            let path = match &voice.source {
                Source::Wave(path) => path,
                Source::Silence { silence, .. } => {
                    assets
                        .voices
                        .insert(id, Arc::new(Clip::silence(*silence, voice.sample_rate)?));
                    continue;
                }
            };
            let key = (path.clone(), voice.sample_rate);
            if let Some(clip) = voices_by_path.get(&key) {
                assets.voices.insert(id, clip.clone());
                continue;
            }
            ensure!(!cancelled(), "audio preparation cancelled");
            let prepared = (|| -> Result<_> {
                let bytes = snapshot.read(path, 129 * 1024 * 1024)?;
                let metadata = resonance_content::field_audio::Voice::from_wave(
                    path,
                    voice.sample_rate,
                    &bytes,
                )?;
                Clip::capture(&metadata, &mut |reference, limit| {
                    snapshot.read(reference, limit)
                })
            })()
            .with_context(|| format!("speech {id}: {path}"));
            ensure!(!cancelled(), "audio preparation cancelled");
            if let Some(clip) = diagnostics.attempt("speech", prepared)? {
                voices_by_path.insert(key, clip.clone());
                assets.voices.insert(id, clip);
            }
        }
        Ok(Arc::new(assets))
    }
}

impl Assets {
    /// Skit clocks and voices have already been resolved from this snapshot.
    /// Decode the current bytes into the same bank used by ordinary dialogue.
    pub(crate) fn capture_skits(
        &mut self,
        skits: &resonance_content::skit::SkitCatalog,
        snapshot: &mut resonance_content::library::Snapshot,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        for (&id, voice) in skits
            .media
            .iter()
            .filter_map(|(id, media)| media.voice.as_ref().map(|voice| (id, voice)))
        {
            ensure!(!cancelled(), "skit audio preparation cancelled");
            let result = Clip::capture(voice, &mut |reference, limit| {
                snapshot.read(reference, limit)
            })
            .with_context(|| format!("skit speech {id}: {}", voice.path));
            ensure!(!cancelled(), "skit audio preparation cancelled");
            if let Some(clip) = self.diagnostics.attempt("skit speech", result)? {
                self.voices.insert(id, clip);
            }
        }
        ensure!(!cancelled(), "skit audio preparation cancelled");
        Ok(())
    }

    fn silent(diagnostics: Diagnostics) -> Self {
        Self {
            diagnostics,
            recordings: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [0.; 128],
            reverbs: [[0., 0., 0.01, 0., 0.]; 2],
            music_reverbs: None,
        }
    }
    fn contains_music(&self, id: &i16) -> bool {
        self.recordings.contains_key(id)
    }
    pub fn voice_durations(&self) -> Arc<BTreeMap<u32, u32>> {
        Arc::new(
            self.voices
                .iter()
                .map(|(&id, clip)| {
                    let seconds =
                        clip.sample_count() as f64 / clip.channels as f64 / f64::from(clip.rate);
                    (
                        id,
                        (seconds * resonance_game::clock::UPDATE_HZ).ceil() as u32,
                    )
                })
                .collect(),
        )
    }
    pub fn load(root: &Path, map: u32) -> Result<Self> {
        let mut snapshot = resonance_content::library::Library::open(root, &[])?.snapshot();
        let path = snapshot.field(map)?;
        let field: resonance_content::field::document::Definition =
            snapshot.json(&path, 1024 * 1024)?;
        field.validate()?;
        let assets = Cache::default().capture_named(
            &field.audio,
            &mut snapshot,
            Diagnostics::new(true),
            || false,
        )?;
        Ok((*assets).clone())
    }
    fn session(self: Arc<Self>) -> (FieldSource, Control) {
        let (send, receive) = mpsc::sync_channel(512);
        let error = Arc::new(Mutex::new(None));
        let rendered = Arc::new(AtomicU64::new(0));
        let completions = Arc::new(Mutex::new(VecDeque::new()));
        let control = Control {
            assets: self.clone(),
            diagnostics: self.diagnostics.clone(),
            completions: completions.clone(),
            send,
            error: error.clone(),
            rendered: rendered.clone(),
            movie_active: false,
            stereo: true,
            levels: [127; 3],
            in_field: true,
            skipping: Default::default(),
        };
        (
            FieldSource {
                completions,
                assets: self,
                receive: Arc::new(Mutex::new(Some(receive))),
                error,
                rendered,
            },
            control,
        )
    }
}

#[derive(Resource, Clone)]
pub(super) struct Control {
    assets: Arc<Assets>,
    diagnostics: Diagnostics,
    completions: Completions,
    send: SyncSender<Message>,
    error: Arc<Mutex<Option<String>>>,
    rendered: Arc<AtomicU64>,
    movie_active: bool,
    stereo: bool,
    levels: [u8; 3],
    in_field: bool,
    skipping: Arc<AtomicBool>,
}
type Completions = Arc<Mutex<VecDeque<(u64, Arc<AtomicBool>)>>>;
fn voice_completion(command: &AudioCommand) -> Option<Arc<AtomicBool>> {
    match command {
        AudioCommand::Voice { completion, .. } => completion.clone(),
        _ => None,
    }
}

enum Message {
    Battle(u64, battle::Command),
    LeaveField,
    EnterField(Arc<Assets>),
    Script(AudioCommand),
    Movie(bool),
    Stereo(bool),
    Levels([u8; 3]),
}

/// Optional development evidence; the normal player retains no event log.
#[derive(Resource, Default)]
pub(super) struct Trace(pub Vec<serde_json::Value>);
impl Control {
    pub(super) fn set_skipping(&self, skipping: bool) -> Result<()> {
        if self.skipping.swap(skipping, Ordering::AcqRel) != skipping && skipping {
            for (_, token) in self.completions.lock().unwrap().drain(..) {
                token.store(true, Ordering::Release);
            }
            self.enqueue(Message::Script(AudioCommand::StopVoice))?;
        }
        Ok(())
    }
    fn enqueue(&self, message: Message) -> Result<()> {
        // Serialize admission with decoder retirement so every accepted voice
        // is either consumed or completed while its receiver is closed.
        let _running = self.running()?;
        self.send
            .try_send(message)
            .context("field audio queue is full or stopped")
    }
    fn leave_field(&mut self) -> Result<()> {
        if self.in_field {
            self.enqueue(Message::LeaveField)?;
            self.in_field = false;
        }
        Ok(())
    }
    fn enter_field(&mut self, assets: impl Into<Arc<Assets>>) -> Result<()> {
        let assets = assets.into();
        self.enqueue(Message::EnterField(assets.clone()))?;
        self.assets = assets;
        self.in_field = true;
        Ok(())
    }
    fn stereo(&mut self, stereo: bool) -> Result<()> {
        if self.stereo != stereo {
            self.enqueue(Message::Stereo(stereo))?;
            self.stereo = stereo;
        }
        Ok(())
    }
    fn levels(&mut self, levels: [u8; 3]) -> Result<()> {
        ensure!(
            levels.iter().all(|&v| v <= 127),
            "invalid field volume settings"
        );
        if self.levels != levels {
            self.enqueue(Message::Levels(levels))?;
            self.levels = levels;
        }
        Ok(())
    }
    fn send(&self, command: AudioCommand) -> Result<()> {
        let completion = voice_completion(&command);
        let result = self.enqueue(Message::Script(command));
        if let Err(error) = result {
            if let Some(token) = completion {
                token.store(true, Ordering::Release);
            }
            self.diagnostics.report("audio request", error)?;
        }
        Ok(())
    }
    fn movie(&mut self, active: bool) -> Result<()> {
        if active != self.movie_active {
            // Music keeps advancing while muted; movie completion restores its gain over two seconds.
            self.enqueue(Message::Movie(active))?;
            self.movie_active = active;
        }
        Ok(())
    }
    fn running(&self) -> Result<std::sync::MutexGuard<'_, Option<String>>> {
        let error = self
            .error
            .lock()
            .map_err(|_| anyhow::anyhow!("audio error lock poisoned"))?;
        ensure!(
            error.is_none(),
            "field audio failed: {}",
            error.as_deref().unwrap_or_default()
        );
        Ok(error)
    }
    pub fn check(&self) -> Result<()> {
        self.running().map(|_| ())
    }
    /// The device-free sink uses the same consumed-frame boundary as live output.
    pub(crate) fn acknowledge_frames(&self, position: u64) {
        self.completions
            .lock()
            .expect("voice completion queue poisoned")
            .retain(|(end, token)| {
                if *end <= position {
                    token.store(true, Ordering::Release);
                    false
                } else {
                    true
                }
            });
    }
    pub fn rendered_frames(&self) -> u64 {
        self.rendered.load(Ordering::Relaxed)
    }
}
#[derive(Asset, TypePath, Clone)]
pub(super) struct FieldSource {
    completions: Completions,
    assets: Arc<Assets>,
    receive: Arc<Mutex<Option<Receiver<Message>>>>,
    error: Arc<Mutex<Option<String>>>,
    rendered: Arc<AtomicU64>,
}

struct ScorePlayer {
    stream: Stream,
    cue: Option<i16>,
    controls: LiveControls,
    slot: Option<u16>,
    volume: f32,
    repeat: Option<Arc<PreparedEffect>>,
}
impl ScorePlayer {
    fn new(package: Arc<PreparedEffect>, synth: &Synthesizer) -> Result<Self> {
        Ok(Self {
            stream: Stream::in_synthesizer(package, synth)?,
            cue: None,
            controls: LiveControls::default(),
            slot: None,
            volume: 1.,
            repeat: None,
        })
    }
    fn prepare_shared(&self, gains: impl FnOnce() -> [f32; CONTROLS_PER_BLOCK]) -> Result<()> {
        self.stream.set_shared_controls(gains().map(|gain| {
            let mut controls = self.controls;
            if self.cue.is_some() {
                // Field sound levels govern the whole cue, including pre-fader
                // reverb. They also remain live for slotted and looping sounds.
                controls.source_volume = self.volume * gain;
            } else {
                controls.volume = self.volume * gain;
            }
            controls
        }))
    }
    fn frame(&mut self) -> Option<BusFrame> {
        let frame = self.stream.shared_frame();
        if self.stream.shared_control_boundary() {
            self.controls.release = false;
        }
        frame
    }
}
struct Music {
    player: MusicPlayer,
    id: i16,
    fade: Fade,
}

struct Spoken {
    clip: Arc<Clip>,
    frame: u64,
    completion: Option<Arc<AtomicBool>>,
}
impl Spoken {
    fn next(&mut self) -> Option<[f32; 2]> {
        let position = self.frame * u64::from(self.clip.rate);
        self.frame += 1;
        // A centered mono stream uses equal-power stereo gain. Independent
        // Dolphin voice prefixes measure approximately 0.706 per channel.
        let gain = if self.clip.channels == 1 {
            std::f32::consts::FRAC_1_SQRT_2
        } else {
            1.
        };
        self.clip.sample(position, RATE, [gain; 2])
    }
}
pub(super) struct Frames {
    synth: Synthesizer,
    battle: Option<battle::State>,
    completions: Completions,
    assets: Arc<Assets>,
    receive: Option<Receiver<Message>>,
    error: Arc<Mutex<Option<String>>>,
    rendered: Arc<AtomicU64>,
    music: Option<Music>,
    /// Room score retained while the native inn jingle (97) plays.
    background_music: Option<i16>,
    sounds: Vec<ScorePlayer>,
    studio: Studio,
    studio_initialized: bool,
    stream_block: [BusFrame; BLOCK_FRAMES],
    music_block: [[f32; 2]; BLOCK_FRAMES],
    voice: Option<Spoken>,
    master: Fade,
    movie_active: bool,
    frame: u64,
    channel: usize,
    output: [f32; 2],
    stereo: bool,
    levels: [u8; 3],
}
impl Drop for Frames {
    fn drop(&mut self) {
        if self.receive.is_some() {
            self.retire("audio decoder stopped".into());
        }
    }
}
impl Frames {
    fn retire(&mut self, reason: String) {
        let mut error = self.error.lock().unwrap_or_else(|error| error.into_inner());
        error.get_or_insert(reason);
        if let Some(receive) = self.receive.take() {
            for message in receive.try_iter() {
                if let Message::Script(command) = message
                    && let Some(token) = voice_completion(&command)
                {
                    token.store(true, Ordering::Release);
                }
            }
        }
        if let Some(voice) = self.voice.take()
            && let Some(token) = voice.completion
        {
            token.store(true, Ordering::Release);
        }
        for (_, token) in self
            .completions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
        {
            token.store(true, Ordering::Release);
        }
        self.music = None;
        self.sounds.clear();
        self.battle = None;
    }
    fn stop_voice(&mut self) -> Result<()> {
        if let Some(voice) = self.voice.take()
            && let Some(completion) = voice.completion
        {
            self.completions
                .lock()
                .map_err(|_| anyhow::anyhow!("voice completion queue poisoned"))?
                .push_back((self.frame, completion));
        }
        Ok(())
    }
    fn initialize_studio(&mut self, assets: &Assets) -> Result<()> {
        if !self.studio_initialized && assets.music_reverbs.is_some() {
            // Fault-tolerant silent startup has no selected effect identity.
            // The first valid preparation supplies both startup effects once.
            let studio = Studio::new(assets.reverbs)?;
            self.studio = studio;
            self.studio_initialized = true;
        }
        Ok(())
    }

    fn command(&mut self, command: AudioCommand) -> Result<()> {
        if let AudioCommand::RepeatSound {
            id,
            pan,
            volume,
            slot,
        } = command
        {
            self.command(AudioCommand::Sound {
                id,
                pan,
                volume,
                slot: Some(slot),
            })?;
            self.sounds.last_mut().unwrap().repeat = Some(self.assets.sounds[&id].clone());
            return Ok(());
        }
        match command {
            AudioCommand::SoundReverb(preset) => self.studio.set_sound_preset(preset)?,
            AudioCommand::Music(command) => {
                let selected = match command {
                    MusicCommand::Play(id) | MusicCommand::PlayJingle(id) => {
                        Some(i16::try_from(id).context("music ID exceeds native range")?)
                    }
                    MusicCommand::Resume => self.background_music,
                    MusicCommand::Stop | MusicCommand::Suspend => None,
                };
                if let Some(id) = selected {
                    ensure!(self.assets.contains_music(&id), "uncooked field music {id}");
                }
                match command {
                    MusicCommand::Play(_) | MusicCommand::Stop => self.background_music = selected,
                    MusicCommand::PlayJingle(_) | MusicCommand::Suspend | MusicCommand::Resume => {}
                }
                if self.music.as_ref().map(|music| music.id) == selected {
                    return Ok(());
                }
                let reverb = if let Some(id) = selected {
                    self.assets
                        .diagnostics
                        .attempt(
                            "music reverb selection",
                            self.assets
                                .music_reverbs
                                .as_ref()
                                .context("missing prepared music reverb selectors")
                                .and_then(|catalog| catalog.for_song(id)),
                        )?
                        .flatten()
                } else {
                    None
                };
                let music = selected
                    .map(|id| -> Result<_> {
                        let recording = &self.assets.recordings[&id];
                        let player = MusicPlayer::recording(recording)?;
                        Ok(Music {
                            player,
                            id,
                            fade: Fade::new(0.0, 1.0, 100)?,
                        })
                    })
                    .transpose()?;
                if let Some(parameters) = reverb {
                    self.studio.set_music_reverb(parameters)?;
                }
                self.music = music;
            }
            AudioCommand::MusicVolume {
                volume,
                duration_ticks,
            } => {
                ensure!(volume < 128, "invalid music volume");
                let duration =
                    (u64::from(duration_ticks) * u64::from(RATE) * UPDATE_RATE_DENOMINATOR)
                        .div_ceil(UPDATE_RATE_NUMERATOR);
                if let Some(music) = &mut self.music {
                    music.fade =
                        Fade::from_frames(music.fade.value(), f32::from(volume) / 127., duration)?;
                }
            }
            AudioCommand::Sound {
                id,
                pan,
                volume,
                slot,
            } => {
                ensure!(pan < 128 && volume < 128, "invalid cue controls");
                let mut sound = ScorePlayer::new(
                    self.assets
                        .sounds
                        .get(&id)
                        .with_context(|| format!("uncooked field sound {id}"))?
                        .clone(),
                    &self.synth,
                )?;
                sound.volume = f32::from(volume) / 127.0;
                sound.cue = Some(id);
                sound.controls = LiveControls {
                    pan: Some(pan),
                    priority: (id
                        == resonance_content::field_audio::ServiceCue::BattleEntry as i16)
                        .then_some(u16::from(u8::MAX - 1)),
                    ..Default::default()
                };
                sound.slot = slot.map(u16::from);
                if let Some(slot) = sound.slot {
                    self.stop_sound(slot);
                }
                self.sounds.push(sound);
            }
            AudioCommand::StopSound(slot) => self.stop_sound(slot),
            AudioCommand::RepeatSound { .. } => unreachable!(),
            AudioCommand::SoundVolume { slot, volume } => {
                ensure!(volume < 128, "invalid sound volume");
                for sound in self.sounds.iter_mut().filter(|s| s.slot == Some(slot)) {
                    sound.volume = f32::from(volume) / 127.0;
                }
            }
            AudioCommand::SoundPan { slot, pan } => {
                ensure!(pan < 128, "invalid sound pan");
                for sound in self.sounds.iter_mut().filter(|s| s.slot == Some(slot)) {
                    sound.controls.pan = Some(pan);
                }
            }
            AudioCommand::Voice {
                resource,
                completion,
            } => {
                let clip = self
                    .assets
                    .voices
                    .get(&resource)
                    .with_context(|| format!("uncooked spoken line {resource:#x}"))?
                    .clone();
                self.stop_voice()?;
                self.voice = Some(Spoken {
                    clip,
                    frame: 0,
                    completion,
                });
            }
            AudioCommand::StopVoice => self.stop_voice()?,
            AudioCommand::SelectBank(bank) => {
                // The native selector addresses eight event banks. The cooker
                // resolves each cue's unique owner across that complete table;
                // all sounds needed by this field are already resident, so a
                // bank switch needs no additional load or playback change.
                ensure!(bank < 8, "invalid event sound bank {bank}")
            }
        }
        Ok(())
    }
    fn stop_sound(&mut self, slot: u16) {
        self.sounds.retain(|sound| sound.slot != Some(slot));
    }
    fn frame(&mut self) -> Result<Option<[f32; 2]>> {
        if self.receive.is_none() {
            return Ok(None);
        }
        loop {
            match self.receive.as_ref().unwrap().try_recv() {
                Ok(Message::Battle(command_id, command)) => {
                    self.battle_command(command_id, command)?
                }
                Ok(Message::LeaveField) => {
                    self.sounds.clear();
                    self.stop_voice()?;
                }
                Ok(Message::EnterField(assets)) => {
                    self.initialize_studio(&assets)?;
                    self.assets = assets;
                    // Music continues across fields, but scripted fades belong
                    // to the scene that requested them.
                    if let Some(music) = &mut self.music {
                        music.fade = Fade::new(1., 1., 0)?;
                    }
                }
                Ok(Message::Script(command)) => {
                    let completion = voice_completion(&command);
                    if let Err(error) = self.command(command) {
                        if let Some(token) = completion {
                            token.store(true, Ordering::Release);
                        }
                        self.assets.diagnostics.report("audio command", error)?;
                    }
                }
                Ok(Message::Stereo(stereo)) => self.stereo = stereo,
                Ok(Message::Levels(levels)) => self.levels = levels,
                Ok(Message::Movie(active)) => {
                    self.movie_active = active;
                    self.master = Fade::new(
                        self.master.value(),
                        if active { 0. } else { 1. },
                        if active { 0 } else { 2000 },
                    )?;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.retire("audio controls disconnected".into());
                    return Ok(None);
                }
            }
        }
        if self.frame.is_multiple_of(BLOCK_FRAMES as u64) {
            self.stream_block.fill([[0; 2]; 3]);
            self.music_block.fill([0.; 2]);
            if let Some(battle) = &mut self.battle {
                battle.prepare_stream_block(&mut self.stream_block, self.frame, &self.synth);
            }
        }
        let mut buses = [[0; 2]; 3];
        let master = self.master;
        let mut levels = self.levels;
        if let Some(battle) = &self.battle {
            levels[1..].copy_from_slice(&battle.event_audio_levels());
        }
        let effects_level = f32::from(levels[1]) / 127.;
        let voice_level = self.assets.voice_gains[usize::from(levels[2])];
        let diagnostics = self.assets.diagnostics.clone();
        if let Some(music) = &mut self.music
            && diagnostics
                .attempt(
                    "music playback",
                    music
                        .player
                        .prepare(
                            block_gains(master, Some(music.fade))
                                .map(|v| v * music.player.gain(levels[0])),
                            self.frame,
                            &mut self.music_block,
                        )
                        .with_context(|| format!("music {}", music.id)),
                )?
                .is_none()
        {
            self.music = None;
        }
        let mut index = 0;
        while index < self.sounds.len() {
            let sound = &mut self.sounds[index];
            sound.controls.mono = !self.stereo;
            if diagnostics
                .attempt(
                    "sound playback",
                    sound.prepare_shared(|| block_gains(master, None).map(|v| v * effects_level)),
                )?
                .is_some()
            {
                index += 1;
            } else {
                self.sounds.remove(index);
            }
        }
        if let Some(battle) = &mut self.battle {
            battle.prepare_shared()?;
        }
        // The synthesizer has already retired only failed entries and prepared
        // the healthy players' current frame before reporting its errors.
        diagnostics.attempt("audio synthesizer", self.synth.advance())?;
        if self
            .music
            .as_ref()
            .is_some_and(|music| music.player.finished(self.frame))
        {
            self.music = None;
        }
        if let Some(music) = &mut self.music {
            music.fade.advance(1);
            if self.battle.as_ref().is_some_and(|state| state.stop_music())
                && music.fade.value() == 0.
            {
                self.music = None;
            }
        }
        self.master.advance(1);
        let mut index = 0;
        while index < self.sounds.len() {
            let sound = &mut self.sounds[index];
            sound.controls.mono = !self.stereo;
            if let Some(frame) = sound.frame() {
                for (bus, source) in buses.iter_mut().zip(frame) {
                    for (target, sample) in bus.iter_mut().zip(source) {
                        *target += sample;
                    }
                }
            } else {
                if let Some(package) = &sound.repeat {
                    let mut restarted = ScorePlayer::new(package.clone(), &self.synth)?;
                    restarted.repeat = Some(package.clone());
                    restarted.cue = sound.cue;
                    restarted.slot = sound.slot;
                    restarted.volume = sound.volume;
                    restarted.controls = sound.controls;
                    *sound = restarted;
                    index += 1;
                    continue;
                }
                self.sounds.remove(index);
                continue;
            }
            index += 1;
        }
        if let Some(battle) = &mut self.battle {
            battle.frame(&mut buses, self.frame, &self.completions)?;
        }
        // Read after every player so submitted PCM and release tails reach
        // the shared studio exactly once, including after a player stops.
        for (bus, unread) in buses.iter_mut().zip(self.synth.unread_frame()) {
            for (sample, unread) in bus.iter_mut().zip(unread) {
                *sample += unread;
            }
        }
        // Submitted stream PCM belongs to this mixer block even if its source
        // was stopped, paused or removed after the block was prepared.
        for (bus, stream) in buses
            .iter_mut()
            .zip(self.stream_block[self.frame as usize % BLOCK_FRAMES])
        {
            for (sample, stream) in bus.iter_mut().zip(stream) {
                *sample += stream;
            }
        }
        // Scores and battle streams share one studio; auxiliary tails outlive
        // their source. Keep wide PCM until the final mix and clip only once.
        let mut output = self
            .studio
            .process(buses)
            .map(|value| value as f32 / 32768.);
        // Recorded music already contains its effects. A prepared block remains
        // audible after a mid-block handoff, like the synthesizer's queued PCM.
        for (target, music) in output
            .iter_mut()
            .zip(self.music_block[self.frame as usize % BLOCK_FRAMES])
        {
            *target += music;
        }
        if let Some(voice) = &mut self.voice {
            if let Some(frame) = voice.next() {
                for (target, value) in output.iter_mut().zip(frame) {
                    *target += value * voice_level;
                }
            } else {
                self.stop_voice()?;
            }
        }
        self.frame += 1;
        self.rendered.store(self.frame, Ordering::Relaxed);
        if !self.stereo {
            output = [(output[0] + output[1]) * 0.5; 2];
        }
        Ok(Some(output.map(|value| {
            if self.movie_active {
                0.
            } else {
                value.clamp(-1.0, 1.0)
            }
        })))
    }
}

/// Preview the control updates in the next transport block.
fn block_gains(master: Fade, music: Option<Fade>) -> [f32; CONTROLS_PER_BLOCK] {
    std::array::from_fn(|quantum| {
        let frames = quantum as u64 * CONTROL_FRAMES as u64;
        master.value_at(frames) * music.as_ref().map_or(1., |fade| fade.value_at(frames))
    })
}
impl Iterator for Frames {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 {
            match self.frame() {
                Ok(Some(frame)) => self.output = frame,
                Ok(None) => return None,
                Err(error) => {
                    if let Err(error) = self.assets.diagnostics.report("audio mixer", error) {
                        self.retire(format!("{error:#}"));
                        return None;
                    }
                    self.output = [0.; 2];
                    self.frame += 1;
                    self.rendered.store(self.frame, Ordering::Relaxed);
                }
            }
        }
        let sample = self.output[self.channel];
        self.channel ^= 1;
        Some(sample)
    }
}
impl Source for Frames {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
impl Decodable for FieldSource {
    type Decoder = Frames;
    fn decoder(&self) -> Frames {
        Frames {
            synth: Synthesizer::default(),
            battle: None,
            completions: self.completions.clone(),
            assets: self.assets.clone(),
            receive: self
                .receive
                .lock()
                .expect("field audio consumer lock")
                .take(),
            error: self.error.clone(),
            rendered: self.rendered.clone(),
            music: None,
            background_music: None,
            sounds: Vec::new(),
            studio: Studio::new(self.assets.reverbs).expect("validated field studio effects"),
            studio_initialized: self.assets.music_reverbs.is_some(),
            stream_block: [[[0; 2]; 3]; BLOCK_FRAMES],
            music_block: [[0.; 2]; BLOCK_FRAMES],
            voice: None,
            master: Fade::new(1.0, 1.0, 0).unwrap(),
            movie_active: false,
            frame: 0,
            channel: 0,
            output: [0.; 2],
            stereo: true,
            levels: [127; 3],
        }
    }
}

pub(super) fn update(world: &mut World) {
    if world.contains_resource::<battle::Playback>()
        || world.contains_resource::<super::new_game::TransitionFailure>()
    {
        return;
    }
    if !world.contains_resource::<super::new_game::Session>() {
        retire(world);
        return;
    }
    // Keep the current music running through loading. The destination's opening
    // cues wait for its prepared scene, then use the new bank on the same mixer.
    if !world
        .resource::<super::loading::Resident>()
        .active
        .load(Ordering::Acquire)
    {
        return;
    }
    let result = (|| -> Result<()> {
        if let Some(control) = world.get_resource::<Control>()
            && let Err(error) = control.check()
        {
            let assets = control.assets.clone();
            let diagnostics = control.diagnostics.clone();
            retire(world);
            diagnostics.report("field audio mixer", error)?;
            if let Some(mut session) = world.get_resource_mut::<super::new_game::Session>() {
                session.audio.get_or_insert(assets);
            }
        }
        if !world.contains_resource::<super::new_game::Session>() {
            retire(world);
            return Ok(());
        }
        if world.resource::<super::new_game::Session>().audio.is_some()
            && !super::new_game::scene_ready(world)
        {
            return Ok(());
        }
        if let Some(assets) = world.resource::<super::new_game::Session>().audio.clone() {
            if let Some(mut control) = world.get_resource_mut::<Control>() {
                control.enter_field(assets)?;
            } else {
                let (source, control) = assets.session();
                let handle = world
                    .resource_mut::<bevy::asset::Assets<FieldSource>>()
                    .add(source);
                world.spawn((AudioPlayer(handle), PlaybackSettings::ONCE));
                world.insert_resource(control);
            }
            let mut session = world.resource_mut::<super::new_game::Session>();
            session.audio = None;
            if session.is_field() {
                session.field_mut().voice_feedback = true;
            }
        }
        let owner = world.resource::<super::new_game::Session>();
        let preferences = owner
            .events()
            .world
            .party
            .as_ref()
            .map(|p| &p.settings.preferences);
        let stereo = preferences.is_none_or(|p| p.stereo);
        let mut levels = preferences.map_or([127; 3], |p| {
            [
                p.volumes.music,
                p.volumes.effects,
                if p.event_voiceover {
                    p.volumes.voice
                } else {
                    0
                },
            ]
        });
        // Music previews while editing. New cues and stereo use the committed
        // settings, including the Back cue emitted when Customize commits.
        if let Some(preview) = owner
            .menu()
            .and_then(resonance_game::menu::Menu::preferences)
        {
            levels[0] = preview.volumes.music;
        }
        let diagnostics = super::diagnostics::policy(world);
        diagnostics.attempt(
            "audio stereo setting",
            world.resource_mut::<Control>().stereo(stereo),
        )?;
        diagnostics.attempt(
            "audio volume setting",
            world.resource_mut::<Control>().levels(levels),
        )?;
        let session = world.resource::<super::new_game::Session>();
        let movie_active = session.movie_owns_audio() || session.events().world.blocked_by_movie();
        world.resource_mut::<Control>().movie(movie_active)?;
        flush_commands(world)?;
        Ok(())
    })();
    if let Err(error) = result
        && super::diagnostics::policy(world)
            .report("field audio adapter", error)
            .is_err()
    {
        if let Some(mut session) = world.get_resource_mut::<super::new_game::Session>() {
            session.events_mut().cancel();
        }
        world.write_message(AppExit::error());
    }
}

fn flush_commands(world: &mut World) -> Result<()> {
    let commands = std::mem::take(
        &mut world
            .resource_mut::<super::new_game::Session>()
            .events_mut()
            .world
            .audio_commands,
    );
    if world.contains_resource::<Trace>() {
        let tick = world.resource::<super::new_game::Session>().events().tick();
        let frame = world.resource::<Control>().rendered_frames();
        let mut trace = world.resource_mut::<Trace>();
        ensure!(
            trace.0.len() + commands.len() <= 4096,
            "field audio evidence limit exceeded"
        );
        for command in &commands {
            trace.0.push(serde_json::json!({"tick":tick,"source_frame":frame,"command":format!("{command:?}")}));
        }
    }
    let control = world
        .get_resource::<Control>()
        .context("field audio session is unavailable")?;
    control.check()?;
    for command in commands {
        control.send(command)?;
    }
    Ok(())
}

/// Field effects and dialogue end at the boundary; music and its clock survive.
/// A full queue defers the transition; a stopped optional mixer cannot block it.
pub(super) fn leave_field(world: &mut World) -> Result<bool> {
    let Some(control) = world.get_resource::<Control>() else {
        return Ok(true);
    };
    let checked = control.check();
    let diagnostics = control.diagnostics.clone();
    let result = checked.and_then(|()| {
        flush_commands(world)?;
        world.resource_mut::<Control>().leave_field()
    });
    if let Err(error) = result {
        let full = matches!(
            error.downcast_ref::<mpsc::TrySendError<Message>>(),
            Some(mpsc::TrySendError::Full(_))
        );
        if !full {
            retire(world);
        }
        diagnostics.report("field audio handoff", error)?;
        return Ok(!full);
    }
    Ok(true)
}

pub(super) fn retire(world: &mut World) {
    world.remove_resource::<Control>();
    let retired: Vec<_> = world
        .query_filtered::<Entity, With<AudioPlayer<FieldSource>>>()
        .iter(world)
        .collect();
    for entity in retired {
        world.despawn(entity);
    }
}

pub(super) fn acknowledge(world: &mut World) {
    let Some(control) = world.get_resource::<Control>().cloned() else {
        return;
    };
    let position = world
        .query_filtered::<&super::audio_output::Sink, With<AudioPlayer<FieldSource>>>()
        .iter(world)
        .next()
        .map(|sink| sink.0.audible_frames());
    if let Some(position) = position {
        control.acknowledge_frames(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_events::input::{Button, Buttons};

    #[test]
    #[ignore = "requires RESONANCE_WORLD_ASSETS; original Meltokio sewer script and PCM synthesis without a device"]
    fn meltokio_sewer_background_audio_survives_idle_updates() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
        );
        let mut scene = crate::field_test::Scene::story(&root, 466, 1_304_000, |entry| {
            entry.position = [472.98508, 3736.6846, 1170.];
            entry.persistent.event_flags.insert(65);
            Ok(())
        })?;
        scene.advance_until(resonance_game::field::FieldSession::player_has_control)?;
        scene.ticks(1800, Default::default())?;
        ensure!(scene.player_has_control(), "sewer idle scene lost control");
        let cues = scene
            .audio
            .commands
            .iter()
            .filter(|(_, _, command)| matches!(command, AudioCommand::Sound { id: 318, .. }))
            .count();
        ensure!(cues >= 2, "sewer background audio did not repeat: {cues}");
        Ok(())
    }

    #[test]
    #[ignore = "requires named RESONANCE_WORLD_ASSETS; plays recorded world music and native vehicle cues without a device"]
    fn original_world_music_and_vehicle_sounds_render_from_verified_assets() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
        );
        let package = crate::overworld::Package::prepare(
            &root,
            &mut Default::default(),
            || false,
            Diagnostics::new(true),
        )?;
        let bank = package.audio;
        for id in [2, 3, 4, 5] {
            let (source, control) = bank.clone().session();
            let mut frames = source.decoder();
            control.send(AudioCommand::Music(MusicCommand::Play(id)))?;
            let mut audible = false;
            for _ in 0..RATE * 2 {
                let frame = frames.frame()?.context("world music ended")?;
                ensure!(frame.iter().all(|v| v.is_finite()), "invalid music sample");
                audible |= frame.iter().any(|v| v.abs() > 0.001);
            }
            ensure!(audible, "silent world music {id}");
        }
        for id in [24, 25] {
            let (source, control) = bank.clone().session();
            let mut frames = source.decoder();
            control.send(AudioCommand::RepeatSound {
                id,
                pan: 64,
                volume: 64,
                slot: 15,
            })?;
            let mut audible = false;
            for index in 0..RATE * 8 {
                let frame = frames.frame()?.context("world vehicle sound ended")?;
                ensure!(
                    frame.iter().all(|v| v.is_finite()),
                    "invalid vehicle sample"
                );
                if index >= RATE * 7 {
                    audible |= frame.iter().any(|v| v.abs() > 0.001);
                }
            }
            ensure!(audible, "silent world vehicle {id}");
            assert_eq!(
                frames
                    .sounds
                    .iter()
                    .filter(|sound| sound.slot == Some(15))
                    .count(),
                1
            );
            control.send(AudioCommand::StopSound(15))?;
            frames.frame()?;
            assert!(frames.sounds.iter().all(|sound| sound.slot != Some(15)));
            assert!(frames.sounds.iter().all(|sound| sound.repeat.is_none()));
        }
        Ok(())
    }

    fn synthetic_assets() -> Assets {
        Assets {
            diagnostics: Diagnostics::new(true),
            recordings: BTreeMap::new(),
            reverbs: [[0.5, 0.5, 1., 0.5, 0.]; 2],
            music_reverbs: Some(MusicReverbs {
                presets: vec![[0.5, 0.5, 1., 0.5, 0.]; 2],
                selectors: vec![1; 112],
            }),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [1.; 128],
        }
    }

    #[test]
    fn failed_sound_preserves_music_and_reports_once_in_each_error_policy() -> Result<()> {
        use resonance_audio::data::{Command, EventKind, ScoreOrigin, VoiceSource};
        for paranoid in [false, true] {
            let mut assets = synthetic_assets();
            assets.diagnostics = Diagnostics::new(paranoid);
            let diagnostics = assets.diagnostics.clone();
            assets.recordings.insert(0, test_music());
            let (mut resources, mut score, tables) = test_score_data(ScoreOrigin::SoundEffect);
            resources.programs.insert(
                1,
                vec![Command::Jump {
                    program: 1,
                    instruction: 0,
                }],
            );
            let EventKind::Notes { source, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id: 9 };
            assets.sounds.insert(
                9,
                Arc::new(Loaded::new(resources, score, tables, assets.reverbs)?.into()),
            );
            let (source, control) = Arc::new(assets.clone()).session();
            let (reference, reference_control) = Arc::new(assets).session();
            let mut frames = source.decoder();
            let mut reference = reference.decoder();
            control.send(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
            reference_control.send(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
            control.send(AudioCommand::Sound {
                id: 9,
                pan: 64,
                volume: 127,
                slot: None,
            })?;
            if paranoid {
                assert!(frames.frame().is_err());
            } else {
                let mut heard = false;
                for _ in 0..320 {
                    let actual = frames.frame()?;
                    assert_eq!(actual, reference.frame()?);
                    heard |= actual.is_some_and(|frame| frame.iter().any(|sample| *sample != 0.));
                }
                assert!(heard);
                assert!(frames.music.is_some());
                assert!(frames.sounds.is_empty());
                assert_eq!(frames.frame, 320);
            }
            let entries = diagnostics.entries();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].occurrences, 1);
            assert!(entries[0].message.contains("instruction budget exhausted"));
        }
        Ok(())
    }

    #[test]
    fn interrupted_voices_finish_at_their_last_consumed_frame() -> Result<()> {
        let mut assets = synthetic_assets();
        assets.voices.insert(
            7,
            Arc::new(test_clip(vec![16384; RATE as usize * 12], RATE, 1)),
        );
        let (source, control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        let replaced = Arc::new(AtomicBool::new(false));
        control.send(AudioCommand::Voice {
            resource: 7,
            completion: Some(replaced.clone()),
        })?;
        assert!(frames.frame()?.unwrap()[0] > 0.);
        let stopped = Arc::new(AtomicBool::new(false));
        control.send(AudioCommand::Voice {
            resource: 7,
            completion: Some(stopped.clone()),
        })?;
        assert!(frames.frame()?.unwrap()[0] > 0.);
        control.acknowledge_frames(0);
        assert!(!replaced.load(Ordering::Acquire));
        control.acknowledge_frames(1);
        assert!(replaced.load(Ordering::Acquire));
        assert!(!stopped.load(Ordering::Acquire));
        control.send(AudioCommand::StopVoice)?;
        assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        control.acknowledge_frames(1);
        assert!(!stopped.load(Ordering::Acquire));
        control.acknowledge_frames(2);
        assert!(stopped.load(Ordering::Acquire));
        assert!(control.completions.lock().unwrap().is_empty());
        Ok(())
    }

    #[test]
    fn rejected_field_handoff_can_retry_the_same_prepared_bank() -> Result<()> {
        let (source, mut control) = Arc::new(synthetic_assets()).session();
        while control.enqueue(Message::Stereo(true)).is_ok() {}
        let mut destination = synthetic_assets();
        destination
            .voices
            .insert(7, Arc::new(test_clip([16384; 32], RATE, 1)));
        let destination = Arc::new(destination);
        assert!(control.enter_field(destination.clone()).is_err());
        let mut frames = source.decoder();
        frames.frame()?;
        assert!(!frames.assets.voices.contains_key(&7));
        control.enter_field(destination.clone())?;
        control.send(AudioCommand::voice(7))?;
        assert!(frames.frame()?.unwrap()[0] > 0.);
        assert!(Arc::ptr_eq(&frames.assets, &destination));
        Ok(())
    }

    #[test]
    fn stopped_field_slots_never_start_new_audio_and_preserve_queued_pcm() -> Result<()> {
        for elapsed in [0, 73] {
            let mut assets = synthetic_assets();
            assets.sounds.insert(1, test_score());
            let (source, control) = Arc::new(assets.clone()).session();
            let (reference, reference_control) = Arc::new(assets).session();
            let mut frames = source.decoder();
            let mut reference = reference.decoder();
            let sound = AudioCommand::Sound {
                id: 1,
                pan: 64,
                volume: 127,
                slot: Some(3),
            };
            control.send(sound.clone())?;
            reference_control.send(sound)?;
            for _ in 0..elapsed {
                assert_eq!(frames.frame()?, reference.frame()?);
            }
            control.send(AudioCommand::StopSound(3))?;
            for _ in elapsed..160 {
                let output = frames.frame()?.unwrap();
                if elapsed == 0 {
                    assert_eq!(output, [0.; 2]);
                } else {
                    assert_eq!(Some(output), reference.frame()?);
                    assert!(output[0] > 0.);
                }
            }
            assert!(frames.sounds.is_empty());
            for _ in 0..480 {
                frames.frame()?;
            }
            assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        }
        Ok(())
    }

    #[test]
    fn failed_field_slot_replacements_leave_the_old_sound_playing() -> Result<()> {
        for paranoid in [false, true] {
            let mut assets = synthetic_assets();
            assets.diagnostics = Diagnostics::new(paranoid);
            assets.sounds.insert(1, test_score());
            let (source, control) = Arc::new(assets.clone()).session();
            let (reference, reference_control) = Arc::new(assets).session();
            let mut frames = source.decoder();
            let mut reference = reference.decoder();
            let sound = |id| AudioCommand::Sound {
                id,
                pan: 64,
                volume: 127,
                slot: Some(3),
            };
            control.send(sound(1))?;
            reference_control.send(sound(1))?;
            for _ in 0..73 {
                assert_eq!(frames.frame()?, reference.frame()?);
            }
            for command in [
                sound(99),
                AudioCommand::Sound {
                    id: 1,
                    pan: 128,
                    volume: 127,
                    slot: Some(3),
                },
            ] {
                control.send(command)?;
                let result = frames.frame();
                if paranoid {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result?, reference.frame()?);
                }
                assert_eq!(frames.sounds.len(), 1);
                assert_eq!(frames.sounds[0].slot, Some(3));
                for _ in 0..200 {
                    assert_eq!(frames.frame()?, reference.frame()?);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn occupied_sound_slots_can_be_replaced_at_the_polyphony_limit() -> Result<()> {
        let mut assets = synthetic_assets();
        assets.sounds.insert(1, test_score());
        let (source, control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        for slot in 0..resonance_audio::sequence::VOICE_BUDGET as u8 {
            control.send(AudioCommand::Sound {
                id: 1,
                pan: 64,
                volume: 127,
                slot: Some(slot),
            })?;
        }
        for _ in 0..73 {
            frames.frame()?;
        }
        control.send(AudioCommand::Sound {
            id: 1,
            pan: 0,
            volume: 32,
            slot: Some(3),
        })?;
        frames.frame()?;
        let replacement = frames
            .sounds
            .iter()
            .filter(|sound| sound.slot == Some(3))
            .collect::<Vec<_>>();
        assert_eq!(replacement.len(), 1);
        assert_eq!(replacement[0].controls.pan, Some(0));
        assert_eq!(replacement[0].volume, 32. / 127.);
        for _ in 0..480 {
            frames.frame()?;
        }
        control.check()?;
        Ok(())
    }

    #[test]
    fn music_replacement_does_not_compete_with_cue_wrappers() -> Result<()> {
        let mut assets = synthetic_assets();
        assets.recordings = [(1, test_music()), (2, test_music())].into();
        let presets = assets.music_reverbs.as_mut().unwrap();
        presets.presets[1] = [0.8, 0.7, 2., 0.4, 0.02];
        presets.selectors[2] = 2;
        let changed_reverb = presets.presets[1];
        let (source, control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        control.send(AudioCommand::Music(resonance_events::MusicCommand::Play(1)))?;
        for _ in 0..73 {
            frames.frame()?;
        }
        let held = (0..64)
            .map(|_| ScorePlayer::new(test_score(), &frames.synth))
            .collect::<Result<Vec<_>>>()?;
        control.send(AudioCommand::Music(resonance_events::MusicCommand::Play(2)))?;
        frames.frame()?;
        assert_eq!(frames.music.as_ref().map(|music| music.id), Some(2));
        assert_eq!(frames.studio.music_reverb(), changed_reverb);
        drop(held);
        let mut audible = false;
        for _ in 0..640 {
            audible |= frames.frame()?.unwrap() != [0.; 2];
        }
        assert!(audible);
        control.check()?;
        Ok(())
    }

    fn prepared_voice() -> (resonance_content::field_audio::Voice, Arc<[u8]>) {
        let mut wave = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut wave,
            hound::WavSpec {
                channels: 1,
                ..PCM_SPEC
            },
        )
        .unwrap();
        writer.write_sample(16384i16).unwrap();
        writer.finalize().unwrap();
        let bytes: Arc<[u8]> = wave.into_inner().into();
        (
            resonance_content::field_audio::Voice {
                path: "audio/valid.wav".into(),
                frames: 1,
                sample_rate: RATE,
                source_sample_rate: RATE,
                channels: 1,
            },
            bytes,
        )
    }

    #[test]
    fn release_reaches_dry_and_aux_once_across_field_and_preset_changes() -> Result<()> {
        let assets = synthetic_assets();
        let mut reference = Studio::new(assets.reverbs)?;
        let (source, mut control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        frames.synth.release([[160, -160], [160, -320], [0; 2]], 0);
        for frame in 0..640 {
            if frame == 73 {
                control.enter_field(synthetic_assets())?;
            }
            if frame == 199 {
                let parameters = [0.2, 0.7, 0.5, 0.4, 0.];
                frames.studio.set_music_reverb(parameters)?;
                reference.set_music_reverb(parameters)?;
            }
            let remaining = (160 - frame).max(0);
            let expected = reference
                .process([[remaining, -remaining], [remaining, -2 * remaining], [0; 2]])
                .map(|sample| sample as f32 / 32768.);
            assert_eq!(frames.frame()?.unwrap(), expected, "frame {frame}");
        }
        Ok(())
    }

    #[test]
    fn silent_start_initializes_both_effects_once_when_valid_assets_arrive() -> Result<()> {
        let (source, mut control) = Arc::new(Assets::silent(Diagnostics::new(false))).session();
        let mut frames = source.decoder();
        assert!(!frames.studio_initialized);
        let assets = synthetic_assets();
        let mut reference = Studio::new(assets.reverbs)?;
        control.enter_field(assets)?;
        assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        reference.process([[0; 2]; 3]);
        assert!(frames.studio_initialized);
        let mut audible_a = false;
        let mut audible_b = false;
        for frame in 0..6007 {
            let buses = if frame < 3000 {
                [[0; 2], [1000; 2], [0; 2]]
            } else {
                [[0; 2], [0; 2], [2000; 2]]
            };
            let expected = reference.process(buses);
            if frame < 3000 {
                audible_a |= expected != [0; 2];
            } else {
                audible_b |= expected != [0; 2];
            }
            assert_eq!(frames.studio.process(buses), expected);
        }
        assert!(audible_a && audible_b);
        let mut another = synthetic_assets();
        another.reverbs = [[0.1, 0.2, 0.3, 0.4, 0.01]; 2];
        control.enter_field(another)?;
        for _ in 0..337 {
            let expected = reference
                .process([[0; 2]; 3])
                .map(|v| (v as f32 / 32768.).clamp(-1., 1.));
            assert_eq!(frames.frame()?.unwrap(), expected);
        }
        Ok(())
    }

    #[test]
    fn invalid_audio_headers_fail_before_loading_media() -> Result<()> {
        use super::named_tests::{fixture, write};
        let root = fixture()?;
        let path = resonance_content::field_audio::mix::PATH;
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join(path))?)?;
        // Header admission is structural, independent of optional media policy.
        for defect in ["missing_catalogue", "invalid_reverb", "voice_gains"] {
            let mut value = original.clone();
            match defect {
                "missing_catalogue" => {
                    value.as_object_mut().unwrap().remove("music_reverbs");
                }
                "invalid_reverb" => value["reverbs"]["field"]["decay_seconds"] = (-1).into(),
                _ => value["voice_gains"] = serde_json::json!([]),
            }
            write(root.path(), path, &serde_json::to_vec(&value)?)?;
            for paranoid in [false, true] {
                assert!(
                    Cache::default()
                        .capture_test_field(
                            "fields/iselia/field.json",
                            &mut resonance_content::library::Library::open(root.path(), &[])?
                                .snapshot(),
                            Diagnostics::new(paranoid),
                            || false
                        )
                        .is_err()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn invalid_music_does_not_discard_healthy_effects_and_speech() -> Result<()> {
        use super::named_tests::{fixture, write};
        let root = fixture()?;
        write(root.path(), "audio/music/test-song.wav", b"not a WAV")?;
        let library = resonance_content::library::Library::open(root.path(), &[])?;
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let result = Cache::default().capture_test_field(
                "fields/iselia/field.json",
                &mut library.snapshot(),
                diagnostics.clone(),
                || false,
            );
            if paranoid {
                assert!(result.is_err());
            } else {
                let loaded = result?;
                assert!(loaded.recordings.is_empty());
                assert_eq!(loaded.sounds.keys().copied().collect::<Vec<_>>(), [3]);
                assert_eq!(loaded.voices.len(), 2);
            }
            assert!(diagnostics.has_errors());
        }
        Ok(())
    }

    #[test]
    fn optional_reverb_errors_preserve_valid_music_and_the_current_effect() -> Result<()> {
        let root = super::named_tests::fixture()?;
        let library = resonance_content::library::Library::open(root.path(), &[])?;
        let base = Cache::default().capture_test_field(
            "fields/iselia/field.json",
            &mut library.snapshot(),
            Diagnostics::new(true),
            || false,
        )?;
        let reverb_choices = MusicReverbs {
            presets: vec![[0.8, 0.7, 2., 0.4, 0.02], [2., 0., 1., 0., 0.]],
            selectors: vec![0, 1, 2, 7],
        };
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut assets = (*base).clone();
            assets.diagnostics = diagnostics.clone();
            assets.recordings = (0..=4)
                .map(|id| (id, base.recordings[&7].clone()))
                .collect();
            assets.music_reverbs = Some(reverb_choices.clone());
            let (source, _control) = Arc::new(assets).session();
            let mut frames = source.decoder();
            frames.command(AudioCommand::Music(resonance_events::MusicCommand::Play(1)))?;
            let effect = frames.studio.music_reverb();
            assert_eq!(effect, reverb_choices.presets[0]);
            assert!(
                !diagnostics.has_errors(),
                "unused invalid settings are irrelevant"
            );
            for id in [2, 3, 4] {
                let result = frames.command(AudioCommand::Music(
                    resonance_events::MusicCommand::Play(id.try_into().unwrap()),
                ));
                if paranoid {
                    assert!(result.is_err());
                    assert_eq!(frames.music.as_ref().map(|music| music.id), Some(1));
                } else {
                    result?;
                    assert_eq!(frames.music.as_ref().map(|music| music.id), Some(id));
                }
                assert_eq!(frames.studio.music_reverb(), effect);
            }
            assert!(diagnostics.has_errors());
            frames.command(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
            assert_eq!(frames.music.as_ref().map(|music| music.id), Some(0));
            assert_eq!(frames.studio.music_reverb(), effect);
        }
        Ok(())
    }

    #[test]
    fn named_voice_curve_controls_native_volume() -> Result<()> {
        let root = super::named_tests::fixture()?;
        let library = resonance_content::library::Library::open(root.path(), &[])?;
        let loaded = Cache::default().capture_test_field(
            "fields/iselia/field.json",
            &mut library.snapshot(),
            Diagnostics::new(true),
            || false,
        )?;
        let (source, mut control) = loaded.session();
        let mut frames = source.decoder();
        let mut sample_at = |level| -> Result<[f32; 2]> {
            control.levels([127, 127, level])?;
            control.send(AudioCommand::voice(65543))?;
            frames.frame()?.context("voice mixer stopped")
        };
        let full = sample_at(127)?;
        assert!(full.into_iter().any(|sample| sample != 0.));
        assert_eq!(sample_at(0)?, [0.; 2]);
        for (sample, full) in sample_at(64)?.into_iter().zip(full) {
            assert!((sample - full * 64. / 127.).abs() < 0.000001);
        }
        Ok(())
    }

    #[test]
    fn tolerant_audio_skips_missing_cues_and_finishes_missing_dialogue_before_valid_audio() {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut assets = synthetic_assets();
            assets.diagnostics = diagnostics.clone();
            assets
                .voices
                .insert(7, Arc::new(test_clip(vec![16384; 32], RATE, 1)));
            let (source, control) = Arc::new(assets).session();
            let missing = Arc::new(AtomicBool::new(false));
            control
                .send(AudioCommand::Voice {
                    resource: 99,
                    completion: Some(missing.clone()),
                })
                .unwrap();
            control
                .send(AudioCommand::Sound {
                    id: 999,
                    pan: 64,
                    volume: 127,
                    slot: None,
                })
                .unwrap();
            let played = Arc::new(AtomicBool::new(false));
            control
                .send(AudioCommand::Voice {
                    resource: 7,
                    completion: Some(played.clone()),
                })
                .unwrap();
            let mut frames = source.decoder();
            if paranoid {
                assert!(frames.frame().is_err());
                assert!(missing.load(Ordering::Acquire));
                assert!(!played.load(Ordering::Acquire));
            } else {
                let pcm = frames.frame().unwrap().unwrap();
                assert!(pcm.iter().all(|sample| *sample > 0.3));
                assert!(missing.load(Ordering::Acquire));
                assert!(!played.load(Ordering::Acquire));
                assert_eq!(diagnostics.entries().len(), 2);
                for _ in 0..32 {
                    frames.frame().unwrap();
                }
                control.acknowledge_frames(frames.frame);
                assert!(played.load(Ordering::Acquire));
                control.check().unwrap();
            }
        }
    }

    #[test]
    fn rejected_voice_requests_complete_and_the_queue_recovers() -> Result<()> {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut assets = synthetic_assets();
            assets.diagnostics = diagnostics.clone();
            assets
                .voices
                .insert(7, Arc::new(test_clip(vec![16384; 32], RATE, 1)));
            let (source, control) = Arc::new(assets).session();
            while control.enqueue(Message::Stereo(true)).is_ok() {}
            let rejected = Arc::new(AtomicBool::new(false));
            let result = control.send(AudioCommand::Voice {
                resource: 7,
                completion: Some(rejected.clone()),
            });
            assert_eq!(result.is_err(), paranoid);
            assert!(rejected.load(Ordering::Acquire));
            assert!(diagnostics.has_errors());
            let mut frames = source.decoder();
            frames.frame()?;
            let played = Arc::new(AtomicBool::new(false));
            control.send(AudioCommand::Voice {
                resource: 7,
                completion: Some(played.clone()),
            })?;
            assert!(frames.frame()?.unwrap().iter().any(|sample| *sample > 0.));
            assert!(!played.load(Ordering::Acquire));
            for _ in 0..32 {
                frames.frame()?;
            }
            control.acknowledge_frames(frames.frame);
            assert!(played.load(Ordering::Acquire));
            let waiting = std::array::from_fn::<_, 3, _>(|_| Arc::new(AtomicBool::new(false)));
            for (index, token) in waiting.iter().enumerate() {
                control.send(AudioCommand::Voice {
                    resource: 7,
                    completion: Some(token.clone()),
                })?;
                if index < 2 {
                    frames.frame()?;
                }
            }
            // One interrupted voice awaits sink consumption, one is playing,
            // and the third request is still queued when the decoder vanishes.
            assert_eq!(control.completions.lock().unwrap().len(), 1);
            assert!(waiting.iter().all(|token| !token.load(Ordering::Acquire)));
            drop(frames);
            assert!(waiting.iter().all(|token| token.load(Ordering::Acquire)));
            assert!(control.completions.lock().unwrap().is_empty());
            let disconnected = Arc::new(AtomicBool::new(false));
            let result = control.send(AudioCommand::Voice {
                resource: 7,
                completion: Some(disconnected.clone()),
            });
            assert_eq!(result.is_err(), paranoid);
            assert!(disconnected.load(Ordering::Acquire));
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked fields; no window or audio device"]
    fn idle_field_observes_decoder_failure_and_recovers_or_exits() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            std::path::PathBuf::from,
        );
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut assets = synthetic_assets();
            assets.diagnostics = diagnostics.clone();
            assets
                .voices
                .insert(7, Arc::new(test_clip([16384; 32], RATE, 1)));
            let (source, control) = Arc::new(assets).session();
            let mut frames = source.decoder();
            let waiting = Arc::new(AtomicBool::new(false));
            let mut session = super::super::new_game::Session::load(&root)?;
            session.audio = None;
            session.field_mut().voice_feedback = true;
            session.field_mut().events.world.audio_commands = vec![AudioCommand::Voice {
                resource: 7,
                completion: Some(waiting.clone()),
            }];
            let resident = super::super::loading::Resident::default();
            resident.active.store(true, Ordering::Release);
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default()))
                .init_asset::<Image>();
            let art = crate::field_view::prepared_test_art(
                session.field_assets(),
                app.world().resource::<AssetServer>(),
            );
            app.init_resource::<bevy::asset::Assets<FieldSource>>()
                .add_message::<AppExit>()
                .insert_resource(super::super::diagnostics::Diagnostics(diagnostics))
                .insert_resource(art)
                .insert_resource(resident)
                .insert_resource(session)
                .insert_resource(control)
                .add_systems(Update, update);
            let handle = app
                .world_mut()
                .resource_mut::<bevy::asset::Assets<FieldSource>>()
                .add(source);
            let retired = app.world_mut().spawn(AudioPlayer(handle)).id();
            app.update();
            assert!(frames.frame()?.unwrap()[0] > 0.);
            assert!(!waiting.load(Ordering::Acquire));
            if paranoid {
                // An accepted voice is still waiting when a later command fails
                // on the actual iterator used by the audio worker.
                app.world().resource::<Control>().send(AudioCommand::Music(
                    resonance_events::MusicCommand::Play(999),
                ))?;
                assert_eq!(frames.next(), None);
            }
            drop(frames);
            assert!(
                app.world()
                    .resource::<super::super::new_game::Session>()
                    .field()
                    .events
                    .world
                    .audio_commands
                    .is_empty()
            );
            app.update();
            assert!(waiting.load(Ordering::Acquire));
            assert!(app.world().get_entity(retired).is_err());
            assert_eq!(
                !app.world().resource::<Messages<AppExit>>().is_empty(),
                paranoid
            );
            if paranoid {
                assert!(!app.world().contains_resource::<Control>());
                continue;
            }
            let handle = {
                let world = app.world_mut();
                world
                    .query::<&AudioPlayer<FieldSource>>()
                    .single(world)?
                    .0
                    .clone()
            };
            let mut replacement = app
                .world()
                .resource::<bevy::asset::Assets<FieldSource>>()
                .get(&handle)
                .unwrap()
                .decoder();
            let completed = Arc::new(AtomicBool::new(false));
            app.world_mut()
                .resource_mut::<super::super::new_game::Session>()
                .field_mut()
                .events
                .world
                .audio_commands
                .push(AudioCommand::Voice {
                    resource: 7,
                    completion: Some(completed.clone()),
                });
            app.update();
            assert!(replacement.frame()?.unwrap()[0] > 0.);
            for _ in 0..32 {
                replacement.frame()?;
            }
            let control = app.world().resource::<Control>();
            control.acknowledge_frames(replacement.frame);
            assert!(completed.load(Ordering::Acquire));
            control.check()?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked fields; no window or audio device"]
    fn field_transition_retries_full_audio_and_retires_stopped_audio() -> Result<()> {
        use super::super::{loading::Resident, new_game};
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            std::path::PathBuf::from,
        );
        for paranoid in [false, true] {
            for stopped in [false, true] {
                let diagnostics = Diagnostics::new(paranoid);
                let mut session = new_game::Session::load(&root)?;
                for tick in 0..3000 {
                    if session.field().events.world.field_transition.is_some() {
                        break;
                    }
                    session
                        .field_mut()
                        .step(resonance_game::field::FieldInput {
                            pressed_buttons: (resonance_events::input::Buttons::default())
                                .with(resonance_events::input::Button::Accept, tick % 120 == 0),
                            ..Default::default()
                        })?;
                }
                let request = session
                    .field()
                    .events
                    .world
                    .field_transition
                    .as_ref()
                    .unwrap();
                assert_eq!(request.map, 340);
                let operation = request.operation.clone();
                session.audio = None;
                session.field_mut().events.world.audio_commands.clear();
                let mut assets = synthetic_assets();
                assets.diagnostics = diagnostics.clone();
                assets
                    .voices
                    .insert(7, Arc::new(test_clip([16384; 32], RATE, 1)));
                let (source, control) = Arc::new(assets).session();
                let mut decoder = Some(source.decoder());
                let waiting = std::array::from_fn::<_, 2, _>(|_| Arc::new(AtomicBool::new(false)));
                for (index, token) in waiting.iter().enumerate() {
                    control.send(AudioCommand::Voice {
                        resource: 7,
                        completion: Some(token.clone()),
                    })?;
                    if index == 0 {
                        decoder.as_mut().unwrap().frame()?;
                    }
                }
                if stopped {
                    drop(decoder.take());
                } else {
                    while control.enqueue(Message::Stereo(true)).is_ok() {}
                }
                let mut resident = Resident::default();
                resident.diagnostics = diagnostics.clone();
                resident.active.store(true, Ordering::Release);
                let mut app = App::new();
                app.init_resource::<bevy::asset::Assets<FieldSource>>()
                    .init_resource::<ButtonInput<KeyCode>>()
                    .add_message::<AppExit>()
                    .insert_resource(super::super::diagnostics::Diagnostics(diagnostics.clone()))
                    .insert_resource(resident)
                    .insert_resource(crate::test_support::run_options(&root))
                    .insert_resource(session)
                    .insert_resource(control)
                    // Match the ordinary player: transition precedes audio update.
                    .add_systems(Update, (new_game::transition, update).chain());
                let handle = app
                    .world_mut()
                    .resource_mut::<bevy::asset::Assets<FieldSource>>()
                    .add(source);
                let entity = app.world_mut().spawn(AudioPlayer(handle)).id();
                app.update();
                assert!(app.world().resource::<Messages<AppExit>>().is_empty());
                if paranoid {
                    assert_eq!(app.world().resource::<new_game::Session>().map_id(), 5);
                    assert!(
                        operation.is_pending(),
                        "failed handoff must remain retryable"
                    );
                }
                if !stopped {
                    let session = app.world().resource::<new_game::Session>();
                    assert_eq!(session.map_id(), 5);
                    assert!(session.field().events.world.field_transition.is_some());
                    assert!(operation.is_pending());
                    assert!(waiting.iter().all(|token| !token.load(Ordering::Acquire)));
                    decoder.as_mut().unwrap().frame()?;
                }
                if paranoid {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .press(KeyCode::Enter);
                }
                app.update();
                if !stopped {
                    let frames = decoder.as_mut().unwrap();
                    frames.frame()?;
                    app.world()
                        .resource::<Control>()
                        .acknowledge_frames(frames.frame);
                }
                let began = std::time::Instant::now();
                while app.world().resource::<new_game::Session>().map_id() == 5 {
                    ensure!(began.elapsed().as_secs() < 30, "field preparation stalled");
                    app.update();
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                let session = app.world().resource::<new_game::Session>();
                assert_eq!(session.map_id(), 340);
                assert!(
                    session.audio.is_some(),
                    "destination audio was not retained"
                );
                assert!(app.world().resource::<Messages<AppExit>>().is_empty());
                assert!(waiting.iter().all(|token| token.load(Ordering::Acquire)));
                assert_eq!(app.world().get_entity(entity).is_err(), stopped);
                assert!(
                    diagnostics
                        .entries()
                        .iter()
                        .all(|entry| entry.scope == "field audio handoff")
                );
            }
        }
        Ok(())
    }

    #[test]
    fn voice_loader_handles_metadata_missing_files_and_replaced_pcm() {
        let (mut voice, mut bytes) = prepared_voice();
        let mut files = resonance_content::prepared::Files::default();
        files.insert(voice.path.clone(), bytes.clone());
        let load = |voice: &resonance_content::field_audio::Voice,
                    files: &resonance_content::prepared::Files| {
            Clip::capture(voice, &mut |asset, limit| {
                let bytes = files.read(asset)?;
                ensure!(bytes.len() <= limit, "voice exceeds size limit");
                Ok(bytes)
            })
        };
        let first = load(&voice, &files).unwrap();
        voice.sample_rate = 48000;
        let faster = load(&voice, &files).unwrap();
        assert!(!Arc::ptr_eq(&first, &faster));
        assert!(Arc::ptr_eq(&first.bytes, &faster.bytes));
        assert_eq!(faster.rate, 48000);
        voice.frames = 2;
        assert!(load(&voice, &files).is_err());
        voice.frames = 1;
        files.remove(&voice.path);
        assert!(load(&voice, &files).is_err());

        // Same valid WAV header and size, but new PCM is accepted directly.
        *Arc::make_mut(&mut bytes).last_mut().unwrap() = 0x20;
        files.insert(voice.path.clone(), bytes);
        let replaced = load(&voice, &files).unwrap();
        assert_eq!(replaced.value(0), 8192);
        assert_eq!(first.value(0), 16384);
        let released = Arc::downgrade(&first);
        let replacement = Arc::downgrade(&replaced);
        drop((first, faster, replaced));
        assert!(released.upgrade().is_none());
        assert!(replacement.upgrade().is_none());
        assert_eq!(load(&voice, &files).unwrap().value(0), 8192);
    }

    #[test]
    fn field_audio_rejects_oversized_owner_before_decoding() -> Result<()> {
        let root = super::named_tests::fixture()?;
        let path = "fields/iselia/field.json";
        fs::File::create(root.path().join(path))?
            .set_len(resonance_content::preparation::JSON_LIMIT as u64 + 1)?;
        let error = Cache::default()
            .capture_test_field(
                path,
                &mut resonance_content::library::Library::open(root.path(), &[])?.snapshot(),
                Diagnostics::new(true),
                || false,
            )
            .err()
            .context("oversized document was accepted")?;
        assert!(format!("{error:#}").contains("content resource exceeds"));
        Ok(())
    }

    #[test]
    fn valid_audio_banks_admit_entries_beyond_former_count_limits() -> Result<()> {
        let root = super::named_tests::fixture()?;
        let mut snapshot = resonance_content::library::Library::open(root.path(), &[])?.snapshot();
        let definition = snapshot
            .json::<resonance_content::field::document::Definition>(
                "fields/iselia/field.json",
                resonance_content::preparation::JSON_LIMIT,
            )?
            .audio;
        let mut resolved = definition.resolve(&mut snapshot)?;
        resolved.music = (0..257)
            .map(|id| (id, resolved.music[&7].clone()))
            .collect();
        resolved.effects = (0..1025)
            .map(|id| (id, resolved.effects[&3].clone()))
            .collect();
        resolved.voices = (0..4097)
            .map(|id| (id, resolved.voices[&65543].clone()))
            .collect();
        let mut environment = definition.environment(&mut snapshot)?;
        environment.music_reverbs.selectors.resize(257, 0);
        let loaded = Cache::default().capture_resolved(
            &environment,
            resolved,
            &mut snapshot,
            Diagnostics::new(true),
            &|| false,
        )?;
        assert_eq!(
            (
                loaded.recordings.len(),
                loaded.sounds.len(),
                loaded.voices.len()
            ),
            (257, 1025, 4097)
        );
        assert!(Arc::ptr_eq(&loaded.sounds[&0], &loaded.sounds[&1024]));
        assert!(Arc::ptr_eq(&loaded.voices[&0], &loaded.voices[&4096]));
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked Iselia fields; no window or audio device"]
    fn field_handoffs_preserve_music_pcm_and_retire_scene_voices() {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        );
        let mut banks = [332, 330, 340]
            .into_iter()
            .map(|id| Assets::load(&root, id).unwrap())
            .collect::<Vec<_>>();
        // A pending line belongs to the old scene even when its ID is reused.
        banks[0]
            .voices
            .insert(1, Arc::new(test_clip(vec![16384; 1000], RATE, 1)));
        let initial = Arc::new(banks[0].clone());
        let (reference, reference_control) = initial.clone().session();
        let (source, mut control) = initial.session();
        let mut reference = reference.decoder();
        let mut frames = source.decoder();
        reference_control
            .send(AudioCommand::Music(MusicCommand::Play(7)))
            .unwrap();
        control
            .send(AudioCommand::Music(MusicCommand::Play(7)))
            .unwrap();
        let mut audible = false;
        for bank in banks.into_iter().skip(1) {
            // Cross score block boundaries and leave a live reverb tail.
            for _ in 0..RATE + 31 {
                let expected = reference.frame().unwrap().unwrap();
                assert_eq!(frames.frame().unwrap().unwrap(), expected);
                audible |= expected.iter().any(|sample| sample.abs() > 0.001);
            }
            let completion = Arc::new(AtomicBool::new(false));
            if frames.assets.voices.contains_key(&1) {
                control
                    .send(AudioCommand::Voice {
                        resource: 1,
                        completion: Some(completion.clone()),
                    })
                    .unwrap();
            }
            let sound = *frames.assets.sounds.keys().next().unwrap();
            control
                .send(AudioCommand::Sound {
                    id: sound,
                    pan: 64,
                    volume: 127,
                    slot: Some(1),
                })
                .unwrap();
            control.leave_field().unwrap();
            control.leave_field().unwrap();
            // Loading can last arbitrarily long without pausing the music.
            for _ in 0..913 {
                assert_eq!(frames.frame().unwrap(), reference.frame().unwrap());
            }
            assert!(frames.voice.is_none() && frames.sounds.is_empty());
            control.acknowledge_frames(frames.frame);
            assert!(control.completions.lock().unwrap().is_empty());
            assert_eq!(
                completion.load(Ordering::Acquire),
                frames.assets.voices.contains_key(&1)
            );
            control.enter_field(bank).unwrap();
            control
                .send(AudioCommand::Music(MusicCommand::Play(7)))
                .unwrap();
        }
        for _ in 0..RATE {
            assert_eq!(frames.frame().unwrap(), reference.frame().unwrap());
        }
        assert!(audible);
        assert_eq!(
            control.rendered_frames(),
            reference_control.rendered_frames()
        );
        // A destination request resolves against its bank, including speech.
        let voice = *frames.assets.voices.keys().next().unwrap();
        let complete = Arc::new(AtomicBool::new(false));
        control
            .send(AudioCommand::Voice {
                resource: voice,
                completion: Some(complete.clone()),
            })
            .unwrap();
        frames.frame().unwrap();
        assert_eq!(frames.voice.as_ref().unwrap().frame, 1);
        assert!(control.completions.lock().unwrap().is_empty());
        assert!(!complete.load(Ordering::Acquire));
        control.check().unwrap();
        drop(control);
        assert!(frames.frame().unwrap().is_none());
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS with the Tower of Salvation; no devices"]
    fn salvation_scene_keeps_kratos_theme_audible_after_battle() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
        );
        let mut field = crate::field_test::Scene::story(&root, 535, 2_302_000, |entry| {
            entry.persistent.party.as_mut().unwrap().formation = vec![1, 2, 3, 4, 9];
            Ok(())
        })?;
        let mut heard_introduction = false;
        let battles = field.replay(|field| {
            if field.events.world.dialogue.values().any(|d| d.body.tokens.iter().any(|token| {
                matches!(token, resonance_events::dialogue::TextToken::Text { text } if text.contains("I am of Cruxis"))
            })) {
                heard_introduction = true;
                assert_eq!(field.audio.frames.music.as_ref().map(|music| music.id), Some(106));
                assert!(field.audio.frames.music.as_ref().unwrap().fade.value() > 0.5, "Kratos theme was muted after the battle");
            }
            Ok(field.events.world.field_transition.is_some())
        })?;
        assert!(heard_introduction);
        assert_eq!(battles, 3);
        assert_eq!(field.story_progress()?, 2_306_000);
        assert_eq!(
            field.events.world.field_transition.as_ref().unwrap().map,
            277
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS with Triet; no window or audio device"]
    fn colette_room_exit_restores_inn_music_before_the_next_scene() -> Result<()> {
        use crate::new_game::{FieldPackage, Session};
        use resonance_game::field::{FieldEntry, FieldInput};

        let mut app = crate::movie::tests::fixture();
        let root = app.world().resource::<crate::RunOptions>().assets.clone();
        let mut session = Session::load(&root)?;
        let package = FieldPackage::prepare(&root, 528, &mut Default::default(), || false)?;
        let mut persistent = session.field().events.persistent_state()?;
        persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 1_203_000)?;
        let field = package.enter(FieldEntry {
            persistent,
            data: Some(Arc::new(resonance_content::session::SessionData::load(
                &package.files,
            )?)),
            available_fields: crate::new_game::available_fields(&root)?,
            ..Default::default()
        })?;
        let package = Arc::new(package);
        session.activate(field, package.clone(), false);
        let mut audio = validation::Playback::new((*package.audio).clone(), session.field_mut());
        app.insert_resource(session);
        app.insert_resource(audio.control.clone());
        let mut heard_scene_music = false;
        for tick in 0..5000 {
            app.world_mut()
                .resource_mut::<Session>()
                .field_mut()
                .step(FieldInput {
                    pressed_buttons: Buttons::default().with(Button::Accept, tick % 2 == 0),
                    held_buttons: Buttons::default().with(Button::Accept, true),
                    ..Default::default()
                })?;
            let exiting = app
                .world()
                .resource::<Session>()
                .events()
                .world
                .field_transition
                .is_some();
            if exiting {
                // Exercise the production scene handoff before its usual audio update.
                let start = std::time::Instant::now();
                while app.world().resource::<Session>().field().map_id == 528 {
                    crate::new_game::transition(app.world_mut());
                    ensure!(start.elapsed().as_secs() < 30, "inn transition stalled");
                    std::thread::yield_now();
                }
            } else {
                flush_commands(app.world_mut())?;
            }
            audio.advance()?;
            heard_scene_music |= audio.frames.music.as_ref().map(|music| music.id) == Some(51);
            if exiting {
                break;
            }
        }
        assert!(heard_scene_music);
        assert_eq!(app.world().resource::<Session>().field().map_id, 526);
        assert_eq!(audio.frames.music.as_ref().map(|music| music.id), Some(8));

        // The next room inherits the same mixer; its script only changes volume.
        let package = FieldPackage::prepare(&root, 529, &mut Default::default(), || false)?;
        let persistent = app
            .world()
            .resource::<Session>()
            .field()
            .events
            .persistent_state()?;
        let field = package.enter(FieldEntry {
            persistent,
            data: Some(Arc::new(resonance_content::session::SessionData::load(
                &package.files,
            )?)),
            available_fields: crate::new_game::available_fields(&root)?,
            ..Default::default()
        })?;
        let package = Arc::new(package);
        app.world_mut()
            .resource_mut::<Session>()
            .activate(field, package.clone(), false);
        app.world_mut()
            .resource_mut::<Control>()
            .enter_field((*package.audio).clone())?;
        for tick in 0..5000 {
            app.world_mut()
                .resource_mut::<Session>()
                .field_mut()
                .step(FieldInput {
                    pressed_buttons: Buttons::default().with(Button::Accept, tick % 2 == 0),
                    held_buttons: Buttons::default().with(Button::Accept, true),
                    ..Default::default()
                })?;
            flush_commands(app.world_mut())?;
            audio.advance()?;
            assert_eq!(audio.frames.music.as_ref().map(|music| music.id), Some(8));
            if app.world().resource::<Session>().field().story_progress()? == 1_204_000 {
                break;
            }
        }
        assert_eq!(
            app.world().resource::<Session>().field().story_progress()?,
            1_204_000
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked outdoor music 77; no window or audio device"]
    fn one_shot_music_finishes_and_can_be_requested_again() {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        );
        let assets = Assets::load(&root, 332).unwrap();
        assert!(assets.recordings.contains_key(&77));
        let (source, _control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        frames
            .command(AudioCommand::Music(MusicCommand::Play(77)))
            .unwrap();
        let mut audible = false;
        let mut count = 0;
        while frames.music.is_some() {
            assert!(count < RATE * 120, "one-shot music did not finish");
            let frame = frames.frame().unwrap().expect("mixer stopped");
            audible |= frame.iter().any(|sample| sample.abs() > 0.001);
            count += 1;
        }
        assert!(audible);
        assert_eq!(frames.music.as_ref().map(|music| music.id), None);
        assert!(frames.frame().unwrap().is_some());
        frames
            .command(AudioCommand::Music(MusicCommand::Play(77)))
            .unwrap();
        assert!(frames.music.is_some());
        assert_eq!(frames.music.as_ref().map(|music| music.id), Some(77));
        assert!((0..RATE * 2).any(|_| {
            frames
                .frame()
                .unwrap()
                .expect("mixer stopped on replay")
                .iter()
                .any(|sample| sample.abs() > 0.001)
        }));
        eprintln!("Music 77 completed after {count} source frames; field mixer remains live");
    }

    #[test]
    fn score_block_gains_preview_elapsed_time_without_advancing_the_fade() {
        let master = Fade::new(1., 1., 0).unwrap();
        let duration_ms = 10;
        let duration_frames = (duration_ms * u64::from(RATE)).div_ceil(1000);
        let mut fade = Fade::new(1., 0., duration_ms).unwrap();
        let elapsed_frames = 64;
        fade.advance(elapsed_frames);
        let expected = std::array::from_fn(|quantum| {
            let elapsed = elapsed_frames + quantum as u64 * CONTROL_FRAMES as u64;
            (1. - elapsed as f64 / duration_frames as f64) as f32
        });
        assert_eq!(block_gains(master, Some(fade)), expected);
        assert_eq!(
            fade.value(),
            expected[0],
            "preview must not advance the fade"
        );
        fade.advance(duration_frames - elapsed_frames - 1);
        assert!(fade.value() > 0.);
        fade.advance(1);
        assert_eq!(block_gains(master, Some(fade)), [0.; CONTROLS_PER_BLOCK]);
    }

    #[test]
    fn field_entry_clears_scripted_music_fades_before_destination_commands() -> Result<()> {
        let mut assets = synthetic_assets();
        assets.recordings.insert(0, test_music());
        let assets = Arc::new(assets);
        let (source, mut control) = assets.clone().session();
        let mut frames = source.decoder();
        control.send(AudioCommand::Music(MusicCommand::Play(0)))?;
        control.levels([0, 127, 127])?;
        frames.frame()?;
        for (volume, duration_ticks) in [(0, 0), (50, 0), (0, 120)] {
            control.send(AudioCommand::MusicVolume {
                volume,
                duration_ticks,
            })?;
            frames.frame()?;
            let previous = frames.music.as_ref().unwrap().fade.value();
            control.leave_field()?;
            frames.frame()?;
            assert!(frames.music.as_ref().unwrap().fade.value() <= previous);

            control.enter_field(assets.clone())?;
            control.send(AudioCommand::Music(MusicCommand::Play(0)))?;
            frames.frame()?;
            assert_eq!(frames.music.as_ref().unwrap().fade.value(), 1.);
            assert_eq!(frames.levels[0], 0, "keep the user's music setting");
        }
        control.enter_field(assets)?;
        control.send(AudioCommand::MusicVolume {
            volume: 50,
            duration_ticks: 0,
        })?;
        frames.frame()?;
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), 50. / 127.);
        Ok(())
    }

    #[test]
    fn script_volume_uses_the_game_clock_and_accepts_long_fades() -> Result<()> {
        let mut assets = synthetic_assets();
        assets.recordings.insert(0, test_music());
        let (source, _control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        frames.command(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
        frames.command(AudioCommand::MusicVolume {
            volume: 50,
            duration_ticks: 0,
        })?;
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), 50. / 127.);
        frames.command(AudioCommand::MusicVolume {
            volume: 0,
            duration_ticks: 1,
        })?;
        // An update is 1001/60000 seconds: 535 frames at the source's 32,028 Hz.
        let one_update = (u64::from(RATE) * 1001).div_ceil(60_000);
        for _ in 0..one_update - 1 {
            frames.frame()?;
        }
        assert!(frames.music.as_ref().unwrap().fade.value() > 0.);
        frames.frame()?;
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), 0.);

        frames.command(AudioCommand::MusicVolume {
            volume: 72,
            duration_ticks: 7200,
        })?;
        // Two minutes of game updates include the rational clock's extra time.
        let duration = (120_120 * u64::from(RATE)).div_ceil(1000);
        frames.music.as_mut().unwrap().fade.advance(duration - 1);
        assert!(frames.music.as_ref().unwrap().fade.value() < 72. / 127.);
        frames.frame()?;
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), 72. / 127.);
        Ok(())
    }

    #[test]
    fn sound_mode_changes_the_mixed_channels_without_restarting_sources() {
        let mut voice_gains = [1.; 128];
        voice_gains[0] = 0.;
        voice_gains[64] = 0.25;
        let assets = Assets {
            diagnostics: Diagnostics::new(true),
            voice_gains,
            voices: [(1, Arc::new(test_clip([16384, -8192].repeat(1000), RATE, 2)))].into(),
            ..synthetic_assets()
        };
        let (source, mut control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        control.send(AudioCommand::voice(1)).unwrap();
        control.stereo(false).unwrap();
        assert_eq!([frames.next(), frames.next()], [Some(0.125); 2]);
        control.stereo(true).unwrap();
        assert_eq!([frames.next(), frames.next()], [Some(0.5), Some(-0.25)]);
        assert_eq!(frames.voice.as_ref().unwrap().frame, 2);
        control.levels([0, 0, 127]).unwrap();
        assert_eq!([frames.next(), frames.next()], [Some(0.5), Some(-0.25)]);
        control.levels([127, 127, 0]).unwrap();
        assert_eq!([frames.next(), frames.next()], [Some(0.); 2]);
        assert_eq!(frames.voice.as_ref().unwrap().frame, 4);
        control.levels([127, 127, 64]).unwrap();
        assert_eq!([frames.next(), frames.next()], [Some(0.125), Some(-0.0625)]);
        control.check().unwrap();
    }

    #[test]
    fn movie_mutes_the_game_bus_without_pausing_sources_and_teardown_disconnects() {
        let assets = Assets {
            diagnostics: Diagnostics::new(true),
            voices: [(1, Arc::new(test_clip(vec![16384; 100000], RATE, 1)))].into(),
            ..synthetic_assets()
        };
        let (source, mut control) = Arc::new(assets).session();
        let mut frames = source.decoder();
        control.movie(true).unwrap();
        control.send(AudioCommand::voice(1)).unwrap();
        assert!(frames.by_ref().take(320).all(|sample| sample == 0.));
        assert_eq!(frames.voice.as_ref().unwrap().frame, 160);
        control.movie(false).unwrap();
        // Speech is outside the two synthesizer groups restored over two
        // seconds, so it returns immediately at centered mono gain.
        let restored: Vec<_> = frames.by_ref().take(130000).collect();
        assert!(
            restored
                .iter()
                .all(|sample| (*sample - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001)
        );
        assert_eq!(frames.master.value(), 1.);
        control.check().unwrap();
        drop(control);
        assert_eq!(frames.next(), None);
        assert!(frames.voice.is_none());
    }
}

#[test]
fn explicit_silence_preserves_clock_without_allocating_pcm() -> Result<()> {
    let clock = resonance_content::field_audio::speech::Clock {
        frames: 64000,
        sample_rate: 32000,
        channels: 2,
    };
    let clip = Clip::silence(clock, Some(32028))?;
    assert_eq!(
        (clip.sample_count(), clip.rate, clip.channels),
        (128000, 32028, 2)
    );
    assert!(clip.bytes.is_empty());
    assert_eq!((clip.value(0), clip.value(127999)), (0, 0));
    assert!(Clip::silence(clock, Some(0)).is_err());
    Ok(())
}
