//! Apply gameplay profile traits to a prepared party or enemy actor.
//! Model bindings, equipment and active conditions are prepared separately.
use super::recoil::Parameters;
use anyhow::Result;
use resonance_battle::{Actor, Side};
use resonance_content::battle_profile;

/// Shared template traits; statistics, equipment and resource bindings stay with
/// their owning preparation step.
pub fn apply(
    recoil: &Parameters,
    profile: &battle_profile::Profile,
    actor: &mut Actor,
) -> Result<()> {
    super::conditions::apply_profile(profile, &mut actor.conditions)?;
    actor.species = profile.species;
    actor.body.center_offset = profile.center_offset;
    actor.body.scale = profile.model_scale;
    actor.effect_scale = profile.effect_scale;
    actor.movement.hover_height = profile.ground_offset;
    actor.movement.flying = profile.traits.flying;
    actor.movement.fixed_height = profile.traits.fixed_height;
    actor.movement.turning_disabled = profile.traits.turning_disabled;
    actor.movement.steering.passes_allied_obstacles = profile.traits.passes_allied_obstacles;
    actor.movement.steering.passable_for_allies = profile.traits.passable_for_allies;
    actor.movement.steering.unrestricted_arena = profile.traits.unrestricted_arena;
    actor.movement.steering.push_obstacle_disabled = profile.traits.push_obstacle_disabled;
    actor.movement.steering.push_immovable = profile.traits.push_immovable;
    actor.reaction.profile = recoil.profile(profile.weight, &profile.traits);
    actor.reaction.recover_in_air = profile.traits.recover_in_air;
    actor.reaction.armor.base = profile.armor;
    actor.reaction.armor.threshold = profile.armor;
    actor.reaction.stagger.threshold = profile.stagger_threshold;
    actor.reaction.stagger.duration = profile.stagger_ticks;
    actor.reaction.stun.resistance = profile.stun_resistance;
    actor.guard.reduction = profile.guard_reduction;
    actor.guard.auto_disabled = profile.traits.auto_guard_disabled;
    actor.guard.allow_airborne = profile.traits.flying;
    // Party proficiency starts from maximum HP; enemies retain the profile value.
    actor.guard.break_pressure = match actor.side {
        Side::Party => actor.equipment.max_hp.max(0) as u32 / 100 + 3,
        Side::Enemy => profile.guard_pressure_limit,
    };
    Ok(())
}
