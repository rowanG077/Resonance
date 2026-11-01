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
        let mut saved_combos = BTreeSet::new();
        let mut assigned_slots = BTreeSet::new();
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
            let mut assigned_artes = BTreeSet::new();
            for mut binding in std::mem::take(&mut member.arte_bindings) {
                let mut saved_inputs = binding.chord.0.clone();
                saved_inputs.sort();
                // Migration must not hide malformed or unknown assignments.
                anyhow::ensure!(
                    member.techniques.contains(&binding.technique)
                        && binding.chord.validate_shape().is_ok()
                        && assigned_artes.insert(binding.technique)
                        && saved_combos.insert(saved_inputs),
                    "invalid saved arte assignment"
                );
                if !binding
                    .chord
                    .single_direction_per_source(&self.settings.preferences.keymap)
                {
                    continue;
                }
                binding.chord = binding.chord.normalized(&self.settings.preferences.keymap);
                // Old left/right bindings can merge. Keep the first saved owner;
                // later conflicting slots become visibly unassigned in the menu.
                if assigned_slots.insert(binding.chord.0.clone()) {
                    member.arte_bindings.push(binding);
                }
            }
        }
        self.validate(data)
    }
}
