//! Cooked musical data. No original bank offsets, bytecode words or pointers.
use crate::{dls, envelope, music_voice::Controls, sample::Sample};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn is_false(value: &bool) -> bool {
    !value
}

fn is_zero(value: &u16) -> bool {
    *value == 0
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variable {
    Local(u8),
    Global(u8),
    Controller(Controller),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Controller {
    Paired(u8),
    PitchBend,
    Surround,
    Lfo,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operand {
    Variable(Variable),
    Constant(i16),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arithmetic {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Arithmetic {
    pub(crate) fn evaluate(self, left: i16, right: i16) -> i16 {
        let (left, right) = (i32::from(left), i32::from(right));
        match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide => left.checked_div(right).unwrap_or(0),
        }
        .clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Equal,
    Less,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub macro_id: u16,
    pub key: u8,
    pub velocity: u8,
    pub pan: u8,
    pub priority: u8,
    pub max_voices: u8,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "parameters", rename_all = "snake_case")]
pub enum Envelope {
    Ordinary(envelope::Parameters),
    Dls(dls::Definition),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Polyphase,
    Linear,
    Direct,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanAxis {
    Pan,
    Surround,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepSlot {
    #[default]
    First,
    Second,
}

impl SweepSlot {
    fn is_first(&self) -> bool {
        matches!(self, Self::First)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct VolumeCurve(#[serde(with = "crate::package::array")] pub [u8; 128]);

impl VolumeCurve {
    /// Interpolate a clamped 16.16 input. Authored output may exceed 127.
    pub(crate) fn translate(&self, volume: u32) -> u32 {
        let index = (volume >> 16) as usize;
        let current = i32::from(self.0[index]);
        let next = i32::from(self.0[(index + 1).min(127)]);
        ((current << 16) + (volume & 65535) as i32 * (next - current)) as u32
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageTarget {
    Handle(Variable),
    Macro(u16),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Command {
    VoiceHandle {
        destination: Variable,
        child: bool,
    },
    SendMessage {
        target: MessageTarget,
        value: Variable,
    },
    ReceiveMessage {
        destination: Variable,
    },
    MessageTrap {
        program: u16,
        instruction: usize,
    },
    ClearMessageTrap,
    Noop,
    End,
    SetVariable {
        destination: Variable,
        value: i16,
    },
    Calculate {
        destination: Variable,
        #[serde(rename = "arithmetic")]
        operation: Arithmetic,
        left: Variable,
        right: Operand,
    },
    Branch {
        comparison: Comparison,
        left: Variable,
        right: Variable,
        invert: bool,
        instruction: usize,
    },
    Jump {
        program: u16,
        instruction: usize,
    },
    SpawnMacro {
        program: u16,
        instruction: u16,
        key_offset: i8,
        priority: u8,
        max_voices: u8,
    },
    RandomBranch {
        minimum: u8,
        program: u16,
        instruction: usize,
    },
    Loop {
        instruction: usize,
        count: u16,
        key_off: bool,
        sample_end: bool,
    },
    RandomLoop {
        instruction: usize,
        /// Exclusive upper bound, sampled only when entering the loop.
        count: u16,
        key_off: bool,
        sample_end: bool,
    },
    Envelope {
        envelope: Envelope,
    },
    PitchEnvelope {
        envelope: dls::Definition,
        sustain: u16,
        depth_8: i16,
    },
    StartSample {
        sample: u16,
    },
    StopSample,
    Release,
    PitchOffset {
        from_original: bool,
        semitones: i8,
        cents: i8,
        #[serde(default, skip_serializing_if = "is_zero")]
        wait_ms: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        from_start: bool,
    },
    SetNote {
        key: u8,
        cents: i8,
        #[serde(default, skip_serializing_if = "is_zero")]
        wait_ms: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        from_start: bool,
    },
    RandomNote {
        low: u8,
        high: u8,
        cents: i8,
        random_cents: bool,
        relative: bool,
    },
    PanRamp {
        axis: PanAxis,
        initial: u8,
        delta: i8,
        milliseconds: u16,
    },
    ScaleVolume {
        from_velocity: bool,
        factor: u16,
    },
    SetVolume {
        factor: u8,
        offset: u8,
        curve: Option<VolumeCurve>,
        from_velocity: bool,
    },
    FadeVolume {
        factor: u8,
        offset: u8,
        curve: Option<VolumeCurve>,
        milliseconds: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        from_silence: bool,
    },
    Auxiliary {
        bus: u8,
        value: u8,
    },
    VolumeControl {
        value: u16,
    },
    SetAge {
        value: u16,
    },
    AddAge {
        value: i16,
    },
    AgePeriod {
        milliseconds: u32,
    },
    PitchSweep {
        #[serde(default, skip_serializing_if = "SweepSlot::is_first")]
        slot: SweepSlot,
        step_hz: i16,
        period: u8,
        wait_ms: u16,
    },
    KeyOffTrap {
        program: u16,
        instruction: usize,
    },
    ClearKeyOffTrap,
    Wait {
        milliseconds: Option<u16>,
        from_start: bool,
        key_off: bool,
        sample_end: bool,
    },
    BeatWait {
        ticks: Option<u16>,
        key_off: bool,
        sample_end: bool,
    },
    /// Uniform duration below the authored bound, drawn from the shared synthesizer.
    RandomWait {
        upper_ms: u16,
        key_off: bool,
    },
    Priority {
        value: u8,
    },
    /// Starting this group ends or releases older voices in the same group.
    ExclusiveGroup {
        group: u8,
        kill: bool,
    },
    VolumeCurve {
        alternate: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        interaural_delay: bool,
    },
    Interpolation {
        mode: Interpolation,
        coefficients: u8,
    },
    ModulationDepth {
        semitones: i8,
        cents: i8,
    },
    Vibrato {
        period_ms: u16,
        /// Unsigned 8.8 semitones after authored sign normalization.
        depth_8: u16,
        /// Start half a cycle ahead.
        reverse: bool,
        scale_by_modulation: bool,
    },
    Lfo {
        period_ms: u16,
    },
    TremoloFromLfo,
    Tremolo {
        scale: u16,
        modulation_scale: u16,
    },
}

pub struct Resources {
    pub programs: BTreeMap<u16, Vec<Command>>,
    pub samples: BTreeMap<u16, std::sync::Arc<Sample>>,
}

impl Command {
    pub(crate) fn variables(&self) -> [Option<Variable>; 3] {
        match *self {
            Self::SetVariable { destination, .. }
            | Self::VoiceHandle { destination, .. }
            | Self::ReceiveMessage { destination } => [Some(destination), None, None],
            Self::SendMessage { target, value } => [
                Some(value),
                match target {
                    MessageTarget::Handle(variable) => Some(variable),
                    MessageTarget::Macro(_) => None,
                },
                None,
            ],
            Self::Calculate {
                destination,
                left,
                right,
                ..
            } => [
                Some(destination),
                Some(left),
                match right {
                    Operand::Variable(variable) => Some(variable),
                    Operand::Constant(_) => None,
                },
            ],
            Self::Branch { left, right, .. } => [Some(left), Some(right), None],
            _ => [None; 3],
        }
    }
}

impl Resources {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.programs.is_empty()
                && self.programs.len() <= 65536
                && self.samples.len() <= 65536,
            "invalid music resource count"
        );
        ensure!(
            self.programs.values().map(Vec::len).sum::<usize>() <= 1_000_000,
            "music program budget exceeded"
        );
        for program in self.programs.values() {
            ensure!(
                !program.is_empty() && program.len() <= 65536,
                "invalid instrument program length"
            );
            for command in program {
                for variable in command.variables().into_iter().flatten() {
                    match variable {
                        Variable::Local(index) | Variable::Global(index) => {
                            ensure!(index < 16, "macro variable index exceeds register bank")
                        }
                        Variable::Controller(Controller::Paired(index)) => {
                            ensure!(index < 32, "paired controller index exceeds bank")
                        }
                        Variable::Controller(_) => {}
                    }
                }
                match *command {
                    Command::SetVariable {
                        destination: Variable::Controller(Controller::Paired(6)),
                        ..
                    }
                    | Command::Calculate {
                        destination: Variable::Controller(Controller::Paired(6)),
                        ..
                    } => {
                        anyhow::bail!("RPN data-entry controller writes are not implemented");
                    }
                    Command::SendMessage {
                        target: MessageTarget::Macro(u16::MAX),
                        ..
                    } => {
                        anyhow::bail!("host message callbacks are not implemented");
                    }
                    Command::Jump {
                        program,
                        instruction,
                    }
                    | Command::KeyOffTrap {
                        program,
                        instruction,
                    } => {
                        ensure!(
                            instruction < self.program(program)?.len(),
                            "instrument jump target is out of bounds"
                        );
                    }
                    Command::StartSample { sample } => {
                        self.sample(sample)?;
                    }
                    Command::RandomBranch {
                        program,
                        instruction,
                        ..
                    }
                    | Command::MessageTrap {
                        program,
                        instruction,
                    } => {
                        ensure!(
                            self.programs
                                .get(&program)
                                .is_none_or(|commands| instruction < commands.len()),
                            "instrument conditional target is out of bounds"
                        );
                    }
                    Command::SpawnMacro {
                        program,
                        instruction,
                        ..
                    } => {
                        ensure!(
                            self.programs
                                .get(&program)
                                .is_none_or(|commands| usize::from(instruction) < commands.len()),
                            "child macro entry is out of bounds"
                        );
                    }
                    Command::Loop { instruction, .. }
                    | Command::RandomLoop { instruction, .. }
                    | Command::Branch { instruction, .. } => {
                        ensure!(
                            instruction < program.len(),
                            "instrument loop target is out of bounds"
                        );
                        ensure!(
                            !matches!(command, Command::RandomLoop { count: 0, .. }),
                            "random loop requires a nonzero bound"
                        );
                    }
                    Command::Interpolation { coefficients, .. } => {
                        ensure!(coefficients < 4, "invalid interpolation coefficients")
                    }
                    Command::PitchEnvelope { sustain, .. } => {
                        ensure!(sustain <= 4095, "invalid pitch envelope sustain")
                    }
                    Command::SetNote { key, .. } => ensure!(key < 128, "invalid instrument key"),
                    Command::Auxiliary { bus, value } => ensure!(
                        bus < 2 && value < 128,
                        "invalid instrument auxiliary control"
                    ),
                    Command::VolumeControl { value } => {
                        ensure!(value < 16384, "invalid volume selector")
                    }
                    Command::Envelope {
                        envelope: Envelope::Ordinary(p),
                    } => ensure!(p.sustain <= 32767, "invalid ordinary sustain"),
                    Command::Envelope {
                        envelope: Envelope::Dls(p),
                    } => ensure!(p.sustain_index <= 128, "invalid DLS sustain index"),
                    _ => {}
                }
            }
        }
        let mut total = 0usize;
        for sample in self.samples.values() {
            ensure!(
                sample.key < 128 && sample.rate > 0 && !sample.pcm.is_empty(),
                "invalid instrument sample"
            );
            total = total
                .checked_add(sample.pcm.len())
                .and_then(|n| n.checked_add(sample.loop_pcm.len()))
                .context("sample size overflow")?;
            ensure!(total <= 32_000_000, "instrument sample budget exceeded");
            crate::resample::SampleCursor::new(sample)?;
        }
        Ok(())
    }
    pub fn program(&self, id: u16) -> Result<&[Command]> {
        self.programs
            .get(&id)
            .map(Vec::as_slice)
            .context("missing instrument program")
    }
    pub fn sample(&self, id: u16) -> Result<&Sample> {
        self.samples
            .get(&id)
            .map(std::sync::Arc::as_ref)
            .context("missing instrument sample")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Notes {
        source: VoiceSource,
        voices: Vec<Note>,
        length: u16,
    },
    Volume {
        value: u8,
    },
    Pan {
        value: u8,
    },
    Expression {
        value: u8,
    },
    Auxiliary {
        bus: u8,
        value: u8,
    },
    PitchBend {
        value: u16,
    },
    Modulation {
        value: u16,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub tick: u32,
    pub channel: u8,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Tempo {
    pub tick: u32,
    pub bpm_1024: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreOrigin {
    /// Arrangement events run in newest-sequence-first order.
    Sequence,
    /// Entry notes allocate in request order before arrangement events.
    SoundEffect,
}

/// Allocation limits apply to the authored instrument or sound, across cues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceSource {
    Sequence {
        group: u16,
        program: u8,
        drums: bool,
    },
    SoundEffect {
        id: u16,
    },
}

impl VoiceSource {
    pub fn origin(self) -> ScoreOrigin {
        match self {
            Self::Sequence { .. } => ScoreOrigin::Sequence,
            Self::SoundEffect { .. } => ScoreOrigin::SoundEffect,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Score {
    pub origin: ScoreOrigin,
    pub initial_bpm_1024: u32,
    pub loop_start_tick: u32,
    /// Inclusive time at which all queued events have finished and looping may resume.
    pub end_tick: u32,
    pub has_master_track: bool,
    pub tempos: Vec<Tempo>,
    pub controls: [Controls; 16],
    pub first_events: Vec<Event>,
    pub loop_events: Vec<Event>,
}

impl Score {
    pub fn validate(&self, resources: &Resources) -> Result<()> {
        if self.origin == ScoreOrigin::SoundEffect {
            ensure!(
                self.tempos.is_empty()
                    && self.loop_events.is_empty()
                    && self.first_events.iter().all(|event| {
                        event.tick == 0 && matches!(&event.kind, EventKind::Notes { .. })
                    }),
                "sound effects require entry notes; timed events belong to sequences"
            );
        }
        ensure!(
            self.loop_start_tick < self.end_tick && self.end_tick < u32::MAX - 65536,
            "invalid musical loop interval"
        );
        ensure!(
            (1..=1024 * 1000).contains(&self.initial_bpm_1024),
            "invalid initial tempo"
        );
        ensure!(
            self.tempos.len() <= 1_000_000
                && self.tempos.windows(2).all(|w| w[0].tick <= w[1].tick)
                && self
                    .tempos
                    .iter()
                    .all(|t| (1..=1024 * 1000).contains(&t.bpm_1024)),
            "invalid tempo stream"
        );
        for control in self.controls {
            control.validate()?;
        }
        for (events, start) in [
            (&self.first_events, 0),
            (&self.loop_events, self.loop_start_tick),
        ] {
            ensure!(
                events.len() <= 1_000_000 && events.windows(2).all(|w| w[0].tick <= w[1].tick),
                "invalid score event order"
            );
            for event in events {
                ensure!(
                    event.channel < 16 && (start..=self.end_tick).contains(&event.tick),
                    "invalid score event location"
                );
                match &event.kind {
                    EventKind::Notes { source, voices, .. } => {
                        ensure!(
                            source.origin() == self.origin,
                            "note allocation source differs from score origin"
                        );
                        ensure!(voices.len() <= 64, "note exceeds voice budget");
                        for note in voices {
                            ensure!(
                                note.key < 128 && note.velocity < 128 && note.pan < 128,
                                "invalid note controls"
                            );
                            resources.program(note.macro_id)?;
                        }
                    }
                    EventKind::Volume { value }
                    | EventKind::Pan { value }
                    | EventKind::Expression { value } => {
                        ensure!(*value < 128, "invalid channel control")
                    }
                    EventKind::Auxiliary { bus, value } => {
                        ensure!(*bus < 2 && *value < 128, "invalid auxiliary control")
                    }
                    EventKind::PitchBend { value } | EventKind::Modulation { value } => {
                        ensure!(*value < 16384, "invalid high-resolution control")
                    }
                }
            }
        }
        Ok(())
    }
}
