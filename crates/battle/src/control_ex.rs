//! Charge, guard latches, and completed-taunt recovery.
use crate::{ActorId, Battle, Cue};
use anyhow::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlExTraits {
    pub counter: bool,
    pub rebound: bool,
    pub roll: bool,
    pub aerial_guard: bool,
    pub timed_guard: bool,
    pub double_jump: bool,
    pub charge: bool,
    pub lucky_charge: bool,
    pub taunt_hp: bool,
    pub taunt_vitals: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChargeLevel {
    #[default]
    None,
    Normal,
    Strong,
}
impl ChargeLevel {
    pub fn charged(self) -> bool {
        self != Self::None
    }
}

/// Live skill counters retained across equipment refresh.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlExState {
    pub charge: ChargeLevel,
    pub charge_hold: u8,
    pub charge_remaining: u32,
    pub guard_hold: u8,
    pub guard_ready: bool,
    pub counter_active: bool,
    pub double_jump_used: bool,
}
impl ControlExState {
    pub(crate) fn recover(&mut self) {
        // Reset ordinary recovery latches while retaining charge.
        self.guard_hold = 0;
        self.guard_ready = false;
        self.counter_active = false;
        self.double_jump_used = false;
    }
    pub(crate) fn advance_charge(&mut self) {
        if self.charge_remaining != 0 {
            self.charge_remaining = self.charge_remaining.saturating_sub(1);
            if self.charge_remaining == 0 {
                self.charge = ChargeLevel::None;
            }
        }
    }
}

impl Battle {
    /// Timed guard continues charging during local hit-stop.
    pub(crate) fn advance_guard_ex(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let owner = &mut self.actors[actor.index()];
        if owner.equipment.control_ex.timed_guard && !owner.control_ex_state.guard_ready {
            owner.control_ex_state.guard_hold = owner.control_ex_state.guard_hold.wrapping_add(1);
            if owner.control_ex_state.guard_hold >= 180 {
                owner.control_ex_state.guard_hold = 180;
                owner.control_ex_state.guard_ready = true;
                cues.push(Cue::GuardReady { actor });
            }
        }
        Ok(())
    }
    pub(crate) fn activate_counter(&mut self, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let owner = &mut self.actors[actor.index()];
        if owner.equipment.control_ex.counter && owner.guard.can_counter() {
            owner.control_ex_state.counter_active = true;
            // These modes can replace the preceding mode without its ordinary refusal.
            owner.reaction.protection.mode = crate::ProtectionMode::Escape;
            owner.reaction.protection.remaining = owner.reaction.protection.remaining.max(15);
            cues.push(Cue::Countered { actor });
            self.request_timed_hold(10, None);
        }
        Ok(())
    }
    pub(crate) fn advance_auto_charge(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        if self.actors[index].control == crate::Control::Auto
            && self.actor_command_ready(actor)
            && !self.actors[index].needs_landing()
        {
            self.advance_attack_charge(actor, cues)?;
        }
        Ok(())
    }

    /// Run during grounded, flying, and fixed-height idle processing after recovery departure.
    pub(crate) fn advance_attack_charge(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        let owner = &mut self.actors[index];
        if !owner.equipment.control_ex.charge {
            return Ok(());
        }
        let held = self
            .cast_inputs
            .get(usize::from(owner.control_slot))
            .is_some_and(|input| input.attack_held);
        if !held {
            owner.control_ex_state.charge_hold = 0;
            return Ok(());
        }
        if owner.control_ex_state.charge.charged() {
            return Ok(());
        }
        owner.control_ex_state.charge_hold = owner.control_ex_state.charge_hold.wrapping_add(1);
        if owner.control_ex_state.charge_hold < 90 {
            return Ok(());
        }
        let succeeds = !owner.equipment.control_ex.lucky_charge || self.random.next_u16() & 1 != 0;
        owner.control_ex_state.charge_hold = 0;
        if succeeds {
            owner.control_ex_state.charge = if owner.equipment.control_ex.lucky_charge {
                ChargeLevel::Strong
            } else {
                ChargeLevel::Normal
            };
            owner.control_ex_state.charge_remaining = 300;
        }
        cues.push(Cue::Charged {
            actor,
            level: owner.control_ex_state.charge,
        });
        Ok(())
    }
    pub(crate) fn complete_taunt_recovery(
        &mut self,
        actor: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let index = actor.index();
        for (enabled, hp, tp) in [
            (
                self.actors[index].equipment.control_ex.taunt_vitals,
                2,
                true,
            ),
            (self.actors[index].equipment.control_ex.taunt_hp, 8, false),
        ] {
            if !enabled {
                continue;
            }
            // Each enabled trait restores its own amount, without a Lucky Healing roll.
            let owner = &mut self.actors[index];
            let (next, nominal) = owner.recovered_hp(hp);
            let applied = next - owner.hp;
            owner.hp = next;
            cues.push(Cue::Recovered {
                kind: crate::RecoveryKind::Hp,
                actor,
                nominal,
                applied,
            });
            if tp {
                let (next, nominal) = owner.recovered_tp(1);
                cues.push(Cue::Recovered {
                    actor,
                    kind: crate::RecoveryKind::Tp,
                    nominal,
                    applied: i32::from(next) - i32::from(owner.tp),
                });
                owner.tp = next;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
