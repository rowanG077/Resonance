//! Party conditions appear above the controlled field character.
use anyhow::Result;
use resonance_events::{GameWorld, effect::BillboardEffect};

const POISON: u32 = 0x20;
const SEVERE_POISON: u32 = 0x40;
const PARALYSIS: u32 = 0x80;
const INACTIVE: u32 = 0x8000_0100; // Knockout or petrification.
const PUFF_HEIGHT: f32 = 150.;

pub(super) fn step(world: &mut GameWorld, effect_tick: u32) -> Result<()> {
    world.paralysis = None;
    if !world.input_enabled || world.field_transition.is_some() || world.blocked_by_movie() {
        return Ok(());
    }
    let conditions = world.party.as_ref().map_or(0, |party| {
        party.formation.iter().fold(0, |flags, id| {
            let member = &party.members[usize::from(id - 1)];
            flags
                | if member.conditions & INACTIVE == 0 {
                    member.conditions
                } else {
                    0
                }
        })
    });
    let period = if conditions & SEVERE_POISON != 0 {
        4
    } else {
        16
    };
    if conditions & PARALYSIS != 0 {
        world.paralysis = Some(resonance_events::effect::Paralysis {
            actor: world.controlled_actor,
            frame: ((effect_tick / 32) & 1) as u8,
        });
    }
    if conditions & (POISON | SEVERE_POISON) == 0 || !effect_tick.is_multiple_of(period) {
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
    let size = (world.random() & 15) as f32
        + if conditions & SEVERE_POISON != 0 {
            16.
        } else {
            8.
        };
    let speed = (world.random() & 31) as f32 / 16. + 2.;
    let puff = BillboardEffect::poison(position, size, speed, world.tick);
    world.emit_billboard(puff).map_err(anyhow::Error::msg)?;
    Ok(())
}
