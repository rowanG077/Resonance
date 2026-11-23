//! Recover editable save values before validating the runtime state.
use super::*;

mod numbers;
mod selections;
pub(super) use numbers::{array, counts, number};
#[cfg(test)]
mod tests;

impl Party {
    /// Apply the one-time legacy outfit correction while preserving other saved values.
    pub fn recover_story_costumes(&mut self, data: &SessionData, flags: &BTreeSet<u16>) {
        if self.story_costumes_initialized {
            return;
        }
        // Presea's recovery scene sets her story flag and changes her costume
        // together. Preserve any selected outfit unlocked by a title.
        let presea = resonance_content::overworld::PartyCharacter::Presea;
        if let Some(member) = self.members.get_mut(presea.index())
            && member.costume == 0
            && !member.owns_selected_costume(data, presea.index())
        {
            member.costume = resonance_content::appearance::story_costume(presea as u32, 0, flags);
        }
        self.story_costumes_initialized = true;
    }

    /// Clamp edited save values to gameplay limits, then validate the result.
    /// Drop unavailable references and reset selections that cannot be used.
    pub fn recover(&mut self, data: &SessionData) -> anyhow::Result<()> {
        self.bind_rules(data);
        self.gald = self.gald.min(MAX_GALD);
        self.grade_hundredths = self.grade_hundredths.min(MAX_GRADE);
        self.unison_gauge = self.unison_gauge.min(MAX_UNISON_GAUGE);
        self.game_clears = self.game_clears.min(MAX_GAME_CLEARS);
        self.battles.recover();

        let mut items = std::mem::take(&mut self.items);
        items.retain(|id, count| {
            let Some(item) = data.items.get(usize::from(*id)) else {
                return false;
            };
            *count = (*count).min(self.item_limit(item));
            *count != 0
        });
        self.items = items;
        self.found_items
            .retain(|&id| usize::from(id) < data.items.len());
        self.recent_items
            .retain(|&id| usize::from(id) < data.items.len());
        self.recent_items.truncate(32);
        if let Some(modifier) = &mut self.encounter_modifier {
            modifier.remaining = modifier.remaining.min(EncounterModifier::DURATION);
            if modifier.remaining == 0 || !(1..=2).contains(&modifier.rate) {
                self.encounter_modifier = None;
            }
        }

        // Recover selections and loadouts before calculating vitals or indexing
        // character-owned rules. The roster's structure must still be intact.
        anyhow::ensure!(
            self.members.len() == data.characters.len(),
            "invalid saved party"
        );
        self.recover_selections(data);
        self.recover_techniques(data);
        let max_level = data.experience.len().saturating_sub(1).min(255) as u8;
        anyhow::ensure!(max_level > 0, "missing experience levels");
        let mut assigned_slots = BTreeSet::new();
        let mut unequipped = Vec::new();
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
            unequipped.extend(member.recover_loadout(data, index));
            member.validate_loadout(data, index)?;
            member.clamp_vitals();
            let mut assigned_artes = BTreeSet::new();
            for mut binding in std::mem::take(&mut member.arte_bindings) {
                if binding.chord.validate_shape().is_err()
                    || !binding
                        .chord
                        .single_direction_per_source(&self.settings.preferences.keymap)
                {
                    continue;
                }
                binding.chord = binding.chord.normalized(&self.settings.preferences.keymap);
                // Old left/right bindings can merge. Keep the first saved owner;
                // later conflicting slots become visibly unassigned in the menu.
                if !assigned_artes.contains(&binding.technique)
                    && !assigned_slots.contains(&binding.chord.0)
                {
                    assigned_artes.insert(binding.technique);
                    assigned_slots.insert(binding.chord.0.clone());
                    member.arte_bindings.push(binding);
                }
            }
        }
        for id in unequipped.into_iter().filter(|&id| id != 0) {
            if let Some(item) = data.items.get(usize::from(id)) {
                let limit = self.item_limit(item);
                let count = self.items.entry(id).or_default();
                *count = count.saturating_add(1).min(limit);
            }
        }
        self.validate(data)
    }

    fn recover_techniques(&mut self, data: &SessionData) {
        for (member, character) in self.members.iter_mut().zip(&data.characters) {
            let known = |id: &u16| character.allowed_techniques.contains(id);
            member.techniques.retain(known);
            member.technique_uses.retain(|id, _| known(id));
            member
                .disabled_techniques
                .retain(|id| member.techniques.contains(id));
            member
                .arte_bindings
                .retain(|binding| member.techniques.contains(&binding.technique));
            member.unison_arte = member.unison_arte.filter(|id| {
                member.techniques.contains(id)
                    && data.rules.as_ref().is_none_or(|rules| {
                        rules
                            .techniques
                            .get(usize::from(*id))
                            .is_some_and(|tech| tech.unison_usable)
                    })
            });
            for slot in &mut member.shortcuts {
                if !member.techniques.contains(slot) {
                    *slot = 0;
                }
            }
        }
        // Resolve assists after every owner's unavailable artes have been removed.
        for index in 0..self.members.len() {
            let shortcuts = self.members[index].assist_shortcuts.map(|slot| {
                slot.filter(|shortcut| {
                    self.members
                        .get(shortcut.character)
                        .is_some_and(|owner| owner.techniques.contains(&shortcut.technique))
                })
            });
            self.members[index].assist_shortcuts = shortcuts;
        }
    }
}
