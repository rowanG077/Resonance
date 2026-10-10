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
    /// External pan offset in fourteen-bit controller units.
    pan_offset: i16,
    priority: Option<u16>,
    events: Peekable<Iter<'a, Event>>,
    tempos: Peekable<Iter<'a, Tempo>>,
    bpm: u32,
    clock: MusicalClock,
    loop_offset: u128,
    voices: Vec<Active<'a>>,
    result: Option<Preview>,
    random: shared::Control,
}
impl Drop for Kernel<'_> {
    fn drop(&mut self) {
        for active in self.voices.iter().filter(|v| !v.retired) {
            self.random.free(active.lease);
        }
    }
}
impl<'a> Kernel<'a> {
    pub(super) fn new(
        bank: &'a Resources,
        song: &'a Score,
        tables: &'a Tables,
        looping: bool,
        random: shared::Control,
        record: bool,
    ) -> Result<Self> {
        ensure!(
            !looping || song.origin == crate::data::ScoreOrigin::Sequence,
            "sound effects cannot loop a score; their macros control repetition"
        );
        let controls = song.controls;
        let first_events = &song.first_events;
        let events = first_events.iter().peekable();
        let tempos = song.tempos.iter().peekable();
        let bpm = song.initial_bpm_1024;
        let voices: Vec<Active<'_>> = Vec::with_capacity(VOICE_BUDGET);
        let result = record.then(Preview::default);

        Ok(Self {
            bank,
            song,
            tables,
            looping,
            frame: 0,
            ended: false,
            paused: false,
            controls,
            pan_offset: 0,
            priority: None,
            events,
            tempos,
            bpm,
            clock: MusicalClock::default(),
            loop_offset: 0,
            voices,
            result,
            random,
        })
    }
    /// Pausing removes active notes while preserving the score cursor.
    pub(super) fn pause(&mut self, paused: bool) {
        if paused && !self.paused {
            self.sync_slots();
            for active in self.voices.iter_mut().filter(|v| !v.retired) {
                active.voice.kill();
            }
            self.retire_finished(self.frame);
            self.voices
                .retain(|active| !active.retired || active.voice.output_pending());
        }
        self.paused = paused;
    }
    pub(super) fn stop(&mut self) {
        self.pause(true);
        self.paused = false;
        self.looping = false;
        self.events = [].iter().peekable();
    }
    /// A failed entry cannot resume; free only its allocations and pending work.
    pub(super) fn abort(&mut self) {
        for active in self.voices.iter().filter(|voice| !voice.retired) {
            self.random.free(active.lease);
        }
        self.voices.clear();
        self.events = [].iter().peekable();
        self.looping = false;
        self.paused = false;
        self.ended = true;
    }
    pub(super) fn output_complete(&self) -> bool {
        !self.paused
            && !self.looping
            && self.events.clone().next().is_none()
            && self.voices.is_empty()
    }
    pub(super) fn sync_slots(&mut self) {
        let random = &self.random;
        for active in &mut self.voices {
            let lease = active.lease;
            if !active.retired && !random.owns(lease) {
                active.voice.kill();
            }
        }
        self.retire_finished(self.frame);
    }
    pub(super) fn ready_voices(&self) -> impl Iterator<Item = shared::Lease> + '_ {
        self.voices
            .iter()
            .filter(|voice| !voice.retired && voice.voice.commands_ready())
            .map(|voice| voice.lease)
    }
    pub(super) fn run_macro(
        &mut self,
        slot: shared::Lease,
        fuel: &mut usize,
    ) -> Result<Option<crate::music_voice::HostRequest>> {
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == slot)
            .unwrap();
        active.voice.prepare_commands(
            active
                .sound_controls
                .as_mut()
                .unwrap_or(&mut self.controls[active.channel]),
            fuel,
        )?;
        self.random.update(slot, &active.voice, true, self.priority);
        let request = active.voice.host_request.take();
        self.retire_finished(self.frame);
        Ok(request)
    }
    pub(super) fn spawn_child(
        &mut self,
        parent: shared::Lease,
        note: crate::data::Note,
        instruction: u16,
    ) -> Result<()> {
        let random = &self.random;
        let Some(lease) = random.child(parent, note, self.priority) else {
            return Ok(());
        };
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == parent)
            .unwrap();
        active.voice.last_child = random.handle(lease);
        let mut voice = active.voice.child(note, instruction, self.frame)?;
        voice.handle = random.handle(lease);
        let (channel, end_tick) = (active.channel, active.end_tick);
        let sound_controls = active.sound_controls.map(|controls| controls.child());
        let lifetime = if let Some(result) = &mut self.result {
            let index = result.voice_lifetimes.len();
            result.voice_lifetimes.push(VoiceLifetime {
                slot: lease.slot,
                start_frame: self.frame as u32,
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
            sound_controls,
            channel,
            end_tick,
            lease,
            retired: false,
            lifetime,
        });
        Ok(())
    }
    fn retire_finished(&mut self, frame: u64) {
        for active in self
            .voices
            .iter_mut()
            .filter(|voice| !voice.retired && voice.voice.is_done())
        {
            active.retired = true;
            self.random.free(active.lease);
            if let Some(result) = &mut self.result {
                result.voice_lifetimes[active.lifetime].end_frame = Some(frame as u32);
            }
        }
    }
    pub(super) fn group_members(&self, group: u8) -> impl Iterator<Item = shared::Lease> + '_ {
        self.voices
            .iter()
            .filter(move |v| !v.retired && v.voice.exclusive_group == group)
            .map(|v| v.lease)
    }
    pub(super) fn macro_members(&self, program: u16) -> impl Iterator<Item = shared::Lease> + '_ {
        self.voices
            .iter()
            .filter(move |v| !v.retired && v.voice.original_macro == program)
            .map(|v| v.lease)
    }
    pub(super) fn send_message(&mut self, lease: shared::Lease, value: i32) -> Result<()> {
        if let Some(active) = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == lease)
        {
            active.voice.send_message(value)?;
        }
        Ok(())
    }
    pub(super) fn apply_group(&mut self, slot: shared::Lease, kill: bool) -> Result<()> {
        let active = self
            .voices
            .iter_mut()
            .find(|v| !v.retired && v.lease == slot)
            .unwrap();
        if kill {
            active.voice.kill();
        } else {
            active.voice.key_off()?;
        }
        self.retire_finished(self.frame);
        Ok(())
    }
    pub(super) fn contains(&self, slot: shared::Lease) -> bool {
        self.voices.iter().any(|v| !v.retired && v.lease == slot)
    }
    fn apply_tempos(&mut self) {
        let tick = self.clock.tick() - self.loop_offset;
        while self
            .tempos
            .peek()
            .is_some_and(|tempo| u128::from(tempo.tick) <= tick)
        {
            self.bpm = self.tempos.next().unwrap().bpm_1024;
        }
    }
    fn dispatch_events(&mut self, frame: u64) -> Result<()> {
        let tick = self.clock.tick() - self.loop_offset;
        while self
            .events
            .peek()
            .is_some_and(|e| u128::from(e.tick) <= tick)
        {
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
                        let mut voice = Voice::new_at(
                            self.bank,
                            self.tables,
                            note,
                            frame,
                            self.random.clone(),
                        )?;
                        let Some(lease) = self.random.allocate(*source, note, self.priority) else {
                            continue;
                        };
                        self.sync_slots();
                        voice.handle = self.random.handle(lease);
                        let lifetime = if let Some(result) = &mut self.result {
                            let index = result.voice_lifetimes.len();
                            result.voice_lifetimes.push(VoiceLifetime {
                                slot: lease.slot,
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
                            end_tick: (source.origin() == crate::data::ScoreOrigin::Sequence)
                                .then_some(
                                    self.loop_offset + u128::from(event.tick) + u128::from(*length),
                                ),
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
    pub(super) fn set_priority(&mut self, priority: Option<u16>) {
        if self.priority == priority {
            return;
        }
        self.priority = priority;
        for active in self.voices.iter().filter(|voice| !voice.retired) {
            self.random
                .update(active.lease, &active.voice, false, priority);
        }
    }
    pub(super) fn prepare_controls(&mut self, input: LiveControls) -> Result<()> {
        self.sync_slots();
        if self.ended || self.paused {
            return Ok(());
        }
        self.set_priority(input.priority);
        let frame = self.frame;
        let song = self.song;
        let loop_tick = song.loop_start_tick;
        let end_tick = song.end_tick;
        let loop_events = &song.loop_events;
        self.pan_offset = input.pan.map_or(0, |pan| {
            (i16::from(pan) - i16::from(crate::mix::CENTER_PAN)) << 7
        });
        for channel in &mut self.controls {
            channel.group_volume = input.volume;
            channel.mono = input.mono;
        }
        self.apply_tempos();
        self.dispatch_events(frame)?;
        while self.looping && self.clock.tick() - self.loop_offset >= u128::from(end_tick) {
            self.loop_offset += u128::from(end_tick - loop_tick);
            self.events = loop_events.iter().peekable();
            self.tempos = song.tempos.iter().peekable();
            self.bpm = song.initial_bpm_1024;
            self.apply_tempos();
            self.dispatch_events(frame)?;
            if let Some(result) = &mut self.result {
                result.loop_starts.push(frame as u32);
            }
        }
        if input.release {
            for active in self.voices.iter_mut().filter(|v| !v.retired) {
                active.voice.key_off()?;
                active.end_tick = None;
            }
        }
        if let Some(result) = &mut self.result {
            result.final_tick =
                u32::try_from(self.clock.tick() - self.loop_offset).unwrap_or(u32::MAX);
        }
        for active in &mut self.voices {
            if active.retired {
                continue;
            }
            if active.end_tick.is_some_and(|end| end <= self.clock.tick()) {
                active.voice.key_off()?;
                active.end_tick = None;
            }
        }
        if let Some(result) = &mut self.result {
            result.maximum_voices = result
                .maximum_voices
                .max(self.voices.iter().filter(|v| !v.retired).count());
        }
        for active in &mut self.voices {
            if let Some(controls) = &mut active.sound_controls {
                controls.group_volume = input.volume;
                controls.mono = input.mono;
            }
            active.voice.set_tempo(self.bpm);
        }
        Ok(())
    }
    pub(super) fn next_quantum(&mut self) -> Result<()> {
        if self.ended {
            return Ok(());
        }
        for frame in self.frame..self.frame + CONTROL_FRAMES as u64 {
            self.frame = frame + 1;
            for active in self
                .voices
                .iter_mut()
                .filter(|v| !v.retired || v.voice.output_pending())
            {
                let controls = active
                    .sound_controls
                    .unwrap_or(self.controls[active.channel]);
                active.voice.prepare_frame(controls, self.pan_offset)?;
            }
            self.retire_finished(frame);
            if !self.paused {
                self.clock.advance(1, self.bpm);
            }
        }
        // Allocation resumes at the next control quantum. Publish the final ages
        // now; finished voices were already retired at their exact source frame.
        for active in self.voices.iter().filter(|voice| !voice.retired) {
            self.random
                .update(active.lease, &active.voice, false, self.priority);
        }
        Ok(())
    }
    pub(super) fn finish_block(&mut self, block: &mut [BusFrame; BLOCK_FRAMES]) -> usize {
        block.fill([[0; 2]; 3]);
        if self.ended {
            return 0;
        }
        for active in &mut self.voices {
            active.voice.mix_block(block);
        }
        self.voices
            .retain(|active| !active.retired || active.voice.output_pending());
        // Completion follows the last queued sample or release tail.
        self.ended =
            !self.paused && !self.looping && self.events.peek().is_none() && self.voices.is_empty();
        BLOCK_FRAMES
    }
    pub(super) fn take_preview(&mut self) -> Option<Preview> {
        self.result.take()
    }
}
