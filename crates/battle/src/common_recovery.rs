use crate::state::ActorTask;
use crate::{ActorId, Battle, Cue, RecoveryKind};
use anyhow::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommonRecoveryTraits {
    pub low_hp: bool,
    pub last_hit: bool,
    pub self_cure: bool,
    /// Regenerate restores both HP and TP while idle.
    pub idle_hp_tp: bool,
    pub idle_hp: bool,
    pub idle_tp: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommonRecoveryState {
    /// Consecutive idle combat updates toward the next recovery.
    pub idle_updates: u16,
    /// Combat updates toward the next Self Cure attempt.
    pub self_cure_updates: u16,
    /// Combat updates toward the next point of last-hit recovery.
    pub last_hit_updates: u8,
    pub last_hit_damage: i32,
    /// Remaining HP recovery from the most recent hit.
    pub last_hit_recovery: i32,
}

// At 60 combat updates per second, Self Cure checks every five seconds and
// idle recovery restores vitals every two seconds. Last-hit recovery restores
// one HP every two updates until its budget is exhausted.
const SELF_CURE_INTERVAL: u16 = 300;
const IDLE_RECOVERY_INTERVAL: u16 = 120;
const LAST_HIT_INTERVAL: u8 = 2;

impl Battle {
    pub fn common_recovery_state(&self, actor: ActorId) -> Result<CommonRecoveryState> {
        self.actor(actor)?;
        Ok(self.runtime[actor.index()].recovery)
    }

    /// Flat recovery runs on combat updates, independent of HUD animation.
    pub(crate) fn advance_common_flat_recovery(&mut self, index: usize, combat: bool) {
        let actor = &mut self.actors[index];
        if !combat || !actor.available() {
            return;
        }
        if actor.equipment.recovery.common.low_hp && actor.hp_percent() < 10 {
            actor.recover_flat_hp(1);
        }
        let state = &mut self.runtime[index].recovery;
        if actor.equipment.recovery.common.last_hit && state.last_hit_recovery > 0 {
            state.last_hit_updates += 1;
            if state.last_hit_updates == LAST_HIT_INTERVAL {
                state.last_hit_updates = 0;
                actor.recover_flat_hp(1);
                state.last_hit_recovery -= 1;
            }
        } else {
            state.last_hit_updates = 0;
        }
    }

    /// Recover from current conditions and activity; display state is output only.
    pub(crate) fn advance_common_ex_recovery(
        &mut self,
        index: usize,
        combat: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if !combat {
            return Ok(());
        }
        let id = ActorId(index as u8);
        let traits = self.actors[index].equipment.recovery.common;
        let state = &mut self.runtime[index].recovery;
        state.self_cure_updates = if traits.self_cure {
            state.self_cure_updates + 1
        } else {
            0
        };
        if state.self_cure_updates == SELF_CURE_INTERVAL {
            state.self_cure_updates = 0;
            let ailments = self.actors[index].conditions.effective();
            // Thawing and condition removal do not depend on the notice or tint.
            self.apply_cure(id, crate::conditions::Cure::Physical, cues);
            if crate::conditions::Cure::Physical.eligible_base(ailments) {
                cues.push(Cue::SelfCured { actor: id });
                let actor = &self.actors[index];
                cues.push(Cue::ExSkillLabel {
                    actor: id,
                    position: std::array::from_fn(|axis| {
                        actor.position[axis] + actor.body.center_offset[axis] * 2.
                    }),
                });
            }
        }
        let actor = &self.actors[index];
        let idle = actor.available()
            && !actor.guard.active
            && actor.guard.recovery == 0
            && actor.movement.locomotion == crate::Locomotion::Idle
            && matches!(self.runtime[index].task(), ActorTask::None);
        let clock = &mut self.runtime[index].recovery.idle_updates;
        if !idle {
            *clock = 0;
            return Ok(());
        }
        *clock += 1;
        if *clock == IDLE_RECOVERY_INTERVAL {
            *clock = 0;
            for (enabled, hp, tp) in [
                (traits.idle_hp_tp, true, true),
                (traits.idle_hp, true, false),
                (traits.idle_tp, false, true),
            ] {
                if enabled {
                    self.common_idle_recovery(id, hp, tp, cues);
                }
            }
        }
        Ok(())
    }

    fn common_idle_recovery(&mut self, id: ActorId, hp: bool, tp: bool, cues: &mut Vec<Cue>) {
        let actor = &mut self.actors[id.index()];
        if hp {
            let before = actor.hp;
            let (value, nominal) = actor.recovered_hp(1);
            actor.hp = value;
            cues.push(Cue::Recovered {
                kind: crate::RecoveryKind::Hp,
                actor: id,
                nominal,
                applied: value - before,
            });
        }
        if tp {
            let (value, nominal) = actor.recovered_tp(1);
            cues.push(Cue::Recovered {
                actor: id,
                kind: RecoveryKind::Tp,
                nominal,
                applied: i32::from(value) - i32::from(actor.tp),
            });
            actor.tp = value;
        }
    }
}

#[cfg(test)]
mod tests;
