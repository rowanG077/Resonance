use crate::conditions::Condition;
use crate::{ActionDefinition, Actor, Side};

fn technical(cost: u32) -> u32 {
    cost - cost / 8
}

fn equipment(owner: &Actor, cost: u32) -> u32 {
    let effective = owner.conditions.effective();
    use resonance_content::menu_data::TpDiscount;
    let discount = if effective.contains(Condition::TpHalf) {
        TpDiscount::Half
    } else if effective.contains(Condition::TpThird) {
        TpDiscount::Third
    } else {
        TpDiscount::None
    };
    discount.apply(cost)
}

fn martial_cost(owner: &Actor, mut cost: u32) -> u32 {
    if owner.side != Side::Party {
        return cost;
    }
    if owner.equipment.tp_cost_reduction {
        cost = technical(cost);
    }
    if owner.equipment.damage.physical_arte_boost {
        cost += cost / 4;
    }
    equipment(owner, cost)
}

/// Admission applies party skill and equipment modifiers to the prepared base cost.
pub(crate) fn catalogue_quote(owner: &Actor, raw: u16) -> u32 {
    martial_cost(owner, u32::from(raw))
}

pub(crate) fn action_quote(owner: &Actor, key: crate::ActionKey, action: &ActionDefinition) -> u32 {
    match &action.execution {
        crate::ActionExecution::Attack(_) => catalogue_quote(owner, action.tp_cost),
        crate::ActionExecution::Casting(_) => spell_quote(
            owner,
            action.tp_cost,
            owner.casting_state.previous_spell == Some(key),
        ),
    }
}

fn spell_quote(owner: &Actor, raw: u16, repeated: bool) -> u32 {
    if owner.side != Side::Party {
        return u32::from(raw);
    }
    let cost = if owner.equipment.casting.reducer && repeated {
        u32::from(raw) * 75 / 100
    } else {
        u32::from(raw)
    };
    let cost = equipment(owner, cost);
    if owner.equipment.tp_cost_reduction {
        technical(cost)
    } else {
        cost
    }
}

pub(crate) fn initialize_random_clock(
    owner: &Actor,
    resuming: bool,
    mut duration: u32,
    mut roll: impl FnMut() -> u16,
) -> u32 {
    if owner.side != Side::Party || resuming {
        return duration;
    }
    let luck_bonus = owner.equipment.luck / 20;
    let short_chance = 5 + luck_bonus;
    let slow_chance = 10_u16.saturating_sub(luck_bonus);
    if owner.equipment.casting.quick && roll() < short_chance {
        duration = 1;
    }
    if owner.equipment.casting.random {
        if roll() < short_chance {
            duration = 1;
        } else if roll() < slow_chance {
            duration = duration.saturating_mul(2);
        }
    }
    duration
}

/// Quotes are deterministic. Only committing an admitted spell may waive its cost.
pub(crate) fn commit_spell_cost(
    owner: &Actor,
    quote: u32,
    roll: impl FnOnce() -> u16,
) -> (u32, bool) {
    let lucky = owner.side == Side::Party
        && owner.equipment.casting.lucky_magic
        && roll() < owner.equipment.luck / 10 + 5;
    (if lucky { 0 } else { quote }, lucky)
}

pub(crate) fn special_guard_debit(owner: &Actor, catalogue_raw: u16) -> u32 {
    let selector = martial_cost(owner, u32::from(owner.equipment.max_tp) / 10);
    if selector == 0 {
        catalogue_quote(owner, catalogue_raw)
    } else {
        selector
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cast_payment_tests;
