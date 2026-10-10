//! Lethal-contact rescue preserves Hurt, Down, or Stun without entering death or ordinary
//! resurrection.
use crate::conditions::{Condition, ConditionSet};
use crate::{ActorId, Battle, Cue, Side};
use anyhow::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LethalRescueTraits {
    pub resurrect: bool,
    pub angel_tear: bool,
    /// Prepared capabilities in consumption priority order.
    pub equipment: [Option<RescueEquipment>; 3],
}

/// Slots are opaque to battle simulation; the host resolves consumed slots to its equipment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescueEquipment {
    Chance,
    Consumable(u8),
    Consumed(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescueKind {
    AngelTear,
    Revive,
    Resurrect,
    Ring,
    Doll(usize),
}
impl RescueKind {
    fn percent(self) -> i16 {
        match self {
            Self::AngelTear => 25,
            Self::Revive | Self::Doll(_) => 50,
            Self::Resurrect => 20,
            Self::Ring => 30,
        }
    }
}

fn select(
    armed: bool,
    entry_conditions: ConditionSet,
    side: Side,
    traits: LethalRescueTraits,
    luck: u16,
    roll: impl FnOnce() -> u16,
) -> Option<RescueKind> {
    if armed {
        return Some(RescueKind::AngelTear);
    }
    if entry_conditions.contains(Condition::Revive) {
        return Some(RescueKind::Revive);
    }
    let party = side == Side::Party;
    let ring = party && traits.equipment.contains(&Some(RescueEquipment::Chance));
    if (ring || traits.resurrect) && roll() < (luck >> 4) + 2 {
        // The skill reward takes precedence even when equipment admitted the roll.
        return Some(if traits.resurrect {
            RescueKind::Resurrect
        } else {
            RescueKind::Ring
        });
    }
    if !party {
        return None;
    }
    traits
        .equipment
        .iter()
        .position(|item| matches!(item, Some(RescueEquipment::Consumable(_))))
        .map(RescueKind::Doll)
}

impl Battle {
    pub fn angel_tear_armed(&self, actor: ActorId) -> Result<bool> {
        self.actor(actor)?;
        Ok(self.runtime[actor.index()].angel_tear_armed)
    }

    pub(crate) fn try_lethal_rescue(
        &mut self,
        id: ActorId,
        entry_conditions: ConditionSet,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let index = id.index();
        let actor = self.actor(id)?;
        let traits = actor.equipment.recovery.lethal;
        let side = actor.side;
        let luck = actor.equipment.luck;
        let Some(kind) = select(
            self.runtime[index].angel_tear_armed,
            entry_conditions,
            side,
            traits,
            luck,
            || self.random.next_u16() % 100,
        ) else {
            return Ok(false);
        };
        self.request_timed_hold(45, Some(id));
        cues.push(Cue::Rescued { actor: id, kind });
        self.actors[index]
            .reaction
            .protection
            .protect_transition(90);
        self.recover_vitals(id, kind.percent(), false, cues)?;
        let label = match kind {
            RescueKind::Ring | RescueKind::Doll(_) => {
                crate::conditions::ConditionLabel::EquipmentEffect
            }
            RescueKind::Revive => crate::conditions::ConditionLabel::Revive,
            _ => crate::conditions::ConditionLabel::ExSkillEffect,
        };
        cues.push(Cue::ConditionLabel {
            actor: id,
            kind: label,
            position: std::array::from_fn(|axis| {
                self.actors[index].body.center_offset[axis] * 2. + self.actors[index].position[axis]
            }),
        });
        match kind {
            RescueKind::AngelTear => self.runtime[index].angel_tear_armed = false,
            RescueKind::Revive => self.actors[index].conditions.consume_revive(),
            RescueKind::Doll(index) => {
                let capability =
                    &mut self.actors[id.index()].equipment.recovery.lethal.equipment[index];
                let Some(RescueEquipment::Consumable(slot)) = *capability else {
                    unreachable!()
                };
                *capability = Some(RescueEquipment::Consumed(slot));
            }
            _ => {}
        }
        Ok(true)
    }
}

#[cfg(test)]
pub(crate) mod tests;
