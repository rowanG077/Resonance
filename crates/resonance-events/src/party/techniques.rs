use super::*;
use super::{
    items::{RECOVERY_CUE, REMEDY_CUE},
    stats::recover,
};
use resonance_content::menu_data::{MenuData, TechniqueUse};

impl Member {
    pub(super) fn acquire_level_techniques(
        &mut self,
        definition: &resonance_content::session::CharacterDefinition,
        can_learn: impl Fn(u16) -> Result<bool, String>,
    ) -> Result<Vec<u16>, String> {
        let mut learned = Vec::new();
        // Catalogue order determines acquisition notices and empty shortcuts.
        for &technique in &definition.allowed_techniques {
            if !self.techniques.contains(&technique)
                && definition
                    .level_techniques
                    .range(..=self.level)
                    .any(|(_, ids)| ids.contains(&technique))
                && can_learn(technique)?
            {
                self.techniques.insert(technique);
                self.disabled_techniques.remove(&technique);
                if let Some(slot) = self.shortcuts.iter_mut().find(|slot| **slot == 0) {
                    *slot = technique;
                }
                learned.push(technique);
            }
        }
        Ok(learned)
    }

    pub fn technique_cost(&self, data: &MenuData, id: u16, at_save_point: bool) -> u16 {
        if at_save_point
            && let Some(cost) = self
                .ex_skills
                .iter()
                .filter_map(|skill| data.ex_skills.skills.get(skill)?.save_point_tp_cost)
                .min()
        {
            return u16::from(cost);
        }
        let tech = &data.techniques[usize::from(id)];
        let cost = if tech.tp_percent {
            self.maximum_vitals()[1] / 10
        } else {
            u16::from(tech.tp)
        };
        self.tp_discount(data).apply(u32::from(cost)) as u16
    }
}

impl Party {
    pub fn assign_unison_arte(
        &mut self,
        data: &MenuData,
        member: usize,
        technique: Option<u16>,
    ) -> Result<bool, String> {
        let target = self.members.get_mut(member).ok_or("Unknown character.")?;
        if let Some(id) = technique {
            if !target.techniques.contains(&id)
                || !data
                    .techniques
                    .get(usize::from(id))
                    .is_some_and(|tech| tech.unison_usable)
            {
                return Err("Choose a learned arte available for Unison.".into());
            }
        }
        let changed = target.unison_arte != technique;
        target.unison_arte = technique;
        Ok(changed)
    }
    /// Forget optional elemental variants across the whole roster, including
    /// reserve members and shortcuts assigned by other characters.
    pub fn forget_elemental_artes(&mut self) {
        for member in 0..self.members.len() {
            for variant in resonance_content::arte::elemental::VARIANTS {
                self.remove_technique(member, variant.id);
            }
        }
    }

    /// Restore each member's current T/S path without changing shared artes or
    /// usage history. Members outside the active formation are included.
    pub fn forget_opposite_path_artes(&mut self) {
        for index in 0..self.members.len() {
            let member = &self.members[index];
            let Some(rules) = &member.rules else {
                continue;
            };
            let forgotten: Vec<_> = member
                .techniques
                .iter()
                .copied()
                .filter(|&id| {
                    rules
                        .data
                        .techniques
                        .get(usize::from(id))
                        .is_some_and(|tech| match tech.route {
                            1 => member.technique_balance > 0,
                            2 => member.technique_balance <= 0,
                            _ => false,
                        })
                })
                .collect();
            for id in forgotten {
                self.remove_technique(index, id);
            }
        }
    }

    pub fn assign_technique(
        &mut self,
        member: usize,
        slot: usize,
        shortcut: Option<TechniqueShortcut>,
    ) -> Result<bool, String> {
        let character = self.members.get(member).ok_or("unknown party member")?;
        if slot >= 6 {
            return Err("unknown technique shortcut".into());
        }
        if let Some(shortcut) = shortcut
            && (!self.contains_member(shortcut.character)
                || self
                    .members
                    .get(shortcut.character)
                    .is_none_or(|m| !m.techniques.contains(&shortcut.technique))
                || slot < 4 && shortcut.character != member)
        {
            return Err("technique is unavailable for this shortcut".into());
        }
        if slot < 4 {
            let id = shortcut.map_or(0, |s| s.technique);
            if character.shortcuts[slot] == id {
                return Ok(false);
            }
            self.members[member].shortcuts[slot] = id;
        } else {
            if character.assist_shortcuts[slot - 4] == shortcut {
                return Ok(false);
            }
            self.members[member].assist_shortcuts[slot - 4] = shortcut;
        }
        Ok(true)
    }

    pub fn forget_technique(
        &mut self,
        data: &MenuData,
        member: usize,
        id: u16,
    ) -> Result<bool, String> {
        let tech = data
            .techniques
            .get(usize::from(id))
            .ok_or("unknown technique")?;
        let target = self.members.get_mut(member).ok_or("unknown party member")?;
        if !target.techniques.contains(&id) || tech.alternatives[0] == 0 {
            return Ok(false);
        }
        // Forgetting clears current membership and assignments while retaining
        // acquisition history and use counts.
        for id in tech.alternatives {
            self.remove_technique(member, id);
        }
        Ok(true)
    }

    pub(crate) fn remove_technique(&mut self, member: usize, id: u16) {
        let target = &mut self.members[member];
        target.techniques.remove(&id);
        if target.unison_arte == Some(id) {
            target.unison_arte = None;
        }
        target
            .arte_bindings
            .retain(|binding| binding.technique != id);
        target.disabled_techniques.remove(&id);
        for slot in &mut target.shortcuts {
            if *slot == id {
                *slot = 0;
            }
        }
        for character in &mut self.members {
            for slot in &mut character.assist_shortcuts {
                if slot.is_some_and(|s| s.character == member && s.technique == id) {
                    *slot = None;
                }
            }
        }
    }

    /// A rejected field spell leaves both TP and its targets unchanged.
    pub fn cast_technique(
        &mut self,
        data: &MenuData,
        caster: usize,
        target: usize,
        id: u16,
        at_save_point: bool,
    ) -> Result<Option<i16>, String> {
        let tech = data
            .techniques
            .get(usize::from(id))
            .ok_or("unknown technique")?;
        let caster_data = self.members.get(caster).ok_or("unknown caster")?;
        let Some(action) = tech.field_use else {
            return Ok(None);
        };
        let cost = caster_data.technique_cost(data, id, at_save_point);
        if !caster_data.can_lead_field()
            || !caster_data.techniques.contains(&id)
            || caster_data.tp < cost
        {
            return Ok(None);
        }
        let all = matches!(
            action,
            TechniqueUse::Recover { party: true, .. } | TechniqueUse::Cure { party: true }
        );
        if !self.contains_member(caster) || !self.contains_member(target) {
            return Ok(None);
        }
        let mut changed = false;
        for &id in &self.formation {
            let index = usize::from(id - 1);
            if !all && index != target {
                continue;
            }
            let member = &mut self.members[index];
            let old = (member.hp, member.ailments);
            match action {
                TechniqueUse::Recover { hp, .. } if !member.knocked_out() => {
                    let maximum = member.maximum_vitals()[0];
                    recover(&mut member.hp, maximum, hp.into());
                }
                TechniqueUse::Cure { .. } if !member.knocked_out() => {
                    member.ailments = Default::default();
                }
                TechniqueUse::Revive => {
                    member.revive(30);
                }
                _ => (),
            }
            changed |= old != (member.hp, member.ailments);
        }
        if changed {
            self.members[caster].tp -= cost;
        }
        Ok(
            changed.then_some(if matches!(action, TechniqueUse::Recover { .. }) {
                RECOVERY_CUE
            } else {
                REMEDY_CUE
            }),
        )
    }
}

impl Party {
    pub fn arte_binding_owner(
        &self,
        chord: &resonance_content::input::InputChord,
    ) -> Option<TechniqueShortcut> {
        self.members
            .iter()
            .enumerate()
            .find_map(|(character, member)| {
                member
                    .arte_bindings
                    .iter()
                    .find(|binding| binding.chord.same_inputs(chord))
                    .map(|binding| TechniqueShortcut {
                        character,
                        technique: binding.technique,
                    })
            })
    }
    pub fn assign_arte_binding(
        &mut self,
        member: usize,
        technique: u16,
        chord: resonance_content::input::InputChord,
    ) -> Result<bool, String> {
        self.write_arte_binding(member, technique, chord, None)
    }
    pub fn transfer_arte_binding(
        &mut self,
        member: usize,
        technique: u16,
        chord: resonance_content::input::InputChord,
        previous: TechniqueShortcut,
    ) -> Result<bool, String> {
        if self.arte_binding_owner(&chord) != Some(previous) {
            return Err("The combo changed. Record it again.".into());
        }
        self.write_arte_binding(member, technique, chord, Some(previous))
    }
    fn write_arte_binding(
        &mut self,
        member: usize,
        technique: u16,
        mut chord: resonance_content::input::InputChord,
        replace: Option<TechniqueShortcut>,
    ) -> Result<bool, String> {
        chord.validate(&self.settings.preferences.keymap)?;
        chord.0.sort();
        let target = self.members.get(member).ok_or("Unknown character.")?;
        if technique == 0 || !target.techniques.contains(&technique) {
            return Err("Learn this arte before assigning a combo.".into());
        }
        let destination = TechniqueShortcut {
            character: member,
            technique,
        };
        if let Some(owner) = self.arte_binding_owner(&chord) {
            if owner != destination && Some(owner) != replace {
                return Err("This combo is assigned to another arte.".into());
            }
        }
        if target
            .arte_bindings
            .iter()
            .any(|b| b.technique == technique && b.chord.same_inputs(&chord))
        {
            return Ok(false);
        }
        // Every validation precedes mutation. One arte owns one combo, and a
        // confirmed transfer removes the old owner before installing the new one.
        for (index, target) in self.members.iter_mut().enumerate() {
            target.arte_bindings.retain(|b| {
                !(index == member && b.technique == technique) && !b.chord.same_inputs(&chord)
            });
        }
        self.members[member]
            .arte_bindings
            .push(resonance_content::input::ArteBinding { technique, chord });
        Ok(true)
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use resonance_content::input::{InputChord, PhysicalInput, StandardAction};
    #[test]
    fn one_combo_per_arte_replaces_roundtrips_and_is_cleared_when_forgotten() {
        let mut party = Party::new(&super::super::tests::data(), Default::default()).unwrap();
        party.members[0].techniques.insert(10);
        let map = party.settings.preferences.keymap.clone();
        for action in StandardAction::ALL {
            for input in map.get(action) {
                let result = party.assign_arte_binding(0, 10, InputChord(vec![input.clone()]));
                assert_eq!(result.is_err(), action.combat_action().is_some());
            }
        }
        for n in 0..32 {
            assert!(
                party
                    .assign_arte_binding(
                        0,
                        10,
                        InputChord(vec![PhysicalInput::Button(format!("Other({n})"))])
                    )
                    .unwrap()
            );
        }
        assert_eq!(party.members[0].arte_bindings.len(), 1);
        let mut restored: Party =
            serde_json::from_slice(&serde_json::to_vec(&party).unwrap()).unwrap();
        assert_eq!(
            restored.members[0].arte_bindings,
            party.members[0].arte_bindings
        );
        assert_eq!(party.settings.preferences.keymap, map);
        // Remapping a free input onto a combat action protects that action immediately.
        restored.settings.preferences.keymap.assign(
            StandardAction::Confirm,
            PhysicalInput::Button("Other(31)".into()),
        );
        assert!(
            restored.members[0].arte_bindings[0]
                .chord
                .validate(&restored.settings.preferences.keymap)
                .is_err()
        );
        restored.members[0].unison_arte = Some(10);
        restored.remove_technique(0, 10);
        assert_eq!(restored.members[0].unison_arte, None);
        assert!(restored.members[0].arte_bindings.is_empty());
    }
    #[test]
    fn global_combo_transfer_requires_the_current_owner_and_is_atomic() {
        let mut party = Party::new(&super::super::tests::data(), Default::default()).unwrap();
        party.members[0].techniques.extend([10, 11]);
        party.members[2].techniques.insert(66);
        let map = party.settings.preferences.keymap.clone();
        let chord = InputChord(vec![
            PhysicalInput::Key("Enter".into()),
            PhysicalInput::Key("KeyW".into()),
        ]);
        let reverse = InputChord(chord.0.iter().cloned().rev().collect());
        let other = InputChord(vec![PhysicalInput::Key("F11".into())]);
        assert!(party.assign_arte_binding(0, 10, chord.clone()).unwrap());
        assert!(!party.assign_arte_binding(0, 10, reverse.clone()).unwrap());
        party.assign_arte_binding(2, 66, other.clone()).unwrap();
        let before = serde_json::to_value(&party).unwrap();
        for (member, arte) in [(0, 11), (2, 66)] {
            assert!(
                party
                    .assign_arte_binding(member, arte, reverse.clone())
                    .is_err()
            );
            assert_eq!(serde_json::to_value(&party).unwrap(), before);
        }
        let previous = TechniqueShortcut {
            character: 0,
            technique: 10,
        };
        assert!(
            party
                .transfer_arte_binding(2, 999, chord.clone(), previous)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
        assert!(
            party
                .transfer_arte_binding(2, 66, reverse, previous)
                .unwrap()
        );
        assert!(party.members[0].arte_bindings.is_empty());
        assert_eq!(party.members[2].arte_bindings.len(), 1);
        assert_eq!(
            party.arte_binding_owner(&chord),
            Some(TechniqueShortcut {
                character: 2,
                technique: 66
            })
        );
        assert_eq!(party.arte_binding_owner(&other), None);
        let before = serde_json::to_value(&party).unwrap();
        assert!(party.transfer_arte_binding(0, 11, chord, previous).is_err());
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
        assert_eq!(party.settings.preferences.keymap, map);
    }
    #[test]
    fn saved_assignments_require_one_combo_per_arte_and_one_owner_per_combo() {
        use resonance_content::input::ArteBinding;
        let data = super::super::tests::data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.members[0].techniques.insert(10);
        party.members[1].techniques.insert(10);
        let chord = InputChord(vec![
            PhysicalInput::Key("Enter".into()),
            PhysicalInput::Key("KeyW".into()),
        ]);
        party.assign_arte_binding(0, 10, chord.clone()).unwrap();
        party.validate(&data).unwrap();
        party.members[0].arte_bindings.push(ArteBinding {
            technique: 10,
            chord: InputChord(vec![PhysicalInput::Key("F11".into())]),
        });
        assert!(
            party
                .validate(&data)
                .unwrap_err()
                .to_string()
                .contains("arte assignment")
        );
        party.members[0].arte_bindings.pop();
        party.members[1].arte_bindings.push(ArteBinding {
            technique: 10,
            chord: InputChord(chord.0.into_iter().rev().collect()),
        });
        assert!(
            party
                .validate(&data)
                .unwrap_err()
                .to_string()
                .contains("arte assignment")
        );
    }
}
