//! Resumable score state. Only the caller owns scheduling and threads.
use super::*;
use crate::{
    data::{Event, Tempo},
    music_voice::Controls,
};
use std::{iter::Peekable, slice::Iter};

pub(super) struct Kernel<'a> {
    bank: &'a Resources,
    song: &'a Score,
    tables: &'a Tables,
    looping: bool,
    frame: u64,
    ended: bool,
    controls: [Controls; 16],
    events: Peekable<Iter<'a, Event>>,
    tempos: Peekable<Iter<'a, Tempo>>,
    bpm: u32,
    time: [u64; 2],
    increments: [u64; 2],
    clock: usize,
    voices: Vec<Active<'a>>,
    available: VecDeque<usize>,
    retained_lfo: [u32; 64],
    studio_order: Vec<usize>,
    callbacks: Vec<usize>,
    result: Option<Preview>,
    end_frame: u64,
    quiet_since: Option<u64>,
    uses_groups: bool,
    block: [BusFrame; 160],
}
impl<'a> Kernel<'a> {
    pub(super) fn new(
        bank: &'a Resources,
        song: &'a Score,
        tables: &'a Tables,
        frames: Option<u32>,
        looping: bool,
        clock_start: ClockStart,
    ) -> Result<Self> {
        tables.validate()?;
        bank.validate()?;
        song.validate(bank)?;
        let (loop_tick, end_tick) = (song.loop_start_tick, song.end_tick);
        let controls = song.controls;
        let first_events = &song.first_events;
        let loop_events = &song.loop_events;
        ensure!(
            first_events.iter().all(|event| event.tick < end_tick)
                && loop_events
                    .iter()
                    .all(|event| (loop_tick..end_tick).contains(&event.tick)),
            "score events fall outside the supported loop interval"
        );
        let events = first_events.iter().peekable();
        let tempos = song.tempos.iter().peekable();
        let bpm = song.initial_bpm_1024;
        // Toggle musical clocks on a loop so held notes retain their pre-loop
        // note-off deadlines while new notes use the rewound timeline.
        let time = [0u64; 2];
        let increments = [match clock_start {
            ClockStart::Cold => 0,
            ClockStart::Running => tick_delta(bpm),
        }; 2];
        let clock = 0;
        let voices: Vec<Active<'_>> = Vec::with_capacity(64);
        // Allocate from a FIFO; reused slots retain audible LFO phase.
        let available: VecDeque<_> = (0..64).collect();
        let retained_lfo = [0u32; 64];
        let studio_order = Vec::<usize>::new();
        let callbacks = Vec::<usize>::new();
        let result = frames.map(|_| Preview {
            pcm: Vec::new(),
            notes: 0,
            maximum_voices: 0,
            final_tick: 0,
            loop_starts: Vec::new(),
            free_voices: Vec::new(),
            lfo_counters: [0; 64],
            voice_lifetimes: Vec::new(),
        });
        let end_frame = frames.map(u64::from).unwrap_or(u64::MAX);
        let quiet_since = None;
        let uses_groups = bank
            .programs
            .values()
            .flatten()
            .any(|command| matches!(command, crate::data::Command::ExclusiveGroup { .. }));

        Ok(Self {
            bank,
            song,
            tables,
            looping,
            frame: 0,
            ended: false,
            controls,
            events,
            tempos,
            bpm,
            time,
            increments,
            clock,
            voices,
            available,
            retained_lfo,
            studio_order,
            callbacks,
            result,
            end_frame,
            quiet_since,
            uses_groups,
            block: [[[0; 2]; 3]; 160],
        })
    }
    pub(super) fn next_block(
        &mut self,
        mut live_controls: impl FnMut(u64) -> LiveControls,
    ) -> Result<Option<&[BusFrame]>> {
        if self.ended || self.frame >= self.end_frame {
            return Ok(None);
        }
        let bank = self.bank;
        let song = self.song;
        let tables = self.tables;
        let loop_tick = song.loop_start_tick;
        let end_tick = song.end_tick;
        let loop_events = &song.loop_events;
        for frame in self.frame..self.end_frame {
            self.frame = frame + 1;
            if frame.is_multiple_of(32) {
                let clock_before_events = self.clock;
                let input = live_controls(frame);
                for (channel, initial) in self.controls.iter_mut().zip(song.controls) {
                    channel.group_volume = input.volume;
                    if let Some(pan) = input.pan {
                        channel.pan =
                            (i16::from(initial.pan) + i16::from(pan) - 64).clamp(0, 127) as u8;
                    }
                }
                if input.release {
                    for active in self
                        .voices
                        .iter_mut()
                        .filter(|v| !v.retired && v.end_tick.is_some())
                    {
                        active.voice.key_off()?;
                        active.end_tick = None;
                    }
                }
                if self.looping && self.time[self.clock] >> 16 >= u64::from(end_tick) {
                    let next_clock = self.clock ^ 1;
                    ensure!(
                        !self.voices.iter().any(|active| !active.retired
                            && active.clock == next_clock
                            && active.end_tick.is_some()),
                        "a held note spans more than one score loop"
                    );
                    self.time[next_clock] =
                        (u64::from(loop_tick) << 16) | (self.time[self.clock] & 65535);
                    self.clock = next_clock;
                    self.events = loop_events.iter().peekable();
                    self.tempos = song.tempos.iter().peekable();
                    if let Some(result) = &mut self.result {
                        result.loop_starts.push(frame as u32);
                    }
                }
                let tick = (self.time[self.clock] >> 16) as u32;
                if let Some(result) = &mut self.result {
                    result.final_tick = tick;
                }
                while self.tempos.peek().is_some_and(|t| t.tick <= tick) {
                    self.bpm = self.tempos.next().unwrap().bpm_1024;
                }
                while self.events.peek().is_some_and(|e| e.tick <= tick) {
                    let event = self.events.next().unwrap();
                    let channel = usize::from(event.channel);
                    match &event.kind {
                        EventKind::Volume { value } => self.controls[channel].volume = *value,
                        EventKind::Pan { value } => self.controls[channel].pan = *value,
                        EventKind::Expression { value } => {
                            self.controls[channel].expression = *value
                        }
                        EventKind::Auxiliary { bus, value } => {
                            self.controls[channel].post[usize::from(*bus)] = *value
                        }
                        EventKind::PitchBend { value } => {
                            self.controls[channel].pitch_bend = *value
                        }
                        EventKind::Modulation { value } => {
                            self.controls[channel].modulation = *value
                        }
                        EventKind::Notes {
                            voices: notes,
                            length,
                        } => {
                            if let Some(result) = &mut self.result {
                                result.notes += 1;
                            }
                            for &note in notes {
                                ensure!(
                                    !self.available.is_empty(),
                                    "preview needs voice allocation/stealing"
                                );
                                let slot = self.available.pop_front().unwrap();
                                let mut voice =
                                    Voice::new_at(bank, tables, note, (frame % 160) as u32)?;
                                voice.restore_lfo(self.retained_lfo[slot]);
                                let lifetime = if let Some(result) = &mut self.result {
                                    let index = result.voice_lifetimes.len();
                                    result.voice_lifetimes.push(VoiceLifetime {
                                        slot,
                                        start_frame: frame as u32,
                                        end_frame: None,
                                        macro_id: note.macro_id,
                                        key: note.key,
                                    });
                                    index
                                } else {
                                    0
                                };
                                self.voices.push(Active {
                                    voice,
                                    channel,
                                    end_tick: Some(event.tick + u32::from(*length)),
                                    clock: self.clock,
                                    slot,
                                    retired: false,
                                    lifetime,
                                });
                            }
                        }
                    }
                }
                for active in &mut self.voices {
                    if active.retired {
                        continue;
                    }
                    if active
                        .end_tick
                        .is_some_and(|end| u64::from(end) <= self.time[active.clock] >> 16)
                    {
                        active.voice.key_off()?;
                        active.end_tick = None;
                    }
                }
                if let Some(result) = &mut self.result {
                    result.maximum_voices = result
                        .maximum_voices
                        .max(self.voices.iter().filter(|v| !v.retired).count());
                }
                // SetTickDelta updates the clock active before event dispatch.
                // With no master track, a newly selected clock keeps its previous
                // increment until the following millisecond. Field starts seed
                // both clocks so the first handoff cannot insert an idle update.
                self.increments[clock_before_events] = tick_delta(self.bpm);
                if self.clock != clock_before_events && song.has_master_track {
                    self.increments[self.clock] = tick_delta(self.bpm);
                }
                for (time_value, increment) in self.time.iter_mut().zip(self.increments) {
                    *time_value += increment;
                }
            }
            let mut finished = Vec::new();
            let mut source_before = [false; 64];
            for active in self.voices.iter().filter(|v| !v.retired) {
                source_before[active.slot] = active.voice.source_active();
            }
            if frame.is_multiple_of(32) {
                // Apply key groups before the next sample. Only group-bearing scores
                // need this preliminary pass; other scores retain normal control ordering.
                if self.uses_groups {
                    for index in 0..self.voices.len() {
                        if self.voices[index].retired {
                            continue;
                        }
                        self.voices[index].voice.prepare_commands()?;
                        if let Some((group, kill)) = self.voices[index].voice.group_request.take() {
                            for (other, active) in self.voices.iter_mut().enumerate() {
                                if other != index
                                    && !active.retired
                                    && active.voice.exclusive_group == group
                                {
                                    if kill {
                                        active.voice.kill();
                                    } else {
                                        active.voice.key_off()?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for active in self.voices.iter_mut().filter(|v| !v.retired) {
                let was_active = source_before[active.slot];
                active.voice.prepare_frame(self.controls[active.channel])?;
                let is_active = active.voice.source_active();
                if was_active != is_active {
                    self.studio_order.retain(|&slot| slot != active.slot);
                    if is_active {
                        // Activated sources enter the studio newest-first.
                        self.studio_order.insert(0, active.slot);
                    }
                }
                if active.voice.is_done() {
                    active.retired = true;
                    if let Some(result) = &mut self.result {
                        result.voice_lifetimes[active.lifetime].end_frame = Some(frame as u32);
                    }
                    self.retained_lfo[active.slot] = active.voice.retained_lfo();
                    finished.push(active.slot);
                }
            }
            // Run sequence events before macros and free slots afterward, retaining
            // pending PCM. New wakes precede completion callbacks. Equal deadlines
            // keep reverse note-creation order after wait-list and runnable-list insertion.
            finished.reverse();
            finished.sort_by_key(|slot| {
                self.callbacks
                    .iter()
                    .position(|s| s == slot)
                    .map_or(0, |i| i + 1)
            });
            self.available.extend(finished);
            self.callbacks.clear();
            if (frame + 1).is_multiple_of(160) || frame + 1 == self.end_frame {
                let mut ordered: Vec<_> = self
                    .studio_order
                    .iter()
                    .map(|&slot| {
                        let active = self
                            .voices
                            .iter()
                            .find(|v| v.slot == slot && !v.retired)
                            .unwrap();
                        (slot, active.voice.priority())
                    })
                    .collect();
                crate::voice_order::completion_order(&mut ordered);
                let mut block = [[[0; 2]; 3]; 160];
                for active in &mut self.voices {
                    active.voice.mix_block(&mut block)?;
                }
                for (slot, _) in ordered {
                    let active = self
                        .voices
                        .iter()
                        .find(|v| v.slot == slot && !v.retired)
                        .unwrap();
                    if !active.voice.source_active() {
                        self.studio_order.retain(|&s| s != slot);
                        if active.voice.waits_for_sample_end() {
                            self.callbacks.push(slot);
                        }
                    }
                }
                self.voices.retain(|active| !active.retired);
                let length = (frame % 160 + 1) as usize;
                if !self.looping && self.events.peek().is_none() && self.voices.is_empty() {
                    let start = *self.quiet_since.get_or_insert(frame);
                    if frame - start >= 32000 * 4 {
                        self.ended = true;
                    }
                } else {
                    self.quiet_since = None;
                }
                self.block = block;
                return Ok(Some(&self.block[..length]));
            }
        }
        Ok(None)
    }
    pub(super) fn finish(mut self) -> Option<Preview> {
        if let Some(result) = &mut self.result {
            result.free_voices = self.available.into_iter().collect();
            for active in &self.voices {
                self.retained_lfo[active.slot] = active.voice.retained_lfo();
            }
            result.lfo_counters = self.retained_lfo;
        }
        self.result
    }
}
