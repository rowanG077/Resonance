//! Compile original resources to the device-independent musical model.
use crate::{
    bank::{Bank, MusicSetup, ObjectKind},
    instrument, read, song,
};
use anyhow::{Context, Result, bail, ensure};
use resonance_audio::{
    data::{self, Command, Envelope, Event, EventKind, Interpolation, Resources, Score},
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

fn command(bank: &Bank<'_>, a: u32, b: u32) -> Result<Command> {
    Ok(match a as u8 {
        0 => Command::End,
        0x06 => Command::Jump {
            program: (a >> 16) as u16,
            instruction: b as usize,
        },
        0x0c => {
            let bytes = bank.object(ObjectKind::Table, (a >> 8) as u16)?;
            let envelope = if a >> 24 == 0 {
                Envelope::Ordinary(crate::parameters::ordinary(bytes)?)
            } else {
                Envelope::Dls(crate::parameters::dls(bytes)?)
            };
            Command::Envelope { envelope }
        }
        0x10 => {
            ensure!(
                a >> 24 == 0 && b == 0,
                "music sample offsets are not implemented"
            );
            Command::StartSample {
                sample: (a >> 8) as u16,
            }
        }
        0x11 => Command::StopSample,
        0x12 => Command::Release,
        0x18 => {
            ensure!(
                b >> 16 == 0,
                "embedded pitch-offset waits are not implemented"
            );
            Command::PitchOffset {
                from_original: a >> 24 != 0,
                semitones: (a >> 8) as i8,
                cents: (a >> 16) as i8,
            }
        }
        0x19 => {
            ensure!(b >> 16 == 0, "embedded SetNote waits are not implemented");
            Command::SetNote {
                key: ((a >> 8) & 127) as u8,
                cents: (a >> 16) as i8,
            }
        }
        0x21 => Command::ScaleVolume {
            from_velocity: a >> 24 != 0,
            factor: (a >> 8) as u16,
        },
        0x0d | 0x0f => {
            ensure!(
                ((a >> 24) | ((b & 255) << 8)) == 65535,
                "custom volume curves are not implemented"
            );
            let factor = (a >> 8) as u8;
            let offset = (a >> 16) as u8;
            if a as u8 == 0x0d {
                Command::SetVolume {
                    factor,
                    offset,
                    from_velocity: (b >> 8) as u8 != 0,
                }
            } else {
                ensure!(
                    (b >> 8) as u8 == 1,
                    "beat-based volume fades are not implemented"
                );
                Command::FadeVolume {
                    factor,
                    offset,
                    milliseconds: (b >> 16) as u16,
                }
            }
        }
        0x4c => {
            ensure!(
                (b >> 8) as u8 == 1 && b & 255 == 0 && a >> 16 == 0 && b >> 16 == 0,
                "only the zero-scale auxiliary variable selector is implemented"
            );
            // A zero-scale signed selector yields the 14-bit midpoint 0x2000.
            // Cook it as 64: playback expands seven-bit controls by shifting left seven.
            Command::Auxiliary { bus: 1, value: 64 }
        }
        0x30 => Command::AddAge {
            value: (a >> 16) as i16,
        },
        0x31 => Command::SetAge {
            value: (a >> 16) as u16,
        },
        0x38 => Command::AgePeriod { milliseconds: b },
        0x1d => {
            ensure!(
                (b >> 8) as u8 == 1 && b & 255 == 0,
                "unsupported pitch-sweep wait"
            );
            Command::PitchSweep {
                step_hz: (a >> 16) as i16,
                period: (a >> 8) as u8,
                wait_ms: (b >> 16) as u16,
            }
        }
        0x20 => {
            let bytes = bank.object(ObjectKind::Table, (a >> 8) as u16)?;
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
            ensure!((a >> 8) as u8 == 0, "only key-off traps are implemented");
            Command::KeyOffTrap {
                program: (a >> 16) as u16,
                instruction: b as usize,
            }
        }
        0x29 => {
            ensure!((a >> 8) as u8 == 0, "only key-off traps are implemented");
            Command::ClearKeyOffTrap
        }
        0x07 => {
            ensure!(
                (a >> 16) as u8 == 0,
                "random instrument waits are not implemented"
            );
            Command::Wait {
                milliseconds: ((b >> 16) != 65535).then_some((b >> 16) as u16),
                from_start: b & 1 != 0,
                key_off: (a >> 8) & 1 != 0,
                sample_end: (a >> 24) & 1 != 0,
            }
        }
        0x36 => Command::Priority {
            value: (a >> 8) as u8,
        },
        0x58 => {
            ensure!((a >> 16) as u8 == 0, "unsupported pedal policy");
            Command::VolumeCurve {
                alternate: (a >> 8) as u8 != 0,
            }
        }
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
        0x1c => {
            ensure!(
                (b >> 8) & 1 != 0,
                "beat-based vibrato periods are not implemented"
            );
            ensure!(
                ((a >> 8) as u8) < 128 && ((a >> 16) as u8) < 128,
                "negative vibrato depths are not implemented"
            );
            Command::Vibrato {
                period_ms: (b >> 16) as u16,
                semitones: (a >> 8) as u8,
                cents: (a >> 16) as u8,
                scale_by_modulation: (a >> 24) & 3 != 0,
            }
        }
        0x50 => {
            ensure!(
                (a >> 8) as u8 == 0 && b == 0,
                "only LFO 0 with default phase is implemented"
            );
            Command::Lfo {
                period_ms: (a >> 16) as u16,
            }
        }
        0x49 => {
            ensure!(a == 0x00648249 && b == 0, "unsupported tremolo selector");
            Command::TremoloFromLfo
        }
        0x23 => Command::Tremolo {
            scale: (a >> 8) as u16,
            modulation_scale: b as u16,
        },
        opcode => bail!("unsupported instrument opcode {opcode:#04x}"),
    })
}

pub fn music(bank: &Bank<'_>, song: &song::Song, setup: &MusicSetup) -> Result<(Resources, Score)> {
    let (loop_start_tick, end_tick) = song.loop_interval()?;
    let mut programs = setup.channels.map(|c| c.program);
    let first_events = events(bank, setup, &song.events(), &mut programs)?;
    let loop_events = events(bank, setup, &song.loop_events()?, &mut programs)?;
    let roots: BTreeSet<_> = first_events
        .iter()
        .chain(&loop_events)
        .flat_map(|e| match &e.kind {
            EventKind::Notes { voices, .. } => voices.as_slice(),
            _ => &[],
        })
        .map(|v| v.macro_id)
        .collect();
    let resources = self::programs(bank, roots)?;
    let score = Score {
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
        controls: setup.channels.map(|c| Controls {
            volume: c.volume,
            pan: c.pan,
            post: [c.reverb, c.chorus],
            ..Default::default()
        }),
        first_events,
        loop_events,
    };
    score.validate(&resources)?;
    Ok((resources, score))
}

fn events(
    bank: &Bank<'_>,
    setup: &MusicSetup,
    events: &[song::Event],
    programs: &mut [u8; 16],
) -> Result<Vec<Event>> {
    let mut output = Vec::new();
    for event in events {
        use song::EventKind as Original;
        let channel = usize::from(event.channel);
        let kind = match event.kind {
            Original::Pattern { program, volume } => {
                if let Some(program) = program
                    && setup.page(event.channel, program).is_some()
                {
                    programs[channel] = program;
                }
                let Some(value) = volume else {
                    continue;
                };
                EventKind::Volume { value }
            }
            Original::Command { command: 0, value } => {
                if setup.page(event.channel, value).is_some() {
                    programs[channel] = value;
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
                let voices = if let Some(page) = setup.page(event.channel, programs[channel]) {
                    instrument::resolve(bank, page, key, velocity, 64)?
                } else {
                    Vec::new()
                };
                EventKind::Notes { voices, length }
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
