use crate::GameWorld;
use crate::ResourceLibrary;
use crate::native::{EventAction, EventCommand, NativeHost};
use crate::operation::Wait;
use anyhow::{Context, Result, ensure};
use std::sync::Arc;
use symphonia_script::Program;
use symphonia_script_vm::{Memory, RunEvent, Vm};

struct Instance {
    handle: i32,
    key: Option<u32>,
    event_actor: i16,
    vm: Vm,
    program: Arc<Program>,
    operations: crate::operation::OperationScope,
    wait: Option<Wait>,
    join: Option<i32>,
    registers: [i32; 6],
    background: Option<Background>,
    callback: Option<Callback>,
}

enum Callback {
    Owned(crate::Operation),
    Trigger,
}

impl Instance {
    fn owned_callback(&self) -> bool {
        matches!(self.callback, Some(Callback::Owned(_)))
    }

    fn trigger_callback(&self) -> bool {
        matches!(self.callback, Some(Callback::Trigger))
    }

    fn new(program: &Arc<Program>, pc: u32, handle: i32, key: Option<u32>) -> Result<Self> {
        Self::with_arguments(program, pc, handle, key, &[])
    }
    fn with_arguments(
        program: &Arc<Program>,
        pc: u32,
        handle: i32,
        key: Option<u32>,
        arguments: &[i32],
    ) -> Result<Self> {
        Ok(Self {
            handle,
            key,
            event_actor: 0,
            vm: Vm::with_arguments(program.clone(), pc, arguments)?,
            program: program.clone(),
            operations: Default::default(),
            wait: None,
            join: None,
            registers: [0; 6],
            background: None,
            callback: None,
        })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(Callback::Owned(completion)) = &self.callback {
            completion.cancel();
        }
        self.operations.cancel();
        self.vm.cancel();
    }
}

#[derive(Default)]
struct Background {
    paused: bool,
    require_control: bool,
}

/// Stable slot order and one shared script data region for up to 32 instances.
/// Scheduling is independent of render frame rate.
pub struct EventRuntime {
    pub world: GameWorld,
    /// Diagnostic retained when the temporary playground abandons field scripts.
    pub exploration_error: Option<String>,
    program: Arc<Program>,
    pub(crate) resources: Arc<ResourceLibrary>,
    memory: Memory,
    instances: Vec<Option<Instance>>,
    next_handle: i32,
    failed: bool,
    interaction: Option<i32>,
    tasks: symphonia_script_vm::Tasks,
}
impl EventRuntime {
    pub fn resources(&self) -> &ResourceLibrary {
        &self.resources
    }
    /// Restart a reusable scene without reloading its program or immutable assets.
    pub fn fresh(&self) -> Result<Self> {
        Self::new(self.program.clone(), self.resources.clone())
    }
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
    pub fn replace_incapacitated_field_leader(&mut self) -> Result<()> {
        let Some(party) = self.world.party.as_mut() else {
            return Ok(());
        };
        let current = usize::try_from(self.world.controlled_actor - 1)?;
        if party.members[current].can_lead_field() {
            return Ok(());
        }
        if let Some(id) = party
            .formation
            .iter()
            .copied()
            .find(|&id| party.members[usize::from(id - 1)].can_lead_field())
        {
            party.field_leader = id;
            self.world
                .select_party_member(&self.resources, i32::from(id))?;
        }
        Ok(())
    }
    pub fn new(program: Arc<Program>, resources: Arc<ResourceLibrary>) -> Result<Self> {
        Self::with_state(program, resources, GameWorld::default(), Memory::default())
    }
    pub fn with_state(
        program: Arc<Program>,
        resources: Arc<ResourceLibrary>,
        world: GameWorld,
        memory: Memory,
    ) -> Result<Self> {
        Self::with_state_policy(program, resources, world, memory, false)
    }
    pub fn with_state_policy(
        program: Arc<Program>,
        resources: Arc<ResourceLibrary>,
        mut world: GameWorld,
        memory: Memory,
        allow_incomplete_scripts: bool,
    ) -> Result<Self> {
        world.sync_actor_order();
        let main = Instance::new(&program, program.entry(), 1, None)?;
        let mut instances: Vec<Option<Instance>> = (0..32).map(|_| None).collect();
        instances[0] = Some(main);
        let mut events = Self {
            world,
            exploration_error: None,
            program,
            resources,
            memory,
            instances,
            next_handle: 2,
            failed: false,
            interaction: None,
            tasks: Default::default(),
        };
        if let Err(error) = events
            .execute()
            .and_then(|()| events.world.update_collision_attachments(&events.resources))
        {
            if !allow_incomplete_scripts {
                return Err(error);
            }
            events.enter_exploration(format!("{error:#}"));
        }
        Ok(events)
    }
    pub fn tick(&self) -> u32 {
        self.world.tick
    }
    /// Read active VM owners without running or polling any event or service.
    pub fn observed_instances(&self) -> Vec<crate::EventInstanceObservation> {
        self.instances
            .iter()
            .enumerate()
            .filter_map(|(slot, instance)| {
                let instance = instance.as_ref()?;
                Some(crate::EventInstanceObservation {
                    slot,
                    handle: instance.handle,
                    key: instance.key,
                    program_entry: instance.program.entry(),
                    legacy_program: instance.program.authored().is_none(),
                    pc: instance.vm.pc(),
                    legacy_return_stack: instance.vm.legacy_return_stack().to_vec(),
                    value_depth: instance.vm.value_depth(),
                    argument_depth: instance.vm.argument_depth(),
                    expression: instance.vm.expression(),
                    join: instance.join,
                    registers: instance.registers,
                    background: instance.background.as_ref().map(|b| {
                        crate::diagnostic::BackgroundObservation {
                            paused: b.paused,
                            require_control: b.require_control,
                        }
                    }),
                    wait: instance
                        .wait
                        .as_ref()
                        .map(|wait| crate::diagnostic::wait(wait, &self.world)),
                })
            })
            .collect()
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
                if let Some(handle) = instance.join {
                    return Some(format!(
                        "event handle {}: joining task {handle}",
                        instance.handle
                    ));
                }
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
    /// Commit shared story variables while retaining this VM's dispatcher registers and locals.
    pub fn copy_script_globals(&mut self, source: &Self) -> Result<()> {
        self.memory.copy_from(
            &source.memory,
            crate::persistent::STORY_GLOBALS_START..crate::persistent::GLOBAL_BYTES,
        )?;
        Ok(())
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
            && !self.world.ring.blocks_control()
            && self.world.input_enabled
            && !self.world.mapped_input_disabled
            && self.world.battle_request.is_none()
            && self.world.screen_request.is_none()
            && !self
                .instances
                .iter()
                .flatten()
                .any(|instance| matches!(instance.wait, Some(Wait::Battle(_))))
            && self.interaction.is_none()
            && self.world.field_exit.is_none()
    }
    /// Taking a request does not release the suspended field. Only completing
    /// its operation allows the next field update to resume the caller.
    pub fn battle_pending(&self) -> bool {
        self.world.battle_request.as_ref().is_some_and(|r| r.is_pending())
            || self.instances.iter().flatten().any(|instance| {
                matches!(&instance.wait, Some(Wait::Battle(operation)) if operation.is_pending())
            })
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
            script_state: self.world.script_state.clone(),
            event_records: self.world.event_records.clone(),
            random_state: self.world.random_state,
            gameplay_random: self.world.gameplay_random,
            tick: self.world.tick,
        })
    }
    /// Prepare a field entry without retiring the live scene. The caller cancels
    /// it only after the destination accepts this state; field-local bytes reset.
    pub fn persistent_state(&self) -> Result<crate::PersistentState> {
        ensure!(!self.failed, "cannot transfer a failed event runtime");
        let mut memory = Memory::default();
        memory.copy_from(&self.memory, 0..crate::persistent::GLOBAL_BYTES)?;
        Ok(crate::PersistentState {
            memory,
            gameplay_random: self.world.gameplay_random,
            party: self.world.party.clone(),
            event_flags: self.world.event_flags.clone(),
            script_state: self.world.script_state.clone(),
            event_records: self.world.event_records.clone(),
            random_state: self.world.random_state,
            tick: self.world.tick,
        })
    }
    /// Actor interaction entries use registry kind zero. Input selection and
    /// reachability belong to the game layer; the scheduler owns exclusivity
    /// and returns control when the foreground event finishes.
    pub fn has_interaction(&self, actor: i32) -> bool {
        !self
            .world
            .actors
            .get(&actor)
            .is_some_and(|a| a.enemy.is_some())
            && u32::try_from(actor)
                .ok()
                .is_some_and(|key| self.program.event(0, key).is_some())
    }
    pub fn interact(&mut self, actor: i32) -> Result<bool> {
        if !self.has_interaction(actor) {
            return Ok(false);
        }
        let Ok(key) = u32::try_from(actor) else {
            return Ok(false);
        };
        if self
            .world
            .actors
            .get(&actor)
            .is_some_and(|a| a.ring_station)
        {
            if !self.player_has_control() || self.interaction.is_some() {
                return Ok(false);
            }
            let program = self
                .resources
                .station_script
                .clone()
                .context("ring-station script was not prepared")?;
            let handle = self
                .world
                .authored_actor(actor)
                .map_err(anyhow::Error::msg)?;
            self.start_authored(program, "field::station::interact", &[handle])?;
            return Ok(true);
        }
        self.start_foreground(0, key, actor as i16)
    }

    /// Enemy contact invokes its configured interaction and supplies the symbol ID.
    pub fn contact_enemy(&mut self, actor: i32) -> Result<bool> {
        let Some(enemy) = self.world.actors.get(&actor).and_then(|a| a.enemy.as_ref()) else {
            return Ok(false);
        };
        if enemy.pause_ticks != 0 {
            return Ok(false);
        }
        let key = u32::from(enemy.event);
        if !self.start_foreground(0, key, actor as i16)? {
            return Ok(false);
        }
        self.memory
            .write(0x24, symphonia_script::Width::S32, actor)?;
        self.world
            .actors
            .get_mut(&actor)
            .unwrap()
            .enemy
            .as_mut()
            .unwrap()
            .pause_ticks = 60;
        Ok(true)
    }
    pub(crate) fn queue_ring_callback(
        &mut self,
        hit: crate::ring::Hit,
        secondary: bool,
    ) -> Result<Option<crate::Operation>> {
        let key = if secondary {
            crate::ring::SECONDARY_CALLBACK
        } else {
            crate::ring::CALLBACK
        };
        let Some(pc) = self.program.event(0, key) else {
            return Ok(None);
        };
        let entry = self
            .instances
            .iter_mut()
            .find(|i| i.is_none())
            .context("event pool exhausted")?;
        let handle = self.next_handle;
        self.next_handle = handle.checked_add(1).context("event handle overflow")?;
        let completion = self.world.operations.begin().map_err(anyhow::Error::msg)?;
        let mut instance = Instance::new(&self.program, pc, handle, Some(key))?;
        instance.event_actor = hit.event_actor();
        instance.callback = Some(Callback::Owned(completion.clone()));
        *entry = Some(instance);
        self.reconcile_control();
        Ok(Some(completion))
    }

    /// Ordinary actor contact carries its own actor context, like ring hits.
    pub fn contact_actor(&mut self, actor: i32) -> Result<bool> {
        const CONTACT_CALLBACK: u32 = (-2_i32) as u32;
        self.start_foreground(0, CONTACT_CALLBACK, actor as i16)
    }

    /// Confirmed triggers use registry kind 2; automatic crossings use kind 1.
    pub fn trigger(&mut self, key: u32, confirmed: bool) -> Result<bool> {
        self.start_foreground(if confirmed { 2 } else { 1 }, key, 0)
    }

    /// Dispatch a sampled contact using its native counter as actor context.
    pub fn contact_trigger(&mut self, index: usize) -> Result<bool> {
        let trigger = self
            .world
            .triggers
            .get(index)
            .context("trigger is missing")?;
        let Some(context) = trigger.activation_context() else {
            return Ok(false);
        };
        let started = self.start_foreground(trigger.registry_kind(), trigger.key, context)?;
        self.world.triggers[index].record_activation(started);
        Ok(started)
    }
    /// World landmarks share registry kind 1 with automatic field crossings.
    /// The native world dispatcher supplies the octant in result word 0x24.
    pub fn enter_landmark(&mut self, key: u16, direction: u8) -> Result<bool> {
        ensure!(direction < 8, "invalid landmark entry direction");
        if !self.start_foreground(1, u32::from(key), 0)? {
            return Ok(false);
        }
        self.memory
            .write(0x24, symphonia_script::Width::S32, i32::from(direction))?;
        Ok(true)
    }
    fn start_foreground(&mut self, kind: u32, key: u32, event_actor: i16) -> Result<bool> {
        ensure!(!self.failed, "event runtime stopped after a script failure");
        if self.exploration_error.is_some() {
            return Ok(false);
        }
        if !self.world.input_enabled || self.interaction.is_some() || self.battle_pending() {
            return Ok(false);
        }
        if kind != 0
            && self
                .instances
                .iter()
                .flatten()
                .any(Instance::trigger_callback)
        {
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
        let mut instance = Instance::new(&self.program, pc, handle, Some(key))?;
        instance.event_actor = event_actor;
        if kind != 0 {
            instance.callback = Some(Callback::Trigger);
        }
        *entry = Some(instance);
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
    /// Queue a prepared source program in the same foreground pool as legacy events.
    /// Compilation and asset preparation belong to the caller before this boundary.
    pub fn start_authored(
        &mut self,
        program: Arc<Program>,
        entry: &str,
        arguments: &[i32],
    ) -> Result<i32> {
        ensure!(!self.failed, "event runtime stopped after a script failure");
        ensure!(
            self.world.input_enabled && self.interaction.is_none(),
            "another event owns field control"
        );
        Vm::validate_bindings::<crate::authored::FieldHost>(&program)?;
        let pc = program
            .authored()
            .context("expected a compiled source program")?
            .functions
            .iter()
            .find(|function| function.name == entry)
            .context("authored event entry is missing")?
            .entry;
        let slot = self
            .instances
            .iter()
            .position(Option::is_none)
            .context("event pool exhausted (32 instances)")?;
        let handle = self.next_handle;
        let instance = Instance::with_arguments(&program, pc, handle, None, arguments)?;
        self.next_handle = handle.checked_add(1).context("event handle overflow")?;
        self.instances[slot] = Some(instance);
        self.interaction = Some(handle);
        self.world.input_enabled = false;
        if let Some(actor) = self.world.actors.get_mut(&self.world.controlled_actor) {
            actor.motion = None;
        }
        Ok(handle)
    }
    /// Whether a queued or suspended event is still executing.
    pub fn is_active(&self, handle: i32) -> bool {
        self.instances.iter().flatten().any(|i| i.handle == handle)
    }
    /// Cancel an authored task and its descendants without retiring the field.
    pub fn cancel_authored(&mut self, handle: i32) -> Result<()> {
        let active = self.instances.iter().any(|instance| {
            instance.as_ref().is_some_and(|instance| {
                instance.handle == handle && instance.program.authored().is_some()
            })
        });
        ensure!(
            active || self.tasks.contains(handle),
            "authored event handle is not active"
        );
        self.cancel_task_tree(handle);
        self.remove_cancelled_dialogue();
        if self.interaction == Some(handle) {
            self.interaction = None;
            self.world.input_enabled = true;
            self.world.mapped_input_disabled = false;
        }
        Ok(())
    }
    fn cancel_task_tree(&mut self, handle: i32) {
        for child in self.tasks.children(handle) {
            self.cancel_task_tree(child);
        }
        for instance in &mut self.instances {
            if instance
                .as_ref()
                .is_some_and(|instance| instance.handle == handle)
            {
                *instance = None;
            }
        }
        self.tasks.remove(handle);
    }
    fn finish_task(&mut self, handle: i32, result: Vec<i32>) {
        for child in self.tasks.children(handle) {
            self.cancel_task_tree(child);
        }
        self.tasks.finish(handle, result);
        self.remove_cancelled_dialogue();
    }
    fn remove_cancelled_dialogue(&mut self) {
        for entry in &mut self.instances {
            if let Some(instance) = entry
                && matches!(&instance.callback, Some(Callback::Owned(op)) if op.progress().outcome == Some(crate::Outcome::Cancelled))
            {
                if self.interaction == Some(instance.handle) {
                    self.interaction = None;
                    self.world.input_enabled = true;
                    self.world.mapped_input_disabled = false;
                }
                *entry = None;
            }
        }
        self.world.reap_authored_resources();
        self.world.dialogue.retain(|_, dialogue| {
            dialogue.operation.progress().outcome != Some(crate::Outcome::Cancelled)
        });
        self.world.choices.retain(|_, choice| {
            choice.operation.progress().outcome != Some(crate::Outcome::Cancelled)
        });
    }
    fn task_error(&mut self, instance: &mut Instance, error: anyhow::Error) -> anyhow::Error {
        if instance.program.authored().is_some()
            || instance.owned_callback()
            || self.tasks.contains(instance.handle)
        {
            let root = self.tasks.root(instance.handle);
            self.cancel_task_tree(root);
            instance.operations.cancel();
            self.remove_cancelled_dialogue();
        }
        error
    }
    /// Scene exit stops the callers and invalidates outstanding callbacks.
    pub fn enter_exploration(&mut self, reason: String) {
        self.cancel();
        self.failed = false;
        self.exploration_error = Some(reason);
        self.world.input_enabled = true;
        self.world.triggers.clear();
        // A missing fade is the native startup blackout, not full visibility.
        self.world.fade = Some(crate::Fade::new(self.world.tick, 0, 0., 0., false));
        self.world.camera = None;
        self.world.skit_request = None;
        self.world.screen_copy_depth = [0.; 2];
        self.world.scene_dissolve = None;
        self.world.next_transition_white = None;
        // Overlay actors have no 3D model. Retire them with their controllers,
        // otherwise exploration leaves an impossible visible actor request.
        for id in std::mem::take(&mut self.world.overlays).keys() {
            self.world.actors.remove(id);
        }
        self.world.billboards.clear();
        self.world.station_transfers.clear();
        self.world.effect_changes.clear();
        self.world.model_particles.clear();
        self.world.refractions.clear();
        for (&id, actor) in &mut self.world.actors {
            actor.motion = None;
            actor.attachment = None;
            actor.enemy = None;
            // Incomplete setup may leave NPCs or puzzle props overlapping the
            // entrance. Preview walking uses the field's static ground mesh.
            if id != self.world.controlled_actor {
                actor.collidable = false;
            }
            if let Some(autonomy) = &mut actor.autonomy {
                autonomy.conversing = false;
                autonomy.activity = crate::Activity::Idle;
                if autonomy.behavior != crate::Behavior::Player {
                    autonomy.behavior = crate::Behavior::Stationary;
                }
            }
        }
    }
    pub fn cancel(&mut self) {
        let mut ring = std::mem::take(&mut self.world.ring);
        ring.cancel(&mut self.world);
        self.instances.iter_mut().for_each(|i| *i = None);
        self.tasks.clear();
        self.world.operations.cancel();
        self.world.reap_authored_resources();
        self.world.dialogue.clear();
        self.world.choices.clear();
        self.world.menu_request = None;
        self.world.screen_request = None;
        self.world.battle_request = None;
        self.world.movie = None;
        self.world.voice = None;
        self.world.rumble = None;
        if let Some(camera) = &mut self.world.field_camera {
            camera.shake = Default::default();
        }
        self.world.field_transition = None;
        self.world.world_transition = None;
        self.world.field_exit = None;
        self.world.preload_field = None;
        self.interaction = None;
        self.world.input_enabled = false;
        self.world.mapped_input_disabled = false;
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
        self.world.reap_authored_resources();
        if self.world.blocked_by_movie()
            || self.battle_pending()
            || self.world.screen_request.is_some()
        {
            return Ok(());
        }
        self.world.actors.retain(|_, actor| !actor.retiring);
        for actor in self.world.actors.values_mut() {
            actor.rendered_scale = None;
        }
        self.world.tick = self.world.tick.checked_add(1).context("clock overflow")?;
        self.world.particles_before_update = self.world.next_particle;
        self.world.effect_tick = effect_tick;
        if self.world.texture_animation_enabled {
            self.world.texture_animation_tick += 1;
            self.world.texture_animation_effect_tick = effect_tick;
        }
        if self
            .world
            .party
            .as_ref()
            .is_some_and(|p| !p.settings.preferences.rumble)
        {
            self.world.rumble = None;
        }
        if !self.world.external_encounter_clock
            && let Some(party) = &mut self.world.party
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
            if dialogue.opening_ready(&self.world.actors) {
                dialogue.opening_actor = None;
            }
        }
        // The view follows the pose presented by the preceding actor update.
        let tracked_actor = self.world.field_camera.as_ref().map(|rig| {
            rig.motion
                .as_ref()
                .map_or(rig.cameras[rig.selected].actor, |motion| motion.actor)
        });
        let attached_position = tracked_actor
            .filter(|id| {
                self.world
                    .actors
                    .get(id)
                    .is_some_and(|actor| actor.attachment.is_some())
            })
            .map(|id| self.world.attached_position(&self.resources, id))
            .transpose()?;
        if let Some(camera) = &mut self.world.field_camera {
            camera.step_positions(|id| {
                attached_position
                    .filter(|_| Some(id) == tracked_actor)
                    .or_else(|| self.world.actors.get(&id).map(|actor| actor.position))
            });
            camera.shake.step(&mut self.world.random_state);
            if let Some(playback) = &self.world.camera
                && let Some(track) = self.resources.camera_tracks.get(&playback.resource)
                && let Some((position, target)) = playback.sample(self.world.tick, track)
            {
                camera.position = position;
                camera.target = self
                    .world
                    .actors
                    .get(&playback.target_actor)
                    .map_or(target, |actor| {
                        std::array::from_fn(|i| actor.position[i] + playback.target_offset[i])
                    });
            }
        }
        self.world.update_collision_attachments(&self.resources)?;
        let prepared = prepare(self)?;
        self.world.step_ambient_sound();
        self.world.step_billboards(effect_tick);
        let player_position = self
            .world
            .actors
            .get(&self.world.controlled_actor)
            .map(|a| a.position);
        // Shared randomness makes the stable actor order observable.
        self.world.sync_actor_order();
        let actor_order = self.world.actor_order.clone();
        let conversation_active = self.interaction.is_some();
        for id in &actor_order {
            if self.world.overlays.contains_key(id) {
                continue;
            }
            self.world
                .emit_stun_effect(*id, &self.resources)
                .map_err(anyhow::Error::msg)?;
            let attached_position = self.world.actors[id]
                .attachment
                .as_ref()
                .filter(|_| {
                    let actor = &self.world.actors[id];
                    actor.cull_outside_view
                        && !actor.appearance.model_hidden
                        && self.world.field_camera.is_some()
                })
                .map(|_| self.world.attached_position(&self.resources, *id))
                .transpose()?;
            let actor = self.world.actors.get_mut(id).unwrap();
            let previous = actor.position;
            let actor_conversation = actor.autonomy.and_then(|ai| ai.dialogue_slot).map_or(
                conversation_active,
                |slot| {
                    self.world
                        .dialogue
                        .get(&slot)
                        .is_some_and(crate::dialogue::Dialogue::holds_actor_activity)
                },
            );
            let ambient = if actor.motion.is_none()
                && actor
                    .enemy
                    .as_ref()
                    .is_some_and(|enemy| enemy.pause_outside_view)
                && self
                    .world
                    .field_camera
                    .as_ref()
                    .is_some_and(|camera| !camera.enemy_active(actor.position))
            {
                crate::autonomy::AmbientMotion {
                    paused: true,
                    ..Default::default()
                }
            } else {
                actor.step_autonomy(
                    self.world.input_enabled,
                    actor_conversation,
                    player_position,
                    &mut || crate::world::random(&mut self.world.random_state),
                )
            };
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
                let dialogue = actor.autonomy.is_some_and(|ai| {
                    ai.conversing && ai.dialogue_slot.is_some()
                }) || self.world.dialogue.values().any(|d| d.operation.is_pending()
                    && matches!(d.anchor, crate::dialogue::DialogueAnchor::Actor(speaker) if speaker == *id));
                actor.select_ordinary_animation(
                    model,
                    self.world.tick,
                    crate::animation::OrdinaryAnimation {
                        movement_speed,
                        turn,
                        walking: ambient.walking,
                        event_controlled: !self.world.input_enabled
                            && *id == self.world.controlled_actor,
                        player_locomotion: self.world.input_enabled
                            && *id == self.world.controlled_actor,
                        dialogue,
                    },
                    None,
                );
            }
            actor.animation_culled = actor.cull_outside_view
                && !actor.appearance.model_hidden
                && self.world.field_camera.as_ref().is_some_and(|camera| {
                    !camera.animates(attached_position.unwrap_or(actor.position))
                });
            if let Some(animation) = &mut actor.animation {
                // Offscreen scripted clips must still advance so their events can finish.
                animation.set_paused(
                    ambient.paused || actor.animation_culled && !actor.scripted_animation,
                    self.world.tick,
                );
            }
            // Capture draw placement before the later field VM update.
            actor.draw_position = Some(actor.position);
        }
        self.world.update_collision_attachments(&self.resources)?;
        self.world.step_enemy_sources();
        self.world.step_model_particles();
        self.step_effects()?;
        self.world.emit_orbit_trails().map_err(anyhow::Error::msg)?;
        let wings_enabled = self.memory.read(0x44, symphonia_script::Width::S32)? != 0;
        self.world.step_wings(&self.resources, wings_enabled)?;
        services(self)?;
        self.world
            .step_ring_stations()
            .map_err(anyhow::Error::msg)?;
        self.world.step_wandering_billboards();
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
        self.world
            .refractions
            .retain(|_, effect| effect.step(self.world.tick));
        self.world.apply_effect_changes();
        self.world.emotes.retain(|_, e| {
            self.world.actors.contains_key(&e.actor)
                && e.duration
                    .is_none_or(|d| self.world.tick - e.start_tick <= d)
        });
        let result = self
            .world
            .step_field_exit(&self.resources)
            .map_err(anyhow::Error::msg)
            .and_then(|()| self.execute())
            .and_then(|()| {
                let colette_progress = self.memory.read(
                    resonance_content::appearance::ANGEL_PROGRESS,
                    symphonia_script::Width::S32,
                )?;
                self.world.step_eyes(&self.resources, colette_progress)
            })
            .and_then(|()| {
                self.world.update_collision_attachments(&self.resources)?;
                // Birth poses are ready for presentation after this update's
                // scripts and emitters have consumed their random inputs.
                self.world.initialize_billboards();
                Ok(())
            });
        self.failed = result.is_err();
        if self.failed {
            let mut ring = std::mem::take(&mut self.world.ring);
            ring.cancel(&mut self.world);
        }
        if let Some(party) = &mut self.world.party {
            const MAX_FIELD_TICKS: u32 = u32::MAX - 15;
            party.travel.scenario_ticks = party
                .travel
                .scenario_ticks
                .saturating_add(1)
                .min(MAX_FIELD_TICKS);
            party.travel.field_countdown = party.travel.field_countdown.saturating_sub(1);
            if !self.world.mapped_input_disabled {
                party.travel.field_ticks = party
                    .travel
                    .field_ticks
                    .saturating_add(1)
                    .min(MAX_FIELD_TICKS);
                party.travel.ring_timer = party.travel.ring_timer.saturating_sub(1);
            }
        }
        self.remove_cancelled_dialogue();
        if result.is_ok() && std::mem::take(&mut self.world.restore_battle_music) {
            self.world
                .audio_commands
                .push(crate::AudioCommand::MusicVolume {
                    volume: 127,
                    duration_ticks: 0,
                });
        }
        result
    }
    /// Authored roots request control until released; their callbacks keep the
    /// lease while running. Foreground scenario interactions retain priority.
    fn reconcile_control(&mut self) {
        let needs_control = |instance: &Instance| {
            instance.owned_callback()
                || (instance.program.authored().is_some()
                    && self.tasks.root(instance.handle) == instance.handle
                    && !self.tasks.control_released(instance.handle))
        };
        if let Some(owner) = self.interaction {
            let authored = self.instances.iter().flatten().any(|i| {
                i.handle == owner && (i.program.authored().is_some() || i.owned_callback())
            });
            let requested = self
                .instances
                .iter()
                .flatten()
                .any(|i| self.tasks.root(i.handle) == owner && needs_control(i));
            if authored && !requested {
                self.interaction = None;
                self.world.input_enabled = true;
            } else if authored {
                self.world.input_enabled = false;
            }
        }
        if self.interaction.is_none() && self.world.input_enabled {
            self.interaction = self
                .instances
                .iter()
                .flatten()
                .find(|i| needs_control(i))
                .map(|i| self.tasks.root(i.handle));
            if self.interaction.is_some() {
                self.world.input_enabled = false;
            }
        }
    }

    fn execute(&mut self) -> Result<()> {
        const AUTHORED_BUDGET: u32 = 32_768;
        // Imported scenes may issue a large finite batch of native particle calls.
        const LEGACY_BUDGET: u32 = 131_072;
        const UPDATE_BUDGET: u32 = LEGACY_BUDGET * 4;
        self.reconcile_control();
        let mut update_budget = UPDATE_BUDGET;
        for slot in 0..self.instances.len() {
            let Some(mut instance) = self.instances[slot].take() else {
                continue;
            };
            if instance.owned_callback()
                && self.interaction != Some(self.tasks.root(instance.handle))
            {
                self.instances[slot] = Some(instance);
                continue;
            }
            if instance.background.as_ref().is_some_and(|b| {
                b.paused
                    || b.require_control && !self.world.input_enabled
                    || self.world.field_transition.is_some()
                    || self.world.world_transition.is_some()
                    || self.world.field_exit.is_some()
                    || self.battle_pending()
            }) {
                if let Some(Wait::Tick(wake)) = &mut instance.wait {
                    *wake = wake.checked_add(1).context("paused event clock overflow")?;
                }
                self.instances[slot] = Some(instance);
                continue;
            }
            if let Some(handle) = instance.join {
                let result = self
                    .tasks
                    .join(instance.handle, handle)
                    .map_err(anyhow::Error::msg);
                let result = result.map_err(|error| self.task_error(&mut instance, error))?;
                let Some(result) = result else {
                    self.instances[slot] = Some(instance);
                    continue;
                };
                instance
                    .vm
                    .complete_task(&result)
                    .map_err(anyhow::Error::new)
                    .map_err(|error| self.task_error(&mut instance, error))?;
                instance.join = None;
            }
            let ready = instance
                .wait
                .as_mut()
                .map(|wait| wait.poll(&mut self.world))
                .transpose()
                .map_err(anyhow::Error::msg)
                .map_err(|error| self.task_error(&mut instance, error))?;
            if ready == Some(false) {
                self.instances[slot] = Some(instance);
                continue;
            }
            if let Some(wait) = instance.wait.take() {
                let battle_completed = matches!(&wait, Wait::Battle(_));
                if matches!(wait, Wait::ControlHandoff(_)) {
                    self.world.input_enabled = true;
                }
                let result = if let Wait::Result(operation) = wait {
                    let Some(crate::Outcome::Completed(value)) = operation.progress().outcome
                    else {
                        anyhow::bail!("service completed without a result");
                    };
                    value
                } else if let Wait::Menu(operation) = wait {
                    ensure!(
                        operation.progress().outcome == Some(crate::Outcome::Completed(Some(0))),
                        "menu completed without its zero result"
                    );
                    for address in [0x24, 0x28] {
                        self.memory
                            .write(address, symphonia_script::Width::S32, 0)?;
                    }
                    Some(0)
                } else if let Wait::Battle(operation) = wait {
                    let Some(crate::Outcome::Completed(Some(value))) = operation.progress().outcome
                    else {
                        anyhow::bail!("battle completed without a result");
                    };
                    crate::battle::Outcome::try_from(value).map_err(anyhow::Error::msg)?;
                    self.memory
                        .write(0x24, symphonia_script::Width::S32, value)?;
                    Some(value)
                } else if let Wait::Choice { result, window } = wait {
                    let progress = result.progress();
                    let Some(crate::Outcome::Completed(Some(value))) = progress.outcome else {
                        anyhow::bail!("choice completed without a selection");
                    };
                    let reason = crate::dialogue::ChoiceExit::try_from(progress.position)
                        .map_err(anyhow::Error::msg)?;
                    if instance.program.authored().is_some() {
                        self.world
                            .choices
                            .retain(|_, c| c.operation.id() != result.id());
                        if let Wait::Complete(notice) = *window {
                            self.world
                                .dialogue
                                .retain(|_, d| d.operation.id() != notice.id());
                        }
                        // Authored choices return a one-based line, or zero when dismissed.
                        Some(if reason == crate::dialogue::ChoiceExit::Confirm {
                            value
                        } else {
                            0
                        })
                    } else {
                        // The script ABI returns the line and writes the exit reason separately.
                        self.memory.write(
                            0x24,
                            symphonia_script::Width::S32,
                            match reason {
                                crate::dialogue::ChoiceExit::Confirm => 0,
                                crate::dialogue::ChoiceExit::Cancel => 1,
                                crate::dialogue::ChoiceExit::Timeout => -1,
                            },
                        )?;
                        Some(value)
                    }
                } else {
                    None
                };
                instance.vm.complete(result, &mut self.memory)?;
                if battle_completed && !self.world.restore_battle_music {
                    // Live combat publishes its result before resuming the caller.
                    // A debug bypass has no scene handoff and resumes immediately.
                    self.instances[slot] = Some(instance);
                    continue;
                }
            }
            let mut commands = Vec::new();
            let mut spawns = Vec::new();
            let mut wait = None;
            let result = if instance.program.authored().is_some() {
                let mut host = crate::authored::FieldHost {
                    world: &mut self.world,
                    resources: &self.resources,
                    program: &instance.program,
                    scenario: &self.program,
                    wait: &mut wait,
                    operations: &mut instance.operations,
                    handle: instance.handle,
                    tasks: &mut self.tasks,
                    spawns: &mut spawns,
                    next_handle: &mut self.next_handle,
                    free_slots: self
                        .instances
                        .iter()
                        .filter(|instance| instance.is_none())
                        .count()
                        .saturating_sub(1),
                };
                instance.vm.run(
                    &mut host,
                    &mut self.memory,
                    update_budget.min(AUTHORED_BUDGET),
                )
            } else {
                let mut host = NativeHost {
                    world: &mut self.world,
                    resources: &self.resources,
                    program: &instance.program,
                    event_actor: instance.event_actor,
                    registers: &mut instance.registers,
                    events: &mut commands,
                    free_slots: self
                        .instances
                        .iter()
                        .enumerate()
                        .fold(0, |free, (i, entry)| {
                            free | if i != slot && entry.is_none() {
                                1 << i
                            } else {
                                0
                            }
                        }),
                    wait: &mut wait,
                };
                instance.vm.run(
                    &mut host,
                    &mut self.memory,
                    update_budget.min(LEGACY_BUDGET),
                )
            };
            let result = result.map_err(|error| {
                let context = format!(
                    "event {:?}, handle {}, update {}, source {:?}",
                    instance.key,
                    instance.handle,
                    self.world.tick,
                    instance.vm.source_trace(error.pc)
                );
                self.task_error(&mut instance, anyhow::Error::new(error).context(context))
            })?;
            update_budget -= result.steps;
            let program = instance.program.clone();
            match result.event {
                RunEvent::Suspended { opcode } => {
                    let wait = wait.with_context(|| {
                        format!("native {opcode:#04x} suspended without a completion condition")
                    })?;
                    if instance.owned_callback() {
                        wait.track(&mut instance.operations)
                            .map_err(anyhow::Error::msg)?;
                    } else if let Wait::Battle(operation) = &wait {
                        instance
                            .operations
                            .track(operation)
                            .map_err(anyhow::Error::msg)?;
                    }
                    instance.wait = Some(wait);
                    self.instances[slot] = Some(instance);
                }
                RunEvent::SuspendedTask { handle } => {
                    instance.join = Some(handle);
                    self.instances[slot] = Some(instance);
                }
                RunEvent::Halted => {
                    if let Some(Callback::Owned(completion)) = instance.callback.take() {
                        completion.complete(None).map_err(anyhow::Error::msg)?;
                        self.tasks.remove(instance.handle);
                    }
                    if let Some(result) = instance.vm.result() {
                        self.finish_task(instance.handle, result);
                    }
                    if self.interaction == Some(instance.handle) {
                        self.interaction = None;
                        self.world.input_enabled = true;
                    }
                }
            }
            for child in spawns {
                // A parent that returned without joining has already cancelled this request.
                if !self.tasks.contains(child.handle) {
                    continue;
                }
                let entry = self
                    .instances
                    .iter_mut()
                    .find(|instance| instance.is_none())
                    .context("event pool exhausted (32 instances)")?;
                *entry = Some(match child.target {
                    crate::authored::SpawnTarget::Task { entry, arguments } => {
                        Instance::with_arguments(&program, entry, child.handle, None, &arguments)?
                    }
                    crate::authored::SpawnTarget::Callback {
                        entry,
                        key,
                        event_actor,
                        completion,
                    } => {
                        let mut callback =
                            Instance::new(&self.program, entry, child.handle, Some(key))?;
                        callback.event_actor = event_actor;
                        callback.callback = Some(Callback::Owned(completion));
                        callback
                    }
                });
            }
            for EventCommand { handle, action } in commands {
                let Some(entry) = handle
                    .checked_sub(1)
                    .and_then(|slot| usize::try_from(slot).ok())
                    .and_then(|slot| self.instances.get_mut(slot))
                else {
                    continue;
                };
                if matches!(action, EventAction::Release) {
                    if entry.as_ref().is_some_and(|i| i.background.is_some()) {
                        *entry = None;
                    }
                    continue;
                }
                let EventAction::Spawn(key) = action else {
                    if let Some(background) = entry.as_mut().and_then(|i| i.background.as_mut()) {
                        match action {
                            EventAction::Pause(paused) => background.paused = paused,
                            EventAction::ControlGate(enabled) => {
                                background.require_control = enabled
                            }
                            EventAction::Spawn(_) | EventAction::Release => unreachable!(),
                        }
                    }
                    continue;
                };
                let pc = self
                    .program
                    .event(2, key)
                    .context("spawned event has no entry")?;
                ensure!(entry.is_none(), "reserved event slot is occupied");
                let handle = self.next_handle;
                self.next_handle = handle.checked_add(1).context("event handle overflow")?;
                let mut spawned = Instance::new(&self.program, pc, handle, Some(key))?;
                spawned.background = Some(Background::default());
                *entry = Some(spawned);
            }
            self.reconcile_control();
            if self.world.blocked_by_movie() {
                break;
            }
        }
        self.world.update_costumes(&self.resources);
        Ok(())
    }
}
