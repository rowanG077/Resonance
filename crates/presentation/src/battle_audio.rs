//! Battle cue adapter on the existing field mixer and audible-frame feedback.
//! Owns voice priority, replacement, and completion independently of gameplay.
use super::*;
#[path = "battle_audio/gains.rs"]
mod gains;
#[path = "battle_audio/pcm.rs"]
mod pcm;
use resonance_battle::{ActorId, Cue, Sound, VoicePriority};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VoiceId(u64);
use resonance_content::{
    battle_audio::{Audio, spatial_controls},
    diagnostics::Diagnostics,
    prepared::Files,
};

const DEFAULT_EFFECT_SPATIAL: [f32; 5] = [320., 5., 64., 0., 127.];
const DEFAULT_VOICE_SPATIAL: [f32; 5] = [320., 5., 64., 24., 104.];

#[derive(Clone)]
pub(crate) struct Assets {
    diagnostics: Diagnostics,
    mixer: Arc<super::Assets>,
    effect_spatial: [f32; 5],
    voice_spatial: [f32; 5],
}
impl Assets {
    pub(crate) fn load(
        files: &Files,
        descriptor: Option<&Audio>,
        cache: &mut Cache,
    ) -> Result<Arc<Self>> {
        let diagnostics = files.diagnostics().clone();
        let Some(descriptor) = descriptor else {
            return Ok(Arc::new(Self {
                mixer: Arc::new(super::Assets::silent(diagnostics.clone())),
                diagnostics,
                effect_spatial: DEFAULT_EFFECT_SPATIAL,
                voice_spatial: DEFAULT_VOICE_SPATIAL,
            }));
        };
        let effect_spatial = diagnostics
            .attempt(
                "battle effect position",
                spatial_controls(descriptor.effect_spatial),
            )?
            .unwrap_or(DEFAULT_EFFECT_SPATIAL);
        let voice_spatial = diagnostics
            .attempt(
                "battle voice position",
                spatial_controls(descriptor.voice_spatial),
            )?
            .unwrap_or(DEFAULT_VOICE_SPATIAL);
        let mixer = Arc::new(super::Assets::load_contents(
            &descriptor.assets,
            diagnostics.clone(),
            cache,
            &mut |asset, limit, role| {
                let entry = descriptor.files.get(&asset.path).with_context(|| {
                    format!("battle audio resource outside inventory: {}", asset.path)
                })?;
                ensure!(
                    entry.sha256 == asset.sha256
                        && entry.roles.contains(&role)
                        && !entry.roles.contains(&AudioRole::Movie),
                    "battle audio dependency binding differs: {}",
                    asset.path
                );
                let bytes = files.read_verified(&asset.path, &entry.sha256, limit)?;
                ensure!(
                    bytes.len() as u64 == entry.bytes,
                    "battle audio dependency size differs: {}",
                    asset.path
                );
                Ok(bytes)
            },
        )?);
        Ok(Arc::new(Self {
            diagnostics,
            mixer,
            effect_spatial,
            voice_spatial,
        }))
    }

    /// Admit available sounds once; missing optional audio is omitted in tolerant mode.
    pub(crate) fn bind(&self, sound: Sound) -> Result<Option<Sound>> {
        self.diagnostics
            .attempt("battle audio binding", self.sound(sound).map(|_| sound))
    }

    fn sound(&self, sound: Sound) -> Result<Source> {
        match sound {
            Sound::Cue(index) => Ok(Source::Score(
                self.mixer
                    .sounds
                    .get(&i16::try_from(index)?)
                    .with_context(|| format!("unprepared battle sound {index}"))?
                    .clone(),
            )),
            Sound::Stream(index) => Ok(Source::Stream(
                self.mixer
                    .voices
                    .get(&u32::from(index))
                    .with_context(|| format!("unprepared battle stream {index}"))?
                    .clone(),
            )),
        }
    }
}

pub(super) enum Source {
    Score(Arc<Loaded>),
    Stream(Arc<Clip>),
}
#[derive(Clone, Copy)]
pub(crate) struct Settings {
    pub music: u8,
    pub effects: u8,
    pub battle_effects: u8,
    pub voice: u8,
    pub stereo: bool,
}
impl Settings {
    fn validate(self) -> Result<()> {
        ensure!(
            [self.music, self.effects, self.battle_effects, self.voice]
                .iter()
                .all(|&v| v < 128),
            "invalid battle volume setting"
        );
        Ok(())
    }
}

/// Insert this resource while battle owns the shared mixer. Existing live and
/// device-free sinks both acknowledge completion through field_audio::acknowledge.
#[derive(Resource)]
pub(crate) struct Playback {
    control: Control,
    assets: Arc<Assets>,
    voices: Vec<PlayingVoice>,
    next_voice: u64,
    handoff: Handoff,
}
struct PlayingVoice {
    actor: ActorId,
    playback: VoiceId,
    priority: VoicePriority,
    complete: Arc<AtomicBool>,
}
#[derive(Clone, Copy)]
enum Handoff {
    Beginning {
        settings: Settings,
        music: Option<i16>,
    },
    Active,
    Ending {
        resume_field: bool,
    },
}
// Diagnostic correlation only. IDs count attempts, not queue order; failed sends
// leave gaps. The mixer receive records define the actual application ordinal.
static NEXT_COMMAND_ID: AtomicU64 = AtomicU64::new(1);

impl Control {
    fn enqueue_battle(&self, command: Command) -> Result<()> {
        let command_id = NEXT_COMMAND_ID.fetch_add(1, Ordering::Relaxed);
        debug!(
            command_id,
            audible_frame = self.rendered_frames(),
            ?command,
            "Battle audio enqueue attempt"
        );
        let result = self.enqueue(Message::Battle(command_id, command));
        debug!(
            command_id,
            accepted = result.is_ok(),
            "Battle audio enqueue result"
        );
        result
    }
}

impl Playback {
    pub(crate) fn begin(
        control: &mut Control,
        assets: Arc<Assets>,
        settings: Settings,
        music: Option<i16>,
    ) -> Result<Self> {
        let diagnostics = &assets.diagnostics;
        let settings = diagnostics
            .attempt(
                "battle audio settings",
                settings.validate().map(|()| settings),
            )?
            .unwrap_or(Settings {
                music: 0,
                effects: 0,
                battle_effects: 0,
                voice: 0,
                stereo: true,
            });
        let music = diagnostics
            .attempt(
                "battle music",
                music
                    .map(|track| {
                        ensure!(
                            assets.mixer.music.contains_key(&track),
                            "unprepared battle music {track}"
                        );
                        Ok(track)
                    })
                    .transpose(),
            )?
            .flatten();
        let mut playback = Self {
            control: control.clone(),
            assets,
            voices: vec![],
            next_voice: 0,
            handoff: Handoff::Beginning { settings, music },
        };
        playback.retry(control)?;
        Ok(playback)
    }
    pub(super) fn returning(&self) -> bool {
        matches!(self.handoff, Handoff::Ending { .. })
    }
    /// Retry full queues; a stopped decoder cannot accept another handoff.
    /// True means End was admitted or abandoned and the owner can retire.
    pub(crate) fn retry(&mut self, control: &mut Control) -> Result<bool> {
        let (command, in_field) = match self.handoff {
            Handoff::Beginning { settings, music } => {
                (Command::Begin(self.assets.clone(), settings, music), false)
            }
            Handoff::Active => return Ok(false),
            Handoff::Ending { resume_field } => (Command::End(resume_field), resume_field),
        };
        if let Err(error) = control
            .check()
            .and_then(|()| control.enqueue_battle(command))
        {
            let full = matches!(
                error.downcast_ref::<mpsc::TrySendError<Message>>(),
                Some(mpsc::TrySendError::Full(_))
            );
            self.assets
                .diagnostics
                .report("battle audio handoff", error)?;
            if full {
                return Ok(false);
            }
            self.voices.clear();
            // End retires immediately when the sink has stopped.
        }
        control.in_field = in_field;
        if self.returning() {
            return Ok(true);
        }
        self.handoff = Handoff::Active;
        Ok(false)
    }
    fn enqueue(&self, command: Command) -> Result<()> {
        ensure!(
            matches!(self.handoff, Handoff::Active),
            "battle audio handoff is pending"
        );
        self.control.enqueue_battle(command)
    }
    pub(crate) fn dispatch(
        &mut self,
        cues: &[Cue],
        mut screen_x: impl FnMut([f32; 3]) -> Option<f32>,
    ) -> Result<()> {
        let healthy = self
            .assets
            .diagnostics
            .attempt("battle audio mixer", self.control.check())?
            .is_some();
        if healthy {
            self.voices
                .retain(|voice| !voice.complete.load(Ordering::Acquire));
        } else {
            self.voices.clear();
        }
        for (cue_ordinal, cue) in cues.iter().enumerate() {
            let _cue = debug_span!("battle_audio_cue", cue_ordinal).entered();
            let result = (|| -> Result<()> {
                ensure!(healthy, "battle audio sink is unavailable");
                let command = match *cue {
                    Cue::GlobalSound { sound, priority } => {
                        let Source::Score(score) = self.assets.sound(sound)? else {
                            anyhow::bail!("effect requires a cue program");
                        };
                        Command::Sound {
                            category: EffectCategory::Battle,
                            binding: sound,
                            score,
                            x: None,
                            priority,
                        }
                    }
                    Cue::Sound {
                        sound,
                        position,
                        priority,
                        ..
                    } => {
                        let Source::Score(score) = self.assets.sound(sound)? else {
                            anyhow::bail!("effect requires a cue program");
                        };
                        let x = screen_x(position);
                        spatial(x, self.assets.effect_spatial)?;
                        Command::Sound {
                            category: EffectCategory::Battle,
                            binding: sound,
                            score,
                            x,
                            priority,
                        }
                    }
                    Cue::Voice {
                        actor,
                        priority,
                        sound,
                        position,
                        centered,
                    } => {
                        if self
                            .voices
                            .iter()
                            .any(|voice| voice.actor == actor && voice.priority > priority)
                        {
                            return Ok(());
                        }
                        let source = self.assets.sound(sound)?;
                        let x = if centered { None } else { screen_x(position) };
                        spatial(x, self.assets.voice_spatial)?;
                        let playback = VoiceId(self.next_voice);
                        self.next_voice += 1;
                        let complete = Arc::new(AtomicBool::new(false));
                        self.enqueue(Command::Voice {
                            actor: actor.index(),
                            playback,
                            binding: sound,
                            source,
                            x,
                            complete: complete.clone(),
                        })?;
                        self.voices.retain(|voice| voice.actor != actor);
                        self.voices.push(PlayingVoice {
                            actor,
                            playback,
                            priority,
                            complete,
                        });
                        return Ok(());
                    }
                    Cue::StopVoice { actor } => {
                        if let Some(index) =
                            self.voices.iter().position(|voice| voice.actor == actor)
                        {
                            self.enqueue(Command::Stop(self.voices[index].playback))?;
                            self.voices.remove(index);
                        }
                        return Ok(());
                    }
                    _ => return Ok(()),
                };
                self.enqueue(command)
            })();
            self.assets
                .diagnostics
                .attempt("battle audio cue", result)?;
        }
        Ok(())
    }
    pub(crate) fn music(&self, track: Option<i16>, fade_ms: u16) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle music",
            (|| {
                if let Some(track) = track {
                    ensure!(
                        self.assets.mixer.music.contains_key(&track),
                        "unprepared battle music {track}"
                    );
                }
                self.enqueue(Command::Music(track, fade_ms))
            })(),
        )?;
        Ok(())
    }
    /// Pause streamed voices while score effects and music continue on their existing mixer
    /// clocks.
    pub(crate) fn pause_voice_streams(&self, paused: bool) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle voice stream pause",
            self.enqueue(Command::PauseVoiceStreams(paused)),
        )?;
        Ok(())
    }
    pub(crate) fn menu_cue(&self, index: u16) -> Result<()> {
        self.menu_sound(index, EffectCategory::Battle, 1)
    }
    /// System feedback takes precedence over combat effects in the shared voice pool.
    pub(crate) fn system_cue(&self, index: u16) -> Result<()> {
        self.menu_sound(index, EffectCategory::System, 0)
    }
    fn menu_sound(&self, index: u16, category: EffectCategory, priority: u8) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle menu sound",
            (|| {
                let binding = Sound::Cue(index);
                let Source::Score(score) = self.assets.sound(binding)? else {
                    anyhow::bail!("menu sound requires a score");
                };
                self.enqueue(Command::Sound {
                    category,
                    binding,
                    score,
                    x: None,
                    priority,
                })
            })(),
        )?;
        Ok(())
    }
    pub(crate) fn finish(&mut self, control: &mut Control, resume_field: bool) -> Result<bool> {
        if resume_field && matches!(self.handoff, Handoff::Beginning { .. }) {
            // Begin never entered the queue, so the field can keep playing.
            return Ok(true);
        }
        self.handoff = Handoff::Ending { resume_field };
        self.retry(control)
    }
}

pub(super) enum Command {
    Begin(Arc<Assets>, Settings, Option<i16>),
    Sound {
        category: EffectCategory,
        binding: Sound,
        score: Arc<Loaded>,
        x: Option<f32>,
        priority: u8,
    },
    Voice {
        actor: usize,
        playback: VoiceId,
        binding: Sound,
        source: Source,
        x: Option<f32>,
        complete: Arc<AtomicBool>,
    },
    PauseVoiceStreams(bool),
    Stop(VoiceId),
    Music(Option<i16>, u16),
    End(bool),
}
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Begin(_, settings, music) => f
                .debug_struct("Begin")
                .field("track", music)
                .field("music", &settings.music)
                .field("effects", &settings.effects)
                .field("battle_effects", &settings.battle_effects)
                .field("voice", &settings.voice)
                .field("stereo", &settings.stereo)
                .finish(),
            Self::Sound {
                category,
                binding,
                x,
                priority,
                ..
            } => f
                .debug_struct("Sound")
                .field("category", category)
                .field("binding", binding)
                .field("x", x)
                .field("priority", priority)
                .finish(),
            Self::Voice {
                actor,
                playback,
                binding,
                x,
                ..
            } => f
                .debug_struct("Voice")
                .field("actor", actor)
                .field("playback", playback)
                .field("binding", binding)
                .field("x", x)
                .finish(),
            Self::PauseVoiceStreams(paused) => {
                f.debug_tuple("PauseVoiceStreams").field(paused).finish()
            }
            Self::Stop(playback) => f.debug_tuple("Stop").field(playback).finish(),
            Self::Music(track, fade_ms) => {
                f.debug_tuple("Music").field(track).field(fade_ms).finish()
            }
            Self::End(resume) => f.debug_tuple("End").field(resume).finish(),
        }
    }
}

struct FieldMusic {
    assets: Arc<super::Assets>,
    music: Option<Music>,
    levels: [u8; 3],
    stereo: bool,
    reverb: [f32; 5],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EffectCategory {
    Battle,
    System,
}

#[allow(clippy::large_enum_variant)] // Keep score admission inline in the audio callback.
enum VoiceSource {
    Score(ScorePlayer),
    Stream {
        gain: [[f32; 2]; 3],
        sampler: pcm::Sampler,
        tail: Option<[[i16; 2]; 3]>,
    },
}
impl VoiceSource {
    fn frame(&mut self, buses: &mut BusFrame) -> bool {
        match self {
            VoiceSource::Score(player) => {
                if let Some(source) = player.frame() {
                    add(buses, source);
                    true
                } else {
                    false
                }
            }
            VoiceSource::Stream {
                gain,
                sampler,
                tail,
            } => {
                if let Some(source) = sampler.sample() {
                    *tail = Some(gains::add(buses, source, *gain));
                    true
                } else {
                    false
                }
            }
        }
    }

    fn break_stream(&mut self, synth: &Synthesizer, queued_frames: u64) {
        if let Self::Stream { tail, .. } = self
            && let Some(products) = tail.take()
        {
            synth.release(products.map(|bus| bus.map(i32::from)), queued_frames);
        }
    }
}
struct Voice {
    actor: usize,
    playback: VoiceId,
    source: VoiceSource,
    submitted_until: u64,
    complete: Arc<AtomicBool>,
}
impl Voice {
    fn submitted_until(&self) -> u64 {
        match &self.source {
            VoiceSource::Score(player) => player.stream.submitted_until(),
            VoiceSource::Stream { .. } => self.submitted_until,
        }
    }
}
pub(super) struct State {
    assets: Arc<Assets>,
    field: FieldMusic,
    settings: Settings,
    effects: Vec<ScorePlayer>,
    voices: Vec<Voice>,
    stop_music: bool,
    voice_streams_paused: bool,
}
impl Frames {
    pub(super) fn battle_command(&mut self, command_id: u64, command: Command) -> Result<()> {
        let _command = debug_span!("battle_audio_apply", command_id).entered();
        debug!(
            source_frame = self.frame,
            ?command,
            "Battle audio command received"
        );
        let diagnostics = match &command {
            Command::Begin(assets, _, _) => assets.diagnostics.clone(),
            _ => self.battle.as_ref().map_or_else(
                || self.assets.diagnostics.clone(),
                |state| state.assets.diagnostics.clone(),
            ),
        };
        let completion = match &command {
            Command::Voice { complete, .. } => Some(complete.clone()),
            _ => None,
        };
        let result = self.apply_battle_command(command);
        debug!(source_frame = self.frame, succeeded = result.is_ok(), error = ?result.as_ref().err(),
            "Battle audio command result");
        if diagnostics
            .attempt("battle mixer command", result)?
            .is_none()
            && let Some(complete) = completion
        {
            complete.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn apply_battle_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Begin(assets, settings, music) => {
                ensure!(self.battle.is_none(), "battle audio already active");
                self.initialize_studio(&assets.mixer)?;
                if let Some(music) = &mut self.music {
                    music.player.stream.pause(true)?;
                }
                let field = FieldMusic {
                    assets: self.assets.clone(),
                    music: self.music.take(),
                    levels: self.levels,
                    stereo: self.stereo,
                    reverb: self.studio.music_reverb(),
                };
                self.sounds.clear();
                self.stop_voice()?;
                self.assets = assets.mixer.clone();
                self.levels = [settings.music, 127, 127];
                self.stereo = settings.stereo;
                self.battle = Some(State {
                    assets,
                    field,
                    settings,
                    effects: vec![],
                    voices: vec![],
                    stop_music: false,
                    voice_streams_paused: false,
                });
                if let Some(music) = music {
                    self.command(AudioCommand::Music(MusicCommand::Play(u16::try_from(
                        music,
                    )?)))?;
                }
                debug!(source_frame = self.frame, "Battle mixer ownership began");
            }
            Command::End(resume) => {
                if !resume && self.battle.is_none() {
                    // A cancelled pending Begin still needs to retire the field
                    // sources when its destination will not resume that field.
                    self.music = None;
                    self.sounds.clear();
                    self.stop_voice()?;
                    return Ok(());
                }
                let mut state = self.battle.take().context("battle audio is not active")?;
                for voice in &mut state.voices {
                    voice.source.break_stream(
                        &self.synth,
                        voice.submitted_until.saturating_sub(self.frame),
                    );
                }
                self.initialize_studio(&state.field.assets)?;
                self.music = None;
                self.assets = state.field.assets;
                self.levels = state.field.levels;
                self.stereo = state.field.stereo;
                if resume {
                    self.music = state.field.music;
                    self.studio.set_music_reverb(state.field.reverb)?;
                    if let Some(music) = &mut self.music {
                        music.player.stream.pause(false)?;
                    }
                }
                debug!(
                    source_frame = self.frame,
                    resume, "Battle mixer ownership ended"
                );
            }
            Command::Music(track, fade) => {
                let state = self.battle.as_ref().context("battle audio is not active")?;
                if let Some(track) = track {
                    ensure!(
                        state.assets.mixer.music.contains_key(&track),
                        "unprepared battle music {track}"
                    );
                    if self.music.as_ref().is_some_and(|music| music.id == track)
                        && !state.stop_music
                    {
                        return Ok(());
                    }
                    self.command(AudioCommand::Music(MusicCommand::Play(u16::try_from(
                        track,
                    )?)))?;
                    self.music.as_mut().unwrap().fade = Fade::new(0., 1., u64::from(fade))?;
                } else if let Some(music) = &mut self.music {
                    music.fade = Fade::new(music.fade.value(), 0., u64::from(fade))?;
                }
                self.battle.as_mut().unwrap().stop_music = track.is_none();
                debug!(
                    source_frame = self.frame,
                    ?track,
                    fade_ms = fade,
                    "Battle music command applied"
                );
            }
            Command::Sound {
                category,
                binding,
                score,
                x,
                priority,
            } => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                let (pan, percent) = spatial(x, state.assets.effect_spatial)?;
                let volume = if x.is_some() {
                    state.settings.battle_effects
                } else {
                    state.settings.effects
                };
                let mut player = ScorePlayer::new(score, false, &self.synth)?;
                player.controls.pan = Some(pan);
                // Battle importance is lower-first; native allocation is higher-first.
                // System feedback occupies the next priority tier, with no reserved slots.
                player.controls.priority = Some(match category {
                    EffectCategory::Battle => u16::from(u8::MAX - priority),
                    EffectCategory::System => u16::from(u8::MAX) + 1,
                });
                player.volume = f32::from(u16::from(volume) * u16::from(percent) / 100) / 127.;
                state.effects.push(player);
                debug!(
                    source_frame = self.frame,
                    ?binding,
                    ?category,
                    pan,
                    level = u16::from(volume) * u16::from(percent) / 100,
                    priority,
                    "Battle sound command applied"
                );
            }
            Command::Voice {
                actor,
                playback,
                binding,
                source,
                x,
                complete,
            } => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                let (pan, percent) = spatial(x, state.assets.voice_spatial)?;
                let level = (u16::from(state.settings.voice) * u16::from(percent) / 100) as u8;
                let source = match source {
                    Source::Score(score) => {
                        let mut player = ScorePlayer::new(score, false, &self.synth)?;
                        player.volume = f32::from(level) / 127.;
                        player.controls.pan = Some(pan);
                        VoiceSource::Score(player)
                    }
                    Source::Stream(clip) => {
                        let gain = gains::for_voice(level, pan, clip.channels);
                        let sampler = pcm::Sampler::new(clip)?;
                        VoiceSource::Stream {
                            gain,
                            sampler,
                            tail: None,
                        }
                    }
                };
                if let Some(index) = state.voices.iter().position(|voice| voice.actor == actor) {
                    state.stop_voice(index, &self.synth, self.frame, &self.completions)?;
                }
                state.voices.push(Voice {
                    actor,
                    playback,
                    source,
                    submitted_until: 0,
                    complete,
                });
                debug!(
                    source_frame = self.frame,
                    actor,
                    ?playback,
                    ?binding,
                    level,
                    pan,
                    "Battle voice command applied"
                );
            }
            Command::PauseVoiceStreams(paused) => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                if state.voice_streams_paused != paused {
                    state.voice_streams_paused = paused;
                    debug!(source_frame = self.frame, paused, "Battle streams paused");
                }
            }
            Command::Stop(playback) => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                if let Some(index) = state.voices.iter().position(|v| v.playback == playback) {
                    state.stop_voice(index, &self.synth, self.frame, &self.completions)?;
                    debug!(
                        source_frame = self.frame,
                        ?playback,
                        "Battle voice stop applied"
                    );
                } else {
                    debug!(
                        source_frame = self.frame,
                        ?playback,
                        "Battle voice stop absent"
                    );
                }
            }
        }
        Ok(())
    }
}
impl State {
    fn stop_voice(
        &mut self,
        index: usize,
        synth: &Synthesizer,
        frame: u64,
        completions: &Completions,
    ) -> Result<()> {
        let mut voice = self.voices.remove(index);
        let submitted_until = voice.submitted_until();
        voice
            .source
            .break_stream(synth, submitted_until.saturating_sub(frame));
        complete(
            completions,
            frame.max(submitted_until),
            voice.complete,
            &self.assets.diagnostics,
        )
    }

    /// Prepare only the PCM that this mixer block will actually submit.
    pub(super) fn prepare_stream_block(
        &mut self,
        block: &mut [BusFrame; BLOCK_FRAMES],
        start: u64,
        synth: &Synthesizer,
    ) {
        for voice in &mut self.voices {
            if self.voice_streams_paused {
                // Decide at the queue boundary, so a cancelled pause never
                // adds a release fade on top of resumed PCM.
                voice.source.break_stream(synth, 0);
            } else if matches!(voice.source, VoiceSource::Stream { .. }) {
                for (offset, buses) in block.iter_mut().enumerate() {
                    if !voice.source.frame(buses) {
                        break;
                    }
                    voice.submitted_until = start + offset as u64 + 1;
                }
            }
        }
    }
    pub(super) fn stop_music(&self) -> bool {
        self.stop_music
    }
    pub(super) fn prepare_shared(&mut self) -> Result<()> {
        let diagnostics = &self.assets.diagnostics;
        let mut index = 0;
        while index < self.effects.len() {
            let player = &mut self.effects[index];
            player.controls.mono = !self.settings.stereo;
            if diagnostics
                .attempt(
                    "battle effect playback",
                    player.prepare_shared(|| [1.; CONTROLS_PER_BLOCK]),
                )?
                .is_some()
            {
                index += 1;
            } else {
                self.effects.remove(index);
            }
        }
        let mut index = 0;
        while index < self.voices.len() {
            let result = match &mut self.voices[index].source {
                VoiceSource::Score(player) => {
                    player.controls.mono = !self.settings.stereo;
                    player.prepare_shared(|| [1.; CONTROLS_PER_BLOCK])
                }
                VoiceSource::Stream { .. } => Ok(()),
            };
            if diagnostics
                .attempt("battle voice playback", result)?
                .is_some()
            {
                index += 1;
            } else {
                self.voices
                    .remove(index)
                    .complete
                    .store(true, Ordering::Release);
            }
        }
        Ok(())
    }
    pub(super) fn frame(
        &mut self,
        buses: &mut BusFrame,
        frame: u64,
        completions: &Completions,
    ) -> Result<()> {
        let diagnostics = &self.assets.diagnostics;
        let mut index = 0;
        while index < self.effects.len() {
            if let Some(source) = self.effects[index].frame() {
                add(buses, source);
                index += 1;
            } else {
                self.effects.remove(index);
            }
        }
        let mut index = 0;
        while index < self.voices.len() {
            let submitted_until = self.voices[index].submitted_until;
            let active = match &mut self.voices[index].source {
                VoiceSource::Stream { sampler, .. } => {
                    !sampler.finished() || frame < submitted_until
                }
                source => source.frame(buses),
            };
            if active {
                index += 1;
            } else {
                debug!(source_frame = frame, actor = self.voices[index].actor,
                        playback = ?self.voices[index].playback, "Battle voice source completed");
                complete(
                    completions,
                    frame,
                    self.voices.remove(index).complete,
                    diagnostics,
                )?;
            }
        }
        Ok(())
    }
}
fn complete(
    queue: &Completions,
    frame: u64,
    token: Arc<AtomicBool>,
    diagnostics: &Diagnostics,
) -> Result<()> {
    let result = (|| {
        let mut queue = queue
            .lock()
            .map_err(|_| anyhow::anyhow!("voice completion queue poisoned"))?;
        queue.push_back((frame, token.clone()));
        Ok(())
    })();
    if diagnostics
        .attempt("battle voice completion", result)?
        .is_none()
    {
        token.store(true, Ordering::Release);
    }
    Ok(())
}

fn spatial(x: Option<f32>, [origin, divisor, center, low, high]: [f32; 5]) -> Result<(u8, u8)> {
    let Some(x) = x else {
        // Sounds without a position use center pan; the configurable center applies only to
        // positioned sounds.
        return Ok((64, 100));
    };
    ensure!(x.is_finite(), "invalid battle audio screen position");
    let percent = (100 - (divisor * ((x - origin).abs() / origin)) as i32).max(90) as u8;
    Ok((
        ((x - origin) / divisor + center).clamp(low, high) as u8,
        percent,
    ))
}
fn add(target: &mut BusFrame, source: BusFrame) {
    for (out, value) in target
        .iter_mut()
        .flatten()
        .zip(source.into_iter().flatten())
    {
        *out += value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_audio::{Audio, PATH};
    const DRY_GAIN: [[f32; 2]; 3] = [[1.; 2], [0.; 2], [0.; 2]];

    fn mixer() -> super::super::Assets {
        super::super::Assets {
            diagnostics: Diagnostics::new(true),
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [1.; 128],
            reverbs: [[0.5, 0.5, 1., 0.5, 0.]; 2],
            music_reverbs: Some(MusicReverbs {
                presets: [[0.5, 0.5, 1., 0.5, 0.]; 2],
                selectors: vec![1; 112],
            }),
        }
    }
    fn assets() -> Arc<Assets> {
        Arc::new(Assets {
            diagnostics: Diagnostics::new(true),
            mixer: Arc::new(mixer()),
            effect_spatial: [320., 5., 64., 0., 127.],
            voice_spatial: [320., 5., 64., 24., 104.],
        })
    }
    fn settings() -> Settings {
        Settings {
            music: 91,
            effects: 87,
            battle_effects: 73,
            voice: 65,
            stereo: false,
        }
    }

    fn fill_command_queue(control: &Control) {
        while control.enqueue(Message::Stereo(true)).is_ok() {}
    }

    #[test]
    fn battle_handoffs_retry_full_queues_without_losing_mixer_ownership() -> Result<()> {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut field = mixer();
            field.diagnostics = diagnostics.clone();
            field.music.insert(0, test_music());
            let (source, mut control) = Arc::new(field).session();
            let mut frames = source.decoder();
            control.send(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
            frames.frame()?;
            let mut prepared = assets();
            let bank = Arc::get_mut(&mut prepared).unwrap();
            bank.diagnostics = diagnostics.clone();
            Arc::get_mut(&mut bank.mixer)
                .unwrap()
                .music
                .insert(1, test_music());
            Arc::get_mut(&mut bank.mixer)
                .unwrap()
                .voices
                .insert(257, Arc::new(test_clip([12000; 3200], RATE, 1)));

            fill_command_queue(&control);
            let beginning = Playback::begin(&mut control, prepared.clone(), settings(), Some(1));
            assert!(control.in_field);
            assert!(frames.battle.is_none());
            assert_eq!(frames.music.as_ref().map(|music| music.id), Some(0));
            let mut playback = if paranoid {
                assert!(beginning.is_err());
                frames.frame()?;
                Playback::begin(&mut control, prepared.clone(), settings(), Some(1))?
            } else {
                let mut playback = beginning?;
                assert!(matches!(playback.handoff, Handoff::Beginning { .. }));
                frames.frame()?;
                assert!(!playback.retry(&mut control)?);
                playback
            };
            frames.frame()?;
            assert!(frames.battle.is_some());
            assert_eq!(frames.music.as_ref().map(|music| music.id), Some(1));
            assert!(!control.in_field);

            fill_command_queue(&control);
            let ending = playback.finish(&mut control, true);
            if paranoid {
                assert!(ending.is_err());
            } else {
                assert!(!ending?);
            }
            assert!(playback.returning());
            assert!(!control.in_field);
            assert!(frames.battle.is_some());
            frames.frame()?;
            assert!(playback.retry(&mut control)?);
            frames.frame()?;
            assert!(control.in_field);
            assert!(frames.battle.is_none());
            assert_eq!(frames.music.as_ref().map(|music| music.id), Some(0));

            let mut next = Playback::begin(&mut control, prepared.clone(), settings(), Some(1))?;
            frames.frame()?;
            assert!(frames.battle.is_some());
            assert_eq!(frames.music.as_ref().map(|music| music.id), Some(1));
            assert!(next.finish(&mut control, true)?);
            frames.frame()?;
            assert!(frames.battle.is_none());

            // Lose an actual decoder while End is waiting for its full queue.
            let cue = || Cue::Voice {
                actor: resonance_battle::ActorId::from_index(0).unwrap(),
                priority: VoicePriority::Action,
                sound: Sound::Stream(257),
                position: [0.; 3],
                centered: true,
            };
            let mut lost = Playback::begin(&mut control, prepared.clone(), settings(), Some(1))?;
            lost.dispatch(&[cue()], |_| None)?;
            frames.frame()?;
            let token = lost.voices[0].complete.clone();
            assert!(!token.load(Ordering::Acquire));
            fill_command_queue(&control);
            let ending = lost.finish(&mut control, true);
            assert!(if paranoid { ending.is_err() } else { !ending? });
            drop(frames);
            assert!(control.check().is_err());
            let retired = lost.retry(&mut control);
            if paranoid {
                assert!(retired.is_err());
                assert!(lost.dispatch(&[], |_| None).is_err());
                assert!(Playback::begin(&mut control, prepared, settings(), None).is_err());
            } else {
                assert!(retired?);
                lost.dispatch(&[], |_| None)?;
                assert!(lost.voices.is_empty());
                assert!(control.in_field);
                // The retained field can complete skipped dialogue, and another
                // encounter still has an adapter for immediate voice feedback.
                let complete = Arc::new(AtomicBool::new(false));
                control.send(AudioCommand::Voice {
                    resource: 257,
                    completion: Some(complete.clone()),
                })?;
                assert!(complete.load(Ordering::Acquire));
                let mut next = Playback::begin(&mut control, prepared, settings(), None)?;
                assert!(matches!(next.handoff, Handoff::Active));
                next.dispatch(&[cue()], |_| None)?;
                assert!(next.voices.is_empty());
                assert!(next.finish(&mut control, true)?);
            }
        }
        Ok(())
    }

    #[test]
    fn battle_begin_and_initial_music_need_only_one_queue_slot() -> Result<()> {
        let (source, mut control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        let mut prepared = assets();
        Arc::get_mut(&mut Arc::get_mut(&mut prepared).unwrap().mixer)
            .unwrap()
            .music
            .insert(1, test_music());
        fill_command_queue(&control);
        frames.receive.as_ref().unwrap().try_recv()?;
        let mut playback = Playback::begin(&mut control, prepared, settings(), Some(1))?;
        assert!(matches!(playback.handoff, Handoff::Active));
        frames.frame()?;
        assert_eq!(frames.music.as_ref().map(|music| music.id), Some(1));
        assert!(playback.finish(&mut control, false)?);
        frames.frame()?;
        assert!(frames.battle.is_none());
        Ok(())
    }

    #[test]
    fn cancelling_pending_begin_preserves_or_retires_the_field_for_its_destination() -> Result<()> {
        for resume in [true, false] {
            let diagnostics = Diagnostics::new(false);
            let mut field = mixer();
            field.diagnostics = diagnostics.clone();
            field.music.insert(0, test_music());
            field
                .voices
                .insert(7, Arc::new(test_clip([16384; 64], RATE, 1)));
            let (source, mut control) = Arc::new(field).session();
            let mut frames = source.decoder();
            control.send(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
            control.send(AudioCommand::voice(7))?;
            frames.frame()?;
            let mut prepared = assets();
            Arc::get_mut(&mut prepared).unwrap().diagnostics = diagnostics;
            fill_command_queue(&control);
            let mut playback = Playback::begin(&mut control, prepared, settings(), None)?;
            assert!(matches!(playback.handoff, Handoff::Beginning { .. }));
            assert_eq!(playback.finish(&mut control, resume)?, resume);
            assert!(control.in_field);
            frames.frame()?;
            if !resume {
                assert!(playback.returning());
                assert!(playback.retry(&mut control)?);
                frames.frame()?;
            }
            assert!(frames.battle.is_none());
            assert_eq!(control.in_field, resume);
            assert_eq!(
                frames.music.as_ref().map(|music| music.id),
                resume.then_some(0)
            );
            assert_eq!(frames.voice.is_some(), resume);
        }
        Ok(())
    }

    #[test]
    fn many_voice_completions_wait_for_each_consumed_frame() -> Result<()> {
        let (_source, control) = Arc::new(mixer()).session();
        let tokens: Vec<_> = (1..=96)
            .map(|frame| {
                let token = Arc::new(AtomicBool::new(false));
                complete(
                    &control.completions,
                    frame,
                    token.clone(),
                    &control.diagnostics,
                )?;
                Ok(token)
            })
            .collect::<Result<_>>()?;
        assert!(tokens.iter().all(|token| !token.load(Ordering::Acquire)));
        control.acknowledge_frames(48);
        for (index, token) in tokens.iter().enumerate() {
            assert_eq!(token.load(Ordering::Acquire), index < 48);
        }
        control.acknowledge_frames(96);
        assert!(tokens.iter().all(|token| token.load(Ordering::Acquire)));
        assert!(control.completions.lock().unwrap().is_empty());
        Ok(())
    }
    fn stream(rate: u32, samples: Vec<i16>, gain: [[f32; 2]; 3]) -> Result<VoiceSource> {
        Ok(VoiceSource::Stream {
            gain,
            sampler: pcm::Sampler::new(Arc::new(test_clip(samples, rate, 1)))?,
            tail: None,
        })
    }

    fn push_stream(frames: &mut Frames, playback: VoiceId, source: VoiceSource) -> Arc<AtomicBool> {
        let complete = Arc::new(AtomicBool::new(false));
        frames.battle.as_mut().unwrap().voices.push(Voice {
            actor: 0,
            playback,
            source,
            submitted_until: 0,
            complete: complete.clone(),
        });
        complete
    }

    #[test]
    fn scene_arbitrates_stereo_voices_and_replaces_only_after_queue_admission() -> Result<()> {
        let (source, mut control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        let mut prepared = assets();
        Arc::get_mut(&mut Arc::get_mut(&mut prepared).unwrap().mixer)
            .unwrap()
            .voices
            .insert(
                1,
                Arc::new(test_clip([12000, -12000].repeat(480), 48000, 2)),
            );
        assert!(prepared.mixer.music.is_empty() && prepared.mixer.sounds.is_empty());
        let mut settings = settings();
        settings.stereo = true;
        let mut playback = Playback::begin(&mut control, prepared, settings, None)?;
        let cue = |priority| Cue::Voice {
            actor: ActorId::from_index(11).unwrap(),
            priority,
            sound: Sound::Stream(1),
            position: [0.; 3],
            centered: true,
        };
        playback.dispatch(&[cue(VoicePriority::Announcement)], |_| None)?;
        let old = playback.voices[0].playback;
        let old_complete = playback.voices[0].complete.clone();
        let first = frames.frame()?.unwrap();
        assert!(first[0] > 0. && first[1] < 0.);
        playback.dispatch(&[cue(VoicePriority::Reaction)], |_| None)?;
        assert_eq!(playback.voices[0].playback, old);
        fill_command_queue(&control);
        assert!(
            playback
                .dispatch(&[cue(VoicePriority::Announcement)], |_| None)
                .is_err()
        );
        assert_eq!(playback.voices[0].playback, old);
        frames.frame()?;
        playback.dispatch(&[cue(VoicePriority::Announcement)], |_| None)?;
        let replacement = playback.voices[0].playback;
        let complete = playback.voices[0].complete.clone();
        assert_ne!(replacement, old);
        frames.frame()?;
        assert_eq!(frames.battle.as_ref().unwrap().voices.len(), 1);
        assert_eq!(
            frames.battle.as_ref().unwrap().voices[0].playback,
            replacement
        );
        for _ in 0..1000 {
            frames.frame()?;
        }
        assert!(frames.battle.as_ref().unwrap().voices.is_empty());
        assert!(!complete.load(Ordering::Acquire));
        control.acknowledge_frames(frames.frame);
        assert!(old_complete.load(Ordering::Acquire));
        assert!(complete.load(Ordering::Acquire));
        playback.dispatch(&[], |_| None)?;
        assert!(playback.voices.is_empty());
        playback.dispatch(&[cue(VoicePriority::Reaction)], |_| None)?;
        assert_eq!(playback.voices.len(), 1);
        playback.dispatch(
            &[Cue::StopVoice {
                actor: ActorId::from_index(11).unwrap(),
            }],
            |_| None,
        )?;
        frames.frame()?;
        assert!(playback.voices.is_empty());
        assert!(frames.battle.as_ref().unwrap().voices.is_empty());
        Ok(())
    }

    #[test]
    fn completion_waits_for_consumed_pcm_including_short_and_muted_clips() -> Result<()> {
        let voice_id = VoiceId(1);
        for rate in [8000, 22050, 32000, 96000] {
            for samples in [32, 12000] {
                let mut ends = Vec::new();
                for gain in [DRY_GAIN, [[0.; 2]; 3]] {
                    let (source, control) = Arc::new(mixer()).session();
                    let mut frames = source.decoder();
                    frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
                    let token = push_stream(
                        &mut frames,
                        voice_id,
                        stream(rate, vec![10000; samples], gain)?,
                    );
                    let bound = (samples as u64 * u64::from(RATE)).div_ceil(u64::from(rate))
                        + u64::from(RATE / 50);
                    let mut last_audible = None;
                    while !frames.battle.as_ref().unwrap().voices.is_empty() {
                        assert!(frames.frame < bound, "voice did not finish");
                        if frames.frame()?.unwrap() != [0.; 2] {
                            last_audible = Some(frames.frame);
                        }
                        assert!(!token.load(Ordering::Acquire));
                    }
                    let end = frames.frame - 1;
                    if gain == DRY_GAIN {
                        assert!(last_audible.is_some_and(|last| last <= end));
                    } else {
                        assert_eq!(last_audible, None);
                    }
                    control.acknowledge_frames(end - 1);
                    assert!(!token.load(Ordering::Acquire));
                    control.acknowledge_frames(end);
                    assert!(token.load(Ordering::Acquire));
                    ends.push(end);
                }
                assert_eq!(ends[0], ends[1], "mute must not change completion");
            }
        }
        Ok(())
    }

    #[test]
    fn stopped_score_voice_waits_for_consumed_queued_pcm() -> Result<()> {
        let playback = VoiceId(1);
        let (source, control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
        let token = Arc::new(AtomicBool::new(false));
        frames.apply_battle_command(Command::Voice {
            actor: 0,
            playback,
            binding: Sound::Cue(1),
            source: Source::Score(test_score()),
            x: None,
            complete: token.clone(),
        })?;
        for _ in 0..73 {
            frames.frame()?;
        }
        frames.apply_battle_command(Command::Stop(playback))?;
        control.acknowledge_frames(73);
        assert!(!token.load(Ordering::Acquire));
        for _ in 73..160 {
            assert!(frames.frame()?.unwrap()[0] > 0.);
        }
        control.acknowledge_frames(159);
        assert!(!token.load(Ordering::Acquire));
        control.acknowledge_frames(160);
        assert!(token.load(Ordering::Acquire));
        Ok(())
    }

    #[test]
    fn pause_retains_cursor_and_filter_without_completing_the_voice() -> Result<()> {
        let voice_id = VoiceId(1);
        let (source, control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
        let samples: Vec<_> = (0..12000).map(|i| (i % 1000) as i16).collect();
        let mut reference = stream(22050, samples.clone(), DRY_GAIN)?;
        let token = push_stream(&mut frames, voice_id, stream(22050, samples, DRY_GAIN)?);
        let mut expected = [[[0; 2]; 3]; BLOCK_FRAMES];
        for bus in &mut expected {
            assert!(reference.frame(bus));
        }
        frames.frame()?;
        assert_eq!(frames.stream_block, expected);
        frames.apply_battle_command(Command::PauseVoiceStreams(true))?;
        frames.apply_battle_command(Command::PauseVoiceStreams(true))?;
        for _ in 1..480 {
            frames.frame()?;
        }
        assert_eq!(frames.stream_block, [[[0; 2]; 3]; BLOCK_FRAMES]);
        control.acknowledge_frames(frames.frame);
        assert!(!token.load(Ordering::Acquire));
        frames.apply_battle_command(Command::PauseVoiceStreams(false))?;
        frames.apply_battle_command(Command::PauseVoiceStreams(false))?;
        expected.fill([[0; 2]; 3]);
        for bus in &mut expected {
            assert!(reference.frame(bus));
        }
        frames.frame()?;
        assert_eq!(frames.stream_block, expected);
        Ok(())
    }

    #[test]
    fn pause_cancelled_before_queued_pcm_drains_matches_uninterrupted_playback() -> Result<()> {
        let voice_id = VoiceId(1);
        let (source, _control) = Arc::new(mixer()).session();
        let (reference, _reference_control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        let mut reference = reference.decoder();
        for player in [&mut frames, &mut reference] {
            player.apply_battle_command(Command::Begin(assets(), settings(), None))?;
            push_stream(
                player,
                voice_id,
                stream(32000, vec![12000; 12000], DRY_GAIN)?,
            );
        }
        for _ in 0..73 {
            assert_eq!(frames.frame()?, reference.frame()?);
        }
        frames.apply_battle_command(Command::PauseVoiceStreams(true))?;
        for _ in 73..101 {
            assert_eq!(frames.frame()?, reference.frame()?);
        }
        frames.apply_battle_command(Command::PauseVoiceStreams(false))?;
        for _ in 101..640 {
            assert_eq!(frames.frame()?, reference.frame()?);
        }
        Ok(())
    }

    #[test]
    fn stop_pause_and_handoff_drain_submitted_pcm_before_releasing_the_tail() -> Result<()> {
        let voice_id = VoiceId(1);
        for command in [
            Command::Stop(voice_id),
            Command::PauseVoiceStreams(true),
            Command::End(false),
        ] {
            let (source, control) = Arc::new(mixer()).session();
            let mut frames = source.decoder();
            frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
            let token = push_stream(
                &mut frames,
                voice_id,
                stream(32000, vec![12000; 12000], DRY_GAIN)?,
            );
            for _ in 0..73 {
                frames.frame()?;
            }
            let submitted = frames.stream_block;
            let stopped = matches!(command, Command::Stop(_));
            let end_battle = matches!(command, Command::End(_));
            frames.apply_battle_command(command)?;
            if end_battle {
                assert!(frames.battle.is_none());
            } else if stopped {
                frames.apply_battle_command(Command::Stop(voice_id))?;
                assert!(frames.battle.as_ref().unwrap().voices.is_empty());
            } else {
                assert_eq!(frames.battle.as_ref().unwrap().voices.len(), 1);
            }
            for bus in submitted.iter().skip(73) {
                assert_eq!(frames.frame()?.unwrap(), bus[0].map(|v| v as f32 / 32768.));
            }
            control.acknowledge_frames(159);
            assert!(!token.load(Ordering::Acquire));
            control.acknowledge_frames(160);
            assert_eq!(token.load(Ordering::Acquire), stopped);
            let tail_start = frames.frame()?.unwrap();
            assert!(tail_start[0] > 0.);
            let mut tail_end = tail_start;
            for _ in 1..160 {
                tail_end = frames.frame()?.unwrap();
            }
            assert!(tail_end[0] < tail_start[0]);
            assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        }
        Ok(())
    }

    #[test]
    fn stale_stop_cannot_touch_a_replacement_and_handoff_drops_its_clip() -> Result<()> {
        let [old_id, new_id] = [VoiceId(1), VoiceId(2)];
        let (source, control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
        let old_done = push_stream(
            &mut frames,
            old_id,
            stream(22050, vec![1234; 18000], DRY_GAIN)?,
        );
        frames.frame()?;
        frames.apply_battle_command(Command::Stop(old_id))?;
        let clip = Arc::new(test_clip(vec![2345; 16000], RATE, 1));
        let weak = Arc::downgrade(&clip);
        let replacement = VoiceSource::Stream {
            sampler: pcm::Sampler::new(clip)?,
            gain: DRY_GAIN,
            tail: None,
        };
        let new_done = push_stream(&mut frames, new_id, replacement);
        frames.apply_battle_command(Command::Stop(old_id))?;
        while frames.frame <= 160 {
            frames.frame()?;
        }
        control.acknowledge_frames(160);
        assert!(old_done.load(Ordering::Acquire));
        assert!(!new_done.load(Ordering::Acquire));
        assert_eq!(frames.battle.as_ref().unwrap().voices[0].playback, new_id);
        frames.apply_battle_command(Command::End(false))?;
        assert!(weak.upgrade().is_none());
        Ok(())
    }

    #[test]
    fn beginning_battle_retires_field_voice_requests() -> Result<()> {
        let mut field = mixer();
        field
            .voices
            .insert(1, Arc::new(test_clip(vec![16384; 1000], RATE, 1)));
        let (source, mut control) = Arc::new(field).session();
        let mut frames = source.decoder();
        control.send(AudioCommand::voice(1))?;
        frames.frame()?;
        assert!(frames.voice.is_some());
        let pending = Arc::new(AtomicBool::new(false));
        control.send(AudioCommand::Voice {
            resource: 1,
            completion: Some(pending.clone()),
        })?;
        let _playback = Playback::begin(&mut control, assets(), settings(), None)?;
        frames.frame()?;
        assert!(frames.voice.is_none());
        control.acknowledge_frames(frames.frame);
        assert!(control.completions.lock().unwrap().is_empty());
        assert!(pending.load(Ordering::Acquire));
        assert!(frames.battle.is_some());
        assert!(!control.in_field);
        Ok(())
    }

    #[test]
    fn ownership_changes_retain_nonzero_studio_history_and_pending_returns() -> Result<()> {
        for resume in [false, true] {
            let field = mixer();
            let mut reference = Studio::new(field.reverbs)?;
            let (source, mut control) = Arc::new(field).session();
            let mut frames = source.decoder();
            for i in 0..6007 {
                let buses = [[0; 2], [1000 + i % 31, -2000], [-3000, 5000 + i % 17]];
                assert_eq!(frames.studio.process(buses), reference.process(buses));
            }
            let mut different = assets();
            Arc::get_mut(&mut Arc::get_mut(&mut different).unwrap().mixer)
                .unwrap()
                .reverbs = [[0.7, 0.7, 2.5, 0.6, 0.05]; 2];
            frames.apply_battle_command(Command::Begin(different, settings(), None))?;
            let mut nonzero = 0;
            for _ in 0..337 {
                let expected = reference.process([[0; 2]; 3]);
                nonzero += usize::from(expected != [0; 2]);
                assert_eq!(frames.studio.process([[0; 2]; 3]), expected);
            }
            frames.apply_battle_command(Command::End(resume))?;
            for _ in 0..337 {
                let expected = reference.process([[0; 2]; 3]);
                nonzero += usize::from(expected != [0; 2]);
                assert_eq!(frames.studio.process([[0; 2]; 3]), expected);
            }
            let mut destination = mixer();
            destination.reverbs = [[0.1, 0.2, 0.3, 0.4, 0.01]; 2];
            control.enter_field(destination)?;
            for _ in 0..337 {
                let expected = reference.process([[0; 2]; 3]);
                nonzero += usize::from(expected != [0; 2]);
                assert_eq!(
                    frames.frame()?.unwrap(),
                    expected.map(|v| (v as f32 / 32768.).clamp(-1., 1.))
                );
            }
            assert!(nonzero > 900, "fixture must retain an audible history");
        }
        Ok(())
    }

    #[test]
    fn battle_return_preserves_scripted_music_gain_and_unfinished_fades() -> Result<()> {
        let (resources, mut score, tables) =
            test_score_data(resonance_audio::data::ScoreOrigin::Sequence);
        score.end_tick = 8;
        score.loop_events = score.first_events.clone();
        let reverbs = [[0., 0., 1., 0., 0.]; 2];
        let track = Arc::new(Loaded::new(resources, score, tables, reverbs)?);
        let mut peaks = Vec::new();
        for (volume, duration_ticks) in [(127, 0), (0, 0), (48, 0), (0, 3)] {
            let mut field = mixer();
            field.reverbs = reverbs;
            field.music_reverbs.as_mut().unwrap().presets = reverbs;
            field.music.insert(0, track.clone());
            let (source, _control) = Arc::new(field).session();
            let mut frames = source.decoder();
            frames.command(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
            frames.command(AudioCommand::MusicVolume {
                volume: 127,
                duration_ticks: 0,
            })?;
            frames.command(AudioCommand::MusicVolume {
                volume,
                duration_ticks,
            })?;
            for _ in 0..480 {
                frames.frame()?;
            }
            let saved = frames.music.as_ref().unwrap().fade;
            let saved_reverb = frames.studio.music_reverb();
            let mut battle = assets();
            Arc::get_mut(&mut Arc::get_mut(&mut battle).unwrap().mixer)
                .unwrap()
                .music
                .insert(1, track.clone());
            frames.apply_battle_command(Command::Begin(battle, settings(), Some(1)))?;
            for _ in 0..960 {
                frames.frame()?;
            }
            frames.apply_battle_command(Command::End(true))?;
            let music = frames.music.as_ref().unwrap();
            assert_eq!(music.id, 0);
            for offset in [0, 32, 1600] {
                assert_eq!(music.fade.value_at(offset), saved.value_at(offset));
            }
            assert_eq!(frames.studio.music_reverb(), saved_reverb);
            let mut peak = 0f32;
            let mut final_frame = [0.; 2];
            for elapsed in 0..2000 {
                assert_eq!(
                    frames.music.as_ref().unwrap().fade.value(),
                    saved.value_at(elapsed)
                );
                final_frame = frames.frame()?.unwrap();
                // Submitted battle samples and native releases have drained after two blocks.
                if elapsed >= 320 {
                    peak = peak.max(final_frame[0].abs());
                }
            }
            if volume == 0 {
                assert_eq!(final_frame, [0.; 2]);
            }
            peaks.push(peak);
        }
        assert!(peaks[0] > 0.);
        assert_eq!(peaks[1], 0., "muted field music became audible on return");
        assert!(
            peaks[2] > 0. && peaks[2] < peaks[0],
            "partial gain was overwritten"
        );
        assert!(peaks[3] > 0., "unfinished fade did not resume");
        Ok(())
    }

    #[test]
    fn battle_return_restores_the_suspended_reverb_parameters() -> Result<()> {
        let field_reverb = [0.8, 0.7, 3.6, 0.6, 0.08];
        let battle_reverb = [0.7, 0.7, 2.5, 0.6, 0.05];
        for resume in [false, true] {
            let (source, _control) = Arc::new(mixer()).session();
            let mut frames = source.decoder();
            // The actual selected effect can differ from the current catalog,
            // including when a song leaves the preceding selection unchanged.
            frames.studio.set_music_reverb(field_reverb)?;
            frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
            frames.studio.set_music_reverb(battle_reverb)?;
            frames.apply_battle_command(Command::End(resume))?;
            assert_eq!(
                frames.studio.music_reverb(),
                if resume { field_reverb } else { battle_reverb }
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked battle music 85/95; no window or audio device"]
    fn music_replacement_and_same_track_calls_preserve_playback() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"),
            Into::into,
        );
        let descriptor: Audio = serde_json::from_slice(&std::fs::read(root.join(PATH))?)?;
        let mut bank = mixer();
        bank.music_reverbs = Some(descriptor.assets.music_reverbs);
        for track in [85, 95] {
            bank.music.insert(
                track,
                Arc::new(Package::load(&root, &descriptor.assets.music[&track].path)?),
            );
        }
        let mut battle_assets = assets();
        Arc::get_mut(&mut battle_assets).unwrap().mixer = Arc::new(bank);
        let (source, _control) = Arc::new(mixer()).session();
        let mut frames = source.decoder();
        frames.apply_battle_command(Command::Begin(battle_assets, settings(), None))?;
        frames.apply_battle_command(Command::Music(Some(85), 250))?;
        for _ in 0..480 {
            frames.frame()?;
        }
        let before = frames.music.as_ref().unwrap().player.stream.shared_frame();
        let fade = frames.music.as_ref().unwrap().fade.value();
        assert!(frames.music.as_ref().unwrap().player.stream.started());
        frames.apply_battle_command(Command::Music(Some(85), 987))?;
        assert_eq!(
            frames.music.as_ref().unwrap().player.stream.shared_frame(),
            before
        );
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), fade);
        assert_eq!(frames.music.as_ref().map(|music| music.id), Some(85));
        assert!(
            frames
                .apply_battle_command(Command::Music(Some(96), 0))
                .is_err()
        );
        assert_eq!(frames.music.as_ref().map(|music| music.id), Some(85));
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), fade);
        frames.apply_battle_command(Command::Music(Some(95), 50))?;
        assert_eq!(frames.music.as_ref().map(|music| music.id), Some(95));
        for _ in 0..480 {
            frames.frame()?;
        }
        assert!(frames.music.as_ref().unwrap().player.stream.started());
        let fade = frames.music.as_ref().unwrap().fade.value();
        frames.apply_battle_command(Command::Music(None, 50))?;
        assert!(frames.battle.as_ref().unwrap().stop_music);
        assert_eq!(frames.music.as_ref().unwrap().fade.value(), fade);
        Ok(())
    }

    #[test]
    fn missing_music_preserves_owner_under_both_error_policies() -> Result<()> {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut assets = assets();
            Arc::get_mut(&mut assets).unwrap().diagnostics = diagnostics.clone();
            Arc::get_mut(&mut Arc::get_mut(&mut assets).unwrap().mixer)
                .unwrap()
                .music
                .insert(0, test_music());
            let (source, mut control) = Arc::new(mixer()).session();
            let mut frames = source.decoder();
            let playback = Playback::begin(&mut control, assets, settings(), Some(0))?;
            frames.frame()?;
            frames.command(AudioCommand::MusicVolume {
                volume: 48,
                duration_ticks: 0,
            })?;
            assert_eq!(playback.music(Some(95), 0).is_err(), paranoid);
            assert_eq!(diagnostics.entries().len(), 1);
            frames.frame()?;
            assert!(!frames.battle.as_ref().unwrap().stop_music);
            assert_eq!(frames.music.as_ref().unwrap().fade.value(), 48. / 127.);
            assert_eq!(frames.music.as_ref().map(|music| music.id), Some(0));
            control.check()?;
        }
        Ok(())
    }

    #[test]
    fn battle_handoff_uses_existing_decoder_and_restores_field_settings() -> Result<()> {
        let (source, mut control) = Arc::new(mixer()).session();
        control.levels([33, 44, 55])?;
        let mut frames = source.decoder();
        frames.frame()?;
        let field = frames.assets.clone();
        let mut playback = Playback::begin(&mut control, assets(), settings(), None)?;
        frames.frame()?;
        assert_eq!(frames.levels, [91, 127, 127]);
        assert!(!frames.stereo);
        assert!(frames.battle.is_some());
        assert!(!Arc::ptr_eq(&frames.assets, &field));
        assert!(playback.finish(&mut control, true)?);
        frames.frame()?;
        assert!(frames.battle.is_none());
        assert!(Arc::ptr_eq(&frames.assets, &field));
        assert_eq!(frames.levels, [33, 44, 55]);
        assert!(frames.stereo);
        assert_eq!(control.rendered_frames(), 3);
        Ok(())
    }
    #[test]
    fn tolerant_audio_omits_missing_sounds_and_admits_available_fallbacks() -> Result<()> {
        let diagnostics = Diagnostics::default();
        let files = super::super::tests::empty_prepared_files(diagnostics.clone());
        let assets = Assets::load(&files, None, &mut Cache::default())?;
        assert_eq!(assets.bind(Sound::Cue(60))?, None);
        assert_eq!(assets.bind(Sound::Stream(7))?, None);
        let mut assets = assets;
        Arc::get_mut(&mut Arc::get_mut(&mut assets).unwrap().mixer)
            .unwrap()
            .voices
            .insert(1, Arc::new(test_clip(vec![0; 480], 48000, 1)));
        assert_eq!(assets.bind(Sound::Stream(1))?, Some(Sound::Stream(1)));
        assert_eq!(
            diagnostics
                .entries()
                .iter()
                .filter(|entry| entry.scope == "battle audio binding")
                .count(),
            2
        );
        let (source, mut control) = assets.mixer.clone().session();
        let mut frames = source.decoder();
        let playback = Playback::begin(&mut control, assets, settings(), None)?;
        playback.menu_cue(60)?;
        playback.system_cue(61)?;
        playback.music(Some(85), 0)?;
        playback.music(None, 0)?;
        frames.frame()?;
        assert!(frames.battle.as_ref().unwrap().stop_music());
        assert!(diagnostics.has_errors());
        control.check()?;
        Ok(())
    }

    #[test]
    fn binding_and_spatial_controls_reject_unprepared_or_invalid_inputs() {
        let assets = assets();
        assert!(assets.bind(Sound::Cue(60)).is_err());
        assert!(assets.bind(Sound::Stream(1)).is_err());
        assert!(spatial(Some(f32::NAN), assets.effect_spatial).is_err());
        assert_eq!(spatial(Some(0.), assets.effect_spatial).unwrap(), (0, 95));
        assert_eq!(
            spatial(Some(640.), assets.voice_spatial).unwrap(),
            (104, 95)
        );
        assert_eq!(spatial(None, assets.voice_spatial).unwrap(), (64, 100));
        assert_eq!(spatial(None, [320., 5., 17., 0., 127.]).unwrap(), (64, 100));
    }

    #[test]
    fn battle_effects_share_voice_allocation_and_keep_system_feedback_audible() -> Result<()> {
        use resonance_audio::{
            data::{Command as Macro, EventKind, ScoreOrigin, VoiceSource},
            sequence::VOICE_BUDGET,
        };
        const SOURCE_FRAMES: usize = 3 * BLOCK_FRAMES;
        let release_end = SOURCE_FRAMES + resonance_audio::RELEASE_FRAMES as usize;
        let reverbs = [[0., 0., 1., 0., 0.]; 2];
        let score = |id, amplitude, finite| -> Result<Arc<Loaded>> {
            let (mut resources, mut score, tables) = test_score_data(ScoreOrigin::SoundEffect);
            let sample = Arc::make_mut(resources.samples.get_mut(&1).unwrap());
            sample.pcm = vec![amplitude; SOURCE_FRAMES];
            sample.loop_pcm = if finite { vec![] } else { sample.pcm.clone() };
            sample.loop_length = if finite { 0 } else { SOURCE_FRAMES as u32 };
            if finite {
                *resources.programs.get_mut(&1).unwrap().last_mut().unwrap() = Macro::End;
            }
            let EventKind::Notes { source, voices, .. } = &mut score.first_events[0].kind else {
                unreachable!()
            };
            *source = VoiceSource::SoundEffect { id };
            voices[0].max_voices = VOICE_BUDGET as u8;
            Ok(Arc::new(Loaded::new(resources, score, tables, reverbs)?))
        };
        let quiet = score(1, 8, false)?;
        let stronger = score(1, 16, false)?;
        let system = score(2, 4096, true)?;
        let mut bank = mixer();
        bank.reverbs = reverbs;
        let (source, _control) = Arc::new(bank).session();
        let mut frames = source.decoder();
        frames.apply_battle_command(Command::Begin(assets(), settings(), None))?;
        let sound = |score: &Arc<Loaded>, category, priority, x| Command::Sound {
            score: score.clone(),
            category,
            priority,
            x,
            binding: Sound::Cue(1),
        };
        // All requests arrive before the first control pass. Important system feedback
        // must still enter a full pool of not-yet-initialized combat voices.
        for index in 0..VOICE_BUDGET {
            frames.apply_battle_command(sound(
                &quiet,
                EffectCategory::Battle,
                1,
                (index == 1).then_some(0.),
            ))?;
        }
        let effects = &frames.battle.as_ref().unwrap().effects;
        assert_eq!(effects[0].controls.pan, Some(64));
        assert_eq!(effects[0].volume, f32::from(settings().effects) / 127.);
        assert_eq!(effects[1].controls.pan, Some(0));
        assert_eq!(
            effects[1].volume,
            f32::from(u16::from(settings().battle_effects) * 95 / 100) / 127.
        );
        frames.apply_battle_command(sound(&system, EffectCategory::System, 0, None))?;
        let mut pcm = Vec::new();
        for frame in 0..release_end + 2 * BLOCK_FRAMES {
            if frame == BLOCK_FRAMES {
                // Higher combat importance can replace combat voices, but not system feedback.
                for _ in 0..VOICE_BUDGET {
                    frames.apply_battle_command(sound(
                        &stronger,
                        EffectCategory::Battle,
                        0,
                        None,
                    ))?;
                }
            }
            pcm.push(frames.frame()?.unwrap()[0]);
            if frame == 2 * BLOCK_FRAMES {
                assert_eq!(frames.battle.as_ref().unwrap().effects.len(), VOICE_BUDGET);
            }
        }
        let combat_only_ceiling = (2 * VOICE_BUDGET * 16) as f32 / 32768.;
        assert!(pcm[0] > combat_only_ceiling && pcm[2 * BLOCK_FRAMES] > combat_only_ceiling);
        assert!(
            pcm[SOURCE_FRAMES] > combat_only_ceiling,
            "system cue lost its native release"
        );
        assert!(
            pcm[SOURCE_FRAMES] > pcm[release_end - 1] && pcm[release_end - 1] > pcm[release_end]
        );
        assert!(
            pcm[release_end..]
                .iter()
                .all(|&value| value == pcm[release_end])
        );
        assert!(
            pcm[release_end] > (VOICE_BUDGET * 8) as f32 / 32768.,
            "higher combat importance did not replace weaker voices"
        );
        assert_eq!(
            frames.battle.as_ref().unwrap().effects.len(),
            VOICE_BUDGET - 1,
            "finished and rejected programs retained persistent players"
        );
        assert!(frames.sounds.is_empty());
        frames.apply_battle_command(Command::End(false))?;
        let drained = (0..3 * BLOCK_FRAMES)
            .map(|_| frames.frame().map(Option::unwrap))
            .collect::<Result<Vec<_>>>()?;
        assert!(
            drained[2 * BLOCK_FRAMES..]
                .iter()
                .all(|&frame| frame == [0.; 2])
        );
        Ok(())
    }
}
