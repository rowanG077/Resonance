//! Persistent battle records. Transient actors and result tasks never enter saves.
use super::Party;
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::{ExTendency, MenuData};

// Shared by runtime counters, save recovery and strict validation.
const MAX_BATTLES: u16 = 9_999;
const MAX_HUNTING_STREAK: u8 = 5;
const MAX_ESCAPES: u32 = 99_999;
const MAX_SHEENA_ESCAPES: u8 = 50;
const MAX_COMBAT_TICKS: u32 = 359_999;
const MAX_GRADE: i32 = super::MAX_GRADE as i32;
const MAX_KILLS: u16 = 5_000;
const MAX_DEATHS_AND_ITEMS: u8 = 250;

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
    #[serde(deserialize_with = "super::recovery::number")]
    pub sheena_escapes: u8,
    #[serde(deserialize_with = "super::recovery::number")]
    pub total: u16,
    /// Victories at Hard or Mania difficulty.
    #[serde(deserialize_with = "super::recovery::number")]
    pub hard_victories: u16,
    /// Consecutive victories against the same field symbol while leading with Presea.
    #[serde(default)]
    pub hunting_symbol: Option<u32>,
    #[serde(default)]
    #[serde(deserialize_with = "super::recovery::number")]
    pub hunting_streak: u8,
    #[serde(deserialize_with = "super::recovery::number")]
    pub escaped: u32,
    /// Duration of the most recently completed battle, not a cumulative clock.
    #[serde(deserialize_with = "super::recovery::number")]
    pub combat_ticks: u32,
    /// Zero before a battle or in saves without this statistic.
    #[serde(default)]
    #[serde(deserialize_with = "super::recovery::number")]
    pub formation_count: u32,
    /// Signed hundredths, with a cap only on the positive total.
    #[serde(deserialize_with = "super::recovery::number")]
    pub grade: i32,
    #[serde(deserialize_with = "super::recovery::number")]
    pub maximum_combo: u16,
    #[serde(deserialize_with = "super::recovery::number")]
    pub maximum_combo_damage: u32,
    /// One entry per character, independent of current formation order.
    #[serde(deserialize_with = "super::recovery::array")]
    pub participation: [u16; 9],
    #[serde(deserialize_with = "super::recovery::array")]
    pub kills: [u16; 9],
    #[serde(deserialize_with = "super::recovery::array")]
    pub deaths: [u8; 9],
    /// Older saves use their death totals until a new playthrough resets eligibility.
    #[serde(default)]
    pub death_this_run: Option<bool>,
    #[serde(deserialize_with = "super::recovery::array")]
    pub items: [u8; 9],
    /// Accepted victory voice groups, recorded at confirmation.
    pub victory_groups: u64,
}

impl BattleStatistics {
    pub(super) fn recover(&mut self) {
        self.total = self.total.min(MAX_BATTLES);
        self.hard_victories = self.hard_victories.min(MAX_BATTLES);
        self.hunting_streak = self.hunting_streak.min(MAX_HUNTING_STREAK);
        self.escaped = self.escaped.min(MAX_ESCAPES);
        self.sheena_escapes = self.sheena_escapes.min(MAX_SHEENA_ESCAPES);
        self.combat_ticks = self.combat_ticks.min(MAX_COMBAT_TICKS);
        self.grade = self.grade.min(MAX_GRADE);
        for count in &mut self.participation {
            *count = (*count).min(MAX_BATTLES);
        }
        for count in &mut self.kills {
            *count = (*count).min(MAX_KILLS);
        }
        for count in self.deaths.iter_mut().chain(&mut self.items) {
            *count = (*count).min(MAX_DEATHS_AND_ITEMS);
        }
    }

    pub fn has_death_this_run(&self) -> bool {
        self.death_this_run
            .unwrap_or_else(|| self.deaths.iter().any(|&n| n != 0))
    }

    pub fn record_deaths(&mut self, character: usize, count: u8) {
        self.death_this_run = Some(self.has_death_this_run() || count != 0);
        self.deaths[character] = self.deaths[character]
            .saturating_add(count)
            .min(MAX_DEATHS_AND_ITEMS);
    }

    pub(super) fn for_new_game_plus(&self) -> Self {
        Self {
            previous_formation: None,
            battle_gel_used: false,
            lloyd_non_wooden_blade_used: false,
            ordinary_escape_used: false,
            death_this_run: Some(false),
            hunting_symbol: None,
            hunting_streak: 0,
            combat_ticks: 0,
            formation_count: 0,
            ..self.clone()
        }
    }

    pub fn count(&self, member: i32) -> std::result::Result<u16, String> {
        match member {
            0 => Ok(self.total),
            1..=9 => Ok(self.participation[(member - 1) as usize]),
            _ => Err("invalid battle history member".into()),
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.total <= MAX_BATTLES
                && self.hard_victories <= MAX_BATTLES
                && self.hunting_streak <= MAX_HUNTING_STREAK
                && self.escaped <= MAX_ESCAPES
                && self.sheena_escapes <= MAX_SHEENA_ESCAPES
                && self.combat_ticks <= MAX_COMBAT_TICKS
                && self.grade <= MAX_GRADE
                && self.participation.iter().all(|&n| n <= MAX_BATTLES)
                && self.kills.iter().all(|&n| n <= MAX_KILLS)
                && self.deaths.iter().all(|&n| n <= MAX_DEATHS_AND_ITEMS)
                && self.items.iter().all(|&n| n <= MAX_DEATHS_AND_ITEMS),
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
            self.sheena_escapes = self
                .sheena_escapes
                .saturating_add(1)
                .min(MAX_SHEENA_ESCAPES);
        }
    }

    pub fn record_hunt(&mut self, symbol: Option<u32>) {
        self.hunting_streak = match symbol {
            Some(_) if symbol == self.hunting_symbol => self
                .hunting_streak
                .saturating_add(1)
                .min(MAX_HUNTING_STREAK),
            Some(_) => 1,
            None => 0,
        };
        self.hunting_symbol = symbol;
    }

    pub fn record_escape(&mut self) {
        self.record_hunt(None);
        self.escaped = self.escaped.saturating_add(1).min(MAX_ESCAPES);
    }

    pub fn add_grade(&mut self, grade: i32) {
        self.grade = self.grade.saturating_add(grade).min(MAX_GRADE);
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
        self.battles.total = self.battles.total.saturating_add(1).min(MAX_BATTLES);
        for &(id, balance) in active {
            let count = &mut self.battles.participation[usize::from(id - 1)];
            *count = count.saturating_add(1).min(MAX_BATTLES);
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

    #[test]
    fn hunter_streak_survives_saves_and_resets_on_another_symbol_or_escape() -> Result<()> {
        let mut history = BattleStatistics::default();
        for _ in 0..4 {
            history.record_hunt(Some(42));
        }
        let mut history: BattleStatistics = serde_json::from_slice(&serde_json::to_vec(&history)?)?;
        history.record_hunt(Some(42));
        assert_eq!(history.hunting_streak, 5);
        history.record_hunt(Some(42));
        assert_eq!(history.hunting_streak, 5);
        history.record_hunt(Some(43));
        assert_eq!(history.hunting_streak, 1);
        history.record_escape();
        assert_eq!((history.hunting_symbol, history.hunting_streak), (None, 0));
        let mut old = serde_json::to_value(&history)?;
        old.as_object_mut().unwrap().remove("hunting_symbol");
        old.as_object_mut().unwrap().remove("hunting_streak");
        let restored: BattleStatistics = serde_json::from_value(old)?;
        assert_eq!(restored, history);
        Ok(())
    }
}
