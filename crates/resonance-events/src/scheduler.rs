use crate::ResourceLibrary;
use crate::native::{EventAction, EventCommand, NativeHost};
use crate::operation::Wait;
use crate::{Animation, GameWorld, animation::slot};
use anyhow::{Context, Result, ensure};
use std::{collections::VecDeque, sync::Arc};
use symphonia_script::Program;
use symphonia_script_vm::{Memory, RunEvent, Vm};

struct Instance {
    handle: i32,
    key: Option<u32>,
    vm: Vm,
    wait: Option<Wait>,
    registers: [i32; 6],
    background: Option<Background>,
    resource_resume: Option<ResourceWaitObservation>,
}

impl Instance {
    fn new(program: &Arc<Program>, pc: u32, handle: i32, key: Option<u32>) -> Result<Self> {
        Ok(Self {
            handle,
            key,
            vm: Vm::new(program.clone(), pc)?,
            wait: None,
            registers: [0; 6],
            background: None,
            resource_resume: None,
        })
    }
}

#[derive(Default)]
struct Background {
    paused: bool,
    require_control: bool,
}

/// One timed ambient wait observed at the start of an oracle replay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackgroundWaitOrigin {
    pub key: u32,
    pub pc: u32,
    pub remaining: u32,
    pub require_control: bool,
}

/// Oracle-observed storage readiness. Only the waiting script is suspended;
/// ordinary execution uses preloaded resources without this schedule.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceWaitObservation {
    /// Word PC immediately after YieldCommand(1, resource_handle).
    pub pc: u32,
    pub resource: i32,
    pub request_tick: u32,
    pub resume_tick: u32,
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
    resource_waits: Option<VecDeque<ResourceWaitObservation>>,
}
impl EventRuntime {
    pub fn restore_field_leader(&mut self) -> Result<()> {
        let id = self
            .world
            .party
            .as_mut()
            .context("party is not initialized")?
            .restore_field_leader();
        self.world
            .select_party_member(&self.resources, i32::from(id))
    }
    pub fn new(program: Arc<Program>, resources: Arc<ResourceLibrary>) -> Result<Self> {
        Self::with_state(program, resources, GameWorld::default(), Memory::default())
    }
    pub fn with_state(
        program: Arc<Program>,
        resources: Arc<ResourceLibrary>,
        mut world: GameWorld,
        memory: Memory,
    ) -> Result<Self> {
        world.sync_actor_order();
        let main = Instance::new(&program, program.entry(), 1, None)?;
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
            resource_waits: None,
        };
        events.execute(true)?;
        Ok(events)
    }
    pub fn tick(&self) -> u32 {
        self.world.tick
    }
    pub fn register_resource_wait_observations(
        &mut self,
        observations: Vec<ResourceWaitObservation>,
    ) -> Result<()> {
        ensure!(
            self.resource_waits.is_none(),
            "resource waits already registered"
        );
        ensure!(
            observations.iter().all(|o| {
                o.request_tick > self.world.tick
                    && o.resume_tick > o.request_tick
                    && self.resources.bindings.contains_key(&o.resource)
                    && o.pc
                        .checked_sub(1)
                        .and_then(|pc| self.program.instruction(pc))
                        == Some((
                            symphonia_script::Op::Native(
                                symphonia_script::NativeCall::YieldCommand as u8,
                            ),
                            o.pc,
                        ))
            }) && observations
                .windows(2)
                .all(|w| w[0].request_tick <= w[1].request_tick),
            "invalid observed resource wait schedule"
        );
        self.resource_waits = Some(observations.into());
        Ok(())
    }
    pub fn finish_resource_wait_observations(&self) -> Result<()> {
        ensure!(
            self.resource_waits.as_ref().is_some_and(VecDeque::is_empty)
                && self
                    .instances
                    .iter()
                    .flatten()
                    .all(|i| i.resource_resume.is_none()),
            "recording ended with unused or unfinished resource waits"
        );
        Ok(())
    }
    pub fn background_waits(&self) -> Vec<BackgroundWaitOrigin> {
        self.instances
            .iter()
            .flatten()
            .filter_map(|instance| {
                let background = instance.background.as_ref()?;
                let Some(Wait::Tick(wake)) = instance.wait else {
                    return None;
                };
                (!background.paused).then_some(BackgroundWaitOrigin {
                    key: instance.key?,
                    pc: instance.vm.pc(),
                    remaining: wake.saturating_sub(self.world.tick),
                    require_control: background.require_control,
                })
            })
            .collect()
    }
    /// Adjust only an existing wait; never seek a VM or replace its stacks.
    pub fn apply_background_wait_origin(&mut self, origin: &BackgroundWaitOrigin) -> Result<()> {
        ensure!(
            (1..=36000).contains(&origin.remaining),
            "invalid ambient wait duration"
        );
        let instance = self
            .instances
            .iter_mut()
            .flatten()
            .find(|i| i.key == Some(origin.key) && i.background.is_some())
            .context("ambient wait event is missing")?;
        let background = instance.background.as_ref().unwrap();
        ensure!(
            instance.vm.pc() == origin.pc
                && !background.paused
                && background.require_control == origin.require_control,
            "ambient event {} is not at the observed wait (PC {:#x}, expected {:#x})",
            origin.key,
            instance.vm.pc(),
            origin.pc
        );
        let Some(Wait::Tick(wake)) = &mut instance.wait else {
            anyhow::bail!("ambient event is not waiting for time");
        };
        *wake = self
            .world
            .tick
            .checked_add(origin.remaining)
            .context("ambient wait clock overflow")?;
        Ok(())
    }
    pub fn apply_flutter_origin(&mut self, origins: &[crate::effect::FlutterOrigin]) -> Result<()> {
        ensure!(origins.len() <= 32, "too many initial leaves");
        let particles = origins
            .iter()
            .map(|origin| {
                let Some(crate::ParticleKind::Flutter(recipe)) =
                    self.resources.particles.get(&origin.kind)
                else {
                    anyhow::bail!("leaf origin requires a cooked flutter recipe");
                };
                origin.particle(self.world.tick, recipe)
            })
            .collect::<Result<Vec<_>>>()?;
        self.world.particles.retain(|p| p.flutter.is_none());
        for particle in particles {
            let born = particle.born;
            self.world
                .emit_particle(particle)
                .map_err(anyhow::Error::msg)?;
            self.world.particles.last_mut().unwrap().born = born;
        }
        Ok(())
    }
    /// Register an existing blink once when preparing an oracle replay.
    pub fn apply_eye_origin(&mut self, id: i32, eyes: crate::EyeBlink) -> Result<()> {
        let cycle = self
            .resources
            .blink
            .as_ref()
            .context("eye blink animation is not cooked")?;
        let tick = usize::from(eyes.tick);
        ensure!(
            tick < cycle.frames.len()
                && eyes.frame == cycle.frames[(tick + cycle.frames.len() - 1) % cycle.frames.len()],
            "eye origin does not follow the blink sequence"
        );
        let actor = self
            .world
            .actors
            .get_mut(&id)
            .context("eye origin actor is missing")?;
        ensure!(
            matches!(actor.appearance.face, crate::Face::Blink) && actor.appearance.eyes.is_some(),
            "eye origin requires an active blink"
        );
        actor.appearance.eyes = Some(eyes);
        Ok(())
    }
    pub fn active_instances(&self) -> usize {
        self.instances.iter().filter(|i| i.is_some()).count()
    }
    /// A caller can still start a foreground scene immediately after releasing input.
    pub fn control_handoff_pending(&self) -> bool {
        self.instances.iter().flatten().any(|i| {
            matches!(
                i.wait,
                Some(Wait::ControlHandoff(_) | Wait::ControlReleased)
            )
        })
    }
    pub fn main_finished(&self) -> bool {
        self.instances
            .iter()
            .flatten()
            .all(|instance| instance.handle != 1)
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
    /// Write a persistent script variable from a native service or developer tool.
    pub fn set_global(&mut self, index: u16, value: i32) -> Result<()> {
        ensure!(
            (crate::persistent::STORY_GLOBALS_START / 4..crate::persistent::GLOBAL_BYTES / 4)
                .contains(&index),
            "invalid persistent script variable"
        );
        self.memory
            .write(index * 4, symphonia_script::Width::S32, value)?;
        Ok(())
    }
    pub fn player_has_control(&self) -> bool {
        // The field supervisor keeps running during exploration. Free control
        // depends on event ownership, not whether the VM has any live stacks.
        !self.failed
            && self.world.input_enabled
            && self.interaction.is_none()
            && self.world.field_exit.is_none()
    }
    pub fn save_progress(&self) -> Result<crate::SavedProgress> {
        ensure!(!self.failed, "cannot save a failed event runtime");
        Ok(crate::SavedProgress {
            script_globals: (0..crate::persistent::GLOBAL_BYTES)
                .step_by(4)
                .map(|offset| {
                    if offset < crate::persistent::STORY_GLOBALS_START {
                        Ok(0)
                    } else {
                        self.memory.read(offset, symphonia_script::Width::S32)
                    }
                })
                .collect::<std::result::Result<_, _>>()?,
            party: self
                .world
                .party
                .clone()
                .context("party has not been initialized")?,
            event_flags: self.world.event_flags.clone(),
            event_records: self.world.event_records.clone(),
            random_state: self.world.random_state,
            gameplay_random: self.world.gameplay_random.clone(),
            tick: self.world.tick,
        })
    }
    /// Prepare a field entry without retiring the live scene. The caller cancels
    /// it only after the destination accepts this state; field-local bytes reset.
    pub fn persistent_state(&self) -> Result<crate::PersistentState> {
        ensure!(!self.failed, "cannot transfer a failed event runtime");
        let mut memory = Memory::default();
        for offset in (0..crate::persistent::GLOBAL_BYTES).step_by(4) {
            let width = symphonia_script::Width::S32;
            memory.write(offset, width, self.memory.read(offset, width)?)?;
        }
        Ok(crate::PersistentState {
            memory,
            gameplay_random: self.world.gameplay_random.clone(),
            party: self.world.party.clone(),
            event_flags: self.world.event_flags.clone(),
            event_records: self.world.event_records.clone(),
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
        *entry = Some(Instance::new(&self.program, pc, handle, Some(key))?);
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
        self.world.menu_request = None;
        self.world.movie = None;
        self.world.voice = None;
        self.world.field_transition = None;
        self.world.field_exit = None;
        self.world.preload_field = None;
        self.interaction = None;
        self.world.input_enabled = false;
    }
    pub fn step(&mut self) -> Result<()> {
        self.step_with_motion(
            self.world.tick.saturating_add(1),
            |_| Ok(()),
            |_, _, _, _| {},
            |_| Ok(()),
        )
    }
    /// Prepare control against the current view, then resolve actor movement
    /// against the scene before scripts observe positions. Preparation returns
    /// scene-owned data to the resolver without storing it in the event runtime.
    /// Wind samples the running effect clock, which advances through field menus.
    pub fn step_with_motion<T>(
        &mut self,
        effect_tick: u32,
        prepare: impl FnOnce(&mut Self) -> Result<T>,
        mut resolve: impl FnMut(&T, i32, &mut crate::Actor, [f32; 3]),
        services: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        ensure!(!self.failed, "event runtime stopped after a script failure");
        if self.world.blocked_by_movie() {
            return Ok(());
        }
        self.world.tick = self.world.tick.checked_add(1).context("clock overflow")?;
        if let Some(party) = &mut self.world.party
            && let Some(modifier) = &mut party.encounter_modifier
        {
            modifier.remaining -= 1;
            if modifier.remaining == 0 {
                party.encounter_modifier = None;
            }
        }
        if let Some(scene) = &mut self.world.skit {
            scene.step(
                self.resources
                    .skits
                    .as_ref()
                    .context("skit catalog missing")?,
            )?;
        }
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
        // The view follows the pose presented by the preceding actor update.
        if let Some(camera) = &mut self.world.field_camera {
            camera.step(&self.world.actors);
        }
        let prepared = prepare(self)?;
        self.world
            .billboards
            .retain(|_, effect| effect.alive(self.world.tick));
        self.world
            .billboards
            .values_mut()
            .for_each(crate::effect::BillboardEffect::step);
        let player_position = self
            .world
            .actors
            .get(&self.world.controlled_actor)
            .map(|a| a.position);
        // Native calls share randomness: update actors in creation order, not ID order.
        self.world.sync_actor_order();
        let actor_order = self.world.actor_order.clone();
        let conversation_active = self.interaction.is_some();
        for id in &actor_order {
            if self.world.overlays.contains_key(id) {
                continue;
            }
            let actor = self.world.actors.get_mut(id).unwrap();
            let previous = actor.position;
            let ambient = actor.step_autonomy(
                self.world.input_enabled,
                conversation_active,
                player_position,
                &mut || crate::world::random(&mut self.world.random_state),
            );
            let turn = actor.turn_direction();
            let movement_speed = actor
                .motion
                .as_ref()
                .map(|motion| motion.speed)
                .or_else(|| ambient.walking.then(|| actor.autonomy.unwrap().speed));
            actor.step_motion();
            resolve(&prepared, *id, actor, previous);
            actor.step_heading(
                self.world.input_enabled && *id == self.world.controlled_actor,
                actor.motion.is_some() || actor.position[..2] != previous[..2],
            );
            if !actor.scripted_animation
                && !ambient.selecting
                && let Some(model) = self.resources.model(actor.resource)
            {
                let dialogue = self.world.dialogue.values().any(|d| d.operation.is_pending()
                    && matches!(d.anchor, crate::dialogue::DialogueAnchor::Actor(speaker) if speaker == *id));
                // The player has a distinct idle pose while an event owns control.
                let event_controlled =
                    !self.world.input_enabled && *id == self.world.controlled_actor;
                let player_locomotion =
                    self.world.input_enabled && *id == self.world.controlled_actor;
                let requested = if let Some(speed) = movement_speed {
                    let running = !ambient.walking && speed > 7.;
                    let event_gait = if running {
                        slot::EVENT_RUN
                    } else {
                        slot::EVENT_WALK
                    };
                    if event_controlled && model.clips.contains_key(&event_gait) {
                        event_gait
                    } else if running && model.clips.contains_key(&slot::RUN) {
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
                        animation.seek(
                            animation.sample(self.world.tick, 0, animation.duration_ticks as f32),
                            self.world.tick,
                        );
                        animation.rate = rate;
                    }
                }
            }
            actor.animation_culled = actor.cull_outside_view
                && !actor.appearance.model_hidden
                && self
                    .world
                    .field_camera
                    .as_ref()
                    .is_some_and(|camera| !camera.animates(actor.position));
            if let Some(animation) = &mut actor.animation {
                animation.set_paused(ambient.paused || actor.animation_culled, self.world.tick);
            }
        }
        services(self)?;
        self.world.particles.retain(|p| p.alive(self.world.tick));
        self.world.overlays.retain(|id, overlay| {
            if let crate::world::OverlayKind::Sprite(sprite) = &mut overlay.kind {
                // Sprite drawing samples alpha before advancing its controller.
                sprite.step(overlay.rgba[3]);
            }
            let expired = matches!(
                overlay.kind,
                crate::world::OverlayKind::LocationCaption { .. }
            ) && overlay.alpha(self.world.tick) == 0;
            if expired {
                self.world.actors.remove(id);
            }
            !expired && self.world.actors.contains_key(id)
        });
        for particle in &mut self.world.particles {
            if let Some(flutter) = &mut particle.flutter {
                flutter.step(&mut particle.position, effect_tick, &mut || {
                    crate::world::random(&mut self.world.random_state)
                });
            }
        }
        self.world
            .refractions
            .retain(|_, effect| self.world.tick.saturating_sub(effect.born) <= effect.lifetime);
        self.world.emotes.retain(|_, e| {
            self.world.actors.contains_key(&e.actor)
                && e.duration
                    .is_none_or(|d| self.world.tick - e.start_tick <= d)
        });
        let result = self
            .world
            .step_field_exit(&self.resources)
            .map_err(anyhow::Error::msg)
            .and_then(|()| self.execute(false))
            .and_then(|()| self.world.step_eyes(&self.resources));
        self.failed = result.is_err();
        result
    }
    fn execute(&mut self, initial_dispatch: bool) -> Result<()> {
        let mut update_budget = 32768;
        for slot in 0..self.instances.len() {
            let Some(mut instance) = self.instances[slot].take() else {
                continue;
            };
            if instance.background.as_ref().is_some_and(|b| {
                b.paused
                    || b.require_control && !self.world.input_enabled
                    || self.world.field_transition.is_some()
                    || self.world.field_exit.is_some()
            }) {
                if let Some(Wait::Tick(wake)) = &mut instance.wait {
                    *wake = wake.checked_add(1).context("paused event clock overflow")?;
                }
                self.instances[slot] = Some(instance);
                continue;
            }
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
                if let Some(observation) = instance.resource_resume.take() {
                    ensure!(
                        self.world.tick == observation.resume_tick,
                        "observed resource resumed at the wrong tick"
                    );
                }
                if matches!(wait, Wait::ControlHandoff(_)) {
                    self.world.input_enabled = true;
                }
                let result = if let Wait::Menu(operation) = wait {
                    ensure!(
                        operation.progress().outcome == Some(crate::Outcome::Completed(Some(0))),
                        "menu completed without its zero result"
                    );
                    for address in [0x24, 0x28] {
                        self.memory
                            .write(address, symphonia_script::Width::S32, 0)?;
                    }
                    Some(0)
                } else if let Wait::Choice { result, .. } = wait {
                    let progress = result.progress();
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
            let mut commands = Vec::new();
            let mut wait = None;
            let mut resource_wait = None;
            let mut host = NativeHost {
                world: &mut self.world,
                resources: &self.resources,
                program: &self.program,
                registers: &mut instance.registers,
                events: &mut commands,
                next_handle: &mut self.next_handle,
                wait: &mut wait,
                resource_waits: self.resource_waits.as_ref(),
                resource_wait: &mut resource_wait,
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
            if let Some(observation) = resource_wait {
                ensure!(
                    instance.vm.pc() == observation.pc,
                    "observed resource wait reached the wrong script PC: {:#x}, expected {:#x}",
                    instance.vm.pc(),
                    observation.pc
                );
                self.resource_waits.as_mut().unwrap().pop_front();
                instance.resource_resume = Some(observation);
            }
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
            for EventCommand { handle, action } in commands {
                let EventAction::Spawn(key) = action else {
                    if let Some(background) = self
                        .instances
                        .iter_mut()
                        .flatten()
                        .find(|i| i.handle == handle)
                        .and_then(|i| i.background.as_mut())
                    {
                        match action {
                            EventAction::Pause(paused) => background.paused = paused,
                            EventAction::ControlGate(enabled) => {
                                background.require_control = enabled
                            }
                            EventAction::Spawn(_) => unreachable!(),
                        }
                    }
                    continue;
                };
                let pc = self
                    .program
                    .event(2, key)
                    .context("spawned event has no entry")?;
                let entry = self
                    .instances
                    .iter_mut()
                    .find(|i| i.is_none())
                    .context("event pool exhausted (32 instances)")?;
                let mut spawned = Instance::new(&self.program, pc, handle, Some(key))?;
                spawned.background = Some(Background::default());
                *entry = Some(spawned);
            }
            if self.world.blocked_by_movie() {
                break;
            }
        }
        // Initialization precedes the first ordinary actor update. Later scripts
        // run after actors, so their immediate binding is this tick's only new pose.
        for id in std::mem::take(&mut self.world.pending_animation_bindings) {
            if let Some(actor) = self.world.actors.get_mut(&id)
                && actor.scripted_animation
                && let Some(animation) = actor.animation.as_mut()
                && animation.start_tick == self.world.tick
            {
                animation.binding_updates = u32::from(initial_dispatch);
                animation.binding_timing = if initial_dispatch {
                    crate::animation::BindingTiming::BeforeDraw
                } else {
                    crate::animation::BindingTiming::AfterDraw
                };
            }
        }
        ensure!(
            self.resource_waits
                .as_ref()
                .and_then(|q| q.front())
                .is_none_or(|o| o.request_tick > self.world.tick)
                && self.instances.iter().flatten().all(|i| i
                    .resource_resume
                    .is_none_or(|o| o.resume_tick > self.world.tick)),
            "missed observed resource request or resume"
        );
        Ok(())
    }
}

impl EventRuntime {
    /// Register an observed ambient phase once; ordinary saves recreate actors.
    pub fn apply_actor_origin(&mut self, id: i32, origin: &crate::ActorOrigin) -> Result<()> {
        let waiting = |instance: &Instance| {
            matches!(&instance.wait, Some(Wait::Service { condition, ready_at: None })
                if matches!(**condition, Wait::ActorAnimation(actor) if actor == id))
        };
        // Background scripts may randomly choose between clips in an actor's
        // own bank. Register their visible phase without seeking the script.
        let ambient_binding = self.world.input_enabled
            && self.interaction.is_none()
            && self.instances.iter().flatten().any(|instance| {
                instance
                    .background
                    .as_ref()
                    .is_some_and(|b| b.require_control && !b.paused)
                    && waiting(instance)
            })
            && !self
                .instances
                .iter()
                .flatten()
                .any(|instance| instance.background.is_none() && waiting(instance));
        let actor = self
            .world
            .actors
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("ambient actor {id} is missing"))?;
        let current = actor
            .autonomy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("actor {id} has no ambient behavior"))?;
        let state = &origin.autonomy;
        // The player's home records its spawn position, not a patrol boundary.
        ensure!(
            current.behavior == state.behavior
                && current.speed == state.speed
                && (current.home == state.home
                    || id == self.world.controlled_actor
                        && current.behavior == crate::Behavior::Player
                        && state
                            .home
                            .iter()
                            .all(|v| v.is_finite() && v.abs() <= 100_000.))
                && current.radius == state.radius
                && (-2..=183).contains(&state.remaining)
                && !state.conversing,
            "ambient origin changes actor {id}'s authored movement settings"
        );
        use crate::animation::slot;
        ensure!(
            origin
                .position
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100_000.)
                && origin.heading.is_finite()
                && origin.target_heading.is_finite()
                && (actor.interaction_anchor && origin.animation_slot.is_none()
                    || actor.scripted_animation
                    || state.activity == crate::Activity::Select
                        && !state.initialized
                        && origin.animation_slot == Some(slot::IDLE)
                    || matches!(
                        (state.activity, origin.animation_slot),
                        (crate::Activity::Idle, Some(slot::IDLE))
                            | (crate::Activity::Walk, Some(slot::WALK))
                    )),
            "invalid ambient pose for actor {id}"
        );
        if id == self.world.controlled_actor {
            ensure!(
                origin.position == actor.position && origin.heading == actor.heading,
                "ambient origin cannot reposition the player"
            );
        }
        let animation = if let Some(slot) = origin.animation_slot {
            let binding = if actor.scripted_animation {
                actor.animation.as_ref()
            } else {
                None
            };
            ensure!(
                !actor.scripted_animation
                    || binding.is_some_and(|animation| {
                        (animation.slot == slot
                            || ambient_binding && animation.resource == actor.resource)
                            && animation.repeat == origin.animation_repeat
                    }),
                "ambient origin changes actor {id}'s scripted binding"
            );
            let resource = binding.map_or(actor.resource, |animation| animation.resource);
            let clip = self
                .resources
                .model(resource)
                .and_then(|m| m.clips.get(&slot))
                .context("ambient animation is not cooked")?;
            ensure!(
                (0. ..=clip.duration_ticks as f32).contains(&origin.animation_sample)
                    && binding.is_none_or(|animation| {
                        animation.slot != slot || animation.duration_ticks == clip.duration_ticks
                    }),
                "ambient animation sample is outside its clip"
            );
            Some(Animation {
                start_frame: origin.animation_sample,
                rate: binding.map_or(1., |animation| animation.rate),
                loop_start: binding.map_or(0., |animation| animation.loop_start),
                repeat: origin.animation_repeat,
                ..Animation::new(resource, slot, clip.duration_ticks, self.world.tick)
            })
        } else {
            ensure!(
                actor.interaction_anchor
                    && actor.animation.is_none()
                    && origin.animation_sample == 0.
                    && !origin.animation_repeat,
                "only a scene locator can omit its animation"
            );
            None
        };
        actor.autonomy = Some(*state);
        actor.position = origin.position;
        actor.heading = origin.heading;
        actor.target_heading = origin.target_heading;
        actor.animation = animation;
        Ok(())
    }
}
