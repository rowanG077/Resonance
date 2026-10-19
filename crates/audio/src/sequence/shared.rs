//! Shared voice allocation, macro scheduling and randomness for music and sounds.
//!
//! The mixer prepares five 32-frame control passes in a 160-frame PCM block.
//! New cues enter the next unrendered block and cannot change queued PCM.
use super::{BusFrame, LiveControls, kernel::Kernel, stream::Owned};
use crate::{
    BLOCK_FRAMES, CONTROLS_PER_BLOCK,
    data::{Note, VoiceSource},
    package::Loaded,
};
use anyhow::{Result, ensure};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
mod allocation;
pub(crate) use allocation::Lease;

#[derive(Clone)]
pub(crate) struct Control(Arc<Mutex<ControlState>>);
struct ControlState {
    state: u32,
    draws: u64,
    pool: allocation::Pool,
    variables: [i32; 16],
}
impl Default for Control {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(ControlState {
            state: 1,
            draws: 0,
            pool: Default::default(),
            variables: [0; 16],
        })))
    }
}
impl Control {
    pub(crate) fn variable(&self, index: u8) -> i32 {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .variables[usize::from(index)]
    }

    pub(crate) fn set_variable(&self, index: u8, value: i32) {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .variables[usize::from(index)] = value;
    }
    pub(crate) fn next(&self) -> u16 {
        let mut state = self.0.lock().expect("synthesizer control lock poisoned");
        state.state = state.state.wrapping_mul(0xa835_1d63);
        state.draws += 1;
        (state.state >> 6) as u16
    }
    pub(crate) fn below(&self, upper: u16) -> u16 {
        self.next() % upper
    }
    pub(super) fn allocate(
        &self,
        source: VoiceSource,
        note: Note,
        priority: Option<u16>,
    ) -> Option<Lease> {
        let mut state = self.0.lock().expect("synthesizer control lock poisoned");
        state.pool.allocate(
            source,
            priority.unwrap_or(u16::from(note.priority)),
            note.max_voices,
        )
    }
    pub(super) fn free(&self, lease: Lease) {
        let mut state = self.0.lock().expect("synthesizer control lock poisoned");
        state.pool.free(lease);
    }
    pub(super) fn child(&self, parent: Lease, note: Note, priority: Option<u16>) -> Option<Lease> {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .child(
                parent,
                priority.unwrap_or(u16::from(note.priority)),
                note.max_voices,
            )
    }
    pub(super) fn owns(&self, lease: Lease) -> bool {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .owns(lease)
    }
    pub(super) fn handle(&self, lease: Lease) -> u32 {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .handle(lease)
    }
    fn resolve(&self, handle: u32) -> Option<Lease> {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .resolve(handle)
    }
    pub(super) fn update(
        &self,
        lease: Lease,
        voice: &crate::music_voice::Voice<'_>,
        initialized: bool,
        priority: Option<u16>,
    ) {
        let (authored, age) = voice.allocation_priority();
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .update(
                lease,
                (priority.unwrap_or(u16::from(authored)), age),
                initialized,
            );
    }
}

#[derive(Clone, Default)]
pub struct Synthesizer(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    random: Control,
    entries: Vec<Entry>,
    frame: u64,
    releases: Vec<crate::release::Release>,
    release_frame: BusFrame,
}
struct Entry {
    state: Owned,
    output: Arc<Mutex<Output>>,
}
struct Output {
    controls: [LiveControls; CONTROLS_PER_BLOCK],
    block: [BusFrame; BLOCK_FRAMES],
    length: usize,
    cursor: usize,
    frame: u64,
    last_read: Option<u64>,
    started: bool,
    paused: bool,
    sequence: bool,
}
pub(super) struct Stream(Arc<Mutex<Output>>);

impl State {
    fn fail(&mut self, index: usize, error: anyhow::Error, errors: &mut Vec<anyhow::Error>) {
        let entry = &mut self.entries[index];
        entry.state.with_dependent_mut(|_, kernel| kernel.abort());
        let mut output = entry.output.lock().expect("shared stream lock poisoned");
        if output.started && output.cursor < output.length {
            self.releases
                .push(crate::release::Release::new(output.block[output.cursor], 0));
        }
        output.length = 0;
        output.started = true;
        errors.push(error.context(format!(
            "audio {:?} entry at source frame {}",
            entry.state.borrow_owner().score().origin,
            self.frame
        )));
    }

    fn send_message(
        &mut self,
        target: crate::music_voice::MessageTarget,
        value: i32,
        full_mailboxes: &mut Vec<Lease>,
        errors: &mut Vec<anyhow::Error>,
    ) {
        let mut targets = Vec::new();
        match target {
            crate::music_voice::MessageTarget::Handle(handle) => {
                if let Some(lease) = self.random.resolve(handle) {
                    for (index, entry) in self.entries.iter().enumerate() {
                        if entry
                            .state
                            .with_dependent(|_, kernel| kernel.contains(lease))
                        {
                            targets.push((lease, index));
                            break;
                        }
                    }
                }
            }
            crate::music_voice::MessageTarget::Macro(program) => {
                for (index, entry) in self.entries.iter().enumerate() {
                    entry.state.with_dependent(|_, kernel| {
                        targets.extend(kernel.macro_members(program).map(|lease| (lease, index)));
                    });
                }
            }
        }
        for (lease, index) in targets {
            if full_mailboxes.contains(&lease) {
                continue;
            }
            if let Err(error) = self.entries[index]
                .state
                .with_dependent_mut(|_, kernel| kernel.send_message(lease, value))
            {
                full_mailboxes.push(lease);
                errors.push(error.context(format!(
                    "audio message recipient at source frame {}",
                    self.frame
                )));
            }
        }
    }

    fn apply_group(
        &mut self,
        caller: usize,
        slot: Lease,
        group: u8,
        kill: bool,
        errors: &mut Vec<anyhow::Error>,
    ) {
        let mut members = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            entry.state.with_dependent(|_, kernel| {
                members.extend(
                    kernel
                        .group_members(group)
                        .filter(|&other| index != caller || other != slot)
                        .map(|slot| (slot, index)),
                );
            });
        }
        for (slot, index) in members {
            if !self.entries[index]
                .state
                .with_dependent(|_, kernel| kernel.contains(slot))
            {
                continue;
            }
            if let Err(error) = self.entries[index]
                .state
                .with_dependent_mut(|_, kernel| kernel.apply_group(slot, kill))
            {
                self.fail(index, error, errors);
            }
        }
    }
}

impl Synthesizer {
    pub(super) fn start(&self, loaded: Arc<Loaded>, looping: bool) -> Result<Stream> {
        self.start_recorded(loaded, looping, false)
    }

    pub(super) fn start_recorded(
        &self,
        loaded: Arc<Loaded>,
        looping: bool,
        record: bool,
    ) -> Result<Stream> {
        let mut shared = self.0.lock().expect("synthesizer lock poisoned");
        let random = shared.random.clone();
        let state = Owned::try_new(loaded, |loaded| {
            Kernel::new(
                loaded.resources(),
                loaded.score(),
                loaded.tables(),
                looping,
                random,
                record,
            )
        })?;
        let output = Arc::new(Mutex::new(Output {
            controls: [LiveControls::default(); CONTROLS_PER_BLOCK],
            block: [[[0; 2]; 3]; BLOCK_FRAMES],
            length: 0,
            cursor: 0,
            frame: 0,
            last_read: None,
            started: false,
            paused: false,
            sequence: state.borrow_owner().score().origin == crate::data::ScoreOrigin::Sequence,
        }));
        shared.entries.push(Entry {
            state,
            output: output.clone(),
        });
        Ok(Stream(output))
    }

    pub(super) fn take_preview(&self, stream: &Stream) -> Option<super::Preview> {
        self.0
            .lock()
            .expect("synthesizer lock poisoned")
            .entries
            .iter_mut()
            .find(|entry| Arc::ptr_eq(&entry.output, &stream.0))
            .and_then(|entry| {
                entry
                    .state
                    .with_dependent_mut(|_, kernel| kernel.take_preview())
            })
    }

    /// Set all players' controls before this call, then read their current frame.
    /// Call once for every source-rate frame, including silence between cues.
    pub fn advance(&self) -> Result<()> {
        let mut shared = self.0.lock().expect("synthesizer lock poisoned");
        let cursor = (shared.frame % BLOCK_FRAMES as u64) as usize;
        let mut errors = Vec::new();
        if cursor == 0 {
            shared.entries.retain(|entry| {
                Arc::strong_count(&entry.output) > 1
                    || entry
                        .state
                        .with_dependent(|_, kernel| !kernel.output_complete())
            });
            for entry in &mut shared.entries {
                let owned = Arc::strong_count(&entry.output) > 1;
                let paused = entry
                    .output
                    .lock()
                    .expect("shared stream lock poisoned")
                    .paused;
                entry.state.with_dependent_mut(|_, kernel| {
                    if !owned && !kernel.output_complete() {
                        kernel.stop();
                    } else if owned {
                        kernel.pause(paused);
                    }
                });
            }
            let completed_before: Vec<_> = shared
                .entries
                .iter()
                .map(|entry| {
                    entry
                        .state
                        .with_dependent(|_, kernel| kernel.output_complete())
                })
                .collect();
            for quantum in 0..CONTROLS_PER_BLOCK {
                // Refresh all priorities before any stream admits notes this quantum.
                for entry in &mut shared.entries {
                    let priority = entry
                        .output
                        .lock()
                        .expect("shared stream lock poisoned")
                        .controls[quantum]
                        .priority;
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.set_priority(priority));
                }
                for index in 0..shared.entries.len() {
                    let controls = {
                        let output = shared.entries[index]
                            .output
                            .lock()
                            .expect("shared stream lock poisoned");
                        let input = output.controls[quantum];
                        LiveControls {
                            // One release per block, at the first requested control quantum.
                            release: input.release
                                && !output.controls[..quantum].iter().any(|c| c.release),
                            ..input
                        }
                    };
                    if let Err(error) = shared.entries[index]
                        .state
                        .with_dependent_mut(|_, kernel| kernel.prepare_controls(controls))
                    {
                        shared.fail(index, error, &mut errors);
                    }
                }
                let mut ready = Vec::new();
                for entry in &mut shared.entries {
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.sync_slots());
                }
                for (index, entry) in shared.entries.iter().enumerate() {
                    entry.state.with_dependent(|_, kernel| {
                        ready.extend(kernel.ready_voices().map(|slot| (index, slot)));
                    });
                }
                // Creation order is deterministic. Children and delivered messages
                // become runnable in the next quantum, after this snapshot.
                let mut messages = VecDeque::new();
                let mut message_counts = vec![0; shared.entries.len()];
                for (index, slot) in ready {
                    if !shared.entries[index]
                        .state
                        .with_dependent(|_, kernel| kernel.contains(slot))
                    {
                        continue;
                    }
                    let result = (|| -> Result<()> {
                        let mut fuel = crate::music_voice::INSTRUCTION_BUDGET;
                        while shared.entries[index]
                            .state
                            .with_dependent(|_, kernel| kernel.contains(slot))
                        {
                            let Some(request) =
                                shared.entries[index]
                                    .state
                                    .with_dependent_mut(|_, kernel| {
                                        kernel.run_macro(slot, &mut fuel)
                                    })?
                            else {
                                break;
                            };
                            match request {
                                crate::music_voice::HostRequest::Group { group, kill } => {
                                    shared.apply_group(index, slot, group, kill, &mut errors);
                                }
                                crate::music_voice::HostRequest::Message { target, value } => {
                                    ensure!(
                                        message_counts[index] < super::MESSAGE_BUDGET,
                                        "audio message-delivery budget exceeded"
                                    );
                                    message_counts[index] += 1;
                                    messages.push_back((index, target, value));
                                }
                                crate::music_voice::HostRequest::Spawn { note, instruction } => {
                                    shared.entries[index].state.with_dependent_mut(
                                        |_, kernel| kernel.spawn_child(slot, note, instruction),
                                    )?;
                                    // Allocation may retire another ready voice, including
                                    // one belonging to another entry.
                                    for entry in &mut shared.entries {
                                        entry
                                            .state
                                            .with_dependent_mut(|_, kernel| kernel.sync_slots());
                                    }
                                }
                            }
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        messages.retain(|(sender, _, _)| *sender != index);
                        shared.fail(index, error, &mut errors);
                    }
                }
                let mut full_mailboxes = Vec::new();
                while let Some((_, target, value)) = messages.pop_front() {
                    shared.send_message(target, value, &mut full_mailboxes, &mut errors);
                }
                for index in 0..shared.entries.len() {
                    if let Err(error) = shared.entries[index]
                        .state
                        .with_dependent_mut(|_, kernel| kernel.next_quantum())
                    {
                        shared.fail(index, error, &mut errors);
                    }
                }
            }
            for (entry, completed) in shared.entries.iter_mut().zip(completed_before) {
                let mut output = entry.output.lock().expect("shared stream lock poisoned");
                let length = entry
                    .state
                    .with_dependent_mut(|_, kernel| kernel.finish_block(&mut output.block));
                output.length = if completed { 0 } else { length };
                output.started = true;
                for control in &mut output.controls {
                    control.release = false;
                }
            }
        }
        let mut release_frame = [[0; 2]; 3];
        for release in &mut shared.releases {
            release.mix(&mut release_frame);
        }
        shared.releases.retain(crate::release::Release::active);
        shared.release_frame = release_frame;

        for entry in &shared.entries {
            let mut output = entry.output.lock().expect("shared stream lock poisoned");
            output.cursor = cursor;
            output.frame = shared.frame;
        }
        shared.frame += 1;
        ensure!(
            errors.is_empty(),
            "{}",
            errors
                .into_iter()
                .map(|error| format!("{error:#}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(())
    }

    /// Fade a stopped source after its already queued frames have played.
    pub fn release(&self, samples: BusFrame, queued_frames: u64) {
        self.0
            .lock()
            .expect("synthesizer lock poisoned")
            .releases
            .push(crate::release::Release::new(samples, queued_frames));
    }

    /// Read after all players: deliver queued PCM exactly once, including a
    /// dropped or paused player's already submitted block and release tails.
    pub fn unread_frame(&self) -> BusFrame {
        let shared = self.0.lock().expect("synthesizer lock poisoned");
        let Some(frame) = shared.frame.checked_sub(1) else {
            return [[0; 2]; 3];
        };
        let mut buses = shared.release_frame;
        for entry in &shared.entries {
            let output = entry.output.lock().expect("shared stream lock poisoned");
            if output.last_read != Some(frame) && output.cursor < output.length {
                for (bus, source) in buses.iter_mut().zip(output.block[output.cursor]) {
                    for (sample, source) in bus.iter_mut().zip(source) {
                        *sample += source;
                    }
                }
            }
        }
        buses
    }

    /// Diagnostics for deterministic offline runs; never resets the stream.
    pub fn random_state(&self) -> (u32, u64) {
        let shared = self.0.lock().expect("synthesizer lock poisoned");
        let random = shared
            .random
            .0
            .lock()
            .expect("synthesizer control lock poisoned");
        (random.state, random.draws)
    }
}

#[cfg(test)]
mod tests;

impl Stream {
    pub(super) fn pause(&self, paused: bool) -> Result<()> {
        let mut output = self.0.lock().expect("shared stream lock poisoned");
        ensure!(output.sequence, "only a sequence can be paused");
        output.paused = paused;
        Ok(())
    }
    pub(super) fn controls(&self, controls: [LiveControls; CONTROLS_PER_BLOCK]) -> Result<()> {
        super::stream::validate_controls(controls)?;
        self.0.lock().expect("shared stream lock poisoned").controls = controls;
        Ok(())
    }
    pub(super) fn frame(&self) -> Option<BusFrame> {
        let mut output = self.0.lock().expect("shared stream lock poisoned");
        let frame = if !output.started {
            Some([[0; 2]; 3])
        } else {
            (output.cursor < output.length).then(|| output.block[output.cursor])
        };
        if frame.is_some() {
            output.last_read = Some(output.frame);
        }
        frame
    }
    pub(super) fn started(&self) -> bool {
        self.0.lock().expect("shared stream lock poisoned").started
    }
    pub(super) fn submitted_until(&self) -> u64 {
        let output = self.0.lock().expect("shared stream lock poisoned");
        if output.started {
            output.frame - output.cursor as u64 + output.length as u64
        } else {
            0
        }
    }
    pub(super) fn control_boundary(&self) -> bool {
        let output = self.0.lock().expect("shared stream lock poisoned");
        output.started && output.cursor == 0
    }
}
