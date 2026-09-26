//! Persistent battle records. Transient actors and result tasks never enter saves.
use super::Party;
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::{ExTendency, MenuData};

/// 800EA428 adds raw equipped item bytes and equipped EX halfwords, then stores
/// one signed byte. Published compound EX rows all have zero tendency.
fn technique_drift(member: &super::Member, data: &MenuData) -> Result<i8> {
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
    Ok(drift as i8)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BattleStatistics {
    /// Save block+1e58, written after entry voice selection. Missing legacy
    /// metadata remains unknown rather than inventing a previous encounter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_formation: Option<u16>,
    pub total: u16,
    /// Victories at Hard or Mania difficulty (57718, save1e56).
    pub hard_victories: u16,
    pub escaped: u32,
    /// Duration of the most recently completed battle, not a cumulative clock.
    pub combat_ticks: u32,
    /// Signed hundredths. The original caps only the positive total.
    pub grade: i32,
    pub maximum_combo: u16,
    pub maximum_combo_damage: u32,
    /// One entry per character, independent of current formation order.
    pub participation: [u16; 9],
    pub kills: [u16; 9],
    pub deaths: [u8; 9],
    pub items: [u8; 9],
    /// Accepted victory voice groups (80AC0/80A80); set at confirmation.
    pub victory_groups: u32,
}

impl BattleStatistics {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.total <= 9_999
                && self.hard_victories <= 9_999
                && self.escaped <= 99_999
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

    pub fn record_escape(&mut self) {
        self.escaped = self.escaped.saturating_add(1).min(99_999);
    }

    pub fn add_grade(&mut self, grade: i16) {
        self.grade = self.grade.wrapping_add(i32::from(grade)).min(99_999_999);
    }
}

impl Party {
    /// 5C38 activation effects. Call once after the entire encounter is prepared.
    /// A candidate party owns this mutation until the encounter is committed.
    pub fn begin_battle(&mut self, data: &MenuData, active: &[u8]) -> Result<()> {
        ensure!(
            (1..=4).contains(&active.len())
                && active
                    .iter()
                    .all(|&id| id > 0 && usize::from(id) <= self.members.len() && id <= 9)
                && active
                    .iter()
                    .enumerate()
                    .all(|(i, id)| !active[..i].contains(id)),
            "invalid active battle formation"
        );
        // Validate the complete activation before any persistent mutation. New
        // compound discovery follows this boundary in CEA8, after 5C38.
        let drift = active
            .iter()
            .map(|&id| technique_drift(&self.members[usize::from(id - 1)], data))
            .collect::<Result<Vec<_>>>()?;
        self.battles.total = self.battles.total.saturating_add(1).min(9_999);
        for &id in active {
            let count = &mut self.battles.participation[usize::from(id - 1)];
            *count = count.saturating_add(1).min(9_999);
        }
        for (&id, drift) in active.iter().zip(drift) {
            let balance = &mut self.members[usize::from(id - 1)].technique_balance;
            *balance = (i16::from(*balance) + 2 * i16::from(drift)).clamp(-100, 100) as i8;
        }
        self.cooking.full = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_statistics_keep_unknown_history_and_known_history_roundtrips() -> Result<()> {
        let mut value = serde_json::to_value(BattleStatistics::default())?;
        assert!(value.get("previous_formation").is_none());
        let legacy: BattleStatistics = serde_json::from_value(value.clone())?;
        assert_eq!(legacy.previous_formation, None);
        value["previous_formation"] = serde_json::json!(0);
        let known: BattleStatistics = serde_json::from_value(value)?;
        assert_eq!(known.previous_formation, Some(0));
        assert_eq!(serde_json::to_value(known)?["previous_formation"], 0);
        Ok(())
    }

    #[test]
    fn signed_grade_and_escape_keep_original_caps() {
        let mut records = BattleStatistics::default();
        records.add_grade(-150);
        assert_eq!(records.grade, -150);
        records.grade = 99_999_990;
        records.add_grade(100);
        assert_eq!(records.grade, 99_999_999);
        records.escaped = 99_999;
        records.record_escape();
        assert_eq!(records.escaped, 99_999);
        records.validate().unwrap();
    }
}
