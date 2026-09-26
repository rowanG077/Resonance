//! Shared voice allocation, macro scheduling and randomness for music and sounds.
//!
//! The audio interrupt prepares five 1ms control passes before submitting a
//! 160-frame DSP block. New cues enter the next unrendered block; a cue cannot
//! insert a random draw into another cue's already prepared PCM.
use super::{BusFrame, ClockStart, LiveControls, kernel::Kernel, stream::Owned};
use crate::{
    data::{Command, Note, Resources, ScoreOrigin, VoiceSource},
    package::Loaded,
};
use anyhow::{Result, ensure};
use std::{
    cmp::Reverse,
    sync::{Arc, Mutex, Weak},
};
mod allocation;
pub(crate) use allocation::Lease;

#[derive(Clone)]
pub(crate) struct Control(Arc<Mutex<ControlState>>);
struct ControlState {
    state: u32,
    draws: u64,
    order: u64,
    pool: allocation::Pool,
    variables: [i32; 16],
}
impl Default for Control {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(ControlState {
            state: 1,
            draws: 0,
            order: 0,
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
    pub(crate) fn schedule(&self) -> u64 {
        let mut state = self.0.lock().expect("synthesizer control lock poisoned");
        state.order += 1;
        state.order
    }
    pub(super) fn allocate(&self, source: VoiceSource, note: Note) -> Option<(Lease, u32)> {
        let mut state = self.0.lock().expect("synthesizer control lock poisoned");
        state.pool.allocate(source, note.priority, note.max_voices)
    }
    pub(super) fn free(&self, lease: Lease, voice: &crate::music_voice::Voice<'_>) {
        let mut state = self.0.lock().expect("synthesizer control lock poisoned");
        state.pool.retain_mailbox(lease, voice.mailbox_state());
        state.pool.free(lease, voice.retained_lfo());
    }
    pub(super) fn child(&self, parent: Lease, note: Note) -> Option<(Lease, u32)> {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .child(parent, note.priority, note.max_voices)
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
    fn resolve(&self, handle: u32) -> Result<Option<Lease>> {
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
    ) {
        self.0
            .lock()
            .expect("synthesizer control lock poisoned")
            .pool
            .update(
                lease,
                voice.allocation_priority(),
                voice.retained_lfo(),
                initialized,
            );
    }
}

fn draws_random(command: &Command) -> bool {
    matches!(
        command,
        Command::RandomWait { .. }
            | Command::RandomNote { .. }
            | Command::RandomBranch { .. }
            | Command::RandomLoop { .. }
    )
}

pub(crate) fn uses_random(resources: &Resources) -> bool {
    resources.programs.values().flatten().any(draws_random)
}

pub(crate) fn requires_shared(resources: &Resources) -> bool {
    uses_random(resources)
        || resources.programs.values().flatten().any(|command| {
            matches!(
                command,
                Command::SpawnMacro { .. }
                    | Command::VoiceHandle { .. }
                    | Command::SendMessage { .. }
                    | Command::ReceiveMessage { .. }
                    | Command::MessageTrap { .. }
                    | Command::ClearMessageTrap
            ) || command.variables().into_iter().flatten().any(|variable| {
                matches!(
                    variable,
                    crate::data::Variable::Global(_) | crate::data::Variable::Controller(_)
                )
            })
        })
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Wake {
    Timer { deadline: u64, order: u64 },
    Runnable(u64),
}
impl Wake {
    fn key(self, offset: u64) -> (u8, Reverse<u64>, u64) {
        match self {
            Self::Timer { deadline, order } => (0, Reverse(offset + deadline), order),
            Self::Runnable(order) => (1, Reverse(order), 0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VoiceKey {
    cue: u64,
    slot: Lease,
}

#[derive(Clone, Default)]
pub struct Synthesizer(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    random: Control,
    entries: Vec<Entry>,
    frame: u64,
    next_id: u64,
    studio: Vec<VoiceKey>,
}
struct Entry {
    id: u64,
    state: Owned,
    output: Weak<Mutex<Output>>,
    started_at: Option<u64>,
}
struct Output {
    controls: [LiveControls; 5],
    block: [BusFrame; 160],
    length: usize,
    cursor: usize,
    started: bool,
    paused: bool,
    sequence: bool,
}
pub(super) struct Stream(Arc<Mutex<Output>>);

impl State {
    fn send_message(
        &mut self,
        target: crate::music_voice::MessageTarget,
        value: i32,
    ) -> Result<()> {
        let mut targets = Vec::new();
        match target {
            crate::music_voice::MessageTarget::Handle(handle) => {
                if let Some(lease) = self.random.resolve(handle)? {
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
                targets.sort_unstable_by_key(|&(lease, _)| lease.slot);
            }
        }
        for (lease, index) in targets {
            self.entries[index]
                .state
                .with_dependent_mut(|_, kernel| kernel.send_message(lease, value));
        }
        Ok(())
    }

    fn priority(&self, key: VoiceKey) -> Option<u32> {
        self.entries
            .iter()
            .find(|entry| entry.id == key.cue)?
            .state
            .with_dependent(|_, kernel| kernel.source_priority(key.slot))
    }

    fn remove_inactive_sources(&mut self) {
        let entries = &self.entries;
        self.studio.retain(|key| {
            entries
                .iter()
                .find(|entry| entry.id == key.cue)
                .is_some_and(|entry| {
                    entry
                        .state
                        .with_dependent(|_, kernel| kernel.source_priority(key.slot).is_some())
                })
        });
    }

    fn update_sources(&mut self) {
        let mut changes = Vec::new();
        for entry in &mut self.entries {
            entry.state.with_dependent_mut(|_, kernel| {
                changes.extend(kernel.source_changes().filter_map(|(slot, order, start)| {
                    start.then_some((
                        order,
                        VoiceKey {
                            cue: entry.id,
                            slot,
                        },
                    ))
                }));
            });
        }
        changes.sort_by_key(|&(order, _)| Reverse(order));
        for (_, key) in changes {
            self.studio.retain(|&old| old != key);
            if self.priority(key).is_some() {
                self.studio.insert(0, key);
            }
        }
        self.remove_inactive_sources();
    }

    fn apply_group(&mut self, caller: usize, slot: Lease, group: u8, kill: bool) -> Result<()> {
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
        members.sort_unstable_by_key(|&(slot, _)| slot.slot);
        for (slot, index) in members {
            self.entries[index]
                .state
                .with_dependent_mut(|_, kernel| kernel.apply_group(slot, kill))?;
        }
        Ok(())
    }
}

impl Synthesizer {
    pub(super) fn start(&self, loaded: Arc<Loaded>, looping: bool) -> Result<Stream> {
        let mut shared = self.0.lock().expect("synthesizer lock poisoned");
        shared.entries.retain(|e| e.output.strong_count() != 0);
        ensure!(
            shared.entries.len() < 64,
            "shared synthesizer cue budget exhausted"
        );
        let random = shared.random.clone();
        let state = Owned::try_new(loaded, |loaded| {
            let mut kernel = Kernel::new(
                &loaded.resources,
                &loaded.score,
                &loaded.tables,
                None,
                looping,
                ClockStart::Running,
            )?;
            kernel.set_random(random);
            Ok::<_, anyhow::Error>(kernel)
        })?;
        let output = Arc::new(Mutex::new(Output {
            controls: [LiveControls::default(); 5],
            block: [[[0; 2]; 3]; 160],
            length: 0,
            cursor: 0,
            started: false,
            paused: false,
            sequence: state.borrow_owner().score.origin == crate::data::ScoreOrigin::Sequence,
        }));
        let id = shared.next_id;
        shared.next_id += 1;
        shared.entries.push(Entry {
            id,
            state,
            output: Arc::downgrade(&output),
            started_at: None,
        });
        Ok(Stream(output))
    }

    /// Set all players' controls before this call, then read their current frame.
    /// Call once for every 32kHz mixer frame, including silence between cues.
    pub fn advance(&self) -> Result<()> {
        let mut shared = self.0.lock().expect("synthesizer lock poisoned");
        let cursor = (shared.frame % 160) as usize;
        if cursor == 0 {
            shared.entries.retain(|e| e.output.strong_count() != 0);
            // Continuing a sequence prepends it to the native sequence list.
            // This vector is traversed in reverse for sequence preparation.
            shared.entries.sort_by_key(|entry| {
                entry.output.upgrade().is_some_and(|output| {
                    entry.state.with_dependent(|_, kernel| kernel.paused())
                        && !output.lock().expect("shared stream lock poisoned").paused
                })
            });
            for entry in &mut shared.entries {
                if let Some(output) = entry.output.upgrade() {
                    let paused = output.lock().expect("shared stream lock poisoned").paused;
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.pause(paused));
                }
            }
            let frame = shared.frame;
            for entry in &mut shared.entries {
                entry.started_at.get_or_insert(frame);
            }
            // Sound effects allocate in request order before the sequence pass.
            // Newly started sequences enter the head of the sequence list.
            let sound_effect = |index: usize| {
                shared.entries[index].state.borrow_owner().score.origin == ScoreOrigin::SoundEffect
            };
            let preparation_order: Vec<_> = (0..shared.entries.len())
                .filter(|&index| sound_effect(index))
                .chain(
                    (0..shared.entries.len())
                        .rev()
                        .filter(|&index| !sound_effect(index)),
                )
                .collect();
            for ms in 0..5 {
                for &index in &preparation_order {
                    let entry = &mut shared.entries[index];
                    let controls = entry
                        .output
                        .upgrade()
                        .map(|output| {
                            let controls =
                                output.lock().expect("shared stream lock poisoned").controls[ms];
                            LiveControls {
                                release: controls.release && ms == 0,
                                ..controls
                            }
                        })
                        .unwrap_or_default();
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.prepare_millisecond(controls))?;
                }
                let mut ready = Vec::new();
                for entry in &mut shared.entries {
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.sync_slots());
                }
                for (index, entry) in shared.entries.iter().enumerate() {
                    entry.state.with_dependent(|_, kernel| {
                        ready.extend(kernel.wakes().map(|(slot, wake)| {
                            (wake.key(entry.started_at.unwrap() * 8), index, slot)
                        }));
                    });
                }
                ready.sort_by_key(|&(key, _, _)| key);
                for entry in &mut shared.entries {
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.wake_timers());
                }
                // Snapshot this pass: group callbacks prepended during dispatch
                // run next millisecond; already-runnable targets retain their place.
                for (_, index, slot) in ready {
                    if !shared.entries[index]
                        .state
                        .with_dependent(|_, kernel| kernel.contains(slot))
                    {
                        continue;
                    }
                    let mut complete = false;
                    for _ in 0..65536 {
                        let request = shared.entries[index]
                            .state
                            .with_dependent_mut(|_, kernel| kernel.run_macro(slot))?;
                        let Some(request) = request else {
                            complete = true;
                            break;
                        };
                        match request {
                            crate::music_voice::HostRequest::Group { group, kill } => {
                                shared.apply_group(index, slot, group, kill)?
                            }
                            crate::music_voice::HostRequest::Message { target, value } => {
                                shared.send_message(target, value)?;
                            }
                            crate::music_voice::HostRequest::Spawn { note, instruction } => {
                                shared.entries[index]
                                    .state
                                    .with_dependent_mut(|_, kernel| {
                                        kernel.spawn_child(slot, note, instruction)
                                    })?;
                                // A child may replace a voice whose macro was already
                                // queued in this pass, including one in another cue.
                                for entry in &mut shared.entries {
                                    entry
                                        .state
                                        .with_dependent_mut(|_, kernel| kernel.sync_slots());
                                }
                            }
                        }
                    }
                    ensure!(complete, "music host instruction budget exhausted");
                }
                for entry in &mut shared.entries {
                    entry.state.with_dependent_mut(|_, kernel| {
                        kernel.next_millisecond(LiveControls::default()).map(|_| ())
                    })?;
                }
                shared.update_sources();
            }
            let mut completed: Vec<_> = shared
                .studio
                .iter()
                .filter_map(|&key| shared.priority(key).map(|priority| (key, priority)))
                .collect();
            crate::voice_order::completion_order(&mut completed);
            for entry in &mut shared.entries {
                entry.state.with_dependent_mut(|_, kernel| {
                    let block = kernel.finish_block()?;
                    if let Some(output) = entry.output.upgrade() {
                        let mut output = output.lock().expect("shared stream lock poisoned");
                        output.length = block.map_or(0, |b| b.len());
                        if let Some(block) = block {
                            output.block[..block.len()].copy_from_slice(block);
                        }
                        output.started = true;
                        for control in &mut output.controls {
                            control.release = false;
                        }
                    }
                    Ok::<_, anyhow::Error>(())
                })?;
            }
            // DSP callbacks visit the reversed partition and prepend runnable
            // voices, producing forward partition order at the next macro pass.
            for (key, _) in completed.into_iter().rev() {
                if let Some(entry) = shared.entries.iter_mut().find(|entry| entry.id == key.cue) {
                    entry
                        .state
                        .with_dependent_mut(|_, kernel| kernel.sample_end_callback(key.slot));
                }
            }
            shared.remove_inactive_sources();
        }

        for entry in &shared.entries {
            if let Some(output) = entry.output.upgrade() {
                output.lock().expect("shared stream lock poisoned").cursor = cursor;
            }
        }
        shared.frame += 1;
        Ok(())
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
impl Stream {
    pub(super) fn pause(&self, paused: bool) -> Result<()> {
        let mut output = self.0.lock().expect("shared stream lock poisoned");
        ensure!(output.sequence, "only a sequence can be paused");
        output.paused = paused;
        Ok(())
    }
    pub(super) fn controls(&self, controls: [LiveControls; 5]) -> Result<()> {
        super::stream::validate_controls(controls)?;
        self.0.lock().expect("shared stream lock poisoned").controls = controls;
        Ok(())
    }
    pub(super) fn frame(&self) -> Option<BusFrame> {
        let output = self.0.lock().expect("shared stream lock poisoned");
        if !output.started {
            return Some([[0; 2]; 3]);
        }
        (output.cursor < output.length).then(|| output.block[output.cursor])
    }
    pub(super) fn started(&self) -> bool {
        self.0.lock().expect("shared stream lock poisoned").started
    }
    pub(super) fn control_boundary(&self) -> bool {
        let output = self.0.lock().expect("shared stream lock poisoned");
        output.started && output.cursor == 0
    }
}
