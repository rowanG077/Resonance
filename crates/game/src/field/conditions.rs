//! Party conditions appear above the controlled field character.
use anyhow::Result;
use resonance_events::{GameWorld, effect::BillboardEffect};

const PUFF_HEIGHT: f32 = 150.;

pub(super) fn step(world: &mut GameWorld, effect_tick: u32) -> Result<()> {
    world.paralysis = None;
    if !world.input_enabled
        || world.field_transition.is_some()
        || world.world_transition.is_some()
        || world.blocked_by_movie()
    {
        return Ok(());
    }
    let (mut poisoned, mut severe, mut paralyzed) = (false, false, false);
    if let Some(party) = &world.party {
        for id in &party.formation {
            let member = &party.members[usize::from(id - 1)];
            if member.knocked_out() || member.ailments.petrified {
                continue;
            }
            poisoned |= member.ailments.poison.has_mild() || member.ailments.poison.has_severe();
            severe |= member.ailments.poison.has_severe();
            paralyzed |= member.ailments.paralysis;
        }
    }
    let period = if severe { 4 } else { 16 };
    if paralyzed {
        world.paralysis = Some(resonance_events::effect::Paralysis {
            actor: world.controlled_actor,
            frame: ((effect_tick / 32) & 1) as u8,
        });
    }
    if !poisoned || !effect_tick.is_multiple_of(period) {
        return Ok(());
    }
    let Some(actor) = world.actors.get(&world.controlled_actor) else {
        return Ok(());
    };
    let mut position = actor.position;
    for axis in &mut position[..2] {
        *axis += (world.random() & 31) as f32 - 15.;
    }
    position[2] += PUFF_HEIGHT;
    let size = (world.random() & 15) as f32 + if severe { 16. } else { 8. };
    let speed = (world.random() & 31) as f32 / 16. + 2.;
    let puff = BillboardEffect::poison(position, size, speed, world.tick);
    world.emit_billboard(puff).map_err(anyhow::Error::msg)?;
    Ok(())
}
