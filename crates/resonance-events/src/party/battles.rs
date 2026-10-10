//! Persistent battle records. Transient actors and result tasks never enter saves.
use super::Party;
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::{ExTendency, MenuData};

/// Combine equipment and single EX tendency before the final balance clamp.
/// Compound EX skills do not contribute tendency.
fn technique_drift(member: &super::Member, data: &MenuData) -> Result<i32> {
    let mut drift = 0i32;
    for &id in &member.equipment {
        if id != 0 {
            drift += i32::from(
                data.items
                    .get(usize::from(id))
                    .context("missing battle equipment")?
                    .properties
                    .technique_drift,
            );
        }
    }
    for &id in &member.ex_skills {
        if id != 0 {
            drift += match data
                .ex_skills
                .skills
                .get(&id)
                .context("missing battle EX skill")?
                .tendency
            {
                Some(ExTendency::Technical) => -1,
                Some(ExTendency::Strike) => 1,
                None => 0,
            };
        }
    }
    Ok(drift)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BattleStatistics {
    /// No previous formation until the first encounter starts.
    pub previous_formation: Option<u16>,
    pub battle_gel_used: bool,
    /// Whether Lloyd entered battle or changed weapons with a non-Wooden Blade.
    pub lloyd_non_wooden_blade_used: bool,
    pub ordinary_escape_used: bool,
    /// Ordinary escapes led by Sheena, capped at 50.
    pub sheena_escapes: u8,
    pub total: u16,
    /// Victories at Hard or Mania difficulty.
    pub hard_victories: u16,
    pub escaped: u32,
    /// Duration of the most recently completed battle, not a cumulative clock.
    pub combat_ticks: u32,
    /// Signed hundredths, with a cap only on the positive total.
    pub grade: i32,
    pub maximum_combo: u16,
    pub maximum_combo_damage: u32,
    /// One entry per character, independent of current formation order.
    pub participation: [u16; 9],
    pub kills: [u16; 9],
    pub deaths: [u8; 9],
    pub items: [u8; 9],
    /// Accepted victory voice groups, recorded at confirmation.
    pub victory_groups: u64,
}

impl BattleStatistics {
    pub fn count(&self, member: i32) -> std::result::Result<u16, String> {
        match member {
            0 => Ok(self.total),
            1..=9 => Ok(self.participation[(member - 1) as usize]),
            _ => Err("invalid battle history member".into()),
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.total <= 9_999
                && self.hard_victories <= 9_999
                && self.escaped <= 99_999
                && self.sheena_escapes <= 50
                && self.combat_ticks <= 359_999
                && self.grade <= 99_999_999
                && self.participation.iter().all(|&n| n <= 9_999)
                && self.kills.iter().all(|&n| n <= 5_000)
                && self.deaths.iter().all(|&n| n <= 250)
                && self.items.iter().all(|&n| n <= 250),
            "invalid saved battle statistics"
        );
        Ok(())
    }

    /// Returning to Wooden Blade cannot erase a recorded weapon change.
    pub fn observe_lloyd_battle_weapon(&mut self, weapon: u16) {
        if weapon != 135 {
            self.lloyd_non_wooden_blade_used = true;
        }
    }

    /// Record an ordinary escape once, before updating the aggregate total.
    pub fn record_ordinary_escape(&mut self, leader_character: u8) {
        self.ordinary_escape_used = true;
        if leader_character == 5 {
            self.sheena_escapes = self.sheena_escapes.saturating_add(1).min(50);
        }
    }

    pub fn record_escape(&mut self) {
        self.escaped = self.escaped.saturating_add(1).min(99_999);
    }

    pub fn add_grade(&mut self, grade: i16) {
        self.grade = self.grade.saturating_add(i32::from(grade)).min(99_999_999);
    }
}

impl super::Member {
    /// Project the balance once while admitting the battle actor.
    /// Equipment and menu reloads use the activated persistent value.
    pub fn activated_technique_balance(&self, data: &MenuData) -> Result<i8> {
        Ok(
            (i32::from(self.technique_balance) + 2 * technique_drift(self, data)?).clamp(-100, 100)
                as i8,
        )
    }
}

impl Party {
    /// Activate after the entire encounter is prepared.
    /// Keep mutations in the candidate party until the encounter is committed.
    pub fn begin_battle(&mut self, active: &[(u8, i8)]) -> Result<()> {
        ensure!(
            (1..=4).contains(&active.len())
                && active.iter().all(|&(id, balance)| id > 0
                    && usize::from(id) <= self.members.len()
                    && id <= 9
                    && (-100..=100).contains(&balance))
                && active
                    .iter()
                    .enumerate()
                    .all(|(i, (id, _))| !active[..i].iter().any(|(other, _)| id == other)),
            "invalid active battle formation"
        );
        // Record Lloyd's equipment when he joins the encounter, regardless of HP
        // or availability. A reserve character does not update this history.
        if active.iter().any(|&(id, _)| id == 1) {
            self.battles
                .observe_lloyd_battle_weapon(self.members[0].equipment[0]);
        }
        self.battles.total = self.battles.total.saturating_add(1).min(9_999);
        for &(id, balance) in active {
            let count = &mut self.battles.participation[usize::from(id - 1)];
            *count = count.saturating_add(1).min(9_999);
            self.members[usize::from(id - 1)].technique_balance = balance;
        }
        self.cooking.full = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_history_roundtrips_and_requires_current_fields() -> Result<()> {
        let history = BattleStatistics {
            previous_formation: Some(0),
            battle_gel_used: true,
            lloyd_non_wooden_blade_used: true,
            ordinary_escape_used: true,
            sheena_escapes: 50,
            victory_groups: (1 << 5) | (1 << 27) | (1 << 54),
            ..Default::default()
        };
        let encoded = serde_json::to_value(&history)?;
        let restored: BattleStatistics = serde_json::from_value(encoded.clone())?;
        assert_eq!(restored, history);
        restored.validate()?;
        for field in [
            "battle_gel_used",
            "lloyd_non_wooden_blade_used",
            "ordinary_escape_used",
            "sheena_escapes",
        ] {
            let mut incomplete = encoded.clone();
            incomplete.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<BattleStatistics>(incomplete).is_err(),
                "{field}"
            );
        }
        Ok(())
    }

    #[test]
    fn signed_grade_and_escape_keep_their_caps() {
        let mut records = BattleStatistics::default();
        records.add_grade(-150);
        assert_eq!(records.grade, -150);
        records.grade = 99_999_990;
        records.add_grade(100);
        assert_eq!(records.grade, 99_999_999);
        records.grade = i32::MIN;
        records.validate().unwrap();
        records.add_grade(-22);
        assert_eq!(records.grade, i32::MIN);
        records.add_grade(22);
        assert_eq!(records.grade, i32::MIN + 22);
        records.escaped = 99_999;
        records.record_escape();
        assert_eq!(records.escaped, 99_999);
        records.validate().unwrap();
    }

    #[test]
    fn ordinary_leader_history_caps_independently_of_total_escapes() -> Result<()> {
        let mut history = BattleStatistics {
            sheena_escapes: 49,
            ..Default::default()
        };
        history.record_ordinary_escape(1);
        assert_eq!(history.sheena_escapes, 49);
        history.record_ordinary_escape(5);
        history.record_ordinary_escape(5);
        assert_eq!(history.sheena_escapes, 50);
        assert!(history.ordinary_escape_used);
        assert_eq!(history.escaped, 0);
        history.record_escape();
        assert_eq!(history.escaped, 1);
        history.validate()?;
        history.sheena_escapes = 51;
        assert!(history.validate().is_err());
        Ok(())
    }

    #[test]
    fn weapon_history_remembers_any_non_wooden_blade() {
        let mut history = BattleStatistics::default();
        history.observe_lloyd_battle_weapon(135);
        assert!(!history.lloyd_non_wooden_blade_used);
        history.observe_lloyd_battle_weapon(136);
        history.observe_lloyd_battle_weapon(135);
        assert!(history.lloyd_non_wooden_blade_used);
    }
}
