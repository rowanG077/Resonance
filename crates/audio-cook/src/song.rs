//! Decode arrangements into timed notes, modulation and pitch events.
//! Keeps musical ticks and exact event ordering; no MIDI conversion or playback.
use crate::read;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

const LIMIT: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", content = "parameters", rename_all = "snake_case")]
pub enum EventKind {
    Pattern {
        program: Option<u8>,
        volume: Option<u8>,
    },
    Note {
        key: u8,
        velocity: u8,
        length: u16,
    },
    /// Original command byte: 0 = program, 1 = extended control 0x82,
    /// high bit set = controller/special sequence command. Kept losslessly.
    Command {
        command: u8,
        value: u8,
    },
    PitchBend(u16),
    Modulation(u16),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub tick: u32,
    pub track: u8,
    pub channel: u8,
    pub kind: EventKind,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: u8,
    pub channel: u8,
    pub end_tick: u32,
    pub loop_region: Option<u16>,
    pub regions: Vec<u32>,
    pub events: Vec<Event>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Tempo {
    pub tick: u32,
    pub bpm_1024: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Song {
    pub has_master_track: bool,
    pub initial_bpm_1024: u32,
    pub loop_start_tick: u32,
    pub tempos: Vec<Tempo>,
    pub tracks: Vec<Track>,
}

fn bpm(value: u32, fractional: bool) -> Result<u32> {
    let value = if fractional {
        value
    } else {
        value.checked_mul(1024).context("song tempo overflow")?
    };
    ensure!((1..=1024 * 1000).contains(&value), "invalid song tempo");
    Ok(value)
}

impl Song {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= 16 * 1024 * 1024, "song exceeds read limit");
        let flags = read::u32(bytes, 16)?;
        ensure!(
            flags & 0x8000_0000 == 0,
            "multi-section arrangements are not implemented yet"
        );
        let fractional = flags & 0x4000_0000 != 0;
        let initial_bpm_1024 = bpm(flags & 0x0fff_ffff, fractional)?;
        let loop_start_tick = read::u32(bytes, 20)?;
        let track_table = read::slice(bytes, read::u32(bytes, 0)? as usize, 64 * 4)?;
        let channels = read::slice(bytes, read::u32(bytes, 8)? as usize, 64)?;
        let pattern_table = read::u32(bytes, 4)? as usize;
        let mut tempos: Vec<Tempo> = Vec::new();
        let mut at = read::u32(bytes, 12)? as usize;
        if at != 0 {
            loop {
                let tick = read::u32(bytes, at)?;
                if tick == u32::MAX {
                    break;
                }
                ensure!(tempos.len() < LIMIT, "song tempo budget exhausted");
                ensure!(
                    tempos.last().is_none_or(|last| last.tick <= tick),
                    "song tempos run backwards"
                );
                tempos.push(Tempo {
                    tick,
                    bpm_1024: bpm(read::u32(bytes, at + 4)?, fractional)?,
                });
                at += 8;
            }
        }
        let mut tracks = Vec::new();
        let mut total = 0;
        for (id, &channel) in channels.iter().enumerate() {
            let mut at = read::u32(track_table, id * 4)? as usize;
            if at == 0 {
                continue;
            }
            ensure!(channel < 16, "song track {id} has an invalid MIDI channel");
            let mut track = Track {
                id: id as u8,
                channel,
                end_tick: 0,
                loop_region: None,
                regions: Vec::new(),
                events: Vec::new(),
            };
            loop {
                ensure!(track.regions.len() < LIMIT, "song region budget exhausted");
                let entry = read::slice(bytes, at, 12)?;
                let tick = read::u32(entry, 0)?;
                let pattern = read::u16(entry, 8)?;
                if pattern >= 0xfffe {
                    // A stop marker is consumed after the pattern finishes;
                    // unlike a loop marker, its time word is unused (often 0).
                    if pattern == 0xfffe {
                        track.end_tick = tick;
                        let index = read::u16(entry, 10)?;
                        ensure!(
                            usize::from(index) < track.regions.len(),
                            "song loop region is out of bounds"
                        );
                        ensure!(loop_start_tick < tick, "song loop does not advance time");
                        track.loop_region = Some(index);
                    }
                    break;
                }
                track.regions.push(tick);
                track.events.push(Event {
                    tick,
                    track: id as u8,
                    channel,
                    kind: EventKind::Pattern {
                        program: (entry[4] != 255).then_some(entry[4]),
                        volume: (entry[5] != 255).then_some(entry[5]),
                    },
                });
                ensure!(
                    [entry[4], entry[5]].iter().all(|v| *v <= 127 || *v == 255),
                    "invalid pattern controls"
                );
                let offset = read::u32(bytes, pattern_table + usize::from(pattern) * 4)? as usize;
                track.end_tick = parse_pattern(bytes, offset, entry, &mut track, &mut total)
                    .with_context(|| format!("track {id}, pattern {pattern}"))?;
                at += 12;
            }
            ensure!(
                track
                    .events
                    .windows(2)
                    .all(|pair| pair[0].tick <= pair[1].tick),
                "overlapping/backward patterns are not supported"
            );
            tracks.push(track);
        }
        ensure!(!tracks.is_empty(), "song has no tracks");
        Ok(Self {
            has_master_track: read::u32(bytes, 12)? != 0,
            initial_bpm_1024,
            loop_start_tick,
            tempos,
            tracks,
        })
    }

    /// First traversal only. Original equal-time insertion goes after existing
    /// events, so simultaneous events from one track do not jump other tracks.
    pub fn events(&self) -> Vec<Event> {
        self.ordered_events(vec![0; self.tracks.len()])
    }

    /// Restart each track at its selected region, retaining the queue's tie order.
    /// Setup regions are not replayed merely because they share the song's clock.
    pub fn loop_events(&self) -> Result<Vec<Event>> {
        let starts = self
            .tracks
            .iter()
            .map(|track| {
                // A stopped track has no loop event. It remains exhausted
                // while the other tracks restart their selected regions.
                let Some(region) = track.loop_region.map(usize::from) else {
                    return Ok(track.events.len());
                };
                track
                    .events
                    .iter()
                    .enumerate()
                    .filter(|(_, event)| matches!(event.kind, EventKind::Pattern { .. }))
                    .nth(region)
                    .map(|(index, _)| index)
                    .context("missing loop region event")
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(self.ordered_events(starts))
    }

    fn ordered_events(&self, starts: Vec<usize>) -> Vec<Event> {
        let mut cursors: Vec<_> = starts.into_iter().map(|start| (start, 0usize)).collect();
        let mut serial = self.tracks.len();
        for (i, cursor) in cursors.iter_mut().enumerate() {
            cursor.1 = i;
        }
        let mut result = Vec::new();
        loop {
            let next = self
                .tracks
                .iter()
                .zip(&cursors)
                .enumerate()
                .filter_map(|(i, (track, &(at, serial)))| {
                    track.events.get(at).map(|e| (e.tick, serial, i))
                })
                .min();
            let Some((_, _, i)) = next else {
                break;
            };
            result.push(self.tracks[i].events[cursors[i].0].clone());
            cursors[i] = (cursors[i].0 + 1, serial);
            serial += 1;
        }
        result
    }

    pub fn playback_interval(&self) -> Result<(u32, u32)> {
        let mut looping = self
            .tracks
            .iter()
            .filter(|track| track.loop_region.is_some());
        let Some(first) = looping.next() else {
            let end = self
                .tracks
                .iter()
                .map(|track| track.end_tick)
                .max()
                .context("song has no tracks")?
                .checked_add(1)
                .context("song end tick overflow")?;
            return Ok((0, end));
        };
        let end = first.end_tick;
        ensure!(
            looping.all(|track| track.end_tick == end)
                && self.tracks.iter().all(|track| {
                    track.end_tick <= end
                        && (track.loop_region.is_some()
                            || track.events.last().is_none_or(|event| event.tick <= end))
                }),
            "song does not have one shared loop interval"
        );
        // Patterns finish before their loop markers are consumed. The shared
        // queue therefore drains after any events at or beyond the marker;
        // note-off deadlines and pattern terminators do not delay the rewind.
        let end = self
            .tracks
            .iter()
            .filter_map(|track| track.events.last())
            .fold(end, |end, event| end.max(event.tick));
        Ok((self.loop_start_tick, end))
    }
}

fn parse_pattern(
    bytes: &[u8],
    offset: usize,
    region: &[u8],
    track: &mut Track,
    total: &mut usize,
) -> Result<u32> {
    let pitch = read::u32(bytes, offset + 4)? as usize;
    let modulation = read::u32(bytes, offset + 8)? as usize;
    let base = read::u32(region, 0)?;
    let mut at = offset + 12;
    let mut tick = 0u32;
    let mut local = Vec::new();
    loop {
        *total += 1;
        ensure!(*total <= LIMIT, "song event budget exhausted");
        let data = read::slice(bytes, at, 4)?;
        tick = tick
            .checked_add(u32::from(read::u16(data, 0)?))
            .context("pattern tick overflow")?;
        let (key, velocity) = (data[2], data[3]);
        if key == 255 && velocity == 255 {
            break;
        }
        at += 4;
        let kind = if key & 128 != 0 {
            EventKind::Command {
                command: velocity,
                value: key & 127,
            }
        } else if key == 0 && velocity == 0 {
            continue; // Time-only record, four bytes rather than a note.
        } else {
            ensure!(velocity <= 127, "invalid note velocity");
            let length = read::u16(bytes, at)?;
            at += 2;
            EventKind::Note {
                key: (i16::from(key) + i16::from(region[10] as i8)).clamp(0, 127) as u8,
                velocity: (i16::from(velocity) + i16::from(region[11] as i8)).clamp(0, 127) as u8,
                length,
            }
        };
        local.push((tick, 2u8, kind));
    }
    for (offset, initial, priority) in [(pitch, 8192u16, 1u8), (modulation, 0, 0)] {
        if offset == 0 {
            continue;
        }
        let mut stream = offset;
        let mut value = initial;
        let mut time = 0u32;
        while let Some((delta_time, delta)) = stream_value(bytes, &mut stream)? {
            *total += 1;
            ensure!(*total <= LIMIT, "song controller budget exhausted");
            time = time
                .checked_add(u32::from(delta_time))
                .context("controller tick overflow")?;
            if time > tick {
                break;
            }
            value = value.wrapping_add_signed(delta);
            local.push((
                time,
                priority,
                if priority == 0 {
                    EventKind::Modulation(value)
                } else {
                    EventKind::PitchBend(value)
                },
            ));
        }
    }
    // Equal-tick events apply modulation first, then pitch, then notes.
    local.sort_by_key(|&(tick, priority, _)| (tick, priority));
    for (tick, _, kind) in local {
        track.events.push(Event {
            tick: base.checked_add(tick).context("song tick overflow")?,
            track: track.id,
            channel: track.channel,
            kind,
        });
    }
    base.checked_add(tick).context("pattern end tick overflow")
}

fn stream_value(bytes: &[u8], at: &mut usize) -> Result<Option<(u16, i16)>> {
    let first = read::slice(bytes, *at, 2)?;
    if first == [0x80, 0] {
        *at += 2;
        return Ok(None);
    }
    let time = if first[0] & 128 != 0 {
        *at += 2;
        (u16::from(first[0] & 127) << 8) | u16::from(first[1])
    } else {
        *at += 1;
        u16::from(first[0])
    };
    let first = read::slice(bytes, *at, 1)?[0];
    let delta = if first & 128 != 0 {
        let word = (u16::from(first & 127) << 8) | u16::from(read::slice(bytes, *at + 1, 1)?[0]);
        *at += 2;
        (word | ((word & 0x4000) << 1)) as i16
    } else {
        *at += 1;
        i16::from((first | ((first & 0x40) << 1)) as i8)
    };
    Ok(Some((time, delta)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_signed_seven_and_fifteen_bit_values_and_terminator() {
        let bytes = [1, 0x7f, 0x81, 0x23, 0xff, 0xfe, 0, 0x3f, 0x80, 0];
        let mut at = 0;
        assert_eq!(stream_value(&bytes, &mut at).unwrap(), Some((1, -1)));
        assert_eq!(stream_value(&bytes, &mut at).unwrap(), Some((291, -2)));
        assert_eq!(stream_value(&bytes, &mut at).unwrap(), Some((0, 63)));
        assert_eq!(stream_value(&bytes, &mut at).unwrap(), None);
        assert!(stream_value(&[0x81], &mut 0).is_err());
        assert!(stream_value(&[1, 0x80], &mut 0).is_err());
    }

    #[test]
    fn pattern_padding_deltas_transposition_and_controller_priority() {
        let mut bytes = vec![0; 12];
        bytes.extend([0, 3, 0, 0]); // Three ticks of padding.
        bytes.extend([0, 2, 60, 120, 0, 9]);
        bytes.extend([0, 0, 255, 255]);
        let stream = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&stream.to_be_bytes());
        bytes[8..12].copy_from_slice(&stream.to_be_bytes());
        bytes.extend([5, 0x7f, 0x80, 0]);
        let mut region = [0; 12];
        region[0..4].copy_from_slice(&100u32.to_be_bytes());
        region[10] = 254;
        region[11] = 20;
        let mut track = Track {
            id: 3,
            channel: 7,
            end_tick: 200,
            loop_region: None,
            regions: vec![],
            events: vec![],
        };
        parse_pattern(&bytes, 0, &region, &mut track, &mut 0).unwrap();
        assert_eq!(
            track.events.iter().map(|e| &e.kind).collect::<Vec<_>>(),
            [
                &EventKind::Modulation(65535),
                &EventKind::PitchBend(8191),
                &EventKind::Note {
                    key: 58,
                    velocity: 127,
                    length: 9
                },
            ]
        );
        assert!(
            track
                .events
                .iter()
                .all(|e| e.tick == 105 && e.track == 3 && e.channel == 7)
        );
    }

    #[test]
    fn simultaneous_tracks_keep_original_queue_insertion_order() {
        let tracks = (0..2)
            .map(|id| Track {
                id,
                channel: id,
                end_tick: 10,
                loop_region: Some(0),
                regions: vec![0],
                events: vec![
                    Event {
                        tick: 0,
                        track: id,
                        channel: id,
                        kind: EventKind::PitchBend(8192)
                    };
                    2
                ],
            })
            .collect();
        let mut song = Song {
            has_master_track: false,
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 1,
            tempos: vec![],
            tracks,
        };
        assert_eq!(
            song.events().iter().map(|e| e.track).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        assert_eq!(song.playback_interval().unwrap(), (1, 10));
        song.tracks[1].end_tick = 11;
        assert!(song.playback_interval().is_err());
    }

    #[test]
    fn arrangement_bounds_channels_and_loop_targets_are_checked() {
        let mut bytes = vec![0; 394];
        for (at, value) in [
            (0, 24u32),
            (4, 344),
            (8, 280),
            (16, 120),
            (20, 1),
            (24, 348),
            (344, 372),
            (348, 10),
            (360, 20),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[280] = 2;
        bytes[352..354].copy_from_slice(&[255, 255]);
        bytes[368..370].copy_from_slice(&0xfffeu16.to_be_bytes());
        bytes[384..394].copy_from_slice(&[0, 2, 60, 100, 0, 15, 0, 0, 255, 255]);
        let song = Song::parse(&bytes).unwrap();
        assert_eq!(song.initial_bpm_1024, 120 * 1024);
        assert_eq!(song.playback_interval().unwrap(), (1, 20));
        assert_eq!(
            song.events()[1],
            Event {
                tick: 12,
                track: 0,
                channel: 2,
                kind: EventKind::Note {
                    key: 60,
                    velocity: 100,
                    length: 15
                },
            }
        );
        assert!(Song::parse(&bytes[..393]).is_err());
        let mut late = bytes.clone();
        late[360..364].copy_from_slice(&11u32.to_be_bytes());
        let late = Song::parse(&late).unwrap();
        assert_eq!(late.playback_interval().unwrap(), (1, 12));
        assert_eq!(late.events(), song.events());
        let mut stopped = bytes.clone();
        stopped[360..364].fill(0); // Stop-marker timestamps are unused.
        stopped[368..370].copy_from_slice(&0xffffu16.to_be_bytes());
        let stopped = Song::parse(&stopped).unwrap();
        assert_eq!(stopped.playback_interval().unwrap(), (0, 13));
        assert_eq!(stopped.events(), song.events());
        assert!(stopped.loop_events().unwrap().is_empty());
        bytes[370..372].copy_from_slice(&7u16.to_be_bytes());
        assert!(Song::parse(&bytes).is_err());
        bytes[370..372].fill(0);
        bytes[280] = 16;
        assert!(Song::parse(&bytes).is_err());
        assert!(Song::parse(&[0; 20]).is_err());
    }

    #[test]
    fn loop_starts_at_selected_regions_without_replaying_setup_controls() {
        let tracks = (0..2)
            .map(|id| {
                let event = |tick, kind| Event {
                    tick,
                    track: id,
                    channel: id,
                    kind,
                };
                Track {
                    id,
                    channel: id,
                    end_tick: 100,
                    loop_region: Some(1),
                    regions: vec![0, 8],
                    events: vec![
                        event(
                            0,
                            EventKind::Pattern {
                                program: Some(4),
                                volume: Some(100),
                            },
                        ),
                        event(
                            0,
                            EventKind::Command {
                                command: 138,
                                value: 40,
                            },
                        ),
                        event(
                            8,
                            EventKind::Pattern {
                                program: None,
                                volume: None,
                            },
                        ),
                        event(
                            8,
                            EventKind::Note {
                                key: 60,
                                velocity: 100,
                                length: 100,
                            },
                        ),
                    ],
                }
            })
            .collect();
        let mut song = Song {
            has_master_track: false,
            initial_bpm_1024: 140 * 1024,
            loop_start_tick: 1,
            tempos: vec![],
            tracks,
        };
        let events = song.loop_events().unwrap();
        assert_eq!(
            events.iter().map(|event| event.track).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        assert!(events.iter().all(|event| event.tick == 8));
        assert!(matches!(
            events[0].kind,
            EventKind::Pattern {
                program: None,
                volume: None
            }
        ));
        song.tracks[1].loop_region = None;
        song.tracks[1].end_tick = 8;
        assert_eq!(song.events().len(), 8);
        assert_eq!(song.playback_interval().unwrap(), (1, 100));
        assert!(
            song.loop_events()
                .unwrap()
                .iter()
                .all(|event| event.track == 0)
        );
    }

    #[test]
    #[ignore = "requires both extracted discs; audits all arrangements without audio playback"]
    fn original_song_census_preserves_terminal_events_and_shared_loop_boundaries() -> Result<()> {
        use crate::{
            bank::{Channel, MusicSetup},
            compile,
        };
        use resonance_audio::data::{EventKind as Cooked, Resources};
        use std::{fs, path::Path};

        let setup = MusicSetup {
            group: 0,
            normal: Default::default(),
            drums: Default::default(),
            channels: [Channel {
                program: 0,
                volume: 127,
                pan: 64,
                reverb: 0,
                chorus: 0,
            }; 16],
        };
        let resources = Resources {
            programs: Default::default(),
            samples: Default::default(),
        };
        let mut count = 0;
        let mut terminal_songs = 0;
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}/files/S"));
            for path in fs::read_dir(root)? {
                let path = path?.path();
                if !path
                    .extension()
                    .is_some_and(|extension| extension == "song")
                {
                    continue;
                }
                let name = path.file_name().unwrap().to_str().unwrap();
                let song = Song::parse(&fs::read(&path)?)?;
                let score = compile::score(&song, &setup, |_, _, _, _| Ok(Vec::new()))?;
                score
                    .validate(&resources)
                    .with_context(|| format!("disc{disc}/{name}"))?;
                for events in [&score.first_events, &score.loop_events] {
                    let terminal: Vec<_> = events
                        .iter()
                        .filter(|event| event.tick == score.end_tick)
                        .collect();
                    match name {
                        "bgm_b005.song" | "bgm_t024.song" => {
                            let (end, channels) = if name == "bgm_b005.song" {
                                (49144, &[4, 5][..])
                            } else {
                                (28984, &[3][..])
                            };
                            assert_eq!(score.end_tick, end);
                            assert_eq!(
                                terminal
                                    .iter()
                                    .map(|event| event.channel)
                                    .collect::<Vec<_>>(),
                                channels
                            );
                            assert!(terminal.iter().all(|event| matches!(
                                event.kind,
                                Cooked::PitchBend { value: 8192 }
                            )));
                        }
                        "bgm_b012.song" => {
                            assert_eq!(score.end_tick, 62016);
                            assert_eq!(terminal.len(), 1);
                            assert_eq!(terminal[0].channel, 12);
                            assert!(matches!(terminal[0].kind, Cooked::Notes { length: 80, .. }));
                            let track = song.tracks.iter().find(|track| track.id == 16).unwrap();
                            assert_eq!(track.end_tick, 62008);
                            assert!(matches!(
                                track.events.last().unwrap().kind,
                                EventKind::Note {
                                    key: 83,
                                    velocity: 90,
                                    length: 80
                                }
                            ));
                        }
                        _ => assert!(terminal.is_empty(), "unexpected boundary events in {name}"),
                    }
                }
                terminal_songs += usize::from(
                    score
                        .first_events
                        .last()
                        .is_some_and(|event| event.tick == score.end_tick),
                );
                count += 1;
            }
        }
        assert_eq!(count, 236);
        assert_eq!(terminal_songs, 6);
        Ok(())
    }
}
