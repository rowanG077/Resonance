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
