//! Shared actor timers run during entry and ordinary updates.
use crate::{Battle, Cue};
use anyhow::Result;

impl Battle {
    pub(crate) fn advance_actor_common(&mut self, index: usize, cues: &mut Vec<Cue>) -> Result<()> {
        self.advance_actor_common_after_action(index, false, cues)
    }

    pub(crate) fn advance_actor_common_after_action(
        &mut self,
        index: usize,
        action_advanced: bool,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        self.restore_event_boss_hp(crate::ActorId(index as u8));
        if !self.unison.staged() {
            self.advance_overlimit(index, cues);
        }
        // Condition clocks start with combat.
        let combat = self.phase() == crate::BattlePhase::Combat;
        self.advance_common_flat_recovery(index, combat);
        let actor = &mut self.actors[index];
        actor.secondary_wind.advance(&mut self.random);
        let runtime = &mut self.runtime[index];
        runtime.target_settle_remaining = runtime.target_settle_remaining.saturating_sub(1);
        if runtime.body_bypass_ticks != 0 {
            runtime.body_bypass_ticks -= 1;
            if runtime.body_bypass_ticks == 0 {
                runtime.body_bypass = false;
            }
        }
        actor.hit_stop = actor.hit_stop.saturating_sub(1);
        actor.guard.recent_hurt_ticks = actor.guard.recent_hurt_ticks.saturating_sub(1);
        actor.reaction.hit_shake_ticks = actor.reaction.hit_shake_ticks.saturating_sub(1);
        actor.reaction.impact_ticks = actor.reaction.impact_ticks.saturating_sub(1);
        actor.reaction.stagger.window = actor.reaction.stagger.window.saturating_sub(1);
        actor.movement.advance_hover_phase();
        self.advance_condition_callbacks(index, combat, cues);
        let actor = &mut self.actors[index];
        actor.reaction.protection.step();
        self.advance_common_ex_recovery(index, combat, cues);
        self.actors[index].control_ex_state.advance_charge();
        self.advance_enemy_behavior(index, action_advanced)?;
        Ok(())
    }
}
