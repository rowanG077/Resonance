use crate::{ActorId, Battle, Cue, RecoveryKind};
use anyhow::Result;

impl Battle {
    pub(crate) fn drain_contact(
        &mut self,
        owner: ActorId,
        damage: i32,
        kind: RecoveryKind,
        percent: u8,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if damage <= 0 || self.actor(owner)?.hp <= 0 {
            return Ok(());
        }
        let nominal = ((i64::from(damage) * i64::from(percent) / 100) as i32).max(1);
        let actor = &mut self.actors[owner.index()];
        let applied = match kind {
            RecoveryKind::Hp => {
                let before = actor.hp;
                actor.recover_flat_hp(nominal);
                actor.hp - before
            }
            RecoveryKind::Tp => {
                let before = actor.tp;
                actor.recover_flat_tp(nominal);
                i32::from(actor.tp) - i32::from(before)
            }
        };
        cues.push(Cue::Recovered {
            floating: true,
            actor: owner,
            kind,
            nominal,
            applied,
        });
        cues.push(Cue::EffectNotice {
            actor: owner,
            text: match kind {
                RecoveryKind::Hp => "DRAIN HP",
                RecoveryKind::Tp => "DRAIN TP",
            }
            .into(),
            duration: 90,
        });
        Ok(())
    }
}
