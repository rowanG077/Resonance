//! Resumable score state. Only the caller owns scheduling and threads.
use super::*;
use crate::{
    data::{Event, Tempo},
    music_voice::Controls,
};
use std::{iter::Peekable, slice::Iter};

#[cfg(test)]
mod tests;

pub(super) struct Kernel<'a> {
    bank: &'a Resources,
    song: &'a Score,
    tables: &'a Tables,
    looping: bool,
    frame: u64,
    ended: bool,
    paused: bool,
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
    uses_groups: bool,
    block: [BusFrame; 160],
    random: Option<shared::Control>,
}
impl Drop for Kernel<'_> {
    fn drop(&mut self) {
        if let Some(random) = &self.random {
            for active in self.voices.iter().filter(|v| !v.retired) {
                random.free(active.lease.unwrap(), &active.voice);
            }
        }
    }
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
        ensure!(
            !looping || song.origin == crate::data::ScoreOrigin::Sequence,
            "sound effects cannot loop a score; their macros control repetition"
        );
        let controls = song.controls;
        let first_events = &song.first_events;
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
            paused: false,
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
            uses_groups,
            block: [[[0; 2]; 3]; 160],
            random: None,
        })
    }
    /// seqPause (80132348) removes active notes but preserves the score cursor.
    pub(super) fn pause(&mut self, paused: bool) {
        if paused && !self.paused {
            self.sync_slots();
            for active in self.voices.iter_mut().filter(|v| !v.retired) {
                active.voice.kill();
                if let Some(random) = &self.random {
                    random.free(active.lease.unwrap(), &active.voice);
                } else {
                    self.available.push_back(active.slot);
                }
                active.retired = true;
            }
            self.voices.clear();
            self.studio_order.clear();
            self.callbacks.clear();
        }
        self.paused = paused;
    }
    pub(super) fn paused(&self) -> bool {
        self.paused
    }
    pub(super) fn next_block(
        &mut self,
        live_controls: impl FnMut(u64) -> LiveControls,
    ) -> Result<Option<&[BusFrame]>> {
        self.advance(self.frame + 160, live_controls)
    }
    pub(super) fn set_random(&mut self, random: shared::Control) {
        self.random = Some(random);
    }
    pub(super) fn sync_slots(&mut self) {
        let Some(random) = &self.random else { return };
        for active in self.voices.iter_mut().filter(|v| !v.retired) {
            if !random.owns(active.lease.unwrap()) {
                active.voice.kill();
                active.retired = true;
            }
        }
        self.studio_order
            .retain(|slot| self.voices.iter().any(|v| !v.retired && v.slot == *slot));
    }
    pub(super) fn wakes(&self) -> impl Iterator<Item = (shared::Lease, shared::Wake)> + '_ {
        self.voices
            .iter()
            .filter(|v| !v.retired)
            .filter_map(|v| v.voice.wake_order().map(|wake| (v.lease.unwrap(), wake)))
    }
    pub(super) fn wake_timers(&mut self) {
        for active in self.voices.iter_mut().filter(|v| !v.retired) {
            active.voice.wake_timer();
        }
    }
    pub(super) fn run_macro(
        &mut self,
        slot: shared::Lease,
    ) -> Result<Option<crate::music_voice::HostRequest>> {
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == Some(slot))
            .unwrap();
        active.voice.prepare_commands(
            active
                .sound_controls
                .as_mut()
                .unwrap_or(&mut self.controls[active.channel]),
        )?;
        self.random
            .as_ref()
            .unwrap()
            .update(slot, &active.voice, true);
        let request = active.voice.host_request.take();
        self.retire(slot);
        Ok(request)
    }
    pub(super) fn spawn_child(
        &mut self,
        parent: shared::Lease,
        note: crate::data::Note,
        instruction: u16,
    ) -> Result<()> {
        let random = self.random.as_ref().unwrap();
        let Some((lease, lfo)) = random.child(parent, note) else {
            return Ok(());
        };
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == Some(parent))
            .unwrap();
        active.voice.last_child = random.handle(lease);
        let mut voice = active.voice.child(note, instruction, self.frame)?;
        voice.handle = random.handle(lease);
        voice.set_random(random.clone());
        voice.restore_lfo(lfo);
        let (channel, end_tick, clock) = (active.channel, active.end_tick, active.clock);
        let sound_controls = active.sound_controls.map(|controls| controls.child());
        self.voices.push(Active {
            voice,
            sound_controls,
            channel,
            end_tick,
            clock,
            slot: lease.slot,
            lease: Some(lease),
            retired: false,
            lifetime: 0,
        });
        Ok(())
    }
    fn retire(&mut self, slot: shared::Lease) {
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == Some(slot))
            .unwrap();
        if active.voice.is_done() {
            active.retired = true;
            self.random.as_ref().unwrap().free(slot, &active.voice);
        }
    }
    pub(super) fn group_members(&self, group: u8) -> impl Iterator<Item = shared::Lease> + '_ {
        self.voices
            .iter()
            .filter(move |v| !v.retired && v.voice.exclusive_group == group)
            .map(|v| v.lease.unwrap())
    }
    pub(super) fn macro_members(&self, program: u16) -> impl Iterator<Item = shared::Lease> + '_ {
        self.voices
            .iter()
            .filter(move |v| !v.retired && v.voice.original_macro == program)
            .map(|v| v.lease.unwrap())
    }
    pub(super) fn send_message(&mut self, lease: shared::Lease, value: i32) {
        if let Some(active) = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == Some(lease))
        {
            active.voice.send_message(value);
        }
    }
    pub(super) fn apply_group(&mut self, slot: shared::Lease, kill: bool) -> Result<()> {
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == Some(slot))
            .unwrap();
        if kill {
            active.voice.kill();
        } else {
            active.voice.key_off()?;
        }
        self.retire(slot);
        Ok(())
    }
    pub(super) fn contains(&self, slot: shared::Lease) -> bool {
        self.voices
            .iter()
            .any(|v| !v.retired && v.lease == Some(slot))
    }
    pub(super) fn source_changes(
        &mut self,
    ) -> impl Iterator<Item = (shared::Lease, u64, bool)> + '_ {
        self.voices
            .iter_mut()
            .filter(|v| !v.retired)
            .filter_map(|v| {
                v.voice
                    .take_source_change()
                    .map(|(order, start)| (v.lease.unwrap(), order, start))
            })
    }
    pub(super) fn source_priority(&self, slot: shared::Lease) -> Option<u32> {
        self.voices
            .iter()
            .find(|v| !v.retired && v.lease == Some(slot))
            .filter(|v| v.voice.studio_active())
            .map(|v| v.voice.priority())
    }
    pub(super) fn sample_end_callback(&mut self, slot: shared::Lease) {
        if let Some(active) = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == Some(slot))
        {
            active.voice.sample_end_callback();
        }
    }
    pub(super) fn next_millisecond(
        &mut self,
        controls: LiveControls,
    ) -> Result<Option<&[BusFrame]>> {
        self.advance(self.frame + 32, |_| controls)
    }
    fn apply_tempos(&mut self) {
        let tick = (self.time[self.clock] >> 16) as u32;
        while self.tempos.peek().is_some_and(|tempo| tempo.tick <= tick) {
            self.bpm = self.tempos.next().unwrap().bpm_1024;
        }
    }
    fn dispatch_events(&mut self, frame: u64) -> Result<()> {
        let tick = (self.time[self.clock] >> 16) as u32;
        while self.events.peek().is_some_and(|e| e.tick <= tick) {
            let event = self.events.next().unwrap();
            let channel = usize::from(event.channel);
            match &event.kind {
                EventKind::Volume { value } => self.controls[channel].set_coarse(7, *value),
                EventKind::Pan { value } => self.controls[channel].set_coarse(10, *value),
                EventKind::Expression { value } => self.controls[channel].set_coarse(11, *value),
                EventKind::Auxiliary { bus, value } => {
                    self.controls[channel].post[usize::from(*bus)] = *value
                }
                EventKind::PitchBend { value } => self.controls[channel].pitch_bend = *value,
                EventKind::Modulation { value } => self.controls[channel].paired[1] = *value,
                EventKind::Notes {
                    source,
                    voices: notes,
                    length,
                } => {
                    if let Some(result) = &mut self.result {
                        result.notes += 1;
                    }
                    for &note in notes {
                        let mut voice = Voice::new_at(self.bank, self.tables, note, frame)?;
                        let (slot, lease, lfo) = if let Some(random) = &self.random {
                            let Some((lease, lfo)) = random.allocate(*source, note) else {
                                continue;
                            };
                            self.sync_slots();
                            (lease.slot, Some(lease), lfo)
                        } else {
                            let slot = self.available.pop_front().ok_or_else(|| {
                                anyhow::anyhow!("preview needs voice allocation/stealing")
                            })?;
                            (slot, None, self.retained_lfo[slot])
                        };
                        if let Some(random) = &self.random {
                            voice.set_random(random.clone());
                            voice.handle = random.handle(lease.unwrap());
                        }
                        voice.restore_lfo(lfo);
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
                            sound_controls: (source.origin()
                                == crate::data::ScoreOrigin::SoundEffect)
                                .then_some(self.controls[channel]),
                            channel,
                            end_tick: Some(event.tick + u32::from(*length)),
                            clock: self.clock,
                            slot,
                            lease,
                            retired: false,
                            lifetime,
                        });
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn prepare_millisecond(&mut self, input: LiveControls) -> Result<()> {
        self.sync_slots();
        if self.ended || self.paused {
            return Ok(());
        }
        let frame = self.frame;
        let song = self.song;
        let loop_tick = song.loop_start_tick;
        let end_tick = song.end_tick;
        let loop_events = &song.loop_events;
        for (channel, initial) in self.controls.iter_mut().zip(song.controls) {
            channel.group_volume = input.volume;
            channel.mono = input.mono;
            if let Some(pan) = input.pan {
                channel.paired[10] = (i32::from(initial.paired[10]) + ((i32::from(pan) - 64) << 7))
                    .clamp(0, 127 << 7) as u16;
            }
        }
        if input.release {
            for active in self.voices.iter_mut().filter(|v| !v.retired) {
                active.voice.key_off()?;
                active.end_tick = None;
            }
        }
        self.apply_tempos();
        // The outgoing clock keeps its tempo for held-note deadlines.
        self.increments[self.clock] = tick_delta(self.bpm);
        self.dispatch_events(frame)?;
        if self.looping && self.time[self.clock] >> 16 >= u64::from(end_tick) {
            let next_clock = self.clock ^ 1;
            ensure!(
                !self.voices.iter().any(|active| !active.retired
                    && active.clock == next_clock
                    && active.end_tick.is_some()),
                "a held note spans more than one score loop"
            );
            self.time[next_clock] = (u64::from(loop_tick) << 16) | (self.time[self.clock] & 65535);
            self.clock = next_clock;
            self.events = loop_events.iter().peekable();
            self.tempos = song.tempos.iter().peekable();
            self.apply_tempos();
            // Without a master track, the incoming clock retains its
            // previous increment until the following millisecond.
            if song.has_master_track {
                self.increments[self.clock] = tick_delta(self.bpm);
            }
            self.dispatch_events(frame)?;
            if let Some(result) = &mut self.result {
                result.loop_starts.push(frame as u32);
            }
        }
        if let Some(result) = &mut self.result {
            result.final_tick = (self.time[self.clock] >> 16) as u32;
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
        for (time_value, increment) in self.time.iter_mut().zip(self.increments) {
            *time_value += increment;
        }
        for active in self.voices.iter_mut().filter(|v| !v.retired) {
            if let Some(controls) = &mut active.sound_controls {
                controls.group_volume = input.volume;
                controls.mono = input.mono;
                if input.pan.is_some() {
                    controls.paired[10] = self.controls[active.channel].paired[10];
                }
            }
            active.voice.set_tempo(self.bpm);
            if self.random.is_some() {
                active.voice.defer_commands();
            }
        }
        Ok(())
    }
    fn advance(
        &mut self,
        limit: u64,
        mut live_controls: impl FnMut(u64) -> LiveControls,
    ) -> Result<Option<&[BusFrame]>> {
        if self.ended || self.frame >= self.end_frame {
            return Ok(None);
        }
        for frame in self.frame..self.end_frame.min(limit) {
            if frame.is_multiple_of(32) && self.random.is_none() {
                self.prepare_millisecond(live_controls(frame))?;
            }
            self.frame = frame + 1;
            let mut finished = Vec::new();
            let mut source_before = [false; 64];
            for active in self.voices.iter().filter(|v| !v.retired) {
                source_before[active.slot] = active.voice.source_active();
            }
            if frame.is_multiple_of(32) {
                // Apply key groups before the next sample. Only group-bearing scores
                // need this preliminary pass; other scores retain normal control ordering.
                if self.uses_groups && self.random.is_none() {
                    for index in 0..self.voices.len() {
                        if self.voices[index].retired {
                            continue;
                        }
                        let active = &mut self.voices[index];
                        active.voice.prepare_commands(
                            active
                                .sound_controls
                                .as_mut()
                                .unwrap_or(&mut self.controls[active.channel]),
                        )?;
                        if let Some(crate::music_voice::HostRequest::Group { group, kill }) =
                            self.voices[index].voice.host_request.take()
                        {
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
                active.voice.prepare_frame(
                    active
                        .sound_controls
                        .unwrap_or(self.controls[active.channel]),
                )?;
                if let Some(random) = &self.random {
                    random.update(active.lease.unwrap(), &active.voice, false);
                }
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
            if ((frame + 1).is_multiple_of(160) || frame + 1 == self.end_frame)
                && self.random.is_none()
            {
                return self.finish_block();
            }
        }
        Ok(None)
    }
    pub(super) fn finish_block(&mut self) -> Result<Option<&[BusFrame]>> {
        if self.ended {
            return Ok(None);
        }
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
        let length = ((self.frame - 1) % 160 + 1) as usize;
        // Source handles end with their macros. The shared studio owns
        // reverb tails; silent padding must not retain voice priority.
        self.ended =
            !self.paused && !self.looping && self.events.peek().is_none() && self.voices.is_empty();
        self.block = block;
        Ok(Some(&self.block[..length]))
    }
    pub(super) fn finish(mut self) -> Option<Preview> {
        if let Some(result) = &mut self.result {
            result.free_voices = self.available.iter().copied().collect();
            for active in &self.voices {
                self.retained_lfo[active.slot] = active.voice.retained_lfo();
            }
            result.lfo_counters = self.retained_lfo;
        }
        self.result.take()
    }
}
