//! One real field step in an isolated, disposable branch. No presentation service
//! receives this session: renderers sample continuous visual endpoints only.
use super::*;

/// Separate preallocated regions for a retained endpoint and transient work.
/// Ordinary nested Rust containers use these regions through the application
/// allocator. A region is rewound only after its previous result is dropped.
pub struct PredictionBuffer {
    scene: resonance_simulation_memory::Arena,
    scratch: resonance_simulation_memory::Arena,
}
impl PredictionBuffer {
    pub const CAPACITY_PER_SLOT: usize = 64 * 1024 * 1024;

    pub fn new() -> Result<Self> {
        Ok(Self {
            scene: resonance_simulation_memory::Arena::new(Self::CAPACITY_PER_SLOT)?,
            scratch: resonance_simulation_memory::Arena::new(Self::CAPACITY_PER_SLOT)?,
        })
    }

    pub fn predict(&mut self, field: &FieldSession, input: FieldInput) -> Result<FieldPrediction> {
        self.execute(|| field.predict_next(input))?
    }

    /// Reuse scene storage after the previous endpoint and its derived values
    /// have been dropped. Scratch work never changes scene ownership.
    pub fn execute<T>(&mut self, work: impl FnOnce() -> T) -> Result<T> {
        Ok(self
            .scene
            .run(|| resonance_content::diagnostics::speculative(work))?)
    }

    /// Reuse transient storage while the scene endpoint remains alive.
    pub fn scratch<T>(&mut self, work: impl FnOnce() -> T) -> Result<T> {
        Ok(self
            .scratch
            .run(|| resonance_content::diagnostics::speculative(work))?)
    }

    /// Retain derived presentation endpoints beside the scene endpoint.
    pub fn retain<T>(&mut self, work: impl FnOnce() -> T) -> Result<T> {
        Ok(self
            .scene
            .append(|| resonance_content::diagnostics::speculative(work))?)
    }

    pub fn used_bytes(&self) -> [usize; 2] {
        [self.scene.used(), self.scratch.used()]
    }
}

/// Retain the isolated scene and the service inputs used to predict it.
/// A handoff has no drawable state but still needs dependency invalidation.
pub struct FieldPrediction {
    pub state: Option<FieldSession>,
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
    pub fn predict_next(&self, input: FieldInput) -> Result<FieldPrediction> {
        let mut fork = resonance_events::OperationFork::default();
        let state = if self.prediction_available() {
            let mut state = self.fork_for_prediction(&mut fork);
            state.step(input)?;
            state.prediction_available().then_some(state)
        } else {
            None
        };
        // Retain observed service dependencies even when the next step hands off.
        Ok(FieldPrediction {
            state,
            sources: fork,
        })
    }

    pub fn fork_for_prediction(&self, fork: &mut resonance_events::OperationFork) -> Self {
        Self {
            diagnostics: self.diagnostics.clone(),
            player_movement: self.player_movement.clone(),
            unison_character: self.unison_character,
            allow_incomplete_scripts: self.allow_incomplete_scripts,
            entered_control: self.entered_control,
            entry_wait_ticks: self.entry_wait_ticks,
            play_time: self.play_time,
            effect_clock: self.effect_clock,
            map_id: self.map_id,
            events: self.events.fork_for_prediction(fork),
            authored_entry: self.authored_entry.clone(),
            menu: self.menu.clone(),
            item_statistics: self.item_statistics,
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
            voice_feedback: self.voice_feedback,
            talking: self.talking.clone(),
        }
    }
}
