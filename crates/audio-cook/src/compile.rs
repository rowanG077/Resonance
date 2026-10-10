//! Compile original resources to the device-independent musical model.
use crate::{
    bank::{Bank, MusicSetup, Page},
    instrument, song,
};
use anyhow::{Context, Result, bail};
use resonance_audio::{
    data::{self, Event, EventKind, Note, Resources, Score},
    music_voice::Controls,
};
use std::collections::BTreeSet;

pub use crate::decode::programs;

pub fn music(
    bank: &Bank<'_>,
    song: &song::Song,
    setup: &MusicSetup,
    sustains: &crate::parameters::Sustains,
) -> Result<(Resources, Score)> {
    let score = score(song, setup, |page, key, velocity| {
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
    let resources = self::programs(bank, roots, sustains)?;
    score.validate(&resources)?;
    Ok((resources, score))
}

/// Bind every note to its cooked instrument voices.
pub fn score(
    song: &song::Song,
    setup: &MusicSetup,
    mut resolve: impl FnMut(Option<Page>, u8, u8) -> Result<Vec<Note>>,
) -> Result<Score> {
    let (loop_start_tick, end_tick) = song.playback_interval()?;
    let mut programs = setup.channels.map(|c| c.program);
    let first = song.events();
    let first_events = events(setup, &first, &mut programs, &mut resolve)?;
    let loop_events = events(setup, &song.loop_events()?, &mut programs, &mut resolve)?;
    Ok(Score {
        origin: data::ScoreOrigin::Sequence,
        initial_bpm_1024: song.initial_bpm_1024,
        loop_start_tick,
        end_tick,
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
    programs: &mut [u8; 16],
    resolve: &mut impl FnMut(Option<Page>, u8, u8) -> Result<Vec<Note>>,
) -> Result<Vec<Event>> {
    let mut output = Vec::new();
    for event in events {
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
                let voices = resolve(setup.page(event.channel, *current_program), key, velocity)?;
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
    use std::collections::BTreeMap;

    fn arrangement() -> (song::Song, MusicSetup) {
        use song::EventKind::{Command, Modulation, Note, Pattern};
        let note = |key| Note {
            key,
            velocity: key + 30,
            length: 2,
        };
        let song = song::Song {
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
    fn score_binding_keeps_program_state_across_loops() -> Result<()> {
        let (song, setup) = arrangement();
        let mut calls = Vec::new();
        let score = score(&song, &setup, |page, key, velocity| {
            calls.push((page.map(|page| page.object), key, velocity));
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
                (None, 60, 90),
                (Some(12), 61, 91),
                (Some(15), 62, 92),
                (Some(15), 61, 91),
                (Some(15), 62, 92),
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
            let error = score(&song, &setup, |_, _, _| Ok(Vec::new()))
                .err()
                .unwrap();
            assert_eq!(
                error.to_string(),
                format!("invalid song channel {channel} at tick 1")
            );
        }
    }
}
