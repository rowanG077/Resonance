//! Apply original party template traits to a session-derived actor candidate.
//! Model bindings, equipment and active conditions are prepared separately.
use super::recoil::Parameters;
use anyhow::{Context, Result, ensure};
use resonance_battle::{Actor, Side};
use resonance_content::{battle_profile, prepared::Files};

/// Character IDs follow the source table (1..=11). The actor is owned by this
/// preparation attempt; a failure cannot mutate persistent or active state.
pub fn party(files: &Files, character: u8, mut actor: Actor) -> Result<Actor> {
    ensure!(
        actor.side == Side::Party,
        "party profile requires a party actor"
    );
    let profile = party_template(files, character)?;
    apply(files, &profile, &mut actor)?;
    Ok(actor)
}

pub(super) fn party_template(files: &Files, character: u8) -> Result<battle_profile::Profile> {
    let table: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    ensure!(table.records.len() == 11, "invalid party profile table");
    character
        .checked_sub(1)
        .and_then(|index| table.records.into_iter().nth(usize::from(index)))
        .context("missing party actor profile")
}

/// Shared template traits; statistics, equipment and resource bindings stay with
/// their owning preparation step.
pub(super) fn apply(
    files: &Files,
    profile: &battle_profile::Profile,
    actor: &mut Actor,
) -> Result<()> {
    let recoil = Parameters::load(files)?;
    actor.body.center_offset = [
        profile.center_offset[0].finite()?,
        profile.center_offset[1].finite()?,
        profile.center_offset[2].finite()?,
    ];
    actor.body.scale = profile.model_scale.finite()?;
    actor.effect_scale = profile.effect_scale.finite()?;
    actor.hud.suppress_bounce = profile.flags & 0x80000 != 0;
    actor.framing = resonance_battle::ActorFraming {
        yaw_offset: f32::from(profile.camera_yaw_offset),
        large: profile.camera_category == 3,
        minimum_radius: f32::from(profile.camera_minimum_radius.max(0)),
    };
    actor.movement.hover_height = profile.ground_offset.finite()?;
    actor.movement.flying = profile.flags & 1 != 0;
    actor.movement.mobility_blocked =
        (profile.condition_flags[1] | profile.intrinsic_conditions[1]) & 0x200 != 0;
    actor.movement.fixed_height = profile.body_flags & 0x4000 != 0;
    actor.movement.turning_disabled = profile.flags & 0x80 != 0;
    actor.movement.steering.passes_allied_obstacles = profile.body_flags & 1 != 0;
    actor.movement.steering.passable_for_allies = profile.body_flags & 0x20 != 0;
    actor.movement.steering.unrestricted_arena = profile.flags & 0x40000000 != 0;
    actor.movement.steering.clearance_category = profile.camera_category;
    actor.reaction.profile = recoil.profile(profile.weight, profile.flags);
    actor.reaction.recover_in_air = profile.flags & 0x100000 != 0;
    actor.reaction.armor.base = profile.armor;
    actor.reaction.armor.threshold = profile.armor;
    actor.reaction.stagger.threshold = profile.stagger_threshold;
    actor.reaction.stagger.duration = profile.stagger_ticks;
    actor.reaction.stun.resistance = profile.stun_resistance;
    actor.reaction.stun.immune = profile.condition_immunity[1] & 4 != 0;
    actor.reaction.stun.shortened = profile.condition_immunity[0] & 0x100 != 0;
    actor.guard.reduction = profile.guard_reduction;
    actor.guard.auto_disabled = profile.flags & 0x4000 != 0;
    actor.guard.allow_airborne = profile.flags & 1 != 0;
    // 1CAA8 writes a halfword after preparing party maximum HP. Enemies keep
    // the independent template value.
    actor.guard.break_pressure = match actor.side {
        Side::Party => (actor.max_hp / 100 + 3) as i16,
        Side::Enemy => profile.guard_pressure_limit,
    };
    Ok(())
}
