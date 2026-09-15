//! Compile original resources to the device-independent musical model.
use crate::{
    bank::{Bank, MusicSetup, ObjectKind, Page},
    instrument, read, song,
};
use anyhow::{Context, Result, bail, ensure};
use resonance_audio::{
    data::{self, Command, Envelope, Event, EventKind, Interpolation, Note, Resources, Score},
    music_voice::Controls,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn programs(bank: &Bank<'_>, roots: impl IntoIterator<Item = u16>) -> Result<Resources> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut unsupported = Vec::new();
    let mut resources = Resources {
        programs: BTreeMap::new(),
        samples: BTreeMap::new(),
    };
    while let Some(id) = pending.pop() {
        if resources.programs.contains_key(&id) {
            continue;
        }
        ensure!(
            resources.programs.len() < 65536,
            "excessive instrument program count"
        );
        let bytes = bank.object(ObjectKind::Macro, id)?;
        ensure!(
            bytes.len().is_multiple_of(8) && bytes.len() <= 65536 * 8,
            "invalid instrument program length"
        );
        let mut program = Vec::new();
        for (pc, bytes) in bytes.chunks_exact(8).enumerate() {
            let a = read::u32(bytes, 0)?;
            let b = read::u32(bytes, 4)?;
            let command = match command(bank, a, b)
                .with_context(|| format!("instrument {id}, instruction {pc}: {a:08x} {b:08x}"))
            {
                Ok(command) => command,
                Err(error) => {
                    ensure!(
                        unsupported.len() < 64,
                        "too many unsupported instrument commands: {}",
                        unsupported.join("\n")
                    );
                    unsupported.push(format!("{error:#}"));
                    continue;
                }
            };
            match command {
                Command::Jump { program, .. } | Command::KeyOffTrap { program, .. } => {
                    pending.push(program)
                }
                Command::RandomBranch { program, .. }
                | Command::SpawnMacro { program, .. }
                | Command::MessageTrap { program, .. }
                    if bank.object(ObjectKind::Macro, program).is_ok() =>
                {
                    pending.push(program)
                }
                Command::StartSample { sample } => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        resources.samples.entry(sample)
                    {
                        entry.insert(bank.sample(sample)?.into());
                    }
                }
                _ => {}
            }
            program.push(command);
        }
        resources.programs.insert(id, program);
    }
    ensure!(
        unsupported.is_empty(),
        "unsupported instrument commands:\n{}",
        unsupported.join("\n")
    );
    resources.validate()?;
    Ok(resources)
}

pub(crate) fn command(bank: &Bank<'_>, a: u32, b: u32) -> Result<Command> {
    let variable = |index: u8| {
        if index & 31 < 16 {
            data::Variable::Local(index & 15)
        } else {
            data::Variable::Global(index & 15)
        }
    };
    let a = a & !0x80;
    Ok(match a as u8 {
        0 => Command::End,
        0x06 => Command::Jump {
            program: (a >> 16) as u16,
            instruction: usize::from(b as u16),
        },
        0x08 => Command::SpawnMacro {
            program: (a >> 16) as u16,
            instruction: b as u16,
            key_offset: (a >> 8) as i8,
            priority: (b >> 16) as u8,
            max_voices: (b >> 24) as u8,
        },
        0x05 | 0x0e | 0x13 | 0x15 | 0x17 | 0x60..=0x65 | 0x70 | 0x71 => {
            crate::decode::command(bank, a, b)?.mixer()?
        }
        0x0c => {
            let bytes = bank.object(ObjectKind::Table, (a >> 8) as u16)?;
            let envelope = if a >> 24 == 0 {
                Envelope::Ordinary(crate::parameters::ordinary(bytes)?)
            } else {
                Envelope::Dls(crate::parameters::dls(bytes)?)
            };
            Command::Envelope { envelope }
        }
        0x10 => crate::decode::command(bank, a, b)?.mixer()?,
        0x11 => Command::StopSample,
        0x12 => Command::Release,
        0x18 => {
            ensure!(
                b >> 16 == 0 || (b >> 8) & 1 != 0,
                "beat-based pitch-offset waits are not implemented"
            );
            Command::PitchOffset {
                from_original: a >> 24 != 0,
                semitones: (a >> 8) as i8,
                cents: (a >> 16) as i8,
                wait_ms: (b >> 16) as u16,
                from_start: b & 1 != 0,
            }
        }
        0x19 => {
            ensure!(
                b >> 16 == 0 || (b >> 8) & 1 != 0,
                "beat-based SetNote waits are not implemented"
            );
            Command::SetNote {
                key: ((a >> 8) & 127) as u8,
                cents: (a >> 16) as i8,
                wait_ms: (b >> 16) as u16,
                from_start: b & 1 != 0,
            }
        }
        0x21 => Command::ScaleVolume {
            from_velocity: a >> 24 != 0,
            factor: (a >> 8) as u16,
        },
        0x0d | 0x0f | 0x14 => crate::decode::command(bank, a, b)?.mixer()?,
        0x40..=0x4c => crate::decode::command(bank, a, b)?.mixer()?,
        0x30 => Command::AddAge {
            value: (a >> 16) as i16,
        },
        0x31 => Command::SetAge {
            value: (a >> 16) as u16,
        },
        0x38 => Command::AgePeriod { milliseconds: b },
        0x1d | 0x1e => {
            ensure!(
                (b >> 8) as u8 == 1 && b & 255 == 0,
                "unsupported pitch-sweep wait"
            );
            Command::PitchSweep {
                slot: if a as u8 == 0x1d {
                    data::SweepSlot::First
                } else {
                    data::SweepSlot::Second
                },
                step_hz: (a >> 16) as i16,
                period: (a >> 8) as u8,
                wait_ms: (b >> 16) as u16,
            }
        }
        0x20 => {
            let bytes = bank.pitch_envelope((a >> 8) as u16)?;
            let coarse = i32::from(b as i8) * 256;
            let fine = i32::from((b >> 8) as i8) * 256 / 100;
            Command::PitchEnvelope {
                envelope: crate::parameters::dls(bytes)?,
                sustain: u16::from_le_bytes(bytes[8..10].try_into()?).min(4095),
                depth_8: (if coarse < 0 {
                    coarse - fine
                } else {
                    coarse + fine
                }) as i16,
            }
        }
        0x28 => {
            let program = (a >> 16) as u16;
            let instruction = (b & 0xffff) as usize;
            match (a >> 8) as u8 {
                0 => Command::KeyOffTrap {
                    program,
                    instruction,
                },
                2 => Command::MessageTrap {
                    program,
                    instruction,
                },
                slot => bail!("unsupported instrument trap slot {slot}"),
            }
        }
        0x29 => match (a >> 8) as u8 {
            0 => Command::ClearKeyOffTrap,
            2 => Command::ClearMessageTrap,
            slot => bail!("unsupported instrument trap slot {slot}"),
        },
        0x2a => Command::SendMessage {
            target: if (a >> 8) as u8 != 0 {
                data::MessageTarget::Handle(variable(b as u8))
            } else {
                ensure!(
                    a >> 16 != 0xffff,
                    "host message callbacks are not implemented"
                );
                data::MessageTarget::Macro((a >> 16) as u16)
            },
            value: variable((b >> 8) as u8),
        },
        0x2b => Command::ReceiveMessage {
            destination: variable((a >> 8) as u8),
        },
        0x2c => Command::VoiceHandle {
            destination: variable((a >> 8) as u8),
            child: (a >> 16) as u8 != 0,
        },
        0x04 | 0x07 => crate::decode::command(bank, a, b)?.mixer()?,
        0x36 => Command::Priority {
            value: (a >> 8) as u8,
        },
        0x58 => Command::VolumeCurve {
            alternate: (a >> 8) as u8 != 0,
            interaural_delay: (a >> 16) as u8 != 0,
        },
        0x59 => Command::ExclusiveGroup {
            group: (a >> 8) as u8,
            kill: (a >> 16) as u8 != 0,
        },
        0x5a => {
            let mode = match (a >> 8) as u8 {
                0 => Interpolation::Polyphase,
                1 => Interpolation::Linear,
                2 => Interpolation::Direct,
                _ => bail!("unsupported interpolation mode"),
            };
            ensure!(((a >> 16) as u8) < 4, "invalid interpolation coefficients");
            Command::Interpolation {
                mode,
                coefficients: (a >> 16) as u8,
            }
        }
        0x22 => Command::ModulationDepth {
            semitones: (a >> 8) as i8,
            cents: (a >> 16) as i8,
        },
        0x1c => crate::decode::command(bank, a, b)?.mixer()?,
        0x50 => {
            ensure!(
                (a >> 8) as u8 == 0 && b == 0,
                "only LFO 0 with default phase is implemented"
            );
            Command::Lfo {
                period_ms: (a >> 16) as u16,
            }
        }
        0x23 => Command::Tremolo {
            scale: (a >> 8) as u16,
            modulation_scale: b as u16,
        },
        opcode => bail!("unsupported instrument opcode {opcode:#04x}"),
    })
}

pub fn music(bank: &Bank<'_>, song: &song::Song, setup: &MusicSetup) -> Result<(Resources, Score)> {
    let score = score(song, setup, |_, page, key, velocity| {
        page.map_or_else(
            || Ok(Vec::new()),
            |page| instrument::resolve(bank, page, key, velocity, 64),
        )
    })?;
    let roots: BTreeSet<_> = score
        .first_events
        .iter()
        .chain(&score.loop_events)
        .flat_map(|e| match &e.kind {
            EventKind::Notes { voices, .. } => voices.as_slice(),
            _ => &[],
        })
        .map(|v| v.macro_id)
        .collect();
    let resources = self::programs(bank, roots)?;
    score.validate(&resources)?;
    Ok((resources, score))
}

/// Bind every note to its cooked instrument voices. Resolver indices refer to
/// all original events in first-traversal then loop order, including events
/// which only change program state and produce no output command.
pub fn score(
    song: &song::Song,
    setup: &MusicSetup,
    mut resolve: impl FnMut(usize, Option<Page>, u8, u8) -> Result<Vec<Note>>,
) -> Result<Score> {
    let (loop_start_tick, end_tick) = song.playback_interval()?;
    let mut programs = setup.channels.map(|c| c.program);
    let first = song.events();
    let first_events = events(setup, &first, 0, &mut programs, &mut resolve)?;
    let loop_events = events(
        setup,
        &song.loop_events()?,
        first.len(),
        &mut programs,
        &mut resolve,
    )?;
    Ok(Score {
        origin: data::ScoreOrigin::Sequence,
        initial_bpm_1024: song.initial_bpm_1024,
        loop_start_tick,
        end_tick,
        has_master_track: song.has_master_track,
        tempos: song
            .tempos
            .iter()
            .map(|t| data::Tempo {
                tick: t.tick,
                bpm_1024: t.bpm_1024,
            })
            .collect(),
        controls: setup.channels.map(|c| {
            let mut controls = Controls::default();
            controls.set_coarse(7, c.volume);
            controls.set_coarse(10, c.pan);
            controls.post = [c.reverb, c.chorus];
            controls
        }),
        first_events,
        loop_events,
    })
}

fn events(
    setup: &MusicSetup,
    events: &[song::Event],
    first_index: usize,
    programs: &mut [u8; 16],
    resolve: &mut impl FnMut(usize, Option<Page>, u8, u8) -> Result<Vec<Note>>,
) -> Result<Vec<Event>> {
    let mut output = Vec::new();
    for (index, event) in events.iter().enumerate() {
        use song::EventKind as Original;
        let current_program = programs
            .get_mut(usize::from(event.channel))
            .with_context(|| {
                format!(
                    "invalid song channel {} at tick {}",
                    event.channel, event.tick
                )
            })?;
        let kind = match event.kind {
            Original::Pattern { program, volume } => {
                if let Some(program) = program
                    && setup.page(event.channel, program).is_some()
                {
                    *current_program = program;
                }
                let Some(value) = volume else {
                    continue;
                };
                EventKind::Volume { value }
            }
            Original::Command { command: 0, value } => {
                if setup.page(event.channel, value).is_some() {
                    *current_program = value;
                }
                continue;
            }
            Original::Command { command, value } => match command {
                135 => EventKind::Volume { value },
                138 => EventKind::Pan { value },
                139 => EventKind::Expression { value },
                219 => EventKind::Auxiliary { bus: 0, value },
                221 => EventKind::Auxiliary { bus: 1, value },
                _ => bail!("unsupported score command {command} at tick {}", event.tick),
            },
            Original::PitchBend(value) => EventKind::PitchBend { value },
            Original::Modulation(value) => EventKind::Modulation { value },
            Original::Note {
                key,
                velocity,
                length,
            } => {
                let voices = resolve(
                    first_index + index,
                    setup.page(event.channel, *current_program),
                    key,
                    velocity,
                )?;
                EventKind::Notes {
                    source: data::VoiceSource::Sequence {
                        group: setup.group,
                        program: *current_program,
                        drums: event.channel == 9,
                    },
                    voices,
                    length,
                }
            }
        };
        output.push(Event {
            tick: event.tick,
            channel: event.channel,
            kind,
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrangement() -> (song::Song, MusicSetup) {
        use song::EventKind::{Command, Modulation, Note, Pattern};
        let note = |key| Note {
            key,
            velocity: key + 30,
            length: 2,
        };
        let song = song::Song {
            has_master_track: false,
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 2,
            tempos: Vec::new(),
            tracks: vec![song::Track {
                id: 0,
                channel: 0,
                end_tick: 8,
                loop_region: Some(1),
                regions: vec![1, 2],
                events: [
                    note(60),
                    Pattern {
                        program: Some(2),
                        volume: None,
                    },
                    Pattern {
                        program: None,
                        volume: Some(100),
                    },
                    note(61),
                    Command {
                        command: 0,
                        value: 5,
                    },
                    note(62),
                    Command {
                        command: 0,
                        value: 99,
                    },
                    Modulation(100),
                ]
                .into_iter()
                .enumerate()
                .map(|(tick, kind)| song::Event {
                    tick: tick as u32,
                    track: 0,
                    channel: 0,
                    kind,
                })
                .collect(),
            }],
        };
        let setup = MusicSetup {
            group: 0,
            normal: [2, 5]
                .map(|program| {
                    (
                        program,
                        Page {
                            object: u16::from(program) + 10,
                            priority: 64,
                            max_voices: 8,
                        },
                    )
                })
                .into(),
            drums: BTreeMap::new(),
            channels: [crate::bank::Channel::default(); 16],
        };
        (song, setup)
    }

    #[test]
    fn score_binding_counts_skipped_events_and_keeps_program_state_across_loops() -> Result<()> {
        let (song, setup) = arrangement();
        let mut calls = Vec::new();
        let score = score(&song, &setup, |index, page, key, velocity| {
            calls.push((index, page.map(|page| page.object), key, velocity));
            Ok(page
                .into_iter()
                .map(|page| Note {
                    macro_id: page.object,
                    key,
                    velocity,
                    pan: 64,
                    priority: page.priority,
                    max_voices: page.max_voices,
                })
                .collect())
        })?;
        assert_eq!(score.origin, data::ScoreOrigin::Sequence);
        assert_eq!(
            calls,
            [
                (0, None, 60, 90),
                (3, Some(12), 61, 91),
                (5, Some(15), 62, 92),
                (9, Some(15), 61, 91),
                (11, Some(15), 62, 92),
            ]
        );
        assert_eq!(
            score
                .first_events
                .iter()
                .map(|e| e.tick)
                .collect::<Vec<_>>(),
            [0, 2, 3, 5, 7]
        );
        assert_eq!(
            score.loop_events.iter().map(|e| e.tick).collect::<Vec<_>>(),
            [2, 3, 5, 7]
        );
        assert!(
            matches!(&score.first_events[0].kind, EventKind::Notes { voices, .. } if voices.is_empty())
        );
        let sources = |events: &[Event]| {
            events
                .iter()
                .filter_map(|event| match event.kind {
                    EventKind::Notes { source, .. } => Some(source),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let source = |program| data::VoiceSource::Sequence {
            group: setup.group,
            program,
            drums: false,
        };
        assert_eq!(
            sources(&score.first_events),
            [source(0), source(2), source(5)]
        );
        assert_eq!(sources(&score.loop_events), [source(5), source(5)]);
        Ok(())
    }

    #[test]
    fn score_binding_rejects_invalid_deserialized_channels() {
        let (mut song, setup) = arrangement();
        for channel in [16, 255] {
            song.tracks[0].events[1].channel = channel;
            let error = score(&song, &setup, |_, _, _, _| Ok(Vec::new()))
                .err()
                .unwrap();
            assert_eq!(
                error.to_string(),
                format!("invalid song channel {channel} at tick 1")
            );
        }
    }
}
