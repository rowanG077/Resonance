//! Qualifying enemy defeats raise item-drop odds for the whole battle.
use crate::{Activity, ActorId, Battle, BattlePhase, Cue, Side};

const HIGH_SPEED_TICKS: u64 = 180;
const CONSECUTIVE_KILL_TICKS: u32 = 120;
const AERIAL_HEIGHT: f32 = 100.;
const AERIAL_MIN_HITS: u16 = 3;
const COMBO_MILESTONE: u16 = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TechnicalSmash {
    pub combo_hits: u16,
    pub no_damage: bool,
    pub aerial: bool,
    pub summon: bool,
    pub high_speed: bool,
    pub consecutive: bool,
    pub counter: bool,
    pub skill: bool,
}

impl TechnicalSmash {
    pub fn bonus_percent(self) -> u16 {
        self.combo_hits / COMBO_MILESTONE * 2
            + [
                (self.no_damage, 4),
                (self.aerial, 2),
                (self.summon, 5),
                (self.high_speed, 10),
                (self.consecutive, 5),
                (self.counter, 2),
                (self.skill, 1),
            ]
            .into_iter()
            .filter_map(|(earned, percent)| earned.then_some(percent))
            .sum::<u16>()
    }
}

impl Battle {
    /// Call before retiring the victim's combo and action, only for confirmed defeats.
    pub(crate) fn record_technical_smash(
        &mut self,
        target: ActorId,
        skill: bool,
        summon: bool,
        cues: &mut Vec<Cue>,
    ) {
        let actor = &self.actors[target.index()];
        if actor.side != Side::Enemy || self.phase() != BattlePhase::Combat {
            return;
        }
        let hits = actor.reaction.combo_hits.clamp(0, i32::from(u16::MAX)) as u16;
        let bonuses = TechnicalSmash {
            combo_hits: hits / COMBO_MILESTONE * COMBO_MILESTONE,
            no_damage: !self.ledger.party_was_hit,
            aerial: actor.position[1] >= AERIAL_HEIGHT && hits >= AERIAL_MIN_HITS,
            summon,
            high_speed: self.update <= HIGH_SPEED_TICKS,
            consecutive: self.ledger.last_enemy_defeat.is_some_and(|previous| {
                self.ledger.elapsed_ticks.saturating_sub(previous) < CONSECUTIVE_KILL_TICKS
            }),
            counter: self.activity(target) == Activity::Action,
            skill,
        };
        self.ledger.last_enemy_defeat = Some(self.ledger.elapsed_ticks);
        let percent = bonuses.bonus_percent();
        if percent != 0 {
            self.ledger.technical_smash_bonus =
                (u16::from(self.ledger.technical_smash_bonus) + percent).min(100) as u8;
            cues.push(Cue::TechnicalSmash {
                bonuses,
                total_percent: self.ledger.technical_smash_bonus,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_bonuses_use_the_final_combo_and_bounded_time_windows() -> anyhow::Result<()> {
        for (tick, gap, hits, height, expected) in [
            (180, 119, 5, 100., 19),
            (181, 120, 4, 100., 2),
            (181, 120, 2, 100., 0),
            (181, 120, 10, 99., 4),
        ] {
            let mut battle =
                crate::tests::prepared(vec![crate::tests::actor(Side::Enemy)], 1).finish()?;
            battle.update = tick;
            battle.ledger.elapsed_ticks = 1000;
            battle.ledger.last_enemy_defeat = Some(1000 - gap);
            battle.ledger.party_was_hit = true;
            battle.actors[0].reaction.combo_hits = hits;
            battle.actors[0].position[1] = height;
            battle.record_technical_smash(ActorId(0), false, false, &mut vec![]);
            assert_eq!(battle.ledger.technical_smash_bonus, expected);
            // A fresh defeat immediately afterwards stacks Continue, Skill, and Summon.
            battle.update = 181;
            battle.actors[0].reaction.combo_hits = 0;
            battle.record_technical_smash(ActorId(0), true, true, &mut vec![]);
            assert_eq!(battle.ledger.technical_smash_bonus, expected + 11);
            battle.ledger.technical_smash_bonus = 99;
            battle.record_technical_smash(ActorId(0), true, true, &mut vec![]);
            assert_eq!(battle.ledger.technical_smash_bonus, 100);
        }
        Ok(())
    }
}
