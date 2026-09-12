//! Atomic inventory operations shared by item and equipment menus.
use super::{Party, stats::recover};
use resonance_content::{
    menu_data::{ItemUse, MenuData},
    session::SessionData,
};

pub(super) const KNOCKED_OUT: u32 = 0x8000_0000;
const PETRIFIED: u32 = 0x100;
pub(super) const INCAPACITATED: u32 = KNOCKED_OUT | PETRIFIED;
pub(super) const CURABLE: u32 = 0xfe3;
pub(super) const REVIVAL_CLEARS: u32 = KNOCKED_OUT | 0x3e0;
pub(super) const RECOVERY_CUE: i16 = resonance_content::field_audio::ServiceCue::Recovery as i16;
pub(super) const REMEDY_CUE: i16 = resonance_content::field_audio::ServiceCue::Remedy as i16;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncounterModifier {
    pub rate: u8,
    pub remaining: u16,
}
impl EncounterModifier {
    pub const DURATION: u16 = 3600;
}
impl super::Member {
    pub fn knocked_out(&self) -> bool {
        self.conditions & KNOCKED_OUT != 0
    }
    pub fn can_lead_field(&self) -> bool {
        self.conditions & INCAPACITATED == 0
    }

    /// Items choose the empty accessory slot first; the Equip page chooses
    /// either accessory explicitly.
    pub fn preferred_equipment_slot(&self, kind: u8) -> Option<usize> {
        match kind {
            0..=2 => Some(usize::from(kind)),
            3 => Some(5),
            4 => Some(if self.equipment[3] == 0 { 3 } else { 4 }),
            _ => None,
        }
    }
}

impl Party {
    pub(super) fn contains_member(&self, member: usize) -> bool {
        self.formation
            .iter()
            .any(|&id| usize::from(id - 1) == member)
    }

    /// Improve weapon and armor by their attack/defense values. Accessories
    /// have situational effects, so automatic selection leaves them alone.
    pub fn optimize_equipment(
        &mut self,
        data: &SessionData,
        menus: &MenuData,
        member: usize,
        thrust: bool,
    ) -> Result<bool, String> {
        if self
            .members
            .get(member)
            .ok_or("unknown party member")?
            .knocked_out()
        {
            return Ok(false);
        }
        let mut next = self.clone();
        let mut changed = false;
        for (kind, slot) in [0, 1, 2, 5].into_iter().enumerate() {
            let stat = if kind == 0 { usize::from(thrust) } else { 2 };
            let current = next.members[member].equipment[slot];
            let mut best = current;
            for &id in next.items.keys() {
                let item = &data.items[usize::from(id)];
                if item.equipment_kind == Some(kind as u8)
                    && item.allowed_characters & (1 << member) != 0
                    && menus.items[usize::from(id)].equipment_stats[stat]
                        > menus.items[usize::from(best)].equipment_stats[stat]
                {
                    best = id;
                }
            }
            changed |= next.equip_slot(data, member, slot, best)?;
        }
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    pub fn equip_slot(
        &mut self,
        data: &SessionData,
        member: usize,
        slot: usize,
        id: u16,
    ) -> Result<bool, String> {
        let old = *self
            .members
            .get(member)
            .and_then(|m| m.equipment.get(slot))
            .ok_or("unknown equipment slot")?;
        if old == id {
            return Ok(false);
        }
        if id != 0 {
            let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
            let category = [0, 1, 2, 4, 4, 3][slot];
            if item.equipment_kind != Some(category)
                || item.allowed_characters & (1 << member) == 0
                || self.items.get(&id).copied().unwrap_or(0) == 0
            {
                return Ok(false);
            }
        }
        if old != 0
            && self.items.get(&old).copied().unwrap_or(0)
                >= data.items[usize::from(old)].stack_limit
        {
            return Err("There is no room in the inventory for the equipped item.".into());
        }
        if id != 0 {
            self.change_item(data, id, -1)?;
        }
        if old != 0 {
            self.change_item(data, old, 1)?;
        }
        let character = &mut self.members[member];
        character.equipment[slot] = id;
        character.clamp_vitals();
        Ok(true)
    }

    pub fn can_use_group_item(&self, menus: &MenuData, id: u16) -> bool {
        let Some(ItemUse::Recover {
            hp,
            tp,
            party: true,
        }) = menus
            .items
            .get(usize::from(id))
            .and_then(|item| item.field_use)
        else {
            return false;
        };
        // Eligibility counts missing vitals even on knocked-out members; applying
        // recovery skips them. Lloyd's condition gates the group action itself.
        !self.members[0].knocked_out()
            && self.formation.iter().any(|&id| {
                let member = &self.members[usize::from(id - 1)];
                let stats = member.stats(menus);
                hp != 0 && member.hp != stats.hp || tp != 0 && member.tp != stats.tp
            })
    }

    pub fn use_item(
        &mut self,
        data: &SessionData,
        menus: &MenuData,
        id: u16,
        member: usize,
    ) -> Result<Option<i16>, String> {
        if !self.contains_member(member) {
            return Err("item target is not in the party".into());
        }
        if self.items.get(&id).copied().unwrap_or(0) == 0 {
            return Ok(None);
        }
        data.items
            .get(usize::from(id))
            .ok_or("unknown inventory item")?;
        let Some(action) = menus
            .items
            .get(usize::from(id))
            .ok_or("unknown item")?
            .field_use
        else {
            return Ok(None);
        };
        let group = matches!(action, ItemUse::Recover { party: true, .. });
        if group && !self.can_use_group_item(menus, id) {
            return Ok(None);
        }
        let targets = match action {
            ItemUse::Recover { party: true, .. } => self
                .formation
                .iter()
                .map(|id| usize::from(*id - 1))
                .collect(),
            _ => vec![member],
        };
        let mut changed = group;
        let mut cue = RECOVERY_CUE;
        if let ItemUse::EncounterRate { rate } = action {
            self.encounter_modifier = Some(EncounterModifier {
                rate,
                remaining: EncounterModifier::DURATION,
            });
            changed = true;
            cue = REMEDY_CUE;
        }
        for index in targets {
            let member = &mut self.members[index];
            let stats = member.stats(menus);
            let dead = member.knocked_out();
            match action {
                ItemUse::Recover { hp, tp, .. } if !dead => {
                    changed |= recover(&mut member.hp, stats.hp, hp.into());
                    changed |= recover(&mut member.tp, stats.tp, tp.into());
                }
                ItemUse::Revive if dead => {
                    member.conditions &= !REVIVAL_CLEARS;
                    recover(&mut member.hp, stats.hp, 30);
                    recover(&mut member.tp, stats.tp, 15);
                    changed = true;
                    cue = REMEDY_CUE;
                }
                ItemUse::Cure if !dead && member.conditions & CURABLE != 0 => {
                    member.conditions &= !CURABLE;
                    changed = true;
                    cue = REMEDY_CUE;
                }
                ItemUse::Herb {
                    stat,
                    amount,
                    percent,
                } if !dead => {
                    let value = member
                        .base_stats
                        .get_mut(stat)
                        .ok_or("unknown growth statistic")?;
                    let gain = if percent {
                        u32::from(*value) * u32::from(amount) / 100
                    } else {
                        u32::from(amount)
                    };
                    let next = (u32::from(*value) + gain).min(match stat {
                        0 => 9999,
                        1 => 999,
                        2 | 3 => 30000,
                        _ => 9990,
                    }) as u16;
                    changed |= next != *value;
                    *value = next;
                }
                _ => {}
            }
        }
        if changed {
            self.change_item(data, id, -1)?;
        }
        Ok(changed.then_some(cue))
    }

    pub fn transform_item(
        &mut self,
        data: &SessionData,
        menus: &MenuData,
        bottle: u16,
        id: u16,
    ) -> Result<bool, String> {
        let target = menus
            .items
            .get(usize::from(id))
            .ok_or("unknown item")?
            .transforms_to;
        if !matches!(
            menus
                .items
                .get(usize::from(bottle))
                .and_then(|i| i.field_use),
            Some(ItemUse::Transform)
        ) || self.items.get(&bottle).copied().unwrap_or(0) == 0
            || self.items.get(&id).copied().unwrap_or(0) == 0
            || target == 0
        {
            return Ok(false);
        }
        if id == bottle || target == bottle || target == id {
            return Ok(false);
        }
        for item in [bottle, id, target] {
            data.items
                .get(usize::from(item))
                .ok_or("unknown inventory item")?;
        }
        if self.items.get(&target).copied().unwrap_or(0)
            >= data.items[usize::from(target)].stack_limit
        {
            return Ok(false);
        }
        self.change_item(data, bottle, -1)?;
        self.change_item(data, id, -1)?;
        self.change_item(data, target, 1)?;
        Ok(true)
    }
}
