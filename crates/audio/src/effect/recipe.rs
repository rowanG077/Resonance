//! Only authored effect instructions and entry voices belong in each cue.
use super::*;
use crate::{
    data::{Command, Event, EventKind, Note, Score, ScoreOrigin, VoiceSource},
    music_voice::Controls,
    package::editable::Sample,
};
use std::collections::BTreeMap;

pub const TABLES: &str = "audio/synthesis.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[serde(deserialize_with = "crate::package::editable::id_table")]
    pub programs: BTreeMap<u16, ProgramSource>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::package::editable::id_table"
    )]
    pub samples: BTreeMap<u16, Sample>,
    pub voices: Vec<Voices>,
    #[serde(default, skip_serializing_if = "zero")]
    pub duration_ticks: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "resonance_content::field_audio::reverb::optional"
    )]
    pub reverbs: Option<[[f32; 5]; 2]>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgramSource {
    Inline(Vec<Command>),
    Shared(String),
}

/// One reusable program with its direct sample bindings. It cannot include
/// another document; calls resolve within the consuming cue's program table.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedProgram {
    pub instructions: Vec<Command>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::package::editable::id_table"
    )]
    pub samples: BTreeMap<u16, Sample>,
}

/// Voices started together. The group shares allocation limits across cue uses;
/// timing and release behavior live in the programs, not sequence wrappers.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voices {
    pub group: u16,
    pub notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "channel_zero")]
    pub channel: u8,
}

fn channel_zero(value: &u8) -> bool {
    *value == 0
}

fn zero(value: &u32) -> bool {
    *value == 0
}

impl Recipe {
    pub fn from_program(program: &Program) -> Result<Self> {
        let score = &program.score;
        ensure!(
            score.origin == ScoreOrigin::SoundEffect
                && score.initial_bpm_1024 == 120 * 1024
                && score.loop_start_tick == 0
                && score.tempos.is_empty()
                && score.loop_events.is_empty()
                && score.controls.iter().all(|control| {
                    // Output mode and source gain are live controls, not authored data.
                    Controls {
                        mono: false,
                        source_volume: 1.,
                        ..*control
                    } == Controls::default()
                }),
            "effect recipe requires entry voices and default sequence controls"
        );
        Ok(Self {
            programs: program
                .programs
                .iter()
                .map(|(&id, code)| (id, ProgramSource::Inline(code.clone())))
                .collect(),
            samples: program.samples.clone(),
            voices: score
                .first_events
                .iter()
                .map(|event| {
                    ensure!(event.tick == 0, "effects start their voices immediately");
                    let EventKind::Notes {
                        source: VoiceSource::SoundEffect { id },
                        voices,
                        ..
                    } = &event.kind
                    else {
                        anyhow::bail!("effect recipe requires sound-effect entry voices");
                    };
                    // Arrangement note lengths never release sound effects;
                    // their programs and live controls own voice lifetime.
                    Ok(Voices {
                        group: *id,
                        notes: voices.clone(),
                        channel: event.channel,
                    })
                })
                .collect::<Result<_>>()?,
            duration_ticks: score.end_tick,
            reverbs: program.reverbs,
        })
    }

    pub fn expand(
        mut self,
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
    ) -> Result<Program> {
        let mut programs = BTreeMap::new();
        for (id, source) in self.programs {
            let instructions = match source {
                ProgramSource::Inline(code) => code,
                ProgramSource::Shared(path) => {
                    crate::package::relative_path(&path)?;
                    let shared: SharedProgram =
                        serde_json::from_slice(&read(&path, 16 * 1024 * 1024)?)
                            .with_context(|| format!("shared effect program {path}"))?;
                    for (sample, binding) in shared.samples {
                        if let Some(previous) = self.samples.get(&sample) {
                            ensure!(
                                previous == &binding,
                                "conflicting sample {sample} in shared effect program {path}"
                            );
                        } else {
                            self.samples.insert(sample, binding);
                        }
                    }
                    shared.instructions
                }
            };
            programs.insert(id, instructions);
        }
        Ok(Program {
            programs,
            samples: self.samples,
            score: Score {
                origin: ScoreOrigin::SoundEffect,
                initial_bpm_1024: 120 * 1024,
                loop_start_tick: 0,
                end_tick: self.duration_ticks,
                tempos: vec![],
                controls: [Controls::default(); 16],
                first_events: self
                    .voices
                    .into_iter()
                    .map(|voices| Event {
                        tick: 0,
                        channel: voices.channel,
                        kind: EventKind::Notes {
                            source: VoiceSource::SoundEffect { id: voices.group },
                            voices: voices.notes,
                            length: 0,
                        },
                    })
                    .collect(),
                loop_events: vec![],
            },
            tables: serde_json::from_slice(&read(TABLES, 4 * 1024 * 1024)?)?,
            reverbs: self.reverbs,
        })
    }
}
