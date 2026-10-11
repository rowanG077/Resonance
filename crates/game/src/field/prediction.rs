//! One real field step in an isolated, disposable branch. No presentation service
//! receives this session: renderers sample continuous visual endpoints only.
use super::*;

/// One committed session plus two alternating, preallocated prediction regions.
/// Ordinary nested Rust containers use these regions through the application
/// allocator. A region is rewound only after its previous result is dropped.
pub struct PredictionBuffer {
    arenas: [resonance_simulation_memory::Arena; 2],
    next: usize,
}
impl PredictionBuffer {
    pub const CAPACITY_PER_SLOT: usize = 64 * 1024 * 1024;

    pub fn new() -> Result<Self> {
        Ok(Self {
            arenas: [
                resonance_simulation_memory::Arena::new(Self::CAPACITY_PER_SLOT)?,
                resonance_simulation_memory::Arena::new(Self::CAPACITY_PER_SLOT)?,
            ],
            next: 0,
        })
    }

    pub fn predict(
        &mut self,
        field: &FieldSession,
        input: FieldInput,
    ) -> Result<Option<FieldPrediction>> {
        self.execute(|| field.predict_next(input))?
    }

    /// Reuse the next scene slot. Values returned by the previous use of that
    /// slot must have been dropped before calling this again.
    pub fn execute<T>(&mut self, work: impl FnOnce() -> T) -> Result<T> {
        let slot = self.next;
        let result = self.arenas[slot].run(work)?;
        self.next = 1 - slot;
        Ok(result)
    }

    /// Transient render calculations use the spare slot while the scene endpoint
    /// remains alive in the other one. This does not rotate scene ownership.
    pub fn scratch<T>(&mut self, work: impl FnOnce() -> T) -> Result<T> {
        Ok(self.arenas[self.next].run(work)?)
    }

    /// Retain derived presentation endpoints in the scene's slot. The other
    /// slot stays available for transient work on every rendered frame.
    pub fn retain<T>(&mut self, work: impl FnOnce() -> T) -> Result<T> {
        Ok(self.arenas[1 - self.next].append(work)?)
    }

    pub fn used_bytes(&self) -> [usize; 2] {
        self.arenas
            .each_ref()
            .map(resonance_simulation_memory::Arena::used)
    }
}

/// Own the complete isolated endpoint until rendering finishes. Keeping its
/// scopes alive preserves operation-backed visual state and avoids a second
/// copy of actors, cameras and animation data.
pub struct FieldPrediction {
    pub state: FieldSession,
    sources: resonance_events::OperationFork,
}
impl FieldPrediction {
    pub fn sources_unchanged(&self) -> bool {
        self.sources.sources_unchanged()
    }
}

impl FieldSession {
    fn prediction_available(&self) -> bool {
        let world = &self.events.world;
        world.field_transition.is_none()
            && world.world_transition.is_none()
            && world.screen_request.is_none()
            && world.battle_request.is_none()
            && !world.blocked_by_movie()
    }

    /// Predict with a copy of current input, never consuming the live input latch.
    /// Scene handoffs retain their committed presentation.
    fn predict_next(&self, input: FieldInput) -> Result<Option<FieldPrediction>> {
        if !self.prediction_available() {
            return Ok(None);
        }
        let mut fork = resonance_events::OperationFork::default();
        let mut branch = self.fork_for_prediction(&mut fork);
        branch.step(input)?;
        if !branch.prediction_available() {
            return Ok(None);
        }
        Ok(Some(FieldPrediction {
            state: branch,
            sources: fork,
        }))
    }

    pub fn fork_for_prediction(&self, fork: &mut resonance_events::OperationFork) -> Self {
        Self {
            allow_incomplete_scripts: self.allow_incomplete_scripts,
            entered_control: self.entered_control,
            entry_wait_ticks: self.entry_wait_ticks,
            play_time: self.play_time,
            effect_clock: self.effect_clock,
            map_id: self.map_id,
            events: self.events.fork_for_prediction(fork),
            authored_entry: self.authored_entry.clone(),
            menu: self.menu.clone(),
            shop: self.shop.as_ref().map(|s| s.fork_for_prediction(fork)),
            crafting: self.crafting.as_ref().map(|s| s.fork_for_prediction(fork)),
            menu_operation: self.menu_operation.as_ref().map(|op| fork.fork(op)),
            menu_resources: self.menu_resources.clone(),
            dialogue: self
                .dialogue
                .iter()
                .map(|(slot, player)| (*slot, player.fork_for_prediction(fork)))
                .collect(),
            choices: self.choices.clone(),
            walkmesh: self.walkmesh.clone(),
            player_fall: self.player_fall.clone(),
            light_regions: self.light_regions.clone(),
            conversation_facing: self.conversation_facing,
            save_points: self.save_points.clone(),
            treasures: self.treasures.clone(),
            blocks: self.blocks.clone(),
            action_hints: self.action_hints.clone(),
            skits: self.skits.clone(),
            active_skit: self
                .active_skit
                .as_ref()
                .map(|s| s.fork_for_prediction(fork)),
            skit_programs: self.skit_programs.clone(),
            voice_feedback: None,
            talking: self.talking.clone(),
        }
    }
}
