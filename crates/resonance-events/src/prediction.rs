//! Explicit operation-aware copies for a disposable simulation branch.
//! Keep the world initializer exhaustive: adding state requires deciding whether
//! it can share immutable data or must detach service ownership.
use crate::{GameWorld, Operation, OperationFork};
use std::collections::BTreeMap;

trait Fork: Sized {
    fn fork(&self, fork: &mut OperationFork) -> Self;
}
impl<T: Fork> Fork for Option<T> {
    fn fork(&self, fork: &mut OperationFork) -> Self {
        self.as_ref().map(|value| value.fork(fork))
    }
}
impl<T: Fork> Fork for Vec<T> {
    fn fork(&self, fork: &mut OperationFork) -> Self {
        self.iter().map(|value| value.fork(fork)).collect()
    }
}
impl<K: Clone + Ord, V: Fork> Fork for BTreeMap<K, V> {
    fn fork(&self, fork: &mut OperationFork) -> Self {
        self.iter()
            .map(|(key, value)| (key.clone(), value.fork(fork)))
            .collect()
    }
}
impl Fork for Operation {
    fn fork(&self, fork: &mut OperationFork) -> Self {
        fork.fork(self)
    }
}

// These records contain exactly one operation; all other fields are values or
// immutable asset references. Scopes and VM instances have explicit constructors.
macro_rules! operation_record {
    ($($record:ty),* $(,)?) => { $(
        impl Fork for $record {
            fn fork(&self, fork: &mut OperationFork) -> Self {
                let mut result = self.clone();
                result.operation = self.operation.fork(fork);
                result
            }
        }
    )* };
}
operation_record!(
    crate::projectile::VisualLift,
    crate::projectile::ChainImpulse,
    crate::camera::FogEffect,
    crate::skit::Request,
    crate::menu::Request,
    crate::session_screen::Request,
    crate::dialogue::Dialogue,
    crate::dialogue::Choice,
    crate::dialogue::Movie,
    crate::FieldTransition,
    crate::WorldTransition,
    crate::effect::BillboardEffect,
    crate::effect::station::Transfer,
    crate::model_particle::ModelParticle,
    crate::effect::RefractionPulse,
    crate::battle::Request,
);

impl Fork for crate::Actor {
    fn fork(&self, fork: &mut OperationFork) -> Self {
        let mut result = self.clone();
        result.operation = self.operation.fork(fork);
        result.visual_lift = self.visual_lift.fork(fork);
        result.chain_impulses = self.chain_impulses.fork(fork);
        result
    }
}

impl GameWorld {
    /// Nested scenes started by speculation inherit its private ID allocator.
    pub fn inherit_prediction_allocator(&mut self, source: &Self) {
        self.operations.inherit_allocator(&source.operations);
    }

    pub(crate) fn fork(&self, fork: &mut OperationFork) -> Self {
        Self {
            battle_event: self.battle_event.clone(),
            environment_wind: self.environment_wind.clone(),
            enemy_encounter_cooldown: self.enemy_encounter_cooldown,
            texture_animation_states: self.texture_animation_states.clone(),
            authored_actors: self.authored_actors.clone(),
            ring: self.ring.fork(fork),
            fog_effects: self.fog_effects.fork(fork),
            tick: self.tick,
            played_ticks: self.played_ticks,
            reset_play_time: self.reset_play_time,
            effect_tick: self.effect_tick,
            current_field: self.current_field,
            ring_scenery: self.ring_scenery,
            debug_session: self.debug_session,
            skit: self.skit.clone(),
            skit_request: self.skit_request.fork(fork),
            menu_request: self.menu_request.fork(fork),
            screen_request: self.screen_request.fork(fork),
            actors: self.actors.fork(fork),
            duplicate_actors: self.duplicate_actors.clone(),
            automatic_wings: self.automatic_wings,
            actor_order: self.actor_order.clone(),
            next_actor_instance: self.next_actor_instance,
            camera: self.camera.clone(),
            fade: self.fade.clone(),
            scene_dissolve: self.scene_dissolve.clone(),
            frame_feedback: self.frame_feedback,
            next_transition_white: self.next_transition_white,
            overlays: self.overlays.clone(),
            effect_settings: self.effect_settings.clone(),
            character_lights: self.character_lights.clone(),
            texture_bindings: self.texture_bindings.clone(),
            texture_animation_enabled: self.texture_animation_enabled,
            texture_animation_tick: self.texture_animation_tick,
            texture_animation_effect_tick: self.texture_animation_effect_tick,
            screen_copy_depth: self.screen_copy_depth,
            dialogue: self.dialogue.fork(fork),
            choices: self.choices.fork(fork),
            party: self.party.clone(),
            field_transition: self.field_transition.fork(fork),
            world_transition: self.world_transition.fork(fork),
            field_exit: self.field_exit.as_ref().map(|exit| exit.fork(fork)),
            preload_field: self.preload_field,
            movie: self.movie.fork(fork),
            voice: self.voice.clone(),
            voice_durations: self.voice_durations.clone(),
            field_camera: self.field_camera.clone(),
            input_enabled: self.input_enabled,
            menu_disabled: self.menu_disabled,
            input: self.input,
            mapped_input_disabled: self.mapped_input_disabled,
            controlled_actor: self.controlled_actor,
            grabbed_block: self.grabbed_block,
            player_size: self.player_size,
            event_flags: self.event_flags.clone(),
            script_state: self.script_state.clone(),
            event_records: self.event_records.clone(),
            calendar_time: self.calendar_time,
            triggers: self.triggers.clone(),
            save_points: self.save_points.clone(),
            treasures: self.treasures.clone(),
            treasure_models: self.treasure_models,
            door_interaction_radius: self.door_interaction_radius,
            exit_door: self.exit_door,
            audio_commands: self.audio_commands.clone(),
            rumble: self.rumble,
            ambient_voices: self.ambient_voices,
            voice_banks: self.voice_banks,
            emotes: self.emotes.clone(),
            damage_numbers: self.damage_numbers.clone(),
            paralysis: self.paralysis,
            billboards: self.billboards.fork(fork),
            station_transfers: self.station_transfers.fork(fork),
            effect_changes: self.effect_changes.clone(),
            particles_before_update: self.particles_before_update,
            effect_palette: self.effect_palette.clone(),
            effect_textures: self.effect_textures.clone(),
            model_particles: self.model_particles.fork(fork),
            refractions: self.refractions.fork(fork),
            random_state: self.random_state,
            gameplay_random: self.gameplay_random,
            battle_request: self.battle_request.fork(fork),
            restore_battle_music: self.restore_battle_music,
            loaded_resources: self.loaded_resources.clone(),
            operations: self.operations.fork(fork),
            next_particle: self.next_particle,
        }
    }
}

impl crate::battle::Request {
    pub fn fork_for_prediction(&self, fork: &mut OperationFork) -> Self {
        self.fork(fork)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_actor_effects_share_only_the_forked_operation() {
        let mut world = GameWorld::default();
        let operation = world.operations.begin().unwrap();
        let mut actor = crate::Actor::new(1, [0.; 3]);
        actor.visual_lift = Some(crate::projectile::VisualLift {
            height: 20.,
            operation: operation.clone(),
        });
        actor.chain_impulses.insert(
            1,
            crate::projectile::ChainImpulse {
                acceleration: [1., 2., 3.],
                operation: operation.clone(),
            },
        );
        world.insert_actor(1, actor);
        let mut fork = OperationFork::default();
        let branch = world.fork(&mut fork);
        let branch_actor = &branch.actors[&1];
        branch_actor.chain_impulses[&1]
            .operation
            .complete(None)
            .unwrap();
        assert_eq!(branch_actor.visual_position()[2], 0.);
        assert_eq!(world.actors[&1].visual_position()[2], 20.);
        drop(branch);
        assert!(operation.is_pending());
    }
}
