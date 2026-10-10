use crate::{ActorId, Battle, Cue, RecoveryKind};

impl Battle {
    pub(crate) fn recover_after_kill(&mut self, owner: ActorId, cues: &mut Vec<Cue>) {
        use crate::conditions::{Condition, ConditionSet};
        let actor = &mut self.actors[owner.index()];
        let conditions = actor.conditions.effective();
        if !conditions.intersects(ConditionSet::of(&[
            Condition::KillHpRecovery,
            Condition::KillTpRecovery,
        ])) {
            return;
        }
        if conditions.contains(Condition::KillHpRecovery) {
            let before = actor.hp;
            let (hp, nominal) = actor.recovered_hp(10);
            actor.hp = hp;
            cues.push(Cue::Recovered {
                kind: RecoveryKind::Hp,
                actor: owner,
                nominal,
                applied: hp - before,
            });
        }
        if conditions.contains(Condition::KillTpRecovery) {
            let (tp, nominal) = actor.recovered_tp(5);
            cues.push(Cue::Recovered {
                actor: owner,
                kind: RecoveryKind::Tp,
                nominal,
                applied: i32::from(tp) - i32::from(actor.tp),
            });
            actor.tp = tp;
        }
    }
}
