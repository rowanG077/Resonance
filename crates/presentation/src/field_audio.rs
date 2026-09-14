//! Field audio adapter: cooked scores, live cue lifetimes and spoken lines.
//! The same source feeds Bevy and the device-free recorder.
use crate::audio_output::{PlaybackSettings, Player as AudioPlayer};
use resonance_playback::{ChannelCount, Decodable, SampleRate, Source};
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
#[cfg(feature = "solari")]
pub(super) use record::record_window;
use resonance_audio::{
    package::{Loaded, Package},
    reverb::Studio,
    sequence::{BusFrame, LiveControls, stream::Stream},
    volume::Fade,
};
use resonance_content::field_audio::{Asset as Reference, FieldAudio};
use resonance_events::AudioCommand;
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

const RATE: u32 = 32028;
pub(super) const PCM_SPEC: hound::WavSpec = hound::WavSpec {
    channels: 2,
    sample_rate: RATE,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
};
struct Clip {
    pcm: Vec<i16>,
    rate: u32,
    channels: usize,
}
#[derive(Clone)]
pub(super) struct Assets {
    music: BTreeMap<i16, Arc<Loaded>>,
    sounds: BTreeMap<i16, Arc<Loaded>>,
    voices: BTreeMap<u32, Arc<Clip>>,
    voice_gains: [f32; 128],
    reverbs: [[f32; 5]; 2],
}

#[derive(Default)]
pub(super) struct Cache {
    banks: BTreeMap<String, Arc<Assets>>,
    samples: resonance_audio::package::SampleCache,
}
impl Cache {
    pub fn load(
        &mut self,
        root: &Path,
        manifest: &str,
        files: &resonance_content::prepared::Files,
    ) -> Result<Arc<Assets>> {
        let mut identity = Sha256::new();
        identity.update(files.read(manifest)?);
        identity.update(files.read("game/skits.json")?);
        let hash = format!("{:x}", identity.finalize());
        if let Some(bank) = self.banks.get(&hash) {
            return Ok(bank.clone());
        }
        let bank = Arc::new(Assets::load_manifest(
            root,
            manifest,
            Some(files),
            &mut self.samples,
        )?);
        self.banks.insert(hash, bank.clone());
        Ok(bank)
    }
}
fn read(
    root: &Path,
    asset: &Reference,
    limit: usize,
    files: Option<&resonance_content::prepared::Files>,
) -> Result<Vec<u8>> {
    asset.validate()?;
    let mut bytes = Vec::new();
    if let Some(files) = files {
        bytes.extend_from_slice(&files.read(&asset.path)?);
    } else {
        fs::File::open(root.join(&asset.path))?
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
    }
    ensure!(
        bytes.len() <= limit && format!("{:x}", Sha256::digest(&bytes)) == asset.sha256,
        "field audio digest or size differs: {}",
        asset.path
    );
    Ok(bytes)
}
impl Assets {
    pub fn voice_durations(&self) -> Arc<BTreeMap<u32, u32>> {
        Arc::new(
            self.voices
                .iter()
                .map(|(&id, clip)| {
                    let seconds =
                        clip.pcm.len() as f64 / clip.channels as f64 / f64::from(clip.rate);
                    (
                        id,
                        (seconds * resonance_game::clock::UPDATE_HZ).ceil() as u32,
                    )
                })
                .collect(),
        )
    }
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_with(root, None)
    }

    pub fn load_with(
        root: &Path,
        files: Option<&resonance_content::prepared::Files>,
    ) -> Result<Self> {
        Self::load_manifest(
            root,
            "fields/iselia-classroom-audio.json",
            files,
            &mut Default::default(),
        )
    }

    fn load_manifest(
        root: &Path,
        manifest: &str,
        files: Option<&resonance_content::prepared::Files>,
        sample_cache: &mut resonance_audio::package::SampleCache,
    ) -> Result<Self> {
        let read_file = |path: &str| -> Result<Vec<u8>> {
            files.map_or_else(
                || Ok(fs::read(root.join(path))?),
                |f| Ok(f.read(path)?.to_vec()),
            )
        };
        let mut manifest: FieldAudio = serde_json::from_slice(&read_file(manifest)?)?;
        manifest.validate()?;
        if let Some(files) = files {
            let skits: resonance_content::skit::SkitCatalog = files.json("game/skits.json")?;
            manifest.voices.extend(
                skits
                    .media
                    .into_iter()
                    .filter_map(|(id, media)| media.voice.map(|voice| (id, voice))),
            );
        }
        let mut packages =
            |references: BTreeMap<i16, Reference>| -> Result<BTreeMap<i16, Arc<Loaded>>> {
                references
                    .into_iter()
                    .map(|(id, asset)| {
                        read(root, &asset, 16 * 1024 * 1024, files)?;
                        Ok((
                            id,
                            Arc::new(Package::load_with(
                                &asset.path,
                                &mut |path, limit| {
                                    let bytes = read_file(path)?;
                                    ensure!(
                                        bytes.len() <= limit,
                                        "audio resource exceeds budget: {path}"
                                    );
                                    Ok(bytes)
                                },
                                sample_cache,
                            )?),
                        ))
                    })
                    .collect()
            };
        let mut voices = BTreeMap::new();
        let mut total = 0usize;
        for (id, voice) in manifest.voices {
            let count = voice.frames as usize * usize::from(voice.channels);
            total = total.checked_add(count).context("voice budget overflow")?;
            ensure!(
                total <= 64_000_000,
                "field voice bank exceeds decoded budget"
            );
            let bytes = read(root, &voice.asset, count * 2 + 1024 * 1024, files)?;
            let mut wave = hound::WavReader::new(Cursor::new(bytes))?;
            let spec = wave.spec();
            ensure!(
                spec.channels == voice.channels
                    && spec.sample_rate == voice.sample_rate
                    && spec.bits_per_sample == 16
                    && spec.sample_format == hound::SampleFormat::Int
                    && wave.duration() == voice.frames,
                "spoken line format differs from metadata"
            );
            let pcm = wave
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(pcm.len() == count, "truncated spoken line");
            voices.insert(
                id,
                Arc::new(Clip {
                    pcm,
                    rate: spec.sample_rate,
                    channels: usize::from(spec.channels),
                }),
            );
        }
        let music = packages(manifest.music)?;
        let sounds = packages(manifest.sounds)?;
        let reverbs = music
            .values()
            .chain(sounds.values())
            .next()
            .context("field audio packages are missing")?
            .reverbs;
        ensure!(
            music
                .values()
                .chain(sounds.values())
                .all(|p| p.reverbs == reverbs),
            "field packages disagree on their shared studio effects"
        );
        Studio::new(reverbs)?;
        Ok(Self {
            music,
            sounds,
            voices,
            voice_gains: manifest.voice_gains.try_into().unwrap(),
            reverbs,
        })
    }
    fn session(self) -> (FieldSource, Control) {
        let (send, receive) = mpsc::sync_channel(512);
        let error = Arc::new(Mutex::new(None));
        let rendered = Arc::new(AtomicU64::new(0));
        let completions = Arc::new(Mutex::new(VecDeque::new()));
        let control = Control {
            voice_requests: Arc::new(Mutex::new(VecDeque::new())),
            completions: completions.clone(),
            send,
            error: error.clone(),
            rendered: rendered.clone(),
            movie_active: false,
            stereo: true,
            levels: [127; 3],
            in_field: true,
        };
        (
            FieldSource {
                completions,
                assets: Arc::new(self),
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
    voice_requests: VoiceRequests,
    completions: Completions,
    send: SyncSender<Message>,
    error: Arc<Mutex<Option<String>>>,
    rendered: Arc<AtomicU64>,
    movie_active: bool,
    stereo: bool,
    levels: [u8; 3],
    in_field: bool,
}
type Completions = Arc<Mutex<VecDeque<(u64, Arc<AtomicBool>)>>>;
type VoiceRequests = Arc<Mutex<VecDeque<(u32, Arc<AtomicBool>)>>>;

impl resonance_game::dialogue::VoiceFeedback for Control {
    fn begin(&self, resource: u32) -> Arc<AtomicBool> {
        let complete = Arc::new(AtomicBool::new(false));
        let mut requests = self
            .voice_requests
            .lock()
            .expect("voice request queue poisoned");
        assert!(requests.len() < 64, "unconsumed dialogue voice requests");
        requests.push_back((resource, complete.clone()));
        complete
    }
}

enum Message {
    LeaveField,
    EnterField(Arc<Assets>),
    Voice(u32, Arc<AtomicBool>),
    Script(AudioCommand),
    Movie(bool),
    Stereo(bool),
    Levels([u8; 3]),
}

/// Optional development evidence; the normal player retains no event log.
#[derive(Resource, Default)]
pub(super) struct Trace(pub Vec<serde_json::Value>);
impl Control {
    fn enqueue(&self, message: Message) -> Result<()> {
        self.send
            .try_send(message)
            .context("field audio queue is full or stopped")
    }
    fn leave_field(&mut self) -> Result<()> {
        if self.in_field {
            self.enqueue(Message::LeaveField)?;
            self.voice_requests
                .lock()
                .expect("voice request queue poisoned")
                .clear();
            self.in_field = false;
        }
        Ok(())
    }
    fn enter_field(&mut self, assets: Assets) -> Result<()> {
        self.enqueue(Message::EnterField(Arc::new(assets)))?;
        self.in_field = true;
        Ok(())
    }
    pub(super) fn settings(&self) -> serde_json::Value {
        serde_json::json!({"stereo":self.stereo,"levels":self.levels})
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
        self.check()?;
        if let AudioCommand::Voice(id) = command {
            let mut requests = self
                .voice_requests
                .lock()
                .expect("voice request queue poisoned");
            if requests
                .front()
                .is_some_and(|(resource, _)| *resource == id)
            {
                let (_, complete) = requests.pop_front().unwrap();
                return self.enqueue(Message::Voice(id, complete));
            }
        }
        self.enqueue(Message::Script(command))
    }
    fn movie(&mut self, active: bool) -> Result<()> {
        if active != self.movie_active {
            // Scores keep advancing while muted; movie completion restores their gain over two seconds.
            self.enqueue(Message::Movie(active))?;
            self.movie_active = active;
        }
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        let error = self
            .error
            .lock()
            .map_err(|_| anyhow::anyhow!("audio error lock poisoned"))?;
        ensure!(
            error.is_none(),
            "field audio failed: {}",
            error.as_deref().unwrap_or_default()
        );
        Ok(())
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
    looping: bool,
    controls: LiveControls,
    samples: [BusFrame; 160],
    cursor: usize,
    length: usize,
    slot: Option<u16>,
    volume: f32,
}
impl ScorePlayer {
    fn new(package: Arc<Loaded>, looping: bool) -> Result<Self> {
        // One-shot music has no repeat traversal, just like a sound cue.
        let looping = looping && !package.score.loop_events.is_empty();
        Ok(Self {
            stream: Stream::new(package, looping)?,
            looping,
            controls: LiveControls::default(),
            samples: [[[0; 2]; 3]; 160],
            cursor: 0,
            length: 0,
            slot: None,
            volume: 1.,
        })
    }
    fn frame(&mut self, gains: impl FnOnce() -> [f32; 5]) -> Result<Option<BusFrame>> {
        if self.cursor == self.length {
            let controls = gains().map(|gain| LiveControls {
                volume: self.volume * gain,
                ..self.controls
            });
            self.length = self.stream.render_block(controls, &mut self.samples)?;
            self.cursor = 0;
            if self.length == 0 {
                return Ok(None);
            }
            self.controls.release = false;
        }
        let frame = self.samples[self.cursor];
        self.cursor += 1;
        Ok(Some(frame))
    }
}
struct Spoken {
    clip: Arc<Clip>,
    frame: u64,
}
impl Spoken {
    fn next(&mut self) -> Option<[f32; 2]> {
        let position = self.frame * u64::from(self.clip.rate);
        let index = (position / u64::from(RATE)) as usize;
        let count = self.clip.pcm.len() / self.clip.channels;
        if index >= count {
            return None;
        }
        let fraction = (position % u64::from(RATE)) as f32 / RATE as f32;
        self.frame += 1;
        Some(std::array::from_fn(|channel| {
            let channel = channel.min(self.clip.channels - 1);
            let a = f32::from(self.clip.pcm[index * self.clip.channels + channel]);
            let b =
                f32::from(self.clip.pcm[(index + 1).min(count - 1) * self.clip.channels + channel]);
            // A centered mono stream uses equal-power stereo gain. Independent
            // Dolphin voice prefixes measure approximately 0.706 per channel.
            let pan = if self.clip.channels == 1 {
                std::f32::consts::FRAC_1_SQRT_2
            } else {
                1.
            };
            (a + (b - a) * fraction) / 32768.0 * pan
        }))
    }
}
pub(super) struct Frames {
    completions: Completions,
    assets: Arc<Assets>,
    receive: Option<Receiver<Message>>,
    error: Arc<Mutex<Option<String>>>,
    rendered: Arc<AtomicU64>,
    music: Option<ScorePlayer>,
    music_id: Option<i16>,
    sounds: Vec<ScorePlayer>,
    studio: Studio,
    voice: Option<Spoken>,
    fade: Fade,
    master: Fade,
    movie_active: bool,
    frame: u64,
    channel: usize,
    output: [f32; 2],
    stereo: bool,
    levels: [u8; 3],
}
impl Frames {
    fn command(&mut self, command: AudioCommand) -> Result<()> {
        match command {
            AudioCommand::Music(id) => {
                let package = (id >= 0)
                    .then(|| {
                        self.assets
                            .music
                            .get(&id)
                            .with_context(|| format!("uncooked field music {id}"))
                    })
                    .transpose()?;
                if self.music_id == (id >= 0).then_some(id) {
                    return Ok(());
                }
                self.music = package
                    .map(|package| ScorePlayer::new(package.clone(), true))
                    .transpose()?;
                self.music_id = (id >= 0).then_some(id);
                // New scores fade from silence to the user’s volume over 100 ms,
                // independently of the previous track’s scripted fade.
                self.fade = Fade::new(0.0, 1.0, 100)?;
            }
            AudioCommand::MusicVolume {
                volume,
                duration_ticks,
            } => {
                ensure!(volume < 128, "invalid music volume");
                // Convert updates to milliseconds in single precision, then round to even.
                // A one-update fade lasts 17 ms.
                let milliseconds = (duration_ticks as f32 * (1000.0 / 60.0)).round_ties_even();
                let duration =
                    u16::try_from(milliseconds as u64).context("music fade is too long")?;
                self.fade = Fade::to_control(self.fade.value(), volume, duration)?;
            }
            AudioCommand::Sound {
                id,
                pan,
                volume,
                slot,
            } => {
                ensure!(
                    pan < 128 && volume < 128 && self.sounds.len() < 64,
                    "invalid cue controls or exhausted voice budget"
                );
                if let Some(slot) = slot {
                    self.release(u16::from(slot));
                }
                let mut sound = ScorePlayer::new(
                    self.assets
                        .sounds
                        .get(&id)
                        .with_context(|| format!("uncooked field sound {id}"))?
                        .clone(),
                    false,
                )?;
                sound.volume = f32::from(volume) / 127.0;
                sound.controls = LiveControls {
                    pan: Some(pan),
                    ..Default::default()
                };
                sound.slot = slot.map(u16::from);
                self.sounds.push(sound);
            }
            AudioCommand::StopSound(slot) => self.release(slot),
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
            AudioCommand::Voice(id) => {
                self.voice = Some(Spoken {
                    clip: self
                        .assets
                        .voices
                        .get(&id)
                        .with_context(|| format!("uncooked spoken line {id:#x}"))?
                        .clone(),
                    frame: 0,
                })
            }
            AudioCommand::StopVoice => self.voice = None,
            AudioCommand::SelectBank(bank) => {
                ensure!(bank == 0 || bank == 2, "uncooked event sound bank {bank}")
            }
        }
        Ok(())
    }
    fn release(&mut self, slot: u16) {
        for sound in self.sounds.iter_mut().filter(|s| s.slot == Some(slot)) {
            sound.controls.release = true;
            sound.slot = None;
        }
    }
    fn frame(&mut self) -> Result<Option<[f32; 2]>> {
        if self.receive.is_none() {
            return Ok(None);
        }
        loop {
            match self.receive.as_ref().unwrap().try_recv() {
                Ok(Message::LeaveField) => {
                    self.sounds.clear();
                    self.voice = None;
                    self.completions
                        .lock()
                        .expect("voice completion queue poisoned")
                        .clear();
                }
                Ok(Message::EnterField(assets)) => {
                    if self.assets.reverbs != assets.reverbs {
                        self.studio = Studio::new(assets.reverbs)?;
                    }
                    self.assets = assets;
                }
                Ok(Message::Script(command)) => self.command(command)?,
                Ok(Message::Voice(id, complete)) => {
                    self.command(AudioCommand::Voice(id))?;
                    let clip = &self.voice.as_ref().unwrap().clip;
                    let frames = (clip.pcm.len() as u64 / clip.channels as u64 * u64::from(RATE))
                        .div_ceil(u64::from(clip.rate));
                    let mut completions = self
                        .completions
                        .lock()
                        .expect("voice completion queue poisoned");
                    ensure!(completions.len() < 64, "voice completion queue full");
                    completions.push_back((self.frame + frames, complete));
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
                    self.receive = None;
                    self.music = None;
                    self.sounds.clear();
                    self.voice = None;
                    return Ok(None);
                }
            }
        }
        let mut buses = [[0; 2]; 3];
        let master = self.master;
        let fade = self.fade;
        let source_frame = self.frame;
        let [music_level, effects_level, _] = self.levels.map(|v| f32::from(v) * (1. / 127.));
        let voice_level = self.assets.voice_gains[usize::from(self.levels[2])];
        if let Some(music) = &mut self.music {
            music.controls.mono = !self.stereo;
            if let Some(frame) = music
                .frame(|| block_gains(source_frame, master, Some(fade)).map(|v| v * music_level))?
            {
                buses = frame;
            } else {
                ensure!(!music.looping, "looping field music ended unexpectedly");
                self.music = None;
                self.music_id = None;
            }
        }
        if self.frame.is_multiple_of(160) {
            self.fade.advance_block();
            self.master.advance_block();
        }
        let mut index = 0;
        while index < self.sounds.len() {
            let sound = &mut self.sounds[index];
            sound.controls.mono = !self.stereo;
            if let Some(frame) = sound
                .frame(|| block_gains(source_frame, master, None).map(|v| v * effects_level))?
            {
                for (bus, source) in buses.iter_mut().zip(frame) {
                    for (target, sample) in bus.iter_mut().zip(source) {
                        *target += sample;
                    }
                }
            } else {
                self.sounds.remove(index);
                continue;
            }
            index += 1;
        }
        // Speech resumes immediately after a movie, while music and effects fade in.
        // Mix scores and effects through one shared studio so track changes preserve
        // reverb tails. Keep wide PCM until the final mix to avoid early clipping.
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
                self.voice = None;
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

/// Known envelope values at the score worker's next five millisecond controls.
/// Sampling copies preserves the mixer's real clock and cannot predict a future
/// script command. Such requests still arrive through the ordinary bounded queue.
fn block_gains(start: u64, mut master: Fade, mut music: Option<Fade>) -> [f32; 5] {
    std::array::from_fn(|millisecond| {
        let value = master.value() * music.as_ref().map_or(1., Fade::value);
        let frame = start + millisecond as u64 * 32;
        if frame.next_multiple_of(160) < frame + 32 {
            master.advance_block();
            if let Some(music) = &mut music {
                music.advance_block();
            }
        }
        value
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
                    error!("Field audio: {error:#}");
                    if let Ok(mut target) = self.error.lock() {
                        *target = Some(format!("{error:#}"));
                    }
                    self.receive = None;
                    self.music = None;
                    self.sounds.clear();
                    self.voice = None;
                    return None;
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
            music_id: None,
            sounds: Vec::new(),
            studio: Studio::new(self.assets.reverbs).expect("validated field studio effects"),
            voice: None,
            fade: Fade::new(1.0, 1.0, 0).unwrap(),
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
            && !super::field_view::ready(world)
    {
        return;
    }
    let result = (|| -> Result<()> {
        if let Some(assets) = world
            .resource_mut::<super::new_game::Session>()
            .audio
            .take()
        {
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
            let control = world.resource::<Control>().clone();
            world
                .resource_mut::<super::new_game::Session>()
                .field
                .voice_feedback = Some(Arc::new(control));
        }
        let commands = std::mem::take(
            &mut world
                .resource_mut::<super::new_game::Session>()
                .field
                .events
                .world
                .audio_commands,
        );
        let field = &world.resource::<super::new_game::Session>().field;
        let preferences = field
            .events
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
        if let Some(preview) = field
            .menu
            .as_ref()
            .and_then(resonance_game::menu::Menu::preferences)
        {
            levels[0] = preview.volumes.music;
        }
        world.resource_mut::<Control>().stereo(stereo)?;
        world.resource_mut::<Control>().levels(levels)?;
        let session = world.resource::<super::new_game::Session>();
        let movie_active =
            session.movie_owns_audio() || session.field.events.world.blocked_by_movie();
        world.resource_mut::<Control>().movie(movie_active)?;
        if world.contains_resource::<Trace>() {
            let tick = world
                .resource::<super::new_game::Session>()
                .field
                .events
                .tick();
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
    })();
    if let Err(error) = result {
        error!("Field audio adapter failed: {error:#}");
        world
            .resource_mut::<super::new_game::Session>()
            .field
            .events
            .cancel();
        world.write_message(AppExit::error());
    }
}

/// Field effects and dialogue end at the boundary; music and its clock survive.
pub(super) fn leave_field(world: &mut World) -> Result<()> {
    if let Some(mut control) = world.get_resource_mut::<Control>() {
        control.leave_field()?;
    }
    Ok(())
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
        control
            .completions
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_assets() -> Assets {
        Assets {
            reverbs: [[0.5, 0.5, 1., 0.5, 0.]; 2],
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [1.; 128],
        }
    }

    #[test]
    #[ignore = "requires cooked Iselia fields; no window or audio device"]
    fn field_handoffs_preserve_music_pcm_and_retire_scene_voices() {
        use resonance_game::dialogue::VoiceFeedback;

        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        );
        let mut samples = Default::default();
        let mut banks = [
            "fields/map-332-audio.json",
            "fields/map-330-audio.json",
            "fields/iselia-classroom-audio.json",
        ]
        .into_iter()
        .map(|manifest| Assets::load_manifest(&root, manifest, None, &mut samples).unwrap())
        .collect::<Vec<_>>();
        // A pending line belongs to the old scene even when its ID is reused.
        banks[0].voices.insert(
            1,
            Arc::new(Clip {
                pcm: vec![16384; 1000],
                rate: RATE,
                channels: 1,
            }),
        );
        let (reference, reference_control) = banks[0].clone().session();
        let (source, mut control) = banks[0].clone().session();
        let mut reference = reference.decoder();
        let mut frames = source.decoder();
        reference_control.send(AudioCommand::Music(7)).unwrap();
        control.send(AudioCommand::Music(7)).unwrap();
        let mut audible = false;
        for bank in banks.into_iter().skip(1) {
            // Cross score block boundaries and leave a live reverb tail.
            for _ in 0..RATE + 31 {
                let expected = reference.frame().unwrap().unwrap();
                assert_eq!(frames.frame().unwrap().unwrap(), expected);
                audible |= expected.iter().any(|sample| sample.abs() > 0.001);
            }
            let completion = control.begin(1);
            if frames.assets.voices.contains_key(&1) {
                control.send(AudioCommand::Voice(1)).unwrap();
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
            assert!(control.voice_requests.lock().unwrap().is_empty());
            // Loading can last arbitrarily long without pausing the music.
            for _ in 0..913 {
                assert_eq!(frames.frame().unwrap(), reference.frame().unwrap());
            }
            assert!(frames.voice.is_none() && frames.sounds.is_empty());
            assert!(control.completions.lock().unwrap().is_empty());
            assert!(!completion.load(Ordering::Acquire));
            control.enter_field(bank).unwrap();
            control.send(AudioCommand::Music(7)).unwrap();
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
        let complete = control.begin(voice);
        control.send(AudioCommand::Voice(voice)).unwrap();
        frames.frame().unwrap();
        assert_eq!(frames.voice.as_ref().unwrap().frame, 1);
        assert_eq!(control.completions.lock().unwrap().len(), 1);
        assert!(!complete.load(Ordering::Acquire));
        control.check().unwrap();
        drop(control);
        assert!(frames.frame().unwrap().is_none());
    }

    #[test]
    #[ignore = "requires cooked outdoor music 77; no window or audio device"]
    fn one_shot_music_finishes_and_can_be_requested_again() {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        );
        let music = Arc::new(Package::load(&root, "audio/field-music-77.json").unwrap());
        assert!(music.score.loop_events.is_empty());
        let assets = Assets {
            reverbs: music.reverbs,
            music: [(77, music)].into(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [1.; 128],
        };
        let (source, _control) = assets.session();
        let mut frames = source.decoder();
        frames.command(AudioCommand::Music(77)).unwrap();
        let mut audible = false;
        let mut count = 0;
        while frames.music.is_some() {
            assert!(count < RATE * 120, "one-shot music did not finish");
            let frame = frames.frame().unwrap().expect("mixer stopped");
            audible |= frame.iter().any(|sample| sample.abs() > 0.001);
            count += 1;
        }
        assert!(audible);
        assert_eq!(frames.music_id, None);
        assert!(frames.frame().unwrap().is_some());
        frames.command(AudioCommand::Music(77)).unwrap();
        assert!(frames.music.is_some());
        assert_eq!(frames.music_id, Some(77));
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
    fn score_blocks_preserve_the_fade_change_after_the_first_millisecond() {
        // The envelope advances after the first control job; later millisecond jobs
        // in the block see the new gain, even when a score starts mid-block.
        let master = Fade::new(1., 1., 0).unwrap();
        let mut fade = Fade::to_control(1., 0, 17).unwrap();
        fade.advance_block();
        assert_eq!(
            block_gains(160, master, Some(fade)),
            [1., 0.705_882_3, 0.705_882_3, 0.705_882_3, 0.705_882_3]
        );
        assert_eq!(
            block_gains(128, master, Some(fade)),
            [1., 1., 0.705_882_3, 0.705_882_3, 0.705_882_3]
        );
        assert_eq!(
            fade.value(),
            1.,
            "preview must not advance the real envelope"
        );
    }

    #[test]
    fn script_volume_commands_match_observed_short_and_long_music_fades() {
        let assets = synthetic_assets();
        let (source, _control) = assets.session();
        let mut frames = source.decoder();
        frames
            .command(AudioCommand::MusicVolume {
                volume: 50,
                duration_ticks: 0,
            })
            .unwrap();
        frames
            .command(AudioCommand::MusicVolume {
                volume: 0,
                duration_ticks: 1,
            })
            .unwrap();
        frames.fade.advance_block();
        frames.fade.advance_block();
        // Independent MusicWatcher observation 510, group 23: 17 ms.
        assert_eq!(frames.fade.value(), 0.277_906_42);
        for _ in 0..4 {
            frames.fade.advance_block();
        }
        frames
            .command(AudioCommand::MusicVolume {
                volume: 72,
                duration_ticks: 120,
            })
            .unwrap();
        for _ in 0..3 {
            frames.fade.advance_block();
        }
        // Observation 826: music 82 starts from zero, reaching amount 72
        // over two seconds, even after the preceding full-volume command.
        assert_eq!(frames.fade.value(), 0.002_834_618);
    }

    #[test]
    fn sound_mode_changes_the_mixed_channels_without_restarting_sources() {
        let mut voice_gains = [1.; 128];
        voice_gains[0] = 0.;
        voice_gains[64] = 0.25;
        let assets = Assets {
            voice_gains,
            voices: [(
                1,
                Arc::new(Clip {
                    pcm: [16384, -8192].repeat(1000),
                    rate: RATE,
                    channels: 2,
                }),
            )]
            .into(),
            ..synthetic_assets()
        };
        let (source, mut control) = assets.session();
        let mut frames = source.decoder();
        control.send(AudioCommand::Voice(1)).unwrap();
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
            voices: [(
                1,
                Arc::new(Clip {
                    pcm: vec![16384; 100000],
                    rate: RATE,
                    channels: 1,
                }),
            )]
            .into(),
            ..synthetic_assets()
        };
        let (source, mut control) = assets.session();
        let mut frames = source.decoder();
        control.movie(true).unwrap();
        control.send(AudioCommand::Voice(1)).unwrap();
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
