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
        let actor = &mut self.actors[owner.index()];
        let bonus = match kind {
            RecoveryKind::Hp if actor.equipment.recovery.life_drain => 50,
            RecoveryKind::Tp if actor.equipment.recovery.spirit_drain => 5,
            _ => 0,
        };
        let nominal = (i64::from(damage) * i64::from(percent) / 100 + bonus)
            .clamp(1, i64::from(i32::MAX)) as i32;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PreparedBattle, Side, tests::actor};

    #[test]
    fn seal_bonuses_are_flat_match_the_drained_vital_and_require_a_hit() -> Result<()> {
        for (kind, percent, hp_bonus, tp_bonus, expected) in [
            (RecoveryKind::Hp, 10, false, false, 10),
            (RecoveryKind::Hp, 10, true, false, 60),
            (RecoveryKind::Hp, 10, false, true, 10),
            (RecoveryKind::Tp, 3, false, false, 3),
            (RecoveryKind::Tp, 3, false, true, 8),
            (RecoveryKind::Tp, 3, true, false, 3),
        ] {
            let mut owner = actor(Side::Party);
            owner.hp = 1;
            owner.tp = 0;
            owner.equipment.max_hp = 100;
            owner.equipment.max_tp = 100;
            owner.equipment.recovery.life_drain = hp_bonus;
            owner.equipment.recovery.spirit_drain = tp_bonus;
            let mut battle =
                PreparedBattle::new(vec![(owner, Default::default())], Default::default(), 1)?
                    .finish()?;
            let mut cues = vec![];
            battle.drain_contact(ActorId(0), 0, kind, percent, &mut cues)?;
            assert!(cues.is_empty());
            battle.drain_contact(ActorId(0), 100, kind, percent, &mut cues)?;
            let recovered = match kind {
                RecoveryKind::Hp => battle.actors[0].hp - 1,
                RecoveryKind::Tp => i32::from(battle.actors[0].tp),
            };
            assert_eq!(recovered, expected);
        }
        Ok(())
    }
}
