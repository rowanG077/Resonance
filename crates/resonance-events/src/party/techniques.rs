use super::*;
use super::{
    items::{CURABLE, INCAPACITATED, KNOCKED_OUT, RECOVERY_CUE, REMEDY_CUE},
    stats::recover,
};
use resonance_content::menu_data::{MenuData, TechniqueUse};

impl Member {
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
        if self.equipment.contains(&407) {
            // Faerie Ring
            cost / 2
        } else if self.equipment.contains(&406) {
            // Emerald Ring
            cost * 2 / 3
        } else {
            cost
        }
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
        for id in tech.alternatives {
            target.techniques.remove(&id);
            target.disabled_techniques.remove(&id);
            for slot in &mut target.shortcuts {
                if *slot == id {
                    *slot = 0;
                }
            }
        }
        for character in &mut self.members {
            for slot in &mut character.assist_shortcuts {
                if slot.is_some_and(|s| {
                    s.character == member && tech.alternatives.contains(&s.technique)
                }) {
                    *slot = None;
                }
            }
        }
        Ok(true)
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
        if caster_data.conditions & INCAPACITATED != 0
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
            let old = (member.hp, member.conditions);
            match action {
                TechniqueUse::Recover { hp, .. } if !member.knocked_out() => {
                    let maximum = member.maximum_vitals()[0];
                    recover(&mut member.hp, maximum, hp.into());
                }
                TechniqueUse::Cure { .. } if !member.knocked_out() => member.conditions &= !CURABLE,
                TechniqueUse::Revive if member.knocked_out() => {
                    member.conditions &= !(KNOCKED_OUT | CURABLE);
                    member.hp = (u32::from(member.maximum_vitals()[0]) * 30 / 100) as u16;
                }
                _ => (),
            }
            changed |= old != (member.hp, member.conditions);
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
