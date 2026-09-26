//! Ordered combat accounting (ACE8, 1C40, 1FB2C, 28DF4 and 57718).
use crate::{ActorId, Battle, BattlePhase, PreparedBattle, Side};
use anyhow::{Result, ensure};

/// A8A8 eligibility at the source event, before result construction. Character
/// IDs belong to the original party catalogue; actor bits belong to this battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleEvent {
    pub character: u8,
    pub title: u8,
    pub eligible_actors: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// 15700: advances through the ending, until the result owner stops visits.
    pub elapsed_ticks: u32,
    /// 15704: state 3 visits only, capped at the original 99:59 display limit.
    pub combat_ticks: u32,
    pub maximum_combo: u16,
    pub maximum_combo_damage: u32,
    pub last_party_killer: Option<ActorId>,
    /// Accepted party contact, including guards, avoidance and absorption.
    pub party_was_hit: bool,
    /// Actor slots, not character IDs. The session maps these on persistence.
    pub kills: Vec<u16>,
    pub deaths: Vec<u8>,
    pub items: Vec<u8>,
    /// Ordered title attempts. The session owns learned titles and the first
    /// successful pending award, so a final combo maximum cannot replace these.
    pub title_events: Vec<TitleEvent>,
    grade: i16,
    rank: u8,
    finalized: bool,
}

impl Ledger {
    pub(crate) fn new(actors: usize, rank: u8) -> Self {
        Self {
            elapsed_ticks: 0,
            combat_ticks: 0,
            maximum_combo: 1,
            maximum_combo_damage: 0,
            last_party_killer: None,
            party_was_hit: false,
            kills: vec![0; actors],
            deaths: vec![0; actors],
            items: vec![0; actors],
            title_events: Vec::new(),
            grade: 0,
            rank,
            finalized: false,
        }
    }

    pub fn grade(&self) -> i16 {
        self.grade
    }

    fn title(&mut self, mut event: TitleEvent) {
        // An actor's first eligibility for each title is sufficient: learned
        // titles never disappear during combat and pending awards do not clear.
        // This bounds storage by title count times roster size, even in a long
        // battle, without moving a later eligible actor ahead of another title.
        for previous in &self.title_events {
            if previous.character == event.character && previous.title == event.title {
                event.eligible_actors &= !previous.eligible_actors;
            }
        }
        if event.eligible_actors != 0 {
            self.title_events.push(event);
        }
    }

    /// ACE8 narrows to signed 16 bits before clamping every individual change.
    fn adjust(&mut self, amount: i32) {
        let limit = (i16::from(self.rank) + 1) * 500;
        self.grade = (i32::from(self.grade).wrapping_add(amount) as i16).clamp(-limit, limit);
    }

    pub(crate) fn advance(&mut self, combat: bool) {
        self.elapsed_ticks = self.elapsed_ticks.wrapping_add(1);
        if combat {
            self.combat_ticks = self.combat_ticks.saturating_add(1).min(359_999);
        }
    }

    pub(crate) fn death(&mut self, actor: ActorId, first_party_slot: bool) {
        self.deaths[actor.index()] = self.deaths[actor.index()].saturating_add(1).min(250);
        self.adjust(if first_party_slot { -100 } else { -50 });
    }

    pub(crate) fn record_combo_damage(&mut self, damage: i32) {
        self.maximum_combo_damage = self.maximum_combo_damage.max(damage as u32);
    }

    pub(crate) fn kill(&mut self, owner: ActorId, remaining_combo: i32) {
        self.last_party_killer = Some(owner);
        self.kills[owner.index()] = self.kills[owner.index()].saturating_add(1).min(5_000);
        self.adjust(0);
        self.adjust(((i32::from(self.rank) + 1) * remaining_combo.wrapping_shl(1)) as i16 as i32);
    }
}

impl PreparedBattle {
    pub fn with_grade_rank(mut self, rank: u8) -> Result<Self> {
        ensure!(rank <= 2, "invalid battle grade rank");
        self.grade_rank = rank;
        Ok(self)
    }
}

impl Battle {
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    pub(crate) fn record_combo_titles(&mut self, hits: i32) {
        // 3C958..3C9D4 calls A8A8 in this order for any ordinary combo target.
        // Availability is sampled before the later 28DF4 death initializer.
        let eligible_actors = self
            .actors
            .iter()
            .enumerate()
            .fold(0, |mask, (index, actor)| {
                mask | if actor.side == Side::Party && actor.available() {
                    1 << index
                } else {
                    0
                }
            });
        for (threshold, title) in [(10, 18), (30, 19), (60, 20), (100, 21)] {
            if hits >= threshold {
                self.ledger.title(TitleEvent {
                    character: 1,
                    title,
                    eligible_actors,
                });
            }
        }
    }

    pub(crate) fn record_normal_title(&mut self, actor: ActorId, selection: u8) {
        // 3DF34 updates 10B1 at the actual normal initializer. The finisher
        // selector4 contributes no bit; the two aerial selectors share one.
        self.actors[actor.index()].reaction.normal_history |= match selection {
            0 => 1,
            1 => 2,
            3 => 4,
            2 => 8,
            5.. => 16,
            _ => 0,
        };
    }

    pub(crate) fn record_technique_title(&mut self, actor: ActorId) {
        let body = &self.actors[actor.index()];
        if body.side == Side::Party
            && body.available()
            && matches!(
                body.control,
                crate::Control::Manual | crate::Control::SemiAuto
            )
            && (body.reaction.normal_history & 31).count_ones() >= 3
        {
            // 1E12C's enabled-arte tail, followed by A8A8's manual-only gate.
            // The game maps the performer bit to character1; another party
            // member performing the same sequence cannot award Lloyd a title.
            self.ledger.title(TitleEvent {
                character: 1,
                title: 22,
                eligible_actors: 1 << actor.index(),
            });
        }
    }

    /// The item executor calls this after an actual party item dispatch (5A4BC).
    pub fn record_item_use(&mut self, actor: ActorId) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat && self.actor(actor)?.side == Side::Party,
            "item use outside combat"
        );
        self.ledger.adjust(-5);
        self.ledger.items[actor.index()] =
            self.ledger.items[actor.index()].saturating_add(1).min(250);
        Ok(())
    }

    /// Ordinary 57718 result construction, before EXP growth or TP recovery.
    /// Conditions use the original 64-bit battle mask, in active party order.
    /// Enemy bonuses contain only reward-admitted instances, in roster order.
    /// NG+ no-grade/double-grade modes must be rejected by the ordinary adapter.
    pub fn finalize_grade(
        &mut self,
        party_conditions: &[u64],
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
            if actor.hp >= actor.max_hp {
                ledger.adjust(25);
            }
            if actor.tp >= actor.max_tp {
                ledger.adjust(25);
            }
            if !actor.available() {
                ledger.adjust(-25);
            }
            if conditions & 0xceb != 0 {
                ledger.adjust(-50);
            }
            if slot == 0 && !actor.available() {
                ledger.adjust(-50);
                break;
            }
        }
        for &grade in enemy_grades {
            ledger.adjust(i32::from(grade));
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
    use std::sync::Arc;

    fn victory(mut party: Vec<crate::Actor>) -> Battle {
        let mut enemy = crate::tests::actor(Side::Enemy);
        enemy.hp = 0;
        party.push(enemy);
        let mut battle = Battle::new(Arc::new(
            PreparedBattle::new(party, vec![], 1, vec![], vec![]).unwrap(),
        ));
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
        ledger.adjust(32_767);
        assert_eq!(ledger.grade(), 500);
        ledger.adjust(32_767);
        assert_eq!(ledger.grade(), -500);
    }

    #[test]
    fn clocks_and_actor_statistics_keep_source_limits() {
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
            ledger.kill(ActorId(1), 0);
        }
        assert_eq!(ledger.deaths, [250, 0]);
        assert_eq!(ledger.kills, [0, 5_000]);
        assert_eq!(ledger.last_party_killer, Some(ActorId(1)));
        assert_eq!(ledger.grade(), -1_500);
    }

    #[test]
    fn result_grade_preserves_order_and_can_only_be_constructed_once() {
        let mut actor = crate::tests::actor(Side::Party);
        actor.hp = actor.max_hp;
        let mut battle = victory(vec![actor]);
        battle.ledger.grade = 490;
        battle.ledger.elapsed_ticks = 300;
        assert_eq!(battle.finalize_grade(&[0], &[-100], 0).unwrap(), 800);
        assert!(battle.finalize_grade(&[0], &[-100], 0).is_err());
        assert_eq!(battle.ledger.grade(), 800);
    }

    #[test]
    fn result_grade_uses_ending_clock_leader_break_and_signed_level_penalty() {
        let mut leader = crate::tests::actor(Side::Party);
        leader.hp = 0;
        let mut ally = crate::tests::actor(Side::Party);
        ally.hp = ally.max_hp;
        let mut battle = victory(vec![leader, ally]);
        battle.ledger.elapsed_ticks = 901;
        battle.ledger.combat_ticks = 200;
        // No time bonus, +25 TP, -25 KO, -50 leader, then the source loop exits.
        // Level difference 3 multiplies signed grade by45%, truncating to zero.
        assert_eq!(battle.finalize_grade(&[0, 0], &[], 3).unwrap(), -22);
    }

    #[test]
    fn title_attempts_preserve_new_actor_eligibility_in_source_order() {
        let mut battle = Battle::new(Arc::new(
            PreparedBattle::new(
                vec![
                    crate::tests::actor(Side::Party),
                    crate::tests::actor(Side::Party),
                    crate::tests::actor(Side::Enemy),
                ],
                vec![],
                1,
                vec![],
                vec![],
            )
            .unwrap(),
        ));
        battle.record_combo_titles(9);
        assert!(battle.ledger.title_events.is_empty());
        battle.record_combo_titles(10);
        battle.actors[0].availability = crate::ActorAvailability::Dead;
        battle.record_combo_titles(30);
        battle.actors[0].availability = crate::ActorAvailability::Active;
        battle.record_combo_titles(30);
        assert_eq!(
            battle.ledger.title_events,
            [
                TitleEvent {
                    character: 1,
                    title: 18,
                    eligible_actors: 3
                },
                TitleEvent {
                    character: 1,
                    title: 19,
                    eligible_actors: 2
                },
                TitleEvent {
                    character: 1,
                    title: 19,
                    eligible_actors: 1
                },
            ]
        );
        for _ in 0..1000 {
            battle.record_combo_titles(100);
        }
        assert_eq!(
            &battle.ledger.title_events[3..],
            [
                TitleEvent {
                    character: 1,
                    title: 20,
                    eligible_actors: 3
                },
                TitleEvent {
                    character: 1,
                    title: 21,
                    eligible_actors: 3
                },
            ]
        );
    }

    #[test]
    fn tetra_slash_needs_three_distinct_normal_bits_and_available_manual_performer() {
        let mut actor = crate::tests::actor(Side::Party);
        actor.control = crate::Control::SemiAuto;
        let mut battle = Battle::new(Arc::new(
            PreparedBattle::new(
                vec![actor, crate::tests::actor(Side::Enemy)],
                vec![],
                1,
                vec![],
                vec![],
            )
            .unwrap(),
        ));
        for selection in [0, 0, 4, 5, 6] {
            battle.record_normal_title(ActorId(0), selection);
        }
        assert_eq!(battle.actors[0].reaction.normal_history, 17);
        battle.record_technique_title(ActorId(0));
        assert!(battle.ledger.title_events.is_empty());
        battle.record_normal_title(ActorId(0), 2);
        battle.actors[0].control = crate::Control::Auto;
        battle.record_technique_title(ActorId(0));
        battle.actors[0].control = crate::Control::SemiAuto;
        battle.actors[0].availability = crate::ActorAvailability::Petrified;
        battle.record_technique_title(ActorId(0));
        assert!(battle.ledger.title_events.is_empty());
        battle.actors[0].availability = crate::ActorAvailability::Active;
        battle.record_technique_title(ActorId(0));
        assert_eq!(
            battle.ledger.title_events,
            [TitleEvent {
                character: 1,
                title: 22,
                eligible_actors: 1
            }]
        );
        crate::reaction::recover(
            &mut battle.actors[0],
            &mut battle.random,
            &mut battle.ledger,
        );
        assert_eq!(battle.actors[0].reaction.normal_history, 0);
    }
}
