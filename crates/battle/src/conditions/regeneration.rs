use crate::{ActorId, Battle, Cue, RecoveryKind};

impl Battle {
    pub(crate) fn advance_condition_callbacks(
        &mut self,
        index: usize,
        combat: bool,
        cues: &mut Vec<Cue>,
    ) {
        let actor = &self.actors[index];
        if !combat || !actor.available() {
            return;
        }
        let tick = self.actors[index].tick_conditions(combat);
        let actor = &mut self.actors[index];
        let percentages = actor.conditions.gear_regeneration();
        for (enabled, kind, percent) in [
            (tick.regenerate_hp, RecoveryKind::Hp, percentages.hp_percent),
            (tick.regenerate_tp, RecoveryKind::Tp, percentages.tp_percent),
        ] {
            if enabled {
                cues.push(actor.recover_percent(
                    ActorId(index as u8),
                    kind,
                    i32::from(percent),
                    false,
                ));
            }
        }
        if tick.poison_percent != 0 {
            cues.push(Cue::PoisonPulse {
                actor: ActorId(index as u8),
            });
        }
    }
}
