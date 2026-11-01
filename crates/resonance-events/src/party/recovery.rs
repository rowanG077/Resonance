//! Save admission repairs amounts with unambiguous bounds; IDs still validate.
use super::*;

mod numbers;
pub(super) use numbers::{array, counts, number};
#[cfg(test)]
mod tests;

impl Party {
    /// Clamp edited save values to gameplay limits, then validate the result.
    /// This never substitutes unknown characters, equipment, skills or locations.
    pub fn recover(&mut self, data: &SessionData) -> anyhow::Result<()> {
        self.bind_rules(data);
        self.gald = self.gald.min(MAX_GALD);
        self.grade_hundredths = self.grade_hundredths.min(MAX_GRADE);
        self.unison_gauge = self.unison_gauge.min(MAX_UNISON_GAUGE);
        self.game_clears = self.game_clears.min(MAX_GAME_CLEARS);
        self.battles.recover();

        let mut items = std::mem::take(&mut self.items);
        items.retain(|id, count| {
            if let Some(item) = data.items.get(usize::from(*id)) {
                *count = (*count).min(self.item_limit(item));
            }
            *count != 0
        });
        self.items = items;
        self.recent_items.truncate(32);
        if let Some(modifier) = &mut self.encounter_modifier {
            modifier.remaining = modifier.remaining.min(EncounterModifier::DURATION);
            if modifier.remaining == 0 {
                self.encounter_modifier = None;
            }
        }

        // Vitals depend on equipment and EX skills. Check their references before
        // calculating maxima so a broken save produces an error rather than a panic.
        anyhow::ensure!(
            self.members.len() == data.characters.len(),
            "invalid saved party"
        );
        let max_level = data.experience.len().saturating_sub(1).min(255) as u8;
        anyhow::ensure!(max_level > 0, "missing experience levels");
        for (index, member) in self.members.iter_mut().enumerate() {
            member.level = member.level.clamp(1, max_level);
            member.experience = member.experience.min(MAX_EXPERIENCE);
            member.overlimit = member.overlimit.min(MAX_OVERLIMIT);
            member.technique_balance = member
                .technique_balance
                .clamp(-MAX_TECHNIQUE_BALANCE, MAX_TECHNIQUE_BALANCE);
            for (stat, limit) in member.base_stats.iter_mut().zip(BASE_STAT_LIMITS) {
                *stat = (*stat).min(limit);
            }
            for mastery in &mut member.cooking {
                *mastery = (*mastery).min(MAX_COOKING_MASTERY);
            }
            for count in member.technique_uses.values_mut() {
                *count = (*count).min(resonance_content::arte::MAX_USES);
            }
            member.validate_loadout(data, index)?;
            member.clamp_vitals();
        }
        self.validate(data)
    }
}
