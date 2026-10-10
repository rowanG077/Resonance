//! Combat statistics and grade accounting.
use crate::conditions::{ConditionSet, PHYSICAL_AILMENTS};
use crate::{ActorId, Battle, BattlePhase, PreparedBattle, Side};
use anyhow::{Result, ensure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    pub assist_commands: u8,
    pub escape_cancellations: u8,
    pub ordinary_escape: Option<ActorId>,
    /// Advances through the ending until results stop updates.
    pub elapsed_ticks: u32,
    /// Combat time, capped at the 99:59 display limit.
    pub combat_ticks: u32,
    pub maximum_combo: u16,
    pub maximum_combo_damage: u32,
    pub last_party_killer: Option<ActorId>,
    /// Accepted party contact, including guards, avoidance and absorption.
    pub party_was_hit: bool,
    /// Whether Magic Lens was actually used.
    pub enemy_was_scanned: bool,
    pub party_item_effect_used: bool,
    /// Actor slots, not character IDs. The session maps these on persistence.
    pub kills: Vec<u16>,
    pub deaths: Vec<u8>,
    pub items: Vec<u8>,
    /// Ordered successful learning events, separate from current membership and counts.
    pub technique_acquisitions: Vec<crate::learning::TechniqueAcquisition>,
    /// Most distinct normal attacks chained into a technique by each actor.
    pub normal_variety: Vec<u8>,
    grade: i16,
    rank: u8,
    finalized: bool,
}

impl Ledger {
    pub(crate) fn new(actors: usize, rank: u8) -> Self {
        Self {
            assist_commands: 0,
            escape_cancellations: 0,
            ordinary_escape: None,
            elapsed_ticks: 0,
            combat_ticks: 0,
            maximum_combo: 1,
            maximum_combo_damage: 0,
            last_party_killer: None,
            party_was_hit: false,
            enemy_was_scanned: false,
            party_item_effect_used: false,
            kills: vec![0; actors],
            deaths: vec![0; actors],
            items: vec![0; actors],
            technique_acquisitions: Vec::new(),
            normal_variety: vec![0; actors],
            grade: 0,
            rank,
            finalized: false,
        }
    }

    pub fn grade(&self) -> i16 {
        self.grade
    }

    pub(crate) fn item_effect(&mut self) {
        self.party_item_effect_used = true;
        self.adjust(-5);
    }

    pub(crate) fn item_consumed(&mut self, actor: ActorId) {
        self.items[actor.index()] = self.items[actor.index()].saturating_add(1).min(250);
    }

    pub(crate) fn adjust(&mut self, amount: i64) {
        let limit = (i64::from(self.rank) + 1) * 500;
        self.grade = i64::from(self.grade)
            .saturating_add(amount)
            .clamp(-limit, limit) as i16;
    }

    pub(crate) fn advance(&mut self, combat: bool) {
        self.elapsed_ticks = self.elapsed_ticks.saturating_add(1);
        if combat {
            self.combat_ticks = self.combat_ticks.saturating_add(1).min(359_999);
        }
    }

    pub(crate) fn death(&mut self, actor: ActorId, first_party_slot: bool) {
        self.deaths[actor.index()] = self.deaths[actor.index()].saturating_add(1).min(250);
        self.adjust(if first_party_slot { -100 } else { -50 });
    }

    pub(crate) fn record_combo_damage(&mut self, damage: i32) {
        self.maximum_combo_damage = self.maximum_combo_damage.max(damage.max(0) as u32);
    }

    pub(crate) fn combo_contact(&mut self, victim: Side, hits: i32) {
        match victim {
            Side::Enemy if hits % 5 == 0 => self.adjust(2),
            Side::Party => self.adjust(if hits % 5 == 0 { -10 } else { -1 }),
            Side::Enemy => {}
        }
    }

    /// Kills award credit; grade comes from combo contacts.
    pub(crate) fn kill(&mut self, owner: ActorId) {
        self.last_party_killer = Some(owner);
        self.kills[owner.index()] = self.kills[owner.index()].saturating_add(1).min(5_000);
    }
}

impl PreparedBattle {
    pub fn with_grade_rank(mut self, rank: u8) -> Result<Self> {
        ensure!(rank <= 2, "invalid battle grade rank");
        self.resources.grade_rank = rank;
        Ok(self)
    }
}

impl Battle {
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Record actual party item use.
    pub fn record_item_use(&mut self, actor: ActorId) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat && self.actor(actor)?.side == Side::Party,
            "item use outside combat"
        );
        self.ledger.item_effect();
        self.ledger.item_consumed(actor);
        Ok(())
    }

    /// Record the completed Magic Lens effect, after the item executor scans
    /// its enemy. This does not perform or authorize an unsupported item action.
    pub fn record_enemy_scan(&mut self, actor: ActorId, target: ActorId) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat
                && self.actor(actor)?.side == Side::Party
                && self.actor(actor)?.available()
                && self.actor(target)?.side == Side::Enemy,
            "enemy scan outside an active party item dispatch"
        );
        self.ledger.enemy_was_scanned = true;
        Ok(())
    }

    /// Construct results before experience growth and recovery, preserving party and reward-
    /// eligible enemy order. New Game Plus grade modes are handled separately.
    pub fn finalize_grade(
        &mut self,
        party_conditions: &[ConditionSet],
        enemy_grades: &[i16],
        level_difference: i8,
    ) -> Result<i16> {
        ensure!(
            self.phase() == BattlePhase::Results
                && self.terminal.result == Some(crate::BattleResult::Victory)
                && !self.ledger.finalized
                && (-8..=8).contains(&level_difference)
                && party_conditions.len()
                    == self.actors.iter().filter(|a| a.side == Side::Party).count(),
            "invalid or repeated result grade construction"
        );
        let ledger = &mut self.ledger;
        ledger.adjust(0);
        match ledger.elapsed_ticks {
            0..=300 => ledger.adjust(100),
            301..=900 => ledger.adjust(50),
            2400.. => ledger.adjust(0),
            _ => {}
        }
        for (slot, (actor, &conditions)) in self
            .actors
            .iter()
            .filter(|a| a.side == Side::Party)
            .zip(party_conditions)
            .enumerate()
        {
            if actor.hp >= actor.equipment.max_hp {
                ledger.adjust(25);
            }
            if actor.tp >= actor.equipment.max_tp {
                ledger.adjust(25);
            }
            if !actor.available() {
                ledger.adjust(-25);
            }
            if conditions.intersects(PHYSICAL_AILMENTS) {
                ledger.adjust(-50);
            }
            if slot == 0 && !actor.available() {
                ledger.adjust(-50);
                break;
            }
        }
        for &grade in enemy_grades {
            ledger.adjust(i64::from(grade));
        }
        if level_difference > 2 {
            let percent = (100 - 10 * (i32::from(level_difference) - 2)).max(50);
            ledger.grade = (i32::from(ledger.grade) * (percent >> 1) / 100) as i16;
        }
        if ledger.grade > 0 {
            ledger.grade *= 2;
        }
        ledger.finalized = true;
        Ok(ledger.grade)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn victory(mut party: Vec<crate::Actor>) -> Battle {
        let mut enemy = crate::tests::actor(Side::Enemy);
        enemy.hp = 0;
        party.push(enemy);
        let mut battle = PreparedBattle::new(
            (party)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        battle.retire_combat().unwrap();
        battle
    }

    #[test]
    fn each_grade_change_clamps_before_the_next_change() {
        let mut ledger = Ledger::new(2, 0);
        ledger.adjust(490);
        ledger.adjust(100);
        ledger.adjust(-100);
        assert_eq!(ledger.grade(), 400);
        ledger.adjust(-1_000);
        ledger.adjust(25);
        assert_eq!(ledger.grade(), -475);
        ledger.adjust(i64::MAX);
        assert_eq!(ledger.grade(), 500);
        ledger.adjust(i64::MAX);
        assert_eq!(ledger.grade(), 500);
        ledger.adjust(i64::MIN);
        assert_eq!(ledger.grade(), -500);
    }

    #[test]
    fn clocks_and_actor_statistics_stay_within_gameplay_bounds() {
        let mut ledger = Ledger::new(2, 2);
        assert_eq!(ledger.maximum_combo, 1);
        ledger.combat_ticks = 359_999;
        ledger.advance(true);
        ledger.advance(false);
        assert_eq!((ledger.elapsed_ticks, ledger.combat_ticks), (2, 359_999));
        for _ in 0..251 {
            ledger.death(ActorId(0), true);
        }
        for _ in 0..5_001 {
            ledger.kill(ActorId(1));
        }
        assert_eq!(ledger.deaths, [250, 0]);
        assert_eq!(ledger.kills, [0, 5_000]);
        assert_eq!(ledger.last_party_killer, Some(ActorId(1)));
        assert_eq!(ledger.grade(), -1_500);
        ledger.elapsed_ticks = u32::MAX;
        ledger.advance(true);
        assert_eq!(ledger.elapsed_ticks, u32::MAX);
        ledger.record_combo_damage(-1);
        assert_eq!(ledger.maximum_combo_damage, 0);
        ledger.record_combo_damage(i32::MAX);
        ledger.record_combo_damage(-1);
        assert_eq!(ledger.maximum_combo_damage, i32::MAX as u32);
    }

    #[test]
    fn result_grade_preserves_order_and_can_only_be_constructed_once() {
        let mut actor = crate::tests::actor(Side::Party);
        actor.hp = actor.equipment.max_hp;
        let mut battle = victory(vec![actor]);
        battle.ledger.grade = 490;
        battle.ledger.elapsed_ticks = 300;
        assert_eq!(
            battle
                .finalize_grade(&[ConditionSet::EMPTY], &[-100], 0)
                .unwrap(),
            800
        );
        assert!(
            battle
                .finalize_grade(&[ConditionSet::EMPTY], &[-100], 0)
                .is_err()
        );
        assert_eq!(battle.ledger.grade(), 800);
    }

    #[test]
    fn result_grade_uses_ending_clock_leader_break_and_signed_level_penalty() {
        let mut leader = crate::tests::actor(Side::Party);
        leader.hp = 0;
        let mut ally = crate::tests::actor(Side::Party);
        ally.hp = ally.equipment.max_hp;
        let mut battle = victory(vec![leader, ally]);
        battle.ledger.elapsed_ticks = 901;
        battle.ledger.combat_ticks = 200;
        // No time bonus, +25 TP, -25 KO, and -50 leader. The level difference scales signed
        // grade to 45%, truncating toward zero.
        assert_eq!(
            battle
                .finalize_grade(&[ConditionSet::EMPTY; 2], &[], 3)
                .unwrap(),
            -22
        );
    }
}
