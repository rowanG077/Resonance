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
        if tick.regenerate_hp {
            let before = actor.hp;
            let (value, nominal) = actor.recovered_hp(i32::from(percentages.hp_percent));
            actor.hp = value;
            cues.push(Cue::Recovered {
                kind: crate::RecoveryKind::Hp,
                actor: ActorId(index as u8),
                nominal,
                applied: value - before,
            });
        }
        if tick.regenerate_tp {
            let (value, nominal) = actor.recovered_tp(i16::from(percentages.tp_percent));
            cues.push(Cue::Recovered {
                actor: ActorId(index as u8),
                kind: RecoveryKind::Tp,
                nominal,
                applied: i32::from(value) - i32::from(actor.tp),
            });
            actor.tp = value;
        }
        if tick.poison_percent != 0 {
            cues.push(Cue::PoisonPulse {
                actor: ActorId(index as u8),
            });
        }
    }
}
