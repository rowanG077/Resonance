//! Named effects are either editable synthesis programs or ordinary recordings.
//! Both kinds use the caller's cue controls, shared voice budget and effects studio.
use crate::{
    BLOCK_FRAMES, CONTROL_FRAMES, CONTROLS_PER_BLOCK, SOURCE_RATE,
    package::{Loaded, SampleCache, editable::Program},
    sequence::{
        BusFrame, LiveControls,
        shared::{EffectVoice, Synthesizer},
    },
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

pub mod bake;
pub mod recipe;
pub use recipe::Recipe;
pub const MAX_WAVE_BYTES: usize = 65 * 1024 * 1024;
const MAX_SAMPLES: usize = 16_000_000;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Definition {
    Synthesized(Recipe),
    Wave(Wave),
    Silence(Silence),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Silence {
    pub frames: u32,
    pub sample_rate: u32,
}

impl Silence {
    fn prepare(self, environment: Option<[[f32; 5]; 2]>) -> Result<Recording> {
        ensure!(
            (1..=8_000_000).contains(&self.frames) && (8000..=192000).contains(&self.sample_rate),
            "invalid silent effect clock"
        );
        let reverbs = environment.context("silent effect requires a playback environment")?;
        for parameters in reverbs {
            crate::reverb::validate_parameters(parameters)?;
        }
        Ok(Recording {
            pcm: Vec::new(),
            frames: self.frames as usize,
            channels: 2,
            rate: self.sample_rate,
            repeat: None,
            gain: 1.,
            sends: [0.; 2],
            release_frames: (5 * u64::from(SOURCE_RATE)).div_ceil(1000),
            reverbs,
        })
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wave {
    pub audio: String,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Loop>,
    #[serde(default = "unit")]
    pub gain: f32,
    #[serde(default)]
    pub sends: [f32; 2],
    #[serde(default = "release_ms")]
    pub release_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverbs: Option<[[f32; 5]; 2]>,
}
fn unit() -> f32 {
    1.
}
fn release_ms() -> u32 {
    5
}

impl Definition {
    pub fn read(
        path: &str,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
    ) -> Result<Self> {
        crate::package::relative_path(path)?;
        if path.ends_with(".wav") {
            Ok(Self::Wave(Wave {
                audio: path.into(),
                repeat: None,
                gain: 1.,
                sends: [0.; 2],
                release_ms: 5,
                reverbs: None,
            }))
        } else {
            ensure!(
                path.ends_with(".json"),
                "effect must be a WAV or JSON recipe"
            );
            Ok(serde_json::from_slice(&read(path, 16 * 1024 * 1024)?)?)
        }
    }

    pub fn reverbs(&self) -> Option<[[f32; 5]; 2]> {
        match self {
            Self::Synthesized(program) => program.reverbs,
            Self::Wave(wave) => wave.reverbs,
            Self::Silence(_) => None,
        }
    }

    /// Native fidelity fixtures can explicitly require a synthesized definition.
    pub fn into_program(
        self,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
    ) -> Result<Program> {
        match self {
            Self::Synthesized(program) => program.expand(read),
            Self::Wave(_) | Self::Silence(_) => anyhow::bail!("expected a synthesized effect"),
        }
    }

    pub fn prepare_in(
        self,
        environment: Option<[[f32; 5]; 2]>,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
        cache: &mut SampleCache,
    ) -> Result<Prepared> {
        Ok(match self {
            Self::Synthesized(program) => {
                Prepared::from(program.expand(read)?.prepare_in(environment, read, cache)?)
            }
            Self::Wave(wave) => Prepared::Wave(Arc::new(wave.prepare(environment, read)?)),
            Self::Silence(clock) => Prepared::Wave(Arc::new(clock.prepare(environment)?)),
        })
    }
}

#[derive(Clone)]
pub enum Prepared {
    Synthesized(Arc<Loaded>),
    Wave(Arc<Recording>),
}
impl From<Loaded> for Prepared {
    fn from(program: Loaded) -> Self {
        Self::Synthesized(Arc::new(program))
    }
}
impl From<Arc<Loaded>> for Prepared {
    fn from(program: Arc<Loaded>) -> Self {
        Self::Synthesized(program)
    }
}
impl From<Arc<Prepared>> for Prepared {
    fn from(effect: Arc<Prepared>) -> Self {
        (*effect).clone()
    }
}
impl Prepared {
    pub fn reverbs(&self) -> [[f32; 5]; 2] {
        match self {
            Self::Synthesized(program) => program.reverbs(),
            Self::Wave(wave) => wave.reverbs,
        }
    }
    pub fn program(&self) -> Option<&Loaded> {
        match self {
            Self::Synthesized(program) => Some(program),
            Self::Wave(_) => None,
        }
    }
}

pub struct Recording {
    pcm: Vec<f32>,
    frames: usize,
    channels: usize,
    rate: u32,
    repeat: Option<Loop>,
    gain: f32,
    sends: [f32; 2],
    release_frames: u64,
    reverbs: [[f32; 5]; 2],
}
impl Wave {
    fn prepare(
        self,
        environment: Option<[[f32; 5]; 2]>,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
    ) -> Result<Recording> {
        crate::package::relative_path(&self.audio)?;
        ensure!(
            self.gain.is_finite()
                && (0.0..=4.0).contains(&self.gain)
                && self
                    .sends
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && self.release_ms <= 5000,
            "invalid recorded effect gain, sends or release"
        );
        ensure!(
            self.reverbs.is_none() || environment.is_none(),
            "shared effects inherit their playback environment; remove inline reverbs"
        );
        let reverbs = self
            .reverbs
            .or(environment)
            .context("recorded effect requires a playback environment")?;
        for parameters in reverbs {
            crate::reverb::validate_parameters(parameters)?;
        }
        let bytes = read(&self.audio, MAX_WAVE_BYTES)
            .with_context(|| format!("recorded effect {}", self.audio))?;
        ensure!(
            bytes.len() <= MAX_WAVE_BYTES,
            "recorded effect exceeds read budget"
        );
        let mut wave = hound::WavReader::new(Cursor::new(&*bytes))
            .with_context(|| format!("decode effect WAV {}", self.audio))?;
        let spec = wave.spec();
        let frames = wave.duration();
        let samples = usize::try_from(frames)?
            .checked_mul(usize::from(spec.channels))
            .context("effect WAV sample count overflow")?;
        ensure!(
            (1..=2).contains(&spec.channels)
                && (8_000..=192_000).contains(&spec.sample_rate)
                && (1..=MAX_SAMPLES).contains(&samples),
            "effect WAV requires nonempty mono/stereo audio at 8000–192000 Hz within the decoded budget"
        );
        let pcm = match (spec.sample_format, spec.bits_per_sample) {
            (hound::SampleFormat::Int, bits @ (8 | 16 | 24 | 32)) => {
                let scale = 2f64.powi(i32::from(bits) - 1);
                wave.samples::<i32>()
                    .map(|value| value.map(|value| (f64::from(value) / scale) as f32))
                    .collect::<std::result::Result<Vec<_>, _>>()?
            }
            (hound::SampleFormat::Float, 32) => wave
                .samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()?,
            _ => anyhow::bail!("effect WAV requires 8/16/24/32-bit PCM or 32-bit float audio"),
        };
        ensure!(
            pcm.len() == samples
                && pcm
                    .iter()
                    .all(|v| v.is_finite() && (-1.0..=1.0).contains(v)),
            "truncated effect WAV or invalid sample amplitude"
        );
        if let Some(repeat) = self.repeat {
            ensure!(
                repeat.start_frame < repeat.end_frame && repeat.end_frame <= frames,
                "effect loop exceeds the current WAV frame count"
            );
        }
        Ok(Recording {
            pcm,
            frames: frames as usize,
            channels: usize::from(spec.channels),
            rate: spec.sample_rate,
            repeat: self.repeat,
            gain: self.gain,
            sends: self.sends,
            release_frames: (u64::from(self.release_ms) * u64::from(SOURCE_RATE)).div_ceil(1000),
            reverbs,
        })
    }
}

pub enum Stream {
    Synthesized(crate::sequence::stream::Stream),
    Wave(WaveStream),
}
impl Stream {
    pub fn in_synthesizer(effect: impl Into<Prepared>, synth: &Synthesizer) -> Result<Self> {
        Ok(match effect.into() {
            Prepared::Synthesized(program) => Self::Synthesized(
                crate::sequence::stream::Stream::in_synthesizer(program, false, synth)?,
            ),
            Prepared::Wave(recording) => Self::Wave(WaveStream {
                recording,
                synth: synth.clone(),
                state: Mutex::new(WaveState::default()),
            }),
        })
    }
    pub fn set_shared_controls(&self, controls: [LiveControls; CONTROLS_PER_BLOCK]) -> Result<()> {
        crate::sequence::stream::validate_controls(controls)?;
        match self {
            Self::Synthesized(stream) => stream.set_shared_controls(controls),
            Self::Wave(stream) => {
                let mut state = stream.state.lock().expect("recorded effect lock poisoned");
                state.controls = controls;
                if stream.synth.frame().is_multiple_of(BLOCK_FRAMES as u64) && !state.finished {
                    if !state.started {
                        state.started = true;
                        state.voice = stream.synth.effect_voice(controls[0].priority.unwrap_or(0));
                        state.finished = state.voice.is_none();
                    }
                    if let Some(voice) = &state.voice {
                        voice.priority(controls[0].priority.unwrap_or(0));
                    }
                }
                Ok(())
            }
        }
    }
    pub fn shared_frame(&self) -> Option<BusFrame> {
        match self {
            Self::Synthesized(stream) => stream.shared_frame(),
            Self::Wave(stream) => stream.frame(),
        }
    }
    pub fn shared_control_boundary(&self) -> bool {
        match self {
            Self::Synthesized(stream) => stream.shared_control_boundary(),
            Self::Wave(stream) => {
                let state = stream.state.lock().expect("recorded effect lock poisoned");
                state.started
                    && state
                        .frame
                        .is_some_and(|frame| frame.is_multiple_of(BLOCK_FRAMES as u64))
            }
        }
    }
    pub fn submitted_until(&self) -> u64 {
        match self {
            Self::Synthesized(stream) => stream.submitted_until(),
            Self::Wave(stream) => stream
                .state
                .lock()
                .expect("recorded effect lock poisoned")
                .frame
                .map_or(0, |frame| frame + 1),
        }
    }
}

pub struct WaveStream {
    recording: Arc<Recording>,
    synth: Synthesizer,
    state: Mutex<WaveState>,
}
struct WaveState {
    position: u64,
    controls: [LiveControls; CONTROLS_PER_BLOCK],
    voice: Option<EffectVoice>,
    started: bool,
    initialized: bool,
    finished: bool,
    released: Option<u64>,
    frame: Option<u64>,
    last: Option<BusFrame>,
}
impl Default for WaveState {
    fn default() -> Self {
        Self {
            position: 0,
            controls: [LiveControls::default(); CONTROLS_PER_BLOCK],
            voice: None,
            started: false,
            initialized: false,
            finished: false,
            released: None,
            frame: None,
            last: None,
        }
    }
}
impl WaveStream {
    fn frame(&self) -> Option<BusFrame> {
        let mut state = self.state.lock().expect("recorded effect lock poisoned");
        let clock = self.synth.frame().saturating_sub(1);
        if state.frame == Some(clock) {
            return state.last;
        }
        state.frame = Some(clock);
        if state.finished {
            state.last = None;
            return None;
        }
        if !state.started {
            state.last = Some([[0; 2]; 3]);
            return state.last;
        }
        if state.voice.as_ref().is_none_or(|voice| !voice.active()) {
            if let Some(last) = state.last.take() {
                self.synth.release(last, 0);
            }
            state.voice = None;
            state.finished = true;
            return None;
        }
        let controls = state.controls[(clock % BLOCK_FRAMES as u64) as usize / CONTROL_FRAMES];
        if controls.release && state.released.is_none() {
            state.released = Some(0);
        }
        let fade = if let Some(released) = state.released {
            if released >= self.recording.release_frames {
                state.finished = true;
                state.last = None;
                state.voice = None;
                return None;
            }
            state.released = Some(released + 1);
            1. - released as f32 / self.recording.release_frames as f32
        } else {
            1.
        };
        let denominator = u64::from(SOURCE_RATE);
        if let Some(repeat) = self.recording.repeat {
            let end = u64::from(repeat.end_frame) * denominator;
            if state.position >= end {
                let start = u64::from(repeat.start_frame) * denominator;
                state.position = start + (state.position - end) % (end - start);
            }
        }
        let index = (state.position / denominator) as usize;
        let frames = self.recording.frames;
        if index >= frames {
            state.finished = true;
            state.last = None;
            state.voice = None;
            return None;
        }
        let next = match self.recording.repeat {
            Some(repeat) if index + 1 == repeat.end_frame as usize => repeat.start_frame as usize,
            _ => (index + 1).min(frames - 1),
        };
        let fraction = (state.position % denominator) as f32 / SOURCE_RATE as f32;
        let pcm = std::array::from_fn::<_, 2, _>(|channel| {
            if self.recording.pcm.is_empty() {
                return 0.;
            }
            let channel = channel.min(self.recording.channels - 1);
            let a = self.recording.pcm[index * self.recording.channels + channel];
            let b = self.recording.pcm[next * self.recording.channels + channel];
            a + (b - a) * fraction
        });
        state.position += u64::from(self.recording.rate);
        let pan = controls.pan.unwrap_or(64);
        let pan = if pan <= 64 {
            f32::from(pan) / 128.
        } else {
            0.5 + f32::from(pan - 64) / 126.
        };
        let gains = if self.recording.channels == 1 {
            [
                (pan * std::f32::consts::FRAC_PI_2).cos(),
                (pan * std::f32::consts::FRAC_PI_2).sin(),
            ]
        } else {
            [
                if pan > 0.5 {
                    ((1. - pan) * std::f32::consts::PI).sin()
                } else {
                    1.
                },
                if pan < 0.5 {
                    (pan * std::f32::consts::PI).sin()
                } else {
                    1.
                },
            ]
        };
        let mut pcm = std::array::from_fn::<_, 2, _>(|channel| pcm[channel] * gains[channel]);
        if controls.mono {
            pcm = [(pcm[0] + pcm[1]) * 0.5; 2];
        }
        let gain = self.recording.gain
            * fade
            * controls.source_volume
            * f32::from(controls.sound_velocity.unwrap_or(127))
            / 127.;
        let buses = [
            controls.volume,
            self.recording.sends[0],
            self.recording.sends[1],
        ]
        .map(|bus| pcm.map(|sample| (sample * gain * bus * 32768.).round() as i32));
        if !state.initialized {
            if let Some(voice) = &state.voice {
                voice.rendered();
            }
            state.initialized = true;
        }
        state.last = Some(buses);
        Some(buses)
    }
}
impl Drop for WaveStream {
    fn drop(&mut self) {
        let state = self.state.get_mut().expect("recorded effect lock poisoned");
        if !state.finished
            && let Some(last) = state.last
        {
            self.synth.release(last, 0);
        }
    }
}

#[cfg(test)]
mod tests;
