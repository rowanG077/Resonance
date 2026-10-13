//! Shared actor timers run during entry and ordinary updates.
use crate::{Battle, Cue};
use anyhow::Result;

impl Battle {
    pub(crate) fn advance_actor_common(&mut self, index: usize, cues: &mut Vec<Cue>) -> Result<()> {
        self.restore_event_boss_hp(crate::ActorId(index as u8));
        if !self.unison.staged() {
            self.advance_overlimit(index, cues);
        }
        // Condition clocks start with combat.
        let combat = self.phase() == crate::BattlePhase::Combat;
        self.advance_common_flat_recovery(index, combat);
        let actor = &mut self.actors[index];
        actor.secondary_wind.advance(&mut self.random);
        self.runtime[index].body_bypass_ticks =
            self.runtime[index].body_bypass_ticks.saturating_sub(1);
        actor.hit_stop = actor.hit_stop.saturating_sub(1);
        actor.guard.recent_hurt_ticks = actor.guard.recent_hurt_ticks.saturating_sub(1);
        actor.reaction.hit_shake_ticks = actor.reaction.hit_shake_ticks.saturating_sub(1);
        actor.reaction.stagger.window = actor.reaction.stagger.window.saturating_sub(1);
        actor.movement.advance_hover_phase();
        self.advance_condition_callbacks(index, combat, cues);
        let actor = &mut self.actors[index];
        actor.reaction.protection.step();
        if let Err(error) = self.advance_common_ex_recovery(index, combat, cues) {
            self.diagnostics
                .report("battle common EX recovery", error)?;
            self.diagnostic = true;
        }
        self.actors[index].control_ex_state.advance_charge();
        self.advance_enemy_behavior(index);
        Ok(())
    }
}
