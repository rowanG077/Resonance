use crate::{ActorId, Battle, Cue, RecoveryKind};

impl Battle {
    pub(crate) fn recover_after_kill(&mut self, owner: ActorId, cues: &mut Vec<Cue>) {
        use crate::conditions::Condition;
        let actor = &mut self.actors[owner.index()];
        let conditions = actor.conditions.effective();
        for (condition, kind, percent) in [
            (Condition::KillHpRecovery, RecoveryKind::Hp, 10),
            (Condition::KillTpRecovery, RecoveryKind::Tp, 5),
        ] {
            if conditions.contains(condition) {
                cues.push(actor.recover_percent(owner, kind, percent, false));
            }
        }
    }
}
