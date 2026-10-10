//! Field audio adapter: cooked scores, live cue lifetimes and spoken lines.
//! The same source feeds Bevy and the device-free recorder.
use crate::audio_output::{PlaybackSettings, Player as AudioPlayer};
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
#[path = "battle_audio.rs"]
pub(crate) mod battle;
#[path = "field_audio_record.rs"]
mod record;
#[path = "field_audio_validation.rs"]
pub(super) mod validation;
#[cfg(test)]
#[path = "field_voice_tests.rs"]
mod voice_tests;
use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
pub use record::record_field_audio;
use resonance_audio::{
    BLOCK_FRAMES, CONTROL_FRAMES, CONTROLS_PER_BLOCK,
    package::{Loaded, Package},
    reverb::Studio,
    sequence::{BusFrame, LiveControls, shared::Synthesizer, stream::Stream},
    volume::Fade,
};
use resonance_content::{
    diagnostics::Diagnostics,
    field_audio::{
        Asset as Reference, DecodedBank, FieldAudio,
        MAX_MANIFEST_BYTES as MAX_AUDIO_MANIFEST_BYTES, MusicReverbs,
    },
    field_preload::Role as AudioRole,
};
use resonance_events::{AudioCommand, MusicCommand};
use resonance_game::clock::{UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{Cursor, Read},
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
pub(crate) fn test_score() -> Arc<Loaded> {
    test_score_kind(resonance_audio::data::ScoreOrigin::SoundEffect)
}
#[cfg(test)]
pub(crate) fn test_music() -> Arc<Loaded> {
    test_score_kind(resonance_audio::data::ScoreOrigin::Sequence)
}
#[cfg(test)]
fn test_score_kind(origin: resonance_audio::data::ScoreOrigin) -> Arc<Loaded> {
    let (resources, score, tables) = test_score_data(origin);
    Arc::new(Loaded::new(resources, score, tables, [[0.5, 0.5, 1., 0.5, 0.]; 2]).unwrap())
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
    music: BTreeMap<i16, Arc<Loaded>>,
    sounds: BTreeMap<i16, Arc<Loaded>>,
    voices: BTreeMap<u32, Arc<Clip>>,
    voice_gains: [f32; 128],
    reverbs: [[f32; 5]; 2],
    music_reverbs: Option<MusicReverbs>,
}

#[derive(Default)]
pub(super) struct Cache {
    manifests: Option<(String, Arc<DecodedBank>)>,
    skits: Option<(String, Arc<resonance_content::skit::SkitCatalog>)>,
    packages: BTreeMap<String, CachedPackage>,
    voices: BTreeMap<(String, u32, u32, u32, u16), std::sync::Weak<Clip>>,
    samples: resonance_audio::package::SampleCache,
}

struct CachedPackage {
    loaded: std::sync::Weak<Loaded>,
    samples: Vec<(Reference, usize)>,
}
fn prepared_json<T: serde::de::DeserializeOwned>(
    files: &resonance_content::prepared::Files,
    path: &str,
    cache: &mut Option<(String, Arc<T>)>,
) -> Result<Arc<T>> {
    let digest = files.digest(path)?;
    let bytes = files.read_verified(path, digest, MAX_AUDIO_MANIFEST_BYTES)?;
    if let Some((cached_digest, value)) = cache
        && cached_digest.as_str() == digest
    {
        return Ok(Arc::clone(value));
    }
    let value = Arc::new(serde_json::from_slice(&bytes).with_context(|| format!("decode {path}"))?);
    *cache = Some((digest.to_owned(), Arc::clone(&value)));
    Ok(value)
}
impl Cache {
    fn prune(&mut self) {
        self.packages
            .retain(|_, package| package.loaded.strong_count() != 0);
        self.voices.retain(|_, voice| voice.strong_count() != 0);
        self.samples.prune();
    }

    pub fn load(
        &mut self,
        manifest: &str,
        files: &resonance_content::prepared::Files,
    ) -> Result<Arc<Assets>> {
        self.prune();
        let diagnostics = files.diagnostics().clone();
        let Some(manifest) = diagnostics.attempt(
            "field audio manifest",
            prepared_json(files, manifest, &mut self.manifests),
        )?
        else {
            return Ok(Arc::new(Assets::silent(diagnostics)));
        };
        let manifest = manifest.checked(&diagnostics)?;
        let skits = diagnostics.attempt(
            "field skit voices",
            prepared_json(files, "game/skits.json", &mut self.skits),
        )?;
        let mut read =
            |asset: &Reference, limit, _| files.read_verified(&asset.path, &asset.sha256, limit);
        let mut assets = Assets::load_contents(manifest, diagnostics, self, &mut read)?;
        if let Some(skits) = skits {
            assets.load_voices(
                skits
                    .media
                    .iter()
                    .filter_map(|(&id, media)| media.voice.as_ref().map(|voice| (id, voice))),
                self,
                &mut read,
            )?;
        }
        Ok(Arc::new(assets))
    }
    fn package(
        &mut self,
        asset: &Reference,
        read: &mut impl FnMut(&Reference, usize, AudioRole) -> Result<Arc<[u8]>>,
    ) -> Result<Arc<Loaded>> {
        asset.validate()?;
        let bytes = read(asset, MAX_AUDIO_MANIFEST_BYTES, AudioRole::AudioPackage)?;
        if let Some(cached) = self.packages.get(&asset.sha256)
            && let Some(loaded) = cached.loaded.upgrade()
        {
            for (sample, limit) in &cached.samples {
                read(sample, *limit, AudioRole::InstrumentSample)?;
            }
            return Ok(loaded);
        }
        let package: Package = serde_json::from_slice(&bytes)?;
        let mut samples = Vec::new();
        let loaded = Arc::new(package.prepare(
            &mut |sample, limit| {
                let reference = Reference {
                    path: sample.path.clone(),
                    sha256: sample.sha256.clone(),
                };
                let bytes = read(&reference, limit, AudioRole::InstrumentSample)?;
                samples.push((reference, limit));
                Ok(bytes)
            },
            &mut self.samples,
        )?);
        self.packages.insert(
            asset.sha256.clone(),
            CachedPackage {
                loaded: Arc::downgrade(&loaded),
                samples,
            },
        );
        Ok(loaded)
    }
    fn voice(
        &mut self,
        voice: &resonance_content::field_audio::Voice,
        read: &mut impl FnMut(&Reference, usize, AudioRole) -> Result<Arc<[u8]>>,
    ) -> Result<Arc<Clip>> {
        voice.validate()?;
        let count = voice.frames as usize * usize::from(voice.channels);
        let bytes = read(&voice.asset, count * 2 + 1024 * 1024, AudioRole::Voice)?;
        let key = (
            voice.asset.sha256.clone(),
            voice.frames,
            voice.sample_rate,
            voice.source_sample_rate,
            voice.channels,
        );
        if let Some(clip) = self.voices.get(&key).and_then(std::sync::Weak::upgrade) {
            return Ok(clip);
        }
        let clip = Arc::new(Clip::prepare(bytes, voice)?);
        self.voices.insert(key, Arc::downgrade(&clip));
        Ok(clip)
    }
}
fn disk_bytes(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= limit as u64,
        "audio resource exceeds read budget"
    );
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "audio resource exceeds read budget");
    Ok(bytes)
}
fn disk_asset(root: &Path, asset: &Reference, limit: usize) -> Result<Arc<[u8]>> {
    asset.validate()?;
    let bytes = disk_bytes(&root.join(&asset.path), limit)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == asset.sha256,
        "audio resource digest differs: {}",
        asset.path
    );
    Ok(bytes.into())
}
impl Assets {
    fn silent(diagnostics: Diagnostics) -> Self {
        Self {
            diagnostics,
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [0.; 128],
            reverbs: [[0., 0., 0.01, 0., 0.]; 2],
            music_reverbs: None,
        }
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
        Self::load_disk(
            root,
            &resonance_content::field::audio_path(map),
            &mut Cache::default(),
        )
    }
    fn load_disk(root: &Path, manifest: &str, cache: &mut Cache) -> Result<Self> {
        resonance_content::validate_asset_path(manifest)?;
        let manifest: DecodedBank =
            serde_json::from_slice(&disk_bytes(&root.join(manifest), MAX_AUDIO_MANIFEST_BYTES)?)?;
        let diagnostics = Diagnostics::new(true);
        Self::load_contents(
            manifest.checked(&diagnostics)?,
            diagnostics,
            cache,
            &mut |asset, limit, _| disk_asset(root, asset, limit),
        )
    }
    fn load_contents(
        manifest: &FieldAudio,
        diagnostics: Diagnostics,
        cache: &mut Cache,
        read: &mut impl FnMut(&Reference, usize, AudioRole) -> Result<Arc<[u8]>>,
    ) -> Result<Self> {
        cache.prune();
        if diagnostics
            .attempt("field audio schema", manifest.validate_header())?
            .is_none()
        {
            return Ok(Self::silent(diagnostics));
        }
        let voice_gains = diagnostics
            .attempt("audio voice levels", manifest.validated_voice_gains())?
            .unwrap_or_else(|| std::array::from_fn(|level| level as f32 / 127.));
        let mut reverbs: Option<[[f32; 5]; 2]> = None;
        let mut packages = |references: &BTreeMap<i16, Reference>,
                            music: bool|
         -> Result<BTreeMap<i16, Arc<Loaded>>> {
            let mut loaded = BTreeMap::new();
            for (&id, asset) in references {
                let result = (|| -> Result<_> {
                    let package = cache.package(asset, read)?;
                    ensure!(
                        !music
                            || package.score().origin
                                == resonance_audio::data::ScoreOrigin::Sequence,
                        "music requires a sequence score"
                    );
                    ensure!(
                        reverbs.is_none_or(|reverbs| package.reverbs()[1] == reverbs[1]),
                        "field packages disagree on the initial auxiliary B effect"
                    );
                    reverbs = Some(package.reverbs());
                    Ok(package)
                })()
                .with_context(|| format!("audio package {id}: {}", asset.path));
                if let Some(package) = diagnostics.attempt("audio package", result)? {
                    loaded.insert(id, package);
                }
            }
            Ok(loaded)
        };
        let expects_scores = !manifest.music.is_empty() || !manifest.sounds.is_empty();
        let music = packages(&manifest.music, true)?;
        let sounds = packages(&manifest.sounds, false)?;
        let reverbs = if expects_scores {
            diagnostics.attempt(
                "audio studio",
                reverbs.context("field audio packages are missing"),
            )?
        } else {
            reverbs
        };
        // A missing initial B configuration cannot establish a valid Studio.
        let music_reverbs = reverbs.is_some().then(|| manifest.music_reverbs.clone());
        let reverbs = reverbs.unwrap_or([[0., 0., 0.01, 0., 0.]; 2]);
        let mut assets = Self {
            diagnostics,
            music,
            sounds,
            voices: BTreeMap::new(),
            voice_gains,
            reverbs,
            music_reverbs,
        };
        assets.load_voices(
            manifest.voices.iter().map(|(&id, voice)| (id, voice)),
            cache,
            read,
        )?;
        Ok(assets)
    }
    fn load_voices<'a>(
        &mut self,
        voices: impl IntoIterator<Item = (u32, &'a resonance_content::field_audio::Voice)>,
        cache: &mut Cache,
        read: &mut impl FnMut(&Reference, usize, AudioRole) -> Result<Arc<[u8]>>,
    ) -> Result<()> {
        for (id, voice) in voices {
            let result = cache
                .voice(voice, read)
                .with_context(|| format!("spoken line {id}: {}", voice.asset.path));
            if let Some(clip) = self.diagnostics.attempt("audio stream", result)? {
                self.voices.insert(id, clip);
            }
        }
        Ok(())
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
            // Scores keep advancing while muted; movie completion restores their gain over two seconds.
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
    controls: LiveControls,
    slot: Option<u16>,
    volume: f32,
    repeat: Option<Arc<Loaded>>,
}
impl ScorePlayer {
    fn new(package: Arc<Loaded>, looping: bool, synth: &Synthesizer) -> Result<Self> {
        // One-shot music has no repeat traversal, just like a sound cue.
        let looping = looping && !package.score().loop_events.is_empty();
        Ok(Self {
            stream: Stream::in_synthesizer(package, looping, synth)?,
            controls: LiveControls::default(),
            slot: None,
            volume: 1.,
            repeat: None,
        })
    }
    fn prepare_shared(&self, gains: impl FnOnce() -> [f32; CONTROLS_PER_BLOCK]) -> Result<()> {
        self.stream
            .set_shared_controls(gains().map(|gain| LiveControls {
                volume: self.volume * gain,
                ..self.controls
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
    player: ScorePlayer,
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
                let package = selected
                    .map(|id| {
                        self.assets
                            .music
                            .get(&id)
                            .with_context(|| format!("uncooked field music {id}"))
                    })
                    .transpose()?;
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
                let music = package
                    .zip(selected)
                    .map(|(package, id)| -> Result<_> {
                        Ok(Music {
                            player: ScorePlayer::new(package.clone(), true, &self.synth)?,
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
                    false,
                    &self.synth,
                )?;
                sound.volume = f32::from(volume) / 127.0;
                sound.controls = LiveControls {
                    pan: Some(pan),
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
            if let Some(battle) = &mut self.battle {
                battle.prepare_stream_block(&mut self.stream_block, self.frame, &self.synth);
            }
        }
        let mut buses = [[0; 2]; 3];
        let master = self.master;
        let [music_level, effects_level, _] = self.levels.map(|v| f32::from(v) * (1. / 127.));
        let voice_level = self.assets.voice_gains[usize::from(self.levels[2])];
        let diagnostics = self.assets.diagnostics.clone();
        if let Some(music) = &mut self.music {
            music.player.controls.mono = !self.stereo;
            if diagnostics
                .attempt(
                    "music playback",
                    music.player.prepare_shared(|| {
                        block_gains(master, Some(music.fade)).map(|v| v * music_level)
                    }),
                )?
                .is_none()
            {
                self.music = None;
            }
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
        if let Some(music) = &mut self.music {
            music.player.controls.mono = !self.stereo;
            match music.player.frame() {
                Some(frame) => buses = frame,
                None => self.music = None,
            }
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
                    let mut restarted = ScorePlayer::new(package.clone(), false, &self.synth)?;
                    restarted.repeat = Some(package.clone());
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
    if world.contains_resource::<battle::Playback>() {
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
        || world.resource::<super::new_game::Session>().audio.is_some()
            && !(if world
                .resource::<super::new_game::Session>()
                .overworld
                .is_some()
            {
                super::overworld::ready(world)
            } else {
                super::field_view::ready(world)
            })
    {
        return;
    }
    let result = (|| -> Result<()> {
        let mut restarting = false;
        if let Some(control) = world.get_resource::<Control>()
            && let Err(error) = control.check()
        {
            let assets = control.assets.clone();
            let diagnostics = control.diagnostics.clone();
            retire(world);
            diagnostics.report("field audio mixer", error)?;
            if let Some(mut session) = world.get_resource_mut::<super::new_game::Session>() {
                restarting = session.audio.is_none();
                session.audio.get_or_insert(assets);
            }
        }
        if !world.contains_resource::<super::new_game::Session>() {
            retire(world);
            return Ok(());
        }
        // Keep music running through loading. A replacement bank and its opening
        // cues wait for the destination's prepared scene.
        if !world
            .resource::<super::loading::Resident>()
            .active
            .load(Ordering::Acquire)
            || !restarting
                && world.resource::<super::new_game::Session>().audio.is_some()
                && !super::field_view::ready(world)
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
            session.field.voice_feedback = true;
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
        if owner.overworld.is_none()
            && let Some(preview) = owner
                .field
                .menu
                .as_ref()
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
    let Some(mut control) = world.get_resource::<Control>().cloned() else {
        return Ok(true);
    };
    let result = control.check().and_then(|()| {
        flush_commands(world)?;
        control.leave_field()
    });
    let diagnostics = control.diagnostics.clone();
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
    #[ignore = "requires RESONANCE_WORLD_ASSETS; synthesizes native world music and vehicles without a device"]
    fn original_world_music_and_vehicle_sounds_render_from_verified_assets() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
        );
        let prepared = resonance_game::overworld::Prepared::load(
            &root,
            &mut Default::default(),
            (0..547).collect(),
            || false,
        )?;
        let bank = Cache::default().load("worlds/audio.json", &prepared.files)?;
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

    pub(super) fn empty_prepared_files(
        diagnostics: Diagnostics,
    ) -> resonance_content::prepared::Files {
        use resonance_content::field_preload::{SHARED_PATH, Shared, VERSION};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(SHARED_PATH),
            serde_json::to_vec(&Shared::<resonance_content::field_preload::File> {
                version: VERSION,
                files: BTreeMap::new(),
            })
            .unwrap(),
        )
        .unwrap();
        let files = resonance_content::prepared::Files::load_with_diagnostics(
            root.path(),
            &[],
            &mut Default::default(),
            || false,
            diagnostics,
        );
        files.unwrap()
    }

    fn synthetic_assets() -> Assets {
        Assets {
            diagnostics: Diagnostics::new(true),
            reverbs: [[0.5, 0.5, 1., 0.5, 0.]; 2],
            music_reverbs: Some(MusicReverbs {
                presets: [[0.5, 0.5, 1., 0.5, 0.]; 2],
                selectors: vec![1; 112],
            }),
            music: BTreeMap::new(),
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
            assets.music.insert(0, test_music());
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
                Arc::new(Loaded::new(resources, score, tables, assets.reverbs)?),
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
    fn descriptor_cache_shares_current_data_and_releases_replaced_data() -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Descriptor {
            value: u32,
        }
        let mut files = resonance_content::prepared::Files::default();
        let path = "audio/descriptor.json";
        files.insert(path.into(), br#"{"value":1}"#.as_slice().into());
        let mut cached = None;
        let first = prepared_json::<Descriptor>(&files, path, &mut cached)?;
        let second = prepared_json(&files, path, &mut cached)?;
        assert!(Arc::ptr_eq(&first, &second));
        let previous = Arc::downgrade(&first);
        drop(first);
        drop(second);
        files.insert(path.into(), br#"{"value":2}"#.as_slice().into());
        let current = prepared_json(&files, path, &mut cached)?;
        assert_eq!(current.value, 2);
        assert!(previous.upgrade().is_none());
        files.remove(path);
        assert!(prepared_json(&files, path, &mut cached).is_err());
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
        assets.music = [(1, test_music()), (2, test_music())].into();
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
            .map(|_| ScorePlayer::new(test_score(), false, &frames.synth))
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
                asset: Reference {
                    path: "audio/valid.wav".into(),
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                },
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
    fn missing_music_catalog_is_diagnosed_without_schema_fallback() {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut files = empty_prepared_files(diagnostics.clone());
            let value = serde_json::json!({"version":FieldAudio::VERSION,
                "music":{},"sounds":{},"voices":{},
                "voice_gains":(0..128).map(|v| v as f32 / 127.).collect::<Vec<_>>()});
            files.insert(
                "audio/missing-catalog.json".into(),
                serde_json::to_vec(&value).unwrap().into(),
            );
            let loaded = Cache::default().load("audio/missing-catalog.json", &files);
            if paranoid {
                assert!(loaded.is_err());
            } else {
                let loaded = loaded.unwrap();
                assert!(loaded.music.is_empty());
                assert!(loaded.music_reverbs.is_none());
            }
            assert!(diagnostics.has_errors());
        }
    }

    #[test]
    fn music_binding_rejects_sound_effect_scores_without_discarding_other_audio() -> Result<()> {
        let mut files = resonance_content::prepared::Files::default();
        let mut references = Vec::new();
        for (name, loaded) in [("music", test_music()), ("sound", test_score())] {
            let path = format!("audio/{name}.json");
            files.insert(
                path.clone(),
                serde_json::to_vec(&serde_json::json!({
                    "version": resonance_audio::package::VERSION,
                    "programs": {"1": [resonance_audio::data::Command::End]},
                    "samples": {}, "score": loaded.score(), "tables": loaded.tables(),
                    "reverbs": loaded.reverbs(),
                }))?
                .into(),
            );
            references.push(Reference {
                sha256: files.digest(&path)?.into(),
                path,
            });
        }
        let manifest = FieldAudio {
            version: FieldAudio::VERSION,
            music_reverbs: synthetic_assets().music_reverbs.unwrap(),
            music: [(1, references[0].clone()), (2, references[1].clone())].into(),
            sounds: [(3, references[1].clone())].into(),
            voices: BTreeMap::new(),
            voice_gains: (0..128).map(|level| level as f32 / 127.).collect(),
        };
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let loaded = Assets::load_contents(
                &manifest,
                diagnostics.clone(),
                &mut Cache::default(),
                &mut |asset, limit, _| files.read_verified(&asset.path, &asset.sha256, limit),
            );
            assert!(diagnostics.has_errors());
            if paranoid {
                assert!(loaded.is_err());
            } else {
                let loaded = loaded?;
                assert_eq!(loaded.music.keys().copied().collect::<Vec<_>>(), [1]);
                assert_eq!(loaded.sounds.keys().copied().collect::<Vec<_>>(), [3]);
            }
        }
        Ok(())
    }

    #[test]
    fn optional_reverb_errors_preserve_valid_music_and_the_current_effect() -> Result<()> {
        let score = test_music();
        let mut files = resonance_content::prepared::Files::default();
        let path = "audio/settings-test.json";
        files.insert(
            path.into(),
            serde_json::to_vec(&serde_json::json!({
                "version": resonance_audio::package::VERSION,
                "programs": {"1": [resonance_audio::data::Command::End]},
                "samples": {}, "score": score.score(), "tables": score.tables(),
                "reverbs": score.reverbs(),
            }))?
            .into(),
        );
        let package = Reference {
            path: path.into(),
            sha256: files.digest(path)?.into(),
        };
        let manifest = FieldAudio {
            version: FieldAudio::VERSION,
            music_reverbs: MusicReverbs {
                presets: [[0.8, 0.7, 2., 0.4, 0.02], [2., 0., 1., 0., 0.]],
                selectors: vec![0, 1, 2, 7],
            },
            music: (0..=4).map(|id| (id, package.clone())).collect(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: (0..128).map(|level| level as f32 / 127.).collect(),
        };
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let assets = Assets::load_contents(
                &manifest,
                diagnostics.clone(),
                &mut Cache::default(),
                &mut |asset, limit, _| files.read_verified(&asset.path, &asset.sha256, limit),
            )?;
            let (source, _control) = Arc::new(assets).session();
            let mut frames = source.decoder();
            frames.command(AudioCommand::Music(resonance_events::MusicCommand::Play(1)))?;
            let effect = frames.studio.music_reverb();
            assert_eq!(effect, manifest.music_reverbs.presets[0]);
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
    fn invalid_voice_curve_uses_native_volume_in_tolerant_mode() -> Result<()> {
        let (voice, bytes) = prepared_voice();
        let mut files = resonance_content::prepared::Files::default();
        files.insert(voice.asset.path.clone(), bytes);
        let manifest = FieldAudio {
            version: FieldAudio::VERSION,
            music_reverbs: synthetic_assets().music_reverbs.unwrap(),
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: [(7, voice)].into(),
            voice_gains: Vec::new(),
        };
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let loaded = Assets::load_contents(
                &manifest,
                diagnostics.clone(),
                &mut Cache::default(),
                &mut |asset, limit, _| files.read_verified(&asset.path, &asset.sha256, limit),
            );
            assert!(diagnostics.has_errors());
            if paranoid {
                assert!(loaded.is_err());
                continue;
            }
            let (source, mut control) = Arc::new(loaded?).session();
            let mut frames = source.decoder();
            let mut sample_at = |level| -> Result<[f32; 2]> {
                control.levels([127, 127, level])?;
                control.send(AudioCommand::voice(7))?;
                frames.frame()?.context("voice mixer stopped")
            };
            let full = sample_at(127)?;
            assert!(full.into_iter().any(|sample| sample != 0.));
            assert_eq!(sample_at(0)?, [0.; 2]);
            for (sample, full) in sample_at(64)?.into_iter().zip(full) {
                assert!((sample - full * 64. / 127.).abs() < 0.000001);
            }
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
            session.field.voice_feedback = true;
            session.field.events.world.audio_commands = vec![AudioCommand::Voice {
                resource: 7,
                completion: Some(waiting.clone()),
            }];
            let resident = super::super::loading::Resident::default();
            resident.active.store(true, Ordering::Release);
            let mut app = App::new();
            app.init_resource::<bevy::asset::Assets<FieldSource>>()
                .add_message::<AppExit>()
                .insert_resource(super::super::diagnostics::Diagnostics(diagnostics))
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
                    .field
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
                .field
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
                    if session.field.events.world.field_transition.is_some() {
                        break;
                    }
                    session.field.step(resonance_game::field::FieldInput {
                        pressed_buttons: (resonance_events::input::Buttons::default())
                            .with(resonance_events::input::Button::Accept, tick % 120 == 0),
                        ..Default::default()
                    })?;
                }
                let request = session
                    .field
                    .events
                    .world
                    .field_transition
                    .as_ref()
                    .unwrap();
                assert_eq!(request.map, 340);
                let operation = request.operation.clone();
                session.audio = None;
                session.field.events.world.audio_commands.clear();
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
                    .add_message::<AppExit>()
                    .insert_resource(super::super::diagnostics::Diagnostics(diagnostics.clone()))
                    .insert_resource(resident)
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
                assert_eq!(
                    !app.world().resource::<Messages<AppExit>>().is_empty(),
                    paranoid
                );
                if paranoid {
                    assert_eq!(app.world().resource::<new_game::Session>().assets.map_id, 5);
                    assert!(!operation.is_pending());
                    continue;
                }
                if !stopped {
                    let session = app.world().resource::<new_game::Session>();
                    assert_eq!(session.assets.map_id, 5);
                    assert!(session.field.events.world.field_transition.is_some());
                    assert!(operation.is_pending());
                    assert!(waiting.iter().all(|token| !token.load(Ordering::Acquire)));
                    decoder.as_mut().unwrap().frame()?;
                    app.update();
                    let frames = decoder.as_mut().unwrap();
                    frames.frame()?;
                    app.world()
                        .resource::<Control>()
                        .acknowledge_frames(frames.frame);
                }
                let session = app.world().resource::<new_game::Session>();
                assert_eq!(session.assets.map_id, 340);
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
    fn tolerant_audio_loading_keeps_valid_stream_after_missing_stream() {
        let (valid, bytes) = prepared_voice();
        let mut missing = valid.clone();
        missing.asset.path = "audio/missing.wav".into();
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut files = empty_prepared_files(diagnostics.clone());
            files.insert(valid.asset.path.clone(), bytes.clone());
            let mut cache = Cache::default();
            cache
                .voice(&valid, &mut |asset, limit, _| {
                    files.read_verified(&asset.path, &asset.sha256, limit)
                })
                .unwrap();
            let manifest = FieldAudio {
                version: FieldAudio::VERSION,
                music_reverbs: synthetic_assets().music_reverbs.unwrap(),
                music: BTreeMap::new(),
                sounds: BTreeMap::new(),
                voices: [(1, missing.clone()), (2, valid.clone())].into(),
                voice_gains: (0..128).map(|v| v as f32 / 127.).collect(),
            };
            let result = Assets::load_contents(
                &manifest,
                diagnostics.clone(),
                &mut cache,
                &mut |asset, limit, _| files.read_verified(&asset.path, &asset.sha256, limit),
            );
            if paranoid {
                assert!(result.is_err());
            } else {
                let assets = result.unwrap();
                assert!(!assets.voices.contains_key(&1));
                assert_eq!(assets.voices[&2].value(0), 16384);
                assert!(diagnostics.has_errors());
            }
        }
    }

    #[test]
    fn voice_cache_observes_metadata_missing_files_and_replaced_pcm() {
        let (mut voice, mut bytes) = prepared_voice();
        let mut files = resonance_content::prepared::Files::default();
        files.insert(voice.asset.path.clone(), bytes.clone());
        let mut cache = Cache::default();
        let load = |cache: &mut Cache,
                    voice: &resonance_content::field_audio::Voice,
                    files: &resonance_content::prepared::Files| {
            cache.voice(voice, &mut |asset, limit, _| {
                files.read_verified(&asset.path, &asset.sha256, limit)
            })
        };
        let first = load(&mut cache, &voice, &files).unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &load(&mut cache, &voice, &files).unwrap()
        ));
        voice.sample_rate = 48000;
        let faster = load(&mut cache, &voice, &files).unwrap();
        assert!(!Arc::ptr_eq(&first, &faster));
        assert!(Arc::ptr_eq(&first.bytes, &faster.bytes));
        assert_eq!(faster.rate, 48000);
        voice.frames = 2;
        assert!(load(&mut cache, &voice, &files).is_err());
        voice.frames = 1;
        files.remove(&voice.asset.path);
        assert!(load(&mut cache, &voice, &files).is_err());

        // Same valid WAV header and size, but new PCM: old bindings cannot reuse it.
        *Arc::make_mut(&mut bytes).last_mut().unwrap() = 0x20;
        files.insert(voice.asset.path.clone(), bytes);
        assert!(load(&mut cache, &voice, &files).is_err());
        voice.asset.sha256 = files.digest(&voice.asset.path).unwrap().to_owned();
        let replaced = load(&mut cache, &voice, &files).unwrap();
        assert_eq!(replaced.value(0), 8192);
        assert_eq!(first.value(0), 16384);
        let released = Arc::downgrade(&first);
        let replacement = Arc::downgrade(&replaced);
        drop((first, faster, replaced));
        assert!(released.upgrade().is_none());
        assert!(replacement.upgrade().is_none());
        cache.prune();
        assert!(cache.voices.is_empty());
        assert_eq!(load(&mut cache, &voice, &files).unwrap().value(0), 8192);
    }

    #[test]
    fn warm_package_cache_keeps_dependency_failures_isolated() {
        use resonance_audio::{
            data::{Score, ScoreOrigin},
            music_voice::Controls,
        };
        let (voice, mut bytes) = prepared_voice();
        let mut files = resonance_content::prepared::Files::default();
        files.insert(voice.asset.path.clone(), bytes.clone());
        files.insert("audio/independent.wav".into(), bytes.clone());
        let mut package = serde_json::json!({
            "version": resonance_audio::package::VERSION,
            "programs": {"1": [resonance_audio::data::Command::End]},
            "samples": {"1": {"path": voice.asset.path, "sha256": voice.asset.sha256,
                "key": 60, "rate": RATE, "first_frames": 1, "loop_start": 0, "loop_length": 0}},
            "score": Score {
                origin: ScoreOrigin::Sequence, initial_bpm_1024: 120 * 1024,
                loop_start_tick: 0, end_tick: 100,
                tempos: vec![], controls: [Controls::default(); 16],
                first_events: vec![], loop_events: vec![],
            },
            "tables": {
                "mix": {"volume": vec![1.; 129], "alternate_volume": vec![1.; 129],
                    "pan": vec![1.; 4], "volume_16_scale": 1., "controller_14_scale": 1., "pan_16_scale": 1.},
                "dls": {"attenuation": vec![0; 194], "inverse": vec![0; 1024], "sustain": vec![0.; 128]},
                "modulation": {"sine": vec![0; 1024], "tremolo": vec![1.; 5]},
                "coefficients": vec![0; 2048],
            },
            "reverbs": vec![[0.5, 0.5, 1., 0.5, 0.]; 2],
        });
        let mut manifest = FieldAudio {
            version: FieldAudio::VERSION,
            music_reverbs: synthetic_assets().music_reverbs.unwrap(),
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: (0..128).map(|v| v as f32 / 127.).collect(),
        };
        for (id, sample) in [(1, voice.asset.path.as_str()), (2, "audio/independent.wav")] {
            package["samples"]["1"]["path"] = sample.into();
            let path = format!("audio/package-{id}.json");
            files.insert(path.clone(), serde_json::to_vec(&package).unwrap().into());
            manifest.sounds.insert(
                id,
                Reference {
                    sha256: files.digest(&path).unwrap().to_owned(),
                    path,
                },
            );
        }
        let mut cache = Cache::default();
        let load = |cache: &mut Cache, files: &resonance_content::prepared::Files, diagnostics| {
            Assets::load_contents(&manifest, diagnostics, cache, &mut |asset, limit, _| {
                files.read_verified(&asset.path, &asset.sha256, limit)
            })
        };
        let first = load(&mut cache, &files, Diagnostics::new(true)).unwrap();
        let reused = load(&mut cache, &files, Diagnostics::new(true)).unwrap();
        assert!(Arc::ptr_eq(&first.sounds[&1], &reused.sounds[&1]));
        assert!(Arc::ptr_eq(
            &first.sounds[&1].resources().samples[&1],
            &first.sounds[&2].resources().samples[&1]
        ));
        let mut missing = files.clone();
        missing.remove(&voice.asset.path);
        *Arc::make_mut(&mut bytes).last_mut().unwrap() ^= 1;
        files.insert(voice.asset.path, bytes);
        for invalid in [&missing, &files] {
            let diagnostics = Diagnostics::default();
            let loaded = load(&mut cache, invalid, diagnostics.clone()).unwrap();
            assert!(!loaded.sounds.contains_key(&1));
            assert!(Arc::ptr_eq(&first.sounds[&2], &loaded.sounds[&2]));
            assert_eq!(diagnostics.entries().len(), 1);
            assert!(load(&mut cache, invalid, Diagnostics::new(true)).is_err());
        }
        let package = Arc::downgrade(&first.sounds[&2]);
        let sample = Arc::downgrade(&first.sounds[&2].resources().samples[&1]);
        let live_sample = first.sounds[&2].resources().samples[&1].clone();
        drop((first, reused));
        assert!(package.upgrade().is_none());
        assert!(sample.upgrade().is_some());
        drop(live_sample);
        assert!(sample.upgrade().is_none());
        cache.prune();
        assert!(cache.packages.is_empty());
        let prepared = load(&mut cache, &files, Diagnostics::default()).unwrap();
        assert!(!prepared.sounds.contains_key(&1));
        assert_eq!(prepared.sounds[&2].resources().samples[&1].pcm, [16384]);
    }

    #[test]
    fn standalone_audio_rejects_oversized_manifest_before_decoding() {
        let path = format!("resonance-audio-manifest-{}.json", std::process::id());
        let root = std::env::temp_dir();
        fs::File::create(root.join(&path))
            .unwrap()
            .set_len(MAX_AUDIO_MANIFEST_BYTES as u64 + 1)
            .unwrap();
        let result = Assets::load_disk(&root, &path, &mut Cache::default());
        fs::remove_file(root.join(path)).unwrap();
        assert!(result.err().unwrap().to_string().contains("read budget"));
    }

    fn prepared_bank(diagnostics: Diagnostics) -> (resonance_content::prepared::Files, FieldAudio) {
        let mut files = empty_prepared_files(diagnostics);
        let (voice, pcm) = prepared_voice();
        files.insert(voice.asset.path.clone(), pcm);
        let score = test_music();
        let path = "audio/entry-test.json";
        files.insert(
            path.into(),
            serde_json::to_vec(&serde_json::json!({
                "version": resonance_audio::package::VERSION,
                "programs": {"1": [resonance_audio::data::Command::End]},
                "samples": {}, "score": score.score(), "tables": score.tables(),
                "reverbs": score.reverbs(),
            }))
            .unwrap()
            .into(),
        );
        let package = Reference {
            path: path.into(),
            sha256: files.digest(path).unwrap().into(),
        };
        files.insert(
            "game/skits.json".into(),
            br#"{"version":2,"skits":[],"portrait_recipes":[]}"#.as_slice().into(),
        );
        (
            files,
            FieldAudio {
                version: FieldAudio::VERSION,
                music_reverbs: synthetic_assets().music_reverbs.unwrap(),
                music: [(1, package.clone())].into(),
                sounds: [(2, package)].into(),
                voices: [(7, voice)].into(),
                voice_gains: (0..128).map(|level| level as f32 / 127.).collect(),
            },
        )
    }

    #[test]
    fn cached_audio_applies_the_current_policy_to_bad_rows() -> Result<()> {
        let mut fixtures = [false, true].map(|paranoid| prepared_bank(Diagnostics::new(paranoid)));
        let mut manifest = serde_json::to_value(&fixtures[0].1)?;
        manifest["voices"]["8"] = serde_json::json!(false);
        let bytes: Arc<[u8]> = serde_json::to_vec(&manifest)?.into();
        let mut cache = Cache::default();
        for (files, _) in &mut fixtures {
            files.insert("audio/mixed.json".into(), bytes.clone());
            let loaded = cache.load("audio/mixed.json", files);
            if files.diagnostics().paranoid() {
                assert!(loaded.is_err());
            } else {
                let loaded = loaded?;
                assert_eq!(loaded.voices.keys().copied().collect::<Vec<_>>(), [7]);
                assert_eq!(loaded.voices[&7].value(0), 16384);
                assert!(loaded.music.contains_key(&1));
                assert!(loaded.sounds.contains_key(&2));
            }
            assert!(files.diagnostics().has_errors());
        }
        Ok(())
    }

    #[test]
    fn battle_audio_preparation_skips_unavailable_descriptors_and_independent_bad_entries()
    -> Result<()> {
        use resonance_content::{battle_audio, field_preload::File};
        let (base_files, base) = prepared_bank(Diagnostics::default());
        let inventory = [
            (base.music[&1].clone(), AudioRole::AudioPackage),
            (base.voices[&7].asset.clone(), AudioRole::Voice),
        ]
        .into_iter()
        .map(|(asset, role)| {
            let bytes = base_files.read(&asset.path).unwrap().len() as u64;
            (
                asset.path,
                File {
                    sha256: asset.sha256,
                    bytes,
                    roles: [role].into(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
        let descriptor = serde_json::to_value(battle_audio::Audio {
            assets: base,
            files: inventory,
            effect_spatial: [320., 5., 64., 0., 127.],
            voice_spatial: [320., 5., 64., 24., 104.],
        })?;
        let mut bad_header = descriptor.clone();
        bad_header["assets"]["version"] = 0.into();
        let mut bad_entry = descriptor;
        bad_entry["assets"]["voices"]["8"] = bad_entry["assets"]["voices"]["7"].clone();
        bad_entry["assets"]["voices"]["8"]["path"] = "audio/broken.wav".into();
        bad_entry["files"]["audio/broken.wav"] = serde_json::json!(false);
        for (bytes, available) in [
            (None, false),
            (Some(b"invalid JSON".to_vec()), false),
            (Some(serde_json::to_vec(&bad_header)?), false),
            (Some(serde_json::to_vec(&bad_entry)?), true),
        ] {
            for paranoid in [false, true] {
                let (mut files, _) = prepared_bank(Diagnostics::new(paranoid));
                if let Some(bytes) = &bytes {
                    files.insert(battle_audio::PATH.into(), bytes.clone().into());
                }
                let prepared = resonance_game::battle::audio::prepare(
                    &std::env::temp_dir(),
                    files,
                    &mut resonance_content::prepared::Cache::default(),
                    || false,
                );
                if paranoid {
                    assert!(prepared.is_err());
                    continue;
                }
                let (mut files, descriptor) = prepared?;
                assert_eq!(descriptor.is_some(), available);
                assert!(files.diagnostics().has_errors());
                // Playback uses the admitted descriptor; it does not decode it again.
                files.remove(battle_audio::PATH);
                let loaded =
                    battle::Assets::load(&files, descriptor.as_ref(), &mut Cache::default())?;
                if let Some(descriptor) = descriptor {
                    assert_eq!(
                        descriptor.assets.voices.keys().copied().collect::<Vec<_>>(),
                        [7]
                    );
                    let previous = files.diagnostics().entries();
                    loaded.bind(resonance_game::battle::voice::Sound::Cue(2))?;
                    loaded.bind(resonance_game::battle::voice::Sound::Stream(7))?;
                    assert_eq!(files.diagnostics().entries(), previous);
                }
            }
        }
        assert!(
            resonance_game::battle::audio::prepare(
                &std::env::temp_dir(),
                base_files,
                &mut resonance_content::prepared::Cache::default(),
                || true,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn valid_audio_banks_admit_entries_beyond_former_count_limits() -> Result<()> {
        let (files, mut manifest) = prepared_bank(Diagnostics::new(true));
        let package = manifest.music[&1].clone();
        let voice = manifest.voices[&7].clone();
        manifest.music = (0..257).map(|id| (id, package.clone())).collect();
        manifest.sounds = (0..1025).map(|id| (id, package.clone())).collect();
        manifest.voices = (0..4097).map(|id| (id, voice.clone())).collect();
        manifest.music_reverbs.selectors.resize(257, 0);
        manifest.validate()?;
        let loaded = Assets::load_contents(
            &manifest,
            Diagnostics::new(true),
            &mut Cache::default(),
            &mut |asset, limit, _| files.read_verified(&asset.path, &asset.sha256, limit),
        )?;
        assert_eq!(
            (loaded.music.len(), loaded.sounds.len(), loaded.voices.len()),
            (257, 1025, 4097)
        );
        assert!(Arc::ptr_eq(&loaded.music[&0], &loaded.sounds[&1024]));
        assert!(Arc::ptr_eq(&loaded.voices[&0], &loaded.voices[&4096]));
        Ok(())
    }

    #[test]
    fn invalid_audio_header_is_rejected_before_reading_individual_entries() {
        let (voice, _) = prepared_voice();
        let manifest = FieldAudio {
            version: FieldAudio::VERSION + 1,
            music_reverbs: synthetic_assets().music_reverbs.unwrap(),
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: [(1, voice)].into(),
            voice_gains: (0..128).map(|v| v as f32 / 127.).collect(),
        };
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let loaded = Assets::load_contents(
                &manifest,
                diagnostics.clone(),
                &mut Cache::default(),
                &mut |_, _, _| panic!("invalid manifest must not start resource loading"),
            );
            if paranoid {
                assert!(loaded.is_err());
            } else {
                assert!(loaded.unwrap().voices.is_empty());
            }
            assert_eq!(diagnostics.entries().len(), 1);
        }
        let mut valid = manifest;
        valid.version = FieldAudio::VERSION;
        valid.validate().unwrap();
        assert_eq!(valid.validated_voice_gains().unwrap()[127], 1.);
        valid.voice_gains[1] = f32::NAN;
        assert!(valid.validated_voice_gains().is_err());
        assert!(valid.validate().is_err());
        valid.voice_gains.clear();
        assert!(valid.validated_voice_gains().is_err());
    }

    #[test]
    #[ignore = "requires cooked Iselia fields; no window or audio device"]
    fn field_handoffs_preserve_music_pcm_and_retire_scene_voices() {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        );
        let mut cache = Cache::default();
        let mut banks = [
            "fields/map-332-audio.json",
            "fields/map-330-audio.json",
            "fields/map-340-audio.json",
        ]
        .into_iter()
        .map(|manifest| Assets::load_disk(&root, manifest, &mut cache).unwrap())
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
        let mut persistent = session.field.events.persistent_state()?;
        persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, 1_203_000)?;
        session.field = package.enter(FieldEntry {
            persistent,
            data: Some(Arc::new(package.files.json("game/session-data.json")?)),
            available_fields: crate::new_game::available_fields(&root)?,
            ..Default::default()
        })?;
        session.assets = package.assets.clone();
        let mut audio = validation::Playback::new((*package.audio).clone(), &mut session.field);
        app.insert_resource(session);
        app.insert_resource(audio.control.clone());
        let mut heard_scene_music = false;
        for tick in 0..5000 {
            app.world_mut()
                .resource_mut::<Session>()
                .field
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
                while app.world().resource::<Session>().field.map_id == 528 {
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
        assert_eq!(app.world().resource::<Session>().field.map_id, 526);
        assert_eq!(audio.frames.music.as_ref().map(|music| music.id), Some(8));

        // The next room inherits the same mixer; its script only changes volume.
        let package = FieldPackage::prepare(&root, 529, &mut Default::default(), || false)?;
        let persistent = app
            .world()
            .resource::<Session>()
            .field
            .events
            .persistent_state()?;
        app.world_mut().resource_mut::<Session>().field = package.enter(FieldEntry {
            persistent,
            data: Some(Arc::new(package.files.json("game/session-data.json")?)),
            available_fields: crate::new_game::available_fields(&root)?,
            ..Default::default()
        })?;
        app.world_mut()
            .resource_mut::<Control>()
            .enter_field((*package.audio).clone())?;
        for tick in 0..5000 {
            app.world_mut()
                .resource_mut::<Session>()
                .field
                .step(FieldInput {
                    pressed_buttons: Buttons::default().with(Button::Accept, tick % 2 == 0),
                    held_buttons: Buttons::default().with(Button::Accept, true),
                    ..Default::default()
                })?;
            flush_commands(app.world_mut())?;
            audio.advance()?;
            assert_eq!(audio.frames.music.as_ref().map(|music| music.id), Some(8));
            if app.world().resource::<Session>().field.story_progress()? == 1_204_000 {
                break;
            }
        }
        assert_eq!(
            app.world().resource::<Session>().field.story_progress()?,
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
        let manifest: FieldAudio = serde_json::from_slice(
            &std::fs::read(root.join(resonance_content::field::audio_path(332))).unwrap(),
        )
        .unwrap();
        let music = Arc::new(Package::load(&root, &manifest.music[&77].path).unwrap());
        assert!(music.score().loop_events.is_empty());
        let assets = Assets {
            diagnostics: Diagnostics::new(true),
            reverbs: music.reverbs(),
            music_reverbs: Some(manifest.music_reverbs),
            music: [(77, music)].into(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [1.; 128],
        };
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
    fn script_volume_uses_the_game_clock_and_accepts_long_fades() -> Result<()> {
        let mut assets = synthetic_assets();
        assets.music.insert(0, test_music());
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
