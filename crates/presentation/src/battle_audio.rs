//! Battle cue adapter on the existing field mixer and audible-frame feedback.
//! The battle core owns voice arbitration; this module plays its ordered cues.
use super::*;
use resonance_battle::{Cue, SoundBinding, VoiceId};
use resonance_content::{
    battle_audio::{Audio, PATH},
    diagnostics::Diagnostics,
    prepared::Files,
};
use resonance_game::battle::voice::Sound;

#[derive(Clone)]
pub(crate) struct Assets {
    diagnostics: Diagnostics,
    mixer: Arc<super::Assets>,
    voice_pan: [[f32; 2]; 31],
    effect_spatial: [f32; 5],
    voice_spatial: [f32; 5],
}
impl Assets {
    pub(crate) fn load(files: &Files, cache: &mut Cache) -> Result<Arc<Self>> {
        let diagnostics = files.diagnostics().clone();
        let descriptor = (|| {
            let bytes = files.read(PATH)?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let descriptor: Audio = serde_json::from_slice(&bytes)?;
            Ok((hash, descriptor))
        })();
        let Some((hash, mut descriptor)) =
            diagnostics.attempt("battle audio descriptor", descriptor)?
        else {
            return Ok(Arc::new(Self {
                mixer: Arc::new(super::Assets::silent(diagnostics.clone())),
                diagnostics,
                voice_pan: [[0.; 2]; 31],
                effect_spatial: [320., 5., 64., 0., 127.],
                voice_spatial: [320., 5., 64., 24., 104.],
            }));
        };
        diagnostics.attempt("battle audio descriptor", descriptor.validate())?;
        // Revalidate all declared bytes, even on a cache hit. A bad dependency
        // silences only packages using it, never an unrelated later cue.
        let mut missing = std::collections::BTreeSet::new();
        for (path, entry) in &descriptor.files {
            let result = (|| {
                let bytes = files.read(path)?;
                ensure!(
                    bytes.len() as u64 == entry.bytes
                        && format!("{:x}", Sha256::digest(&bytes)) == entry.sha256,
                    "battle audio dependency differs: {path}"
                );
                Ok(())
            })();
            if diagnostics
                .attempt("battle audio dependency", result)?
                .is_none()
            {
                missing.insert(path.clone());
            }
        }
        let mut invalid_packages = std::collections::BTreeSet::new();
        for reference in descriptor
            .assets
            .music
            .values()
            .chain(descriptor.assets.sounds.values())
        {
            let result = (|| {
                ensure!(
                    !missing.contains(&reference.path),
                    "unavailable battle audio package: {}",
                    reference.path
                );
                let package: Package = files.json(&reference.path)?;
                for sample in package.samples.values() {
                    ensure!(
                        !missing.contains(&sample.path),
                        "unavailable battle audio sample: {}",
                        sample.path
                    );
                    ensure!(
                        descriptor
                            .files
                            .get(&sample.path)
                            .is_some_and(|file| file.sha256 == sample.sha256
                                && file.roles.contains(
                                    &resonance_content::field_preload::Role::InstrumentSample
                                )),
                        "battle package sample is outside its inventory: {}",
                        sample.path
                    );
                }
                Ok(())
            })();
            if diagnostics
                .attempt("battle audio package", result)?
                .is_none()
            {
                invalid_packages.insert(reference.path.clone());
            }
        }
        if missing.is_empty()
            && invalid_packages.is_empty()
            && let Some(assets) = cache.battle.get(&hash)
        {
            return Ok(Arc::new(Self {
                mixer: Arc::new(super::Assets {
                    diagnostics: diagnostics.clone(),
                    ..(*assets.mixer).clone()
                }),
                diagnostics,
                ..(**assets).clone()
            }));
        }
        descriptor
            .assets
            .music
            .retain(|_, reference| !invalid_packages.contains(&reference.path));
        descriptor
            .assets
            .sounds
            .retain(|_, reference| !invalid_packages.contains(&reference.path));
        descriptor
            .assets
            .voices
            .retain(|_, voice| !missing.contains(&voice.asset.path));
        let voice_pan = diagnostics
            .attempt(
                "battle audio pan table",
                (|| {
                    ensure!(
                        descriptor.voice_pan.len() == 31
                            && descriptor
                                .voice_pan
                                .iter()
                                .flatten()
                                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                        "invalid battle voice pan table"
                    );
                    Ok(descriptor.voice_pan.try_into().unwrap())
                })(),
            )?
            .unwrap_or([[0.; 2]; 31]);
        let spatial_controls = |controls: [f32; 5]| -> Result<[f32; 5]> {
            let [origin, divisor, center, low, high] = controls;
            ensure!(
                controls.iter().all(|v| v.is_finite())
                    && origin > 0.
                    && divisor > 0.
                    && low >= 0.
                    && low <= center
                    && center <= high
                    && high <= 127.,
                "invalid battle spatial controls"
            );
            Ok(controls)
        };
        let effect_spatial = diagnostics
            .attempt(
                "battle effect position",
                spatial_controls(descriptor.effect_spatial),
            )?
            .unwrap_or([320., 5., 64., 0., 127.]);
        let voice_spatial = diagnostics
            .attempt(
                "battle voice position",
                spatial_controls(descriptor.voice_spatial),
            )?
            .unwrap_or([320., 5., 64., 24., 104.]);
        let assets = Arc::new(Self {
            diagnostics,
            mixer: Arc::new(super::Assets::load_contents(
                Path::new(""),
                descriptor.assets,
                Some(files),
                &mut cache.samples,
            )?),
            voice_pan,
            effect_spatial,
            voice_spatial,
        });
        // Incomplete banks are retried on the next preparation, not immortalized
        // by the descriptor identity after their missing files are repaired.
        if !assets.diagnostics.has_errors() {
            cache.battle.insert(hash, assets.clone());
        }
        Ok(assets)
    }
    /// Readiness reflects the already verified and decoded package inventory.
    pub(crate) fn music_ready(&self, track: u16) -> bool {
        i16::try_from(track).is_ok_and(|track| self.mixer.music.contains_key(&track))
    }

    /// Stable, typed discriminator; neither field is an original memory address.
    pub(crate) fn bind(&self, sound: Sound) -> Result<SoundBinding> {
        let binding = match sound {
            Sound::Cue(index) => SoundBinding { resource: 0, index },
            Sound::Stream(index) => SoundBinding { resource: 1, index },
        };
        self.diagnostics
            .attempt("battle audio binding", self.sound(binding))?;
        Ok(binding)
    }
    fn sound(&self, binding: SoundBinding) -> Result<Source> {
        match binding.resource {
            0 => Ok(Source::Score(
                self.mixer
                    .sounds
                    .get(&i16::try_from(binding.index)?)
                    .with_context(|| format!("unprepared battle sound {}", binding.index))?
                    .clone(),
            )),
            1 => Ok(Source::Stream(
                self.mixer
                    .voices
                    .get(&u32::from(binding.index))
                    .with_context(|| format!("unprepared battle stream {}", binding.index))?
                    .clone(),
            )),
            _ => anyhow::bail!("unknown battle sound binding {:?}", binding),
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
    pending: Vec<(VoiceId, Arc<AtomicBool>)>,
}
impl Control {
    /// Main mode9 pauses slot0 before loading the selected battle song.
    /// The existing battle Owner keeps this pending phase until Begin takes it.
    pub(crate) fn prepare_battle_entry(&mut self) -> Result<()> {
        // Main retires field cue handles before pausing slot0. Keep that
        // cleanup even if preparation fails before the battle bank is ready.
        self.leave_field()?;
        self.enqueue(Message::Battle(Command::PrepareEntry))?;
        Ok(())
    }
    pub(crate) fn cancel_battle_entry(&mut self, resume: bool) -> Result<()> {
        self.enqueue(Message::Battle(Command::CancelEntry(resume)))?;
        self.in_field = resume;
        Ok(())
    }
}

impl Playback {
    pub(crate) fn begin(
        control: &mut Control,
        assets: Arc<Assets>,
        settings: Settings,
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
        diagnostics.attempt("battle audio mixer", control.check())?;
        diagnostics.attempt(
            "battle audio begin",
            control.enqueue(Message::Battle(Command::Begin(assets.clone(), settings))),
        )?;
        control
            .voice_requests
            .lock()
            .expect("voice request queue poisoned")
            .clear();
        control.in_field = false;
        Ok(Self {
            control: control.clone(),
            assets,
            pending: vec![],
        })
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
        for cue in cues {
            let voice = match *cue {
                Cue::Voice { playback, .. } => Some(playback),
                _ => None,
            };
            let result = (|| -> Result<()> {
                // A stopped sink cannot consume any queued cue. Each skipped
                // voice is still acknowledged below so actor scripts can finish.
                ensure!(healthy, "battle audio sink is unavailable");
                let command = match *cue {
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
                        ensure!(priority <= 15, "invalid battle sound priority");
                        Command::Sound { score, x, priority }
                    }
                    Cue::Voice {
                        actor,
                        playback,
                        sound,
                        position,
                        centered,
                    } => {
                        ensure!(
                            !self.pending.iter().any(|(id, _)| *id == playback),
                            "duplicate battle voice playback"
                        );
                        let source = self.assets.sound(sound)?;
                        let x = if centered { None } else { screen_x(position) };
                        spatial(x, self.assets.voice_spatial)?;
                        let complete = Arc::new(AtomicBool::new(false));
                        self.control.enqueue(Message::Battle(Command::Voice {
                            actor: actor.index(),
                            playback,
                            source,
                            x,
                            complete: complete.clone(),
                        }))?;
                        self.pending.push((playback, complete));
                        return Ok(());
                    }
                    Cue::VoiceStopped { playback } => Command::Stop(playback),
                    _ => return Ok(()),
                };
                self.control.enqueue(Message::Battle(command))
            })();
            if self
                .assets
                .diagnostics
                .attempt("battle audio cue", result)?
                .is_none()
                && let Some(playback) = voice
            {
                // A duplicate refers to the original live voice; do not
                // acknowledge that earlier playback before it finishes.
                if !self.pending.iter().any(|(id, _)| *id == playback) {
                    self.pending
                        .push((playback, Arc::new(AtomicBool::new(true))));
                }
            }
        }
        Ok(())
    }
    /// Played voices finish after source termination and sink consumption.
    /// Diagnosed cues that could not play complete immediately.
    pub(crate) fn completed(&mut self) -> Result<Vec<VoiceId>> {
        if self
            .assets
            .diagnostics
            .attempt("battle audio mixer", self.control.check())?
            .is_none()
        {
            for (_, token) in &self.pending {
                token.store(true, Ordering::Release);
            }
        }
        let mut completed = Vec::new();
        self.pending.retain(|(id, token)| {
            if token.load(Ordering::Acquire) {
                completed.push(*id);
                false
            } else {
                true
            }
        });
        Ok(completed)
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
                self.control
                    .enqueue(Message::Battle(Command::Music(track, fade_ms)))
            })(),
        )?;
        Ok(())
    }
    /// 9C20/A0E4 pauses CRI voice slots0..6; 3534/A0A8 resumes them.
    /// Score effects and music continue on their existing mixer clocks.
    pub(crate) fn pause_voice_streams(&self, paused: bool) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle voice stream pause",
            self.control
                .enqueue(Message::Battle(Command::PauseVoiceStreams(paused))),
        )?;
        Ok(())
    }
    /// Battle result-card cues use 9E38 priority1 and centered placement.
    pub(crate) fn menu_cue(&self, index: u16) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle menu cue",
            (|| {
                let Source::Score(score) =
                    self.assets.sound(SoundBinding { resource: 0, index })?
                else {
                    anyhow::bail!("menu cue requires a score");
                };
                self.control.enqueue(Message::Battle(Command::Sound {
                    score,
                    x: None,
                    priority: 1,
                }))
            })(),
        )?;
        Ok(())
    }
    /// 70888/A1978 allocate the first free main-system slot7..14. These
    /// menu sounds share the ordinary mixer, outside the battle effect ring.
    pub(crate) fn system_cue(&self, index: u16) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle system cue",
            (|| {
                let Source::Score(score) =
                    self.assets.sound(SoundBinding { resource: 0, index })?
                else {
                    anyhow::bail!("system cue requires a score");
                };
                self.control
                    .enqueue(Message::Battle(Command::SystemCue(score)))
            })(),
        )?;
        Ok(())
    }
    pub(crate) fn finish(self, control: &mut Control, resume_field: bool) -> Result<()> {
        self.assets.diagnostics.attempt(
            "battle audio finish",
            control.enqueue(Message::Battle(Command::End(resume_field))),
        )?;
        control.in_field = resume_field;
        Ok(())
    }
}

pub(super) enum Command {
    PrepareEntry,
    CancelEntry(bool),
    SystemCue(Arc<Loaded>),
    Begin(Arc<Assets>, Settings),
    Sound {
        score: Arc<Loaded>,
        x: Option<f32>,
        priority: u8,
    },
    Voice {
        actor: usize,
        playback: VoiceId,
        source: Source,
        x: Option<f32>,
        complete: Arc<AtomicBool>,
    },
    PauseVoiceStreams(bool),
    Stop(VoiceId),
    Music(Option<i16>, u16),
    End(bool),
}
struct FieldMusic {
    assets: Arc<super::Assets>,
    player: Option<ScorePlayer>,
    id: Option<i16>,
    levels: [u8; 3],
    stereo: bool,
}
struct Effect {
    player: ScorePlayer,
    generation: u64,
}
#[derive(Clone, Copy, Default)]
struct Slot {
    priority: u8,
    generation: u64,
}
#[allow(clippy::large_enum_variant)] // Keep score admission inline in the audio callback.
enum VoiceSource {
    Score(ScorePlayer),
    Stream {
        clip: Arc<Clip>,
        frame: u64,
        gain: [f32; 2],
    },
}
impl VoiceSource {
    fn frame(
        &mut self,
        buses: &mut BusFrame,
        speech: &mut [f32; 2],
        streams_paused: bool,
    ) -> Result<bool> {
        Ok(match self {
            VoiceSource::Score(player) => {
                if let Some(source) = player.frame(|| [1.; 5])? {
                    add(buses, source);
                    true
                } else {
                    false
                }
            }
            VoiceSource::Stream { clip, frame, gain } => {
                if streams_paused {
                    return Ok(true);
                }
                // CRI's nominal 32kHz mixer is clocked by the 32028Hz DAC.
                let source = clip.sample(*frame * u64::from(clip.rate), 32000, *gain);
                *frame += 1;
                if let Some(source) = source {
                    for (out, value) in speech.iter_mut().zip(source) {
                        *out += value;
                    }
                    true
                } else {
                    false
                }
            }
        })
    }
}
struct Voice {
    actor: usize,
    playback: VoiceId,
    source: VoiceSource,
    complete: Arc<AtomicBool>,
}
pub(super) struct State {
    assets: Arc<Assets>,
    field: FieldMusic,
    settings: Settings,
    effects: Vec<Effect>,
    voices: Vec<Voice>,
    slots: [Slot; 15],
    cursor: usize,
    generation: u64,
    stop_music: bool,
    voice_streams_paused: bool,
}
impl Frames {
    pub(super) fn battle_command(&mut self, command: Command) -> Result<()> {
        let diagnostics = match &command {
            Command::Begin(assets, _) => assets.diagnostics.clone(),
            _ => self.battle.as_ref().map_or_else(
                || self.assets.diagnostics.clone(),
                |state| state.assets.diagnostics.clone(),
            ),
        };
        let completion = match &command {
            Command::Voice { complete, .. } => Some(complete.clone()),
            _ => None,
        };
        if diagnostics
            .attempt("battle mixer command", self.apply_battle_command(command))?
            .is_none()
            && let Some(complete) = completion
        {
            complete.store(true, Ordering::Release);
        }
        Ok(())
    }
    fn apply_battle_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::PrepareEntry => {
                ensure!(self.battle.is_none(), "battle audio already active");
                if let Some(music) = &mut self.music {
                    music.stream.pause(true)?;
                }
                debug!(source_frame = self.frame, "Battle entry paused field music");
            }
            Command::CancelEntry(resume) => {
                ensure!(self.battle.is_none(), "battle entry already transferred");
                if resume {
                    if let Some(music) = &mut self.music {
                        music.stream.pause(false)?;
                    }
                    self.fade = Fade::new(0., 1., 100)?;
                } else {
                    self.music = None;
                    self.music_id = None;
                }
                debug!(
                    source_frame = self.frame,
                    resume, "Battle entry music returned"
                );
            }
            Command::Begin(assets, settings) => {
                ensure!(self.battle.is_none(), "battle audio already active");
                let studio = Studio::new(assets.mixer.reverbs)?;
                if let Some(music) = &mut self.music {
                    music.stream.pause(true)?;
                }
                let field = FieldMusic {
                    assets: self.assets.clone(),
                    player: self.music.take(),
                    id: self.music_id.take(),
                    levels: self.levels,
                    stereo: self.stereo,
                };
                self.sounds.clear();
                self.voice = None;
                self.completions
                    .lock()
                    .expect("voice completion queue poisoned")
                    .clear();
                self.assets = assets.mixer.clone();
                self.studio = studio;
                self.levels = [settings.music, 127, 127];
                self.stereo = settings.stereo;
                self.battle = Some(State {
                    assets,
                    field,
                    settings,
                    effects: vec![],
                    voices: vec![],
                    slots: [Slot::default(); 15],
                    cursor: 0,
                    generation: 0,
                    stop_music: false,
                    voice_streams_paused: false,
                });
                debug!(source_frame = self.frame, "Battle mixer ownership began");
            }
            Command::End(resume) => {
                let state = self.battle.take().context("battle audio is not active")?;
                self.music = None;
                self.music_id = None;
                self.assets = state.field.assets;
                self.levels = state.field.levels;
                self.stereo = state.field.stereo;
                self.studio = Studio::new(self.assets.reverbs)?;
                if resume {
                    self.music = state.field.player;
                    self.music_id = state.field.id;
                    if let Some(music) = &mut self.music {
                        music.stream.pause(false)?;
                    }
                    self.fade = Fade::new(0., 1., 100)?;
                }
                debug!(
                    source_frame = self.frame,
                    resume, "Battle mixer ownership ended"
                );
            }
            Command::Music(track, fade) => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                if track.is_some() && self.music_id == track && !state.stop_music {
                    return Ok(());
                }
                state.stop_music = track.is_none();
                if let Some(track) = track {
                    self.command(AudioCommand::Music(track))?;
                    self.fade = Fade::new(0., 1., fade)?;
                } else {
                    self.fade = Fade::new(self.fade.value(), 0., fade)?;
                }
                debug!(
                    source_frame = self.frame,
                    ?track,
                    fade_ms = fade,
                    "Battle music command applied"
                );
            }
            Command::SystemCue(score) => {
                let state = self.battle.as_ref().context("battle audio is not active")?;
                let Some(slot) =
                    (7..15).find(|slot| !self.sounds.iter().any(|sound| sound.slot == Some(*slot)))
                else {
                    return Ok(());
                };
                let mut sound = ScorePlayer::new(score, false, &self.synth)?;
                sound.controls.pan = Some(64);
                sound.volume = f32::from(state.settings.effects) / 127.;
                sound.slot = Some(slot);
                self.sounds.push(sound);
                debug!(source_frame = self.frame, "Battle system cue applied");
            }
            Command::Sound { score, x, priority } => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                let (pan, percent) = spatial(x, state.assets.effect_spatial)?;
                let mut checked = 0;
                while checked < 15 {
                    let queued = state.slots[state.cursor].priority;
                    // 9E38 queries the stored priority byte as the native slot
                    // index (not the ring cursor); preserve that lookup.
                    let active = if queued < 15 {
                        let generation = state.slots[usize::from(queued)].generation;
                        state.effects.iter().any(|e| e.generation == generation)
                    } else {
                        state
                            .voices
                            .iter()
                            .any(|v| v.actor == 0 && matches!(v.source, VoiceSource::Score(_)))
                    };
                    if queued >= priority || !active {
                        break;
                    }
                    state.cursor = (state.cursor + 1) % 15;
                    checked += 1;
                }
                if checked == 15 {
                    return Ok(());
                }
                ensure!(state.effects.len() < 64, "battle effect budget exhausted");
                let volume = if x.is_some() {
                    state.settings.battle_effects
                } else {
                    state.settings.effects
                };
                let mut player = ScorePlayer::new(score, false, &self.synth)?;
                player.controls.pan = Some(pan);
                player.volume = f32::from(u16::from(volume) * u16::from(percent) / 100) / 127.;
                state.generation += 1;
                state.effects.push(Effect {
                    player,
                    generation: state.generation,
                });
                state.slots[state.cursor] = Slot {
                    priority,
                    generation: state.generation,
                };
                state.cursor = (state.cursor + 1) % 15;
                debug!(source_frame = self.frame, "Battle sound command applied");
            }
            Command::Voice {
                actor,
                playback,
                source,
                x,
                complete,
            } => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                ensure!(
                    actor < 7 && !state.voices.iter().any(|v| v.actor == actor),
                    "battle voice replaced without a stop cue"
                );
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
                        let pan = ((i16::from(pan) - 64) / 4).clamp(-15, 15);
                        let gain = state.assets.voice_pan[(pan + 15) as usize]
                            .map(|v| v * self.assets.voice_gains[usize::from(level)]);
                        VoiceSource::Stream {
                            clip,
                            frame: 0,
                            gain,
                        }
                    }
                };
                state.voices.push(Voice {
                    actor,
                    playback,
                    source,
                    complete,
                });
                debug!(
                    source_frame = self.frame,
                    actor, "Battle voice command applied"
                );
            }
            Command::PauseVoiceStreams(paused) => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                state.voice_streams_paused = paused;
            }
            Command::Stop(playback) => {
                let state = self.battle.as_mut().context("battle audio is not active")?;
                if let Some(index) = state.voices.iter().position(|v| v.playback == playback) {
                    complete(
                        &self.completions,
                        self.frame,
                        state.voices.remove(index).complete,
                        &state.assets.diagnostics,
                    )?;
                    debug!(source_frame = self.frame, "Battle voice stop applied");
                }
            }
        }
        Ok(())
    }
}
impl State {
    pub(super) fn stop_music(&self) -> bool {
        self.stop_music
    }
    pub(super) fn prepare_shared(&mut self) -> Result<()> {
        let diagnostics = &self.assets.diagnostics;
        let mut index = 0;
        while index < self.effects.len() {
            let player = &mut self.effects[index].player;
            player.controls.mono = !self.settings.stereo;
            if diagnostics
                .attempt("battle effect playback", player.prepare_shared(|| [1.; 5]))?
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
                    player.prepare_shared(|| [1.; 5])
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
    ) -> Result<[f32; 2]> {
        let diagnostics = &self.assets.diagnostics;
        let mut index = 0;
        while index < self.effects.len() {
            if let Some(Some(source)) = diagnostics.attempt(
                "battle effect playback",
                self.effects[index].player.frame(|| [1.; 5]),
            )? {
                add(buses, source);
                index += 1;
            } else {
                self.effects.remove(index);
            }
        }
        let mut speech = [0.; 2];
        let mut index = 0;
        while index < self.voices.len() {
            match diagnostics.attempt(
                "battle voice playback",
                self.voices[index]
                    .source
                    .frame(buses, &mut speech, self.voice_streams_paused),
            )? {
                Some(true) => index += 1,
                Some(false) => complete(
                    completions,
                    frame,
                    self.voices.remove(index).complete,
                    diagnostics,
                )?,
                None => self
                    .voices
                    .remove(index)
                    .complete
                    .store(true, Ordering::Release),
            }
        }
        Ok(speech)
    }
    pub(super) fn stop_failed_scores(&mut self) {
        self.field.player = None;
        self.field.id = None;
        self.effects.clear();
        self.voices.retain(|voice| {
            if matches!(voice.source, VoiceSource::Score(_)) {
                voice.complete.store(true, Ordering::Release);
                false
            } else {
                true
            }
        });
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
        ensure!(queue.len() < 64, "voice completion queue full");
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
        return Ok((center as u8, 100));
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
    fn mixer() -> super::super::Assets {
        super::super::Assets {
            diagnostics: Diagnostics::new(true),
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
            voice_gains: [1.; 128],
            reverbs: [[0.5, 0.5, 1., 0.5, 0.]; 2],
        }
    }
    fn assets() -> Arc<Assets> {
        Arc::new(Assets {
            diagnostics: Diagnostics::new(true),
            mixer: Arc::new(mixer()),
            voice_pan: [[1.; 2]; 31],
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
    #[test]
    fn stream_completion_waits_for_source_end_and_consumed_audio_even_when_muted() -> Result<()> {
        for gain in [[1.; 2], [0.; 2]] {
            let (source, control) = mixer().session();
            let _frames = source.decoder();
            let token = Arc::new(AtomicBool::new(false));
            let mut voice = VoiceSource::Stream {
                clip: Arc::new(Clip {
                    pcm: vec![10000, 20000],
                    rate: 24000,
                    channels: 1,
                }),
                frame: 0,
                gain,
            };
            let mut buses = [[0; 2]; 3];
            for frame in 0..3 {
                let mut speech = [0.; 2];
                assert!(voice.frame(&mut buses, &mut speech, false)?);
                assert_eq!(speech == [0.; 2], gain == [0.; 2]);
                control.acknowledge_frames(frame);
                assert!(!token.load(Ordering::Acquire));
            }
            assert!(!voice.frame(&mut buses, &mut [0.; 2], false)?);
            complete(
                &control.completions,
                3,
                token.clone(),
                &Diagnostics::new(true),
            )?;
            control.acknowledge_frames(2);
            assert!(!token.load(Ordering::Acquire));
            control.acknowledge_frames(3);
            assert!(token.load(Ordering::Acquire));
        }
        Ok(())
    }
    #[test]
    fn command_pause_holds_stream_pcm_and_resumes_the_same_sample() -> Result<()> {
        let (source, mut control) = mixer().session();
        let mut frames = source.decoder();
        let playback = Playback::begin(&mut control, assets(), settings())?;
        playback.pause_voice_streams(true)?;
        frames.frame()?;
        assert!(frames.battle.as_ref().unwrap().voice_streams_paused);
        let mut voice = VoiceSource::Stream {
            clip: Arc::new(Clip {
                pcm: vec![10000, 20000],
                rate: 32000,
                channels: 1,
            }),
            frame: 0,
            gain: [1.; 2],
        };
        let mut buses = [[0; 2]; 3];
        for _ in 0..30 {
            let mut speech = [0.; 2];
            assert!(voice.frame(&mut buses, &mut speech, true)?);
            assert_eq!(speech, [0.; 2]);
        }
        let VoiceSource::Stream { frame, .. } = &voice else {
            unreachable!()
        };
        assert_eq!(*frame, 0);
        playback.pause_voice_streams(false)?;
        frames.frame()?;
        assert!(!frames.battle.as_ref().unwrap().voice_streams_paused);
        let mut speech = [0.; 2];
        assert!(voice.frame(&mut buses, &mut speech, false)?);
        assert_ne!(speech, [0.; 2]);
        let VoiceSource::Stream { frame, .. } = &voice else {
            unreachable!()
        };
        assert_eq!(*frame, 1);
        Ok(())
    }
    #[test]
    fn failed_prebank_entry_retires_field_cues_without_swapping_the_bank() -> Result<()> {
        use resonance_game::dialogue::VoiceFeedback;
        for resume in [false, true] {
            let mut field = mixer();
            field.voices.insert(
                1,
                Arc::new(Clip {
                    pcm: vec![16384; 1000],
                    rate: RATE,
                    channels: 1,
                }),
            );
            let (source, mut control) = field.session();
            let mut frames = source.decoder();
            control.send(AudioCommand::Voice(1))?;
            frames.frame()?;
            assert!(frames.voice.is_some());
            let field = frames.assets.clone();
            let _pending = control.begin(1);
            assert!(!control.voice_requests.lock().unwrap().is_empty());
            control.prepare_battle_entry()?;
            assert!(control.voice_requests.lock().unwrap().is_empty());
            frames.frame()?;
            assert!(frames.voice.is_none());
            assert!(frames.completions.lock().unwrap().is_empty());
            assert!(frames.battle.is_none());
            assert!(Arc::ptr_eq(&frames.assets, &field));
            control.cancel_battle_entry(resume)?;
            frames.frame()?;
            assert!(frames.battle.is_none());
            assert!(Arc::ptr_eq(&frames.assets, &field));
            assert_eq!(control.in_field, resume);
            control.check()?;
        }
        Ok(())
    }
    #[test]
    fn battle_handoff_uses_existing_decoder_and_restores_field_settings() -> Result<()> {
        let (source, mut control) = mixer().session();
        control.levels([33, 44, 55])?;
        let mut frames = source.decoder();
        frames.frame()?;
        let field = frames.assets.clone();
        let playback = Playback::begin(&mut control, assets(), settings())?;
        frames.frame()?;
        assert_eq!(frames.levels, [91, 127, 127]);
        assert!(!frames.stereo);
        assert!(frames.battle.is_some());
        assert!(!Arc::ptr_eq(&frames.assets, &field));
        playback.finish(&mut control, true)?;
        frames.frame()?;
        assert!(frames.battle.is_none());
        assert!(Arc::ptr_eq(&frames.assets, &field));
        assert_eq!(frames.levels, [33, 44, 55]);
        assert!(frames.stereo);
        assert_eq!(control.rendered_frames(), 3);
        Ok(())
    }
    #[test]
    fn tolerant_missing_bank_preserves_typed_bindings_and_collects_missing_requests() -> Result<()>
    {
        let diagnostics = Diagnostics::default();
        let files = Files::load_with_diagnostics(
            Path::new(""),
            &[],
            &mut Default::default(),
            || false,
            diagnostics.clone(),
        )?;
        let assets = Assets::load(&files, &mut Cache::default())?;
        assert_eq!(
            assets.bind(Sound::Cue(60))?,
            SoundBinding {
                resource: 0,
                index: 60
            }
        );
        assert_eq!(
            assets.bind(Sound::Stream(7))?,
            SoundBinding {
                resource: 1,
                index: 7
            }
        );
        assert!(
            assets
                .sound(SoundBinding {
                    resource: 2,
                    index: 0
                })
                .is_err()
        );
        let (source, mut control) = (*assets.mixer).clone().session();
        let mut frames = source.decoder();
        let playback = Playback::begin(&mut control, assets, settings())?;
        playback.menu_cue(60)?;
        playback.system_cue(61)?;
        playback.music(Some(85), 0)?;
        playback.music(None, 0)?;
        frames.frame()?;
        assert!(frames.battle.as_ref().unwrap().stop_music());
        assert!(diagnostics.entries().len() >= 6);
        control.check()?;
        Ok(())
    }

    #[test]
    fn binding_and_spatial_controls_reject_unprepared_or_invalid_inputs() {
        let assets = assets();
        assert!(assets.bind(Sound::Cue(60)).is_err());
        assert!(assets.bind(Sound::Stream(1)).is_err());
        assert!(
            assets
                .sound(SoundBinding {
                    resource: 2,
                    index: 0
                })
                .is_err()
        );
        assert!(spatial(Some(f32::NAN), assets.effect_spatial).is_err());
        assert_eq!(spatial(Some(0.), assets.effect_spatial).unwrap(), (0, 95));
        assert_eq!(
            spatial(Some(640.), assets.voice_spatial).unwrap(),
            (104, 95)
        );
        assert_eq!(spatial(None, assets.voice_spatial).unwrap(), (64, 100));
    }
}
