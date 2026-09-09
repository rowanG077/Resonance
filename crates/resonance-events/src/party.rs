//! Session-owned party state. Rendering and event bytecode do not own inventory.
use resonance_content::session::SessionData;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct Member {
    pub affinity: i32,
    pub level: u8,
    pub experience: u32,
    pub base_stats: [u16; 7],
    pub hp: u16,
    pub tp: u16,
    pub conditions: u32,
    pub luck: u8,
    pub overlimit: u8,
    pub equipment: [u16; 6],
    pub techniques: BTreeSet<u16>,
    pub shortcuts: [u16; 4],
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::session::{CharacterDefinition, ItemDefinition, StatGrowth};

    fn data() -> SessionData {
        SessionData {
            version: 1,
            executable_sha256: "0".repeat(64),
            experience: vec![0, 0, 10, 30, 60],
            items: (0..4)
                .map(|id| ItemDefinition {
                    equipment_kind: (id != 0).then_some(0),
                    allowed_characters: 511,
                    stack_limit: if id == 3 { 1 } else { 20 },
                })
                .collect(),
            characters: (0..9)
                .map(|_| CharacterDefinition {
                    affinity: 0,
                    level: 1,
                    experience: 0,
                    base_stats: [100, 20, 30, 40, 50, 60, 70],
                    luck: 10,
                    overlimit: 50,
                    equipment: [0; 6],
                    techniques: vec![],
                    allowed_techniques: vec![10],
                    shortcuts: [0; 4],
                    growth: std::array::from_fn(|_| StatGrowth {
                        base: 1,
                        random: 1,
                        title_bonus: 0,
                    }),
                    level_techniques: [(2, vec![10])].into(),
                })
                .collect(),
        }
    }

    #[test]
    fn equipment_transfers_items_and_tracks_stack_limits_and_discoveries() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.change_item(&data, 1, 3).unwrap();
        party.change_item(&data, 2, 1).unwrap();
        party.equip(&data, 0, 1).unwrap();
        assert_eq!(party.items[&1], 2);
        party.equip(&data, 0, 2).unwrap();
        assert_eq!(party.items[&1], 3);
        assert!(!party.items.contains_key(&2));
        assert_eq!(party.members[0].equipment[0], 2);
        party.unequip(&data, 0, 0).unwrap();
        assert_eq!(party.items[&2], 1);
        assert_eq!(party.members[0].equipment[0], 0);
        assert!(party.change_item(&data, 3, 10).unwrap());
        assert_eq!(party.items[&3], 1);
        assert!(!party.change_item(&data, 3, 1).unwrap());
        party.change_item(&data, 3, -8).unwrap();
        assert!(!party.items.contains_key(&3));
        assert!(party.found_items.contains(&3));
        assert_eq!(party.recent_items[0], 3);
    }

    #[test]
    fn level_recovery_and_currency_update_persistent_state() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        let mut draws = 0;
        party
            .raise_level(&data, 0, 3, || {
                draws += 1;
                3
            })
            .unwrap();
        assert_eq!(draws, 14);
        assert_eq!(party.members[0].experience, 30);
        assert_eq!(party.members[0].hp, 104);
        assert_eq!(party.members[0].tp, 24);
        assert_eq!(party.members[0].shortcuts, [10, 0, 0, 0]);
        party
            .raise_level(&data, 0, 2, || {
                panic!("lower level must not draw random numbers")
            })
            .unwrap();
        assert_eq!(party.members[0].level, 3);
        party.members[0].conditions = 0xff;
        party.members[0].hp = 1;
        party.heal(|| 207);
        assert_eq!(party.members[0].hp, 104);
        assert_eq!(party.members[0].conditions, 0);
        assert_eq!(party.members[0].luck, 7);
        assert_eq!(party.members[0].overlimit, 40);
        assert_eq!(party.add_gald(500), 500);
        assert_eq!(party.add_gald(-600), 0);
        assert_eq!(party.spent_gald, 500);
        assert_eq!(party.add_gald(i32::MAX), 99_999_999);
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub skit_titles: bool,
    pub rumble: bool,
    pub stereo: bool,
    /// Manual 0, semi-auto 1, auto 2; one entry per battle controller.
    pub battle_controls: [u8; 4],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            skit_titles: true,
            rumble: true,
            stereo: true,
            battle_controls: [1, 2, 2, 2],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Party {
    pub members: Vec<Member>,
    pub formation: Vec<u8>,
    pub items: BTreeMap<u16, u8>,
    pub found_items: BTreeSet<u16>,
    pub recent_items: Vec<u16>,
    pub gald: u32,
    pub spent_gald: u32,
    pub settings: Settings,
}
impl Party {
    pub fn new(data: &SessionData, settings: Settings) -> anyhow::Result<Self> {
        data.validate()?;
        Ok(Self {
            members: data
                .characters
                .iter()
                .map(|character| Member {
                    affinity: character.affinity,
                    level: character.level,
                    experience: character.experience,
                    base_stats: character.base_stats,
                    hp: character.base_stats[0],
                    tp: character.base_stats[1],
                    conditions: 0,
                    luck: character.luck,
                    overlimit: character.overlimit,
                    equipment: character.equipment,
                    techniques: character.techniques.iter().copied().collect(),
                    shortcuts: character.shortcuts,
                })
                .collect(),
            formation: vec![1],
            items: BTreeMap::new(),
            found_items: BTreeSet::new(),
            recent_items: Vec::new(),
            gald: 0,
            spent_gald: 0,
            settings,
        })
    }
    pub fn change_item(&mut self, data: &SessionData, id: u16, delta: i8) -> Result<bool, String> {
        let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
        let previous = self.items.get(&id).copied().unwrap_or(0);
        if delta > 0 && previous == item.stack_limit || delta <= 0 && previous == 0 {
            return Ok(false);
        }
        let count =
            (i16::from(previous) + i16::from(delta)).clamp(0, i16::from(item.stack_limit)) as u8;
        if count == 0 {
            self.items.remove(&id);
        } else {
            self.items.insert(id, count);
        }
        if delta > 0 {
            self.found_items.insert(id);
            self.recent_items.retain(|old| *old != id);
            self.recent_items.insert(0, id);
            self.recent_items.truncate(32);
        }
        Ok(true)
    }
    pub fn unequip(
        &mut self,
        data: &SessionData,
        member: usize,
        slot: usize,
    ) -> Result<(), String> {
        let equipment = self
            .members
            .get_mut(member)
            .ok_or("unknown party member")?
            .equipment
            .get_mut(slot)
            .ok_or("unknown equipment slot")?;
        let item = std::mem::take(equipment);
        if item != 0 {
            self.change_item(data, item, 1)?;
        }
        Ok(())
    }
    pub fn equip(&mut self, data: &SessionData, member: usize, id: u16) -> Result<(), String> {
        let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
        let character = self.members.get(member).ok_or("unknown party member")?;
        if self.items.get(&id).copied().unwrap_or(0) == 0
            || item.allowed_characters & (1 << member) == 0
        {
            return Ok(());
        }
        let slot = match item.equipment_kind {
            Some(0..=2) => usize::from(item.equipment_kind.unwrap()),
            Some(3) => 5,
            Some(4) => {
                if character.equipment[3] == 0 {
                    3
                } else {
                    4
                }
            }
            _ => return Ok(()),
        };
        self.unequip(data, member, slot)?;
        self.change_item(data, id, -1)?;
        self.members[member].equipment[slot] = id;
        Ok(())
    }
    pub fn add_gald(&mut self, amount: i32) -> u32 {
        let previous = self.gald;
        self.gald = (i64::from(previous) + i64::from(amount)).clamp(0, 99_999_999) as u32;
        self.spent_gald = self
            .spent_gald
            .saturating_add(previous.saturating_sub(self.gald));
        self.gald
    }
    pub fn heal(&mut self, mut random: impl FnMut() -> u32) {
        for member in &mut self.members {
            member.hp = member.base_stats[0];
            member.tp = member.base_stats[1];
            member.conditions = 0;
            member.luck = (random() % 100) as u8;
            member.overlimit = member.overlimit.saturating_sub(10);
        }
    }
    pub fn raise_level(
        &mut self,
        data: &SessionData,
        index: usize,
        level: u8,
        mut random: impl FnMut() -> u32,
    ) -> Result<(), String> {
        if level == 0 || usize::from(level) >= data.experience.len() {
            return Err("invalid target level".into());
        }
        let definition = data.characters.get(index).ok_or("unknown party member")?;
        let member = self.members.get_mut(index).ok_or("unknown party member")?;
        while member.level < level {
            member.level += 1;
            member.experience = data.experience[usize::from(member.level)];
            for (index, growth) in definition.growth.iter().enumerate() {
                let gain = u32::from(growth.base)
                    + random() % (u32::from(growth.random) + 1)
                    + u32::from(growth.title_bonus);
                member.base_stats[index] =
                    (u32::from(member.base_stats[index]) + gain).min(match index {
                        0 => 9999,
                        1 => 999,
                        _ => 32767,
                    }) as u16;
            }
        }
        member.hp = member.base_stats[0];
        member.tp = member.base_stats[1];
        for (_, techniques) in definition.level_techniques.range(..=member.level) {
            for &technique in techniques {
                if member.techniques.insert(technique)
                    && let Some(slot) = member.shortcuts.iter_mut().find(|slot| **slot == 0)
                {
                    *slot = technique;
                }
            }
        }
        Ok(())
    }
}
