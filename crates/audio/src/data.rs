//! Cooked musical data. No original bank offsets, bytecode words or pointers.
use crate::{dls, envelope, music_voice::Controls, sample::Sample};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn is_false(value: &bool) -> bool {
    !value
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Polyphase,
    Linear,
    Direct,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Command {
    End,
    Jump {
        program: u16,
        instruction: usize,
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
    },
    SetNote {
        key: u8,
        cents: i8,
    },
    ScaleVolume {
        from_velocity: bool,
        factor: u16,
    },
    SetVolume {
        factor: u8,
        offset: u8,
        from_velocity: bool,
    },
    FadeVolume {
        factor: u8,
        offset: u8,
        milliseconds: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        from_silence: bool,
    },
    Auxiliary {
        bus: u8,
        value: u8,
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
        semitones: u8,
        cents: u8,
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
                match *command {
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
                    Command::Interpolation { coefficients, .. } => {
                        ensure!(coefficients < 4, "invalid interpolation coefficients")
                    }
                    Command::Vibrato {
                        semitones, cents, ..
                    } => ensure!(semitones < 128 && cents < 128, "invalid vibrato depths"),
                    Command::PitchEnvelope { sustain, .. } => {
                        ensure!(sustain <= 4095, "invalid pitch envelope sustain")
                    }
                    Command::SetNote { key, .. } => ensure!(key < 128, "invalid instrument key"),
                    Command::Auxiliary { bus, value } => ensure!(
                        bus < 2 && value < 128,
                        "invalid instrument auxiliary control"
                    ),
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
    Notes { voices: Vec<Note>, length: u16 },
    Volume { value: u8 },
    Pan { value: u8 },
    Expression { value: u8 },
    Auxiliary { bus: u8, value: u8 },
    PitchBend { value: u16 },
    Modulation { value: u16 },
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

#[derive(Clone, Serialize, Deserialize)]
pub struct Score {
    pub initial_bpm_1024: u32,
    pub loop_start_tick: u32,
    pub end_tick: u32,
    pub has_master_track: bool,
    pub tempos: Vec<Tempo>,
    pub controls: [Controls; 16],
    pub first_events: Vec<Event>,
    pub loop_events: Vec<Event>,
}

impl Score {
    pub fn validate(&self, resources: &Resources) -> Result<()> {
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
            ensure!(
                [
                    control.volume,
                    control.expression,
                    control.pan,
                    control.post[0],
                    control.post[1]
                ]
                .into_iter()
                .all(|v| v < 128)
                    && control.pitch_bend < 16384
                    && control.modulation < 16384
                    && control.group_volume.is_finite()
                    && (0.0..=1.0).contains(&control.group_volume),
                "invalid initial music controls"
            );
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
                    event.channel < 16 && (start..self.end_tick).contains(&event.tick),
                    "invalid score event location"
                );
                match &event.kind {
                    EventKind::Notes { voices, .. } => {
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
