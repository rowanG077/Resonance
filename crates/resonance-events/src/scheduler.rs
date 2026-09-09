use crate::ResourceLibrary;
use crate::native::NativeHost;
use crate::operation::Wait;
use crate::{Animation, GameWorld, animation::slot};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;
use symphonia_script::Program;
use symphonia_script_vm::{Memory, RunEvent, Vm};

struct Instance {
    handle: i32,
    key: Option<u32>,
    vm: Vm,
    wait: Option<Wait>,
    registers: [i32; 6],
}

/// Stable slot order and one shared script data region, following the original
/// 32-instance event pool. Scheduling is independent of render frame rate.
pub struct EventRuntime {
    pub world: GameWorld,
    program: Arc<Program>,
    resources: Arc<ResourceLibrary>,
    memory: Memory,
    instances: Vec<Option<Instance>>,
    next_handle: i32,
    failed: bool,
    interaction: Option<i32>,
}
impl EventRuntime {
    pub fn new(program: Arc<Program>, resources: Arc<ResourceLibrary>) -> Result<Self> {
        Self::with_state(program, resources, GameWorld::default(), Memory::default())
    }
    pub fn with_state(
        program: Arc<Program>,
        resources: Arc<ResourceLibrary>,
        world: GameWorld,
        memory: Memory,
    ) -> Result<Self> {
        let main = Instance {
            handle: 1,
            key: None,
            vm: Vm::new(program.clone(), program.entry())?,
            wait: None,
            registers: [0; 6],
        };
        let mut instances: Vec<Option<Instance>> = (0..32).map(|_| None).collect();
        instances[0] = Some(main);
        let mut events = Self {
            world,
            program,
            resources,
            memory,
            instances,
            next_handle: 2,
            failed: false,
            interaction: None,
        };
        events.execute()?;
        Ok(events)
    }
    pub fn tick(&self) -> u32 {
        self.world.tick
    }
    pub fn active_instances(&self) -> usize {
        self.instances.iter().filter(|i| i.is_some()).count()
    }
    /// Read-only wait inventory for bounded replay failures and developer tools.
    pub fn pending_operations(&self) -> Vec<String> {
        self.instances
            .iter()
            .flatten()
            .filter_map(|instance| {
                instance.wait.as_ref().map(|wait| {
                    format!(
                        "event {:?}, handle {}: {wait:?}",
                        instance.key, instance.handle
                    )
                })
            })
            .collect()
    }
    pub fn memory(&self) -> &Memory {
        &self.memory
    }
    /// Retire the scene and copy only global script memory; field-local bytes reset.
    pub fn take_persistent(&mut self) -> Result<crate::PersistentState> {
        self.cancel();
        let mut memory = Memory::default();
        for offset in (0..0x400).step_by(4) {
            let width = symphonia_script::Width::S32;
            memory.write(offset, width, self.memory.read(offset, width)?)?;
        }
        Ok(crate::PersistentState {
            memory,
            party: self.world.party.take(),
            event_flags: std::mem::take(&mut self.world.event_flags),
            event_records: std::mem::take(&mut self.world.event_records),
            random_state: self.world.random_state,
            tick: self.world.tick,
        })
    }
    /// Actor interaction entries use registry kind zero. Input selection and
    /// reachability belong to the game layer; the scheduler owns exclusivity
    /// and returns control when the foreground event finishes.
    pub fn has_interaction(&self, actor: i32) -> bool {
        u32::try_from(actor)
            .ok()
            .is_some_and(|key| self.program.event(0, key).is_some())
    }
    pub fn interact(&mut self, actor: i32) -> Result<bool> {
        let Ok(key) = u32::try_from(actor) else {
            return Ok(false);
        };
        self.start_foreground(0, key)
    }
    /// Confirmed triggers use registry kind 2; automatic crossings use kind 1.
    pub fn trigger(&mut self, key: u32, confirmed: bool) -> Result<bool> {
        self.start_foreground(if confirmed { 2 } else { 1 }, key)
    }
    fn start_foreground(&mut self, kind: u32, key: u32) -> Result<bool> {
        ensure!(!self.failed, "event runtime stopped after a script failure");
        if !self.world.input_enabled || self.interaction.is_some() {
            return Ok(false);
        }
        let Some(pc) = self.program.event(kind, key) else {
            return Ok(false);
        };
        let entry = self
            .instances
            .iter_mut()
            .find(|i| i.is_none())
            .context("event pool exhausted (32 instances)")?;
        let handle = self.next_handle;
        self.next_handle = handle.checked_add(1).context("event handle overflow")?;
        *entry = Some(Instance {
            handle,
            key: Some(key),
            vm: Vm::new(self.program.clone(), pc)?,
            wait: None,
            registers: [0; 6],
        });
        self.interaction = Some(handle);
        // A crossing may immediately fail its story condition. Queue it exclusively,
        // but let the script take control explicitly; stopping movement here would
        // restart the player’s gait on every inactive trigger.
        if kind == 0 {
            self.world.input_enabled = false;
            if let Some(controlled) = self.world.actors.get_mut(&self.world.controlled_actor) {
                controlled.motion = None;
            }
        }
        Ok(true)
    }
    /// Scene exit stops the callers and invalidates outstanding callbacks.
    pub fn cancel(&mut self) {
        self.instances.iter_mut().for_each(|i| *i = None);
        self.world.operations.cancel();
        self.world.dialogue.clear();
        self.world.choices.clear();
        self.world.movie = None;
        self.world.voice = None;
        self.world.field_transition = None;
        self.world.preload_field = None;
        self.interaction = None;
        self.world.input_enabled = false;
    }
    pub fn step(&mut self) -> Result<()> {
        self.step_with_motion(|_, _, _| {})
    }
    /// Resolve movement against the scene before scripts observe actor positions.
    /// The event crate owns intent; the caller owns terrain and collision data.
    pub fn step_with_motion(
        &mut self,
        mut resolve: impl FnMut(i32, &mut crate::Actor, [f32; 3]),
    ) -> Result<()> {
        ensure!(!self.failed, "event runtime stopped after a script failure");
        if self.world.blocked_by_movie() {
            return Ok(());
        }
        self.world.tick = self.world.tick.checked_add(1).context("clock overflow")?;
        for dialogue in self.world.dialogue.values_mut() {
            if let Some(id) = dialogue.opening_actor
                && self
                    .world
                    .actors
                    .get(&id)
                    .is_none_or(|a| a.heading == a.target_heading)
            {
                dialogue.opening_actor = None;
            }
        }
        for (id, actor) in &mut self.world.actors {
            let turn = actor.turn_direction();
            let previous = actor.position;
            actor.step_motion();
            resolve(*id, actor, previous);
            actor.step_heading(self.world.input_enabled && *id == self.world.controlled_actor);
            if !actor.scripted_animation
                && let Some(model) = self.resources.model(actor.resource)
            {
                let dialogue = self.world.dialogue.values().any(|d| d.operation.is_pending()
                    && matches!(d.anchor, crate::dialogue::DialogueAnchor::Actor(speaker) if speaker == *id));
                // The player has a distinct idle pose while an event owns control.
                let event_controlled =
                    !self.world.input_enabled && *id == self.world.controlled_actor;
                let player_locomotion =
                    self.world.input_enabled && *id == self.world.controlled_actor;
                let requested = if let Some(motion) = &actor.motion {
                    if motion.speed > 7. {
                        slot::RUN
                    } else {
                        slot::WALK
                    }
                } else if turn != 0. && model.clips.contains_key(&slot::TURN_RIGHT) {
                    if turn < 0. {
                        slot::TURN_LEFT
                    } else {
                        slot::TURN_RIGHT
                    }
                } else if dialogue {
                    // Use the event conversation pose when the script owns the player.
                    [
                        if event_controlled {
                            slot::EVENT_TALK
                        } else {
                            slot::TALK
                        },
                        slot::TALK_FALLBACK,
                        slot::IDLE,
                    ]
                    .into_iter()
                    .find(|slot| model.clips.contains_key(slot))
                    .unwrap_or(actor.idle_animation)
                } else if event_controlled && model.clips.contains_key(&slot::EVENT_IDLE) {
                    slot::EVENT_IDLE
                } else {
                    actor.idle_animation
                };
                let slot = if model.clips.contains_key(&requested) {
                    requested
                } else {
                    slot::IDLE
                };
                if model.clips.contains_key(&slot)
                    && actor.animation.as_ref().is_none_or(|a| a.slot != slot)
                {
                    actor.animation = Some(Animation {
                        blend_ticks: if matches!(slot, slot::TURN_RIGHT | slot::TURN_LEFT)
                            || player_locomotion && matches!(slot, slot::WALK | slot::RUN)
                        {
                            2
                        } else {
                            8
                        },
                        repeat: !matches!(slot, slot::TURN_RIGHT | slot::TURN_LEFT),
                        ..Animation::new(
                            actor.resource,
                            slot,
                            model.clips[&slot].duration_ticks,
                            self.world.tick,
                        )
                    });
                }
                if player_locomotion
                    && let Some(motion) = &actor.motion
                    && let Some(animation) = &mut actor.animation
                    && matches!(slot, slot::WALK | slot::RUN)
                {
                    // Scale player gait with movement speed. Script-directed movement
                    // keeps its independent authored rate and blend duration.
                    let rate = motion.speed / if slot == slot::WALK { 2. } else { 10. };
                    if animation.rate != rate {
                        animation.start_frame =
                            animation.sample(self.world.tick, 0, animation.duration_ticks as f32);
                        animation.phase_tick = self.world.tick;
                        animation.rate = rate;
                    }
                }
            }
        }
        self.world.particles.retain(|p| p.alive(self.world.tick));
        self.world
            .billboards
            .retain(|_, effect| effect.alive(self.world.tick));
        self.world
            .billboards
            .values_mut()
            .for_each(crate::effect::BillboardEffect::step);
        self.world.emotes.retain(|_, e| {
            self.world.actors.contains_key(&e.actor)
                && e.duration
                    .is_none_or(|d| self.world.tick - e.start_tick <= d)
        });
        if let Some(camera) = &mut self.world.field_camera {
            camera.step(&self.world.actors);
        }
        let result = self.execute();
        self.failed = result.is_err();
        result
    }
    fn execute(&mut self) -> Result<()> {
        let mut update_budget = 32768;
        for slot in 0..self.instances.len() {
            let Some(mut instance) = self.instances[slot].take() else {
                continue;
            };
            if instance
                .wait
                .as_mut()
                .is_some_and(|wait| matches!(wait.poll(&self.world), Ok(false)))
            {
                self.instances[slot] = Some(instance);
                continue;
            }
            if let Some(mut wait) = instance.wait.take() {
                wait.poll(&self.world).map_err(anyhow::Error::msg)?;
                if matches!(wait, Wait::ControlHandoff(_)) {
                    self.world.input_enabled = true;
                }
                let result = if let Wait::Choice(operation) = wait {
                    let progress = operation.progress();
                    let Some(crate::Outcome::Completed(Some(value))) = progress.outcome else {
                        anyhow::bail!("choice completed without a selection");
                    };
                    // Return both the selected line and the confirm/cancel/timeout reason.
                    self.memory.write(
                        0x24,
                        symphonia_script::Width::S32,
                        match progress.position {
                            0 => 0,
                            1 => 1,
                            2 => -1,
                            _ => anyhow::bail!("invalid choice completion reason"),
                        },
                    )?;
                    Some(value)
                } else {
                    None
                };
                instance.vm.complete(result, &mut self.memory)?;
            }
            let mut spawns = Vec::new();
            let mut wait = None;
            let mut host = NativeHost {
                world: &mut self.world,
                resources: &self.resources,
                program: &self.program,
                registers: &mut instance.registers,
                spawns: &mut spawns,
                next_handle: &mut self.next_handle,
                wait: &mut wait,
            };
            let result = instance
                .vm
                .run(&mut host, &mut self.memory, update_budget.min(8192))
                .with_context(|| {
                    format!(
                        "event {:?}, handle {}, update {}",
                        instance.key, instance.handle, self.world.tick
                    )
                })?;
            update_budget -= result.steps;
            if let RunEvent::Suspended { opcode } = result.event {
                instance.wait = Some(wait.with_context(|| {
                    format!("native {opcode:#04x} suspended without a completion condition")
                })?);
                self.instances[slot] = Some(instance);
            } else if self.interaction == Some(instance.handle) {
                self.interaction = None;
                self.world.input_enabled = true;
            }
            for (handle, key) in spawns {
                let pc = self
                    .program
                    .event(2, key)
                    .context("spawned event has no entry")?;
                let entry = self
                    .instances
                    .iter_mut()
                    .find(|i| i.is_none())
                    .context("event pool exhausted (32 instances)")?;
                *entry = Some(Instance {
                    handle,
                    key: Some(key),
                    vm: Vm::new(self.program.clone(), pc)?,
                    wait: None,
                    registers: [0; 6],
                });
            }
            if self.world.blocked_by_movie() {
                break;
            }
        }
        // Clip assignment evaluates once before the ordinary actor update. Apply
        // that initial sample after same-dispatch seek and rate changes.
        for id in std::mem::take(&mut self.world.pending_animation_bindings) {
            if let Some(actor) = self.world.actors.get_mut(&id)
                && actor.scripted_animation
                && let Some(animation) = actor.animation.as_mut()
                && animation.start_tick == self.world.tick
            {
                animation.binding_updates = 1;
            }
        }
        Ok(())
    }
}
