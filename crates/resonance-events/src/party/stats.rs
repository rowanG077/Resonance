use super::Member;
use resonance_content::menu_data::{Element, ExSkillData, ExStat, MenuData};

pub(super) fn recover(current: &mut u16, maximum: u16, percent: u16) -> bool {
    let previous = *current;
    *current = (u32::from(previous) + u32::from(maximum) * u32::from(percent) / 100)
        .min(u32::from(maximum)) as u16;
    *current != previous
}

pub struct EquipmentTraits {
    pub attack_element: Option<Element>,
    pub resistance: [i16; 8],
    pub effects: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Stats {
    pub hp: u16,
    pub tp: u16,
    pub strength: u16,
    pub slash: u16,
    pub thrust: u16,
    pub defense: u16,
    pub intelligence: u16,
    pub accuracy: u16,
    pub evasion: u16,
    pub luck: u16,
}

impl Member {
    pub(super) fn clamp_vitals(&mut self) {
        let [hp, tp] = self.maximum_vitals();
        self.hp = self.hp.min(hp);
        self.tp = self.tp.min(tp);
    }

    pub fn equipment_traits(&self, data: &MenuData) -> EquipmentTraits {
        let properties = |slot: usize| &data.items[usize::from(self.equipment[slot])].properties;
        let effects: Vec<_> = [0, 1, 2, 5, 3, 4]
            .into_iter()
            .filter(|&slot| self.equipment[slot] != 0)
            .flat_map(|slot| properties(slot).effects.iter().copied())
            .collect();
        let mut traits = EquipmentTraits {
            attack_element: [0, 4, 3]
                .into_iter()
                .filter_map(|slot| properties(slot).attack_element)
                .next_back(),
            resistance: [0; 8],
            effects: effects
                .iter()
                .copied()
                .filter(|id| {
                    !effects
                        .iter()
                        .any(|other| data.status.equipment_effects[other].suppresses.contains(id))
                })
                .collect(),
        };
        for slot in 0..6 {
            if self.equipment[slot] != 0 {
                for (&element, &value) in &properties(slot).resistance {
                    traits.resistance[element as usize] += i16::from(value);
                }
            }
        }
        traits
    }
    pub fn preview_equipment(&self, data: &MenuData, slot: usize, id: u16) -> Stats {
        let mut preview = self.clone();
        preview.equipment[slot] = id;
        preview.stats(data)
    }
    pub fn maximum_vitals(&self) -> [u16; 2] {
        self.vitals_with_ex(
            self.ex_rules.as_ref().map(|r| r.data.as_ref()),
            self.ex_rules.as_ref().map_or(0, |r| r.character),
        )
    }
    pub(super) fn vitals_with_ex(&self, data: Option<&ExSkillData>, character: usize) -> [u16; 2] {
        let mut vitals = std::array::from_fn(|index| {
            let base = u32::from(self.base_stats[index]);
            let accessories = self
                .equipment
                .iter()
                .filter(|&&id| id == 454 + index as u16)
                .count() as u32;
            (base + ((base * 30 + 50) / 100) * accessories).min(if index == 0 { 9999 } else { 999 })
                as u16
        });
        for bonus in self.ex_bonuses(data, character) {
            let index = match bonus.stat {
                ExStat::MaxHp => 0,
                ExStat::MaxTp => 1,
                _ => continue,
            };
            let amount = (u32::from(self.base_stats[index]) * u32::from(bonus.percent) + 50) / 100;
            vitals[index] =
                (u32::from(vitals[index]) + amount).min(if index == 0 { 9999 } else { 999 }) as u16;
        }
        vitals
    }
    pub fn stats(&self, data: &MenuData) -> Stats {
        let [_, _, strength, defense, intelligence, evasion, accuracy] = self.base_stats;
        let [hp, tp] = self.maximum_vitals();
        let mut stats = Stats {
            hp,
            tp,
            strength: strength / 10,
            slash: strength / 10,
            thrust: strength / 10,
            defense: defense / 10,
            intelligence: intelligence / 10,
            accuracy: accuracy / 10,
            evasion: evasion / 10,
            luck: u16::from(self.luck),
        };
        let add = |stat: &mut u16, bonus: i16, cap| {
            *stat = (i32::from(*stat) + i32::from(bonus)).clamp(0, cap) as u16
        };
        // Percent equipment bonuses are rounded from the base statistic, before
        // adding them to the derived value. Each accessory applies independently.
        let percent = |stat: &mut u16, base: u16, percentage: u16, divisor: u32, cap| {
            add(
                stat,
                ((u32::from(base) * u32::from(percentage) + divisor / 2) / divisor) as i16,
                cap,
            );
        };
        for &id in self.equipment.iter().filter(|&&id| id != 0) {
            let [
                slash,
                thrust,
                defense_bonus,
                intelligence,
                accuracy,
                evasion,
                luck,
            ] = data.items[usize::from(id)].equipment_stats;
            add(&mut stats.slash, slash, 3000);
            add(&mut stats.thrust, thrust, 3000);
            add(&mut stats.defense, defense_bonus, 3000);
            add(&mut stats.intelligence, intelligence, 999);
            add(&mut stats.accuracy, accuracy, 999);
            add(&mut stats.evasion, evasion, 999);
            add(&mut stats.luck, luck, 999);
            match id {
                419 => percent(&mut stats.defense, defense, 10, 1000, 3000), // Guardian Symbol
                399 => percent(&mut stats.defense, defense, 15, 1000, 3000), // Blue Talisman
                398 => percent(&mut stats.defense, defense, 5, 1000, 3000),  // Talisman
                418 => {
                    // Warrior Symbol
                    for value in [&mut stats.strength, &mut stats.slash, &mut stats.thrust] {
                        percent(value, strength, 10, 1000, 3000);
                    }
                }
                _ => {}
            }
        }
        for bonus in self.ex_bonuses(
            Some(&data.ex_skills),
            self.ex_rules.as_ref().map_or(0, |r| r.character),
        ) {
            let (target, base, cap) = match bonus.stat {
                ExStat::Strength => {
                    for value in [&mut stats.strength, &mut stats.slash, &mut stats.thrust] {
                        percent(value, strength, bonus.percent.into(), 1000, 3000);
                    }
                    continue;
                }
                ExStat::Defense => (&mut stats.defense, defense, 3000),
                ExStat::Accuracy => (&mut stats.accuracy, accuracy, 999),
                ExStat::Evasion => (&mut stats.evasion, evasion, 999),
                ExStat::Luck => (&mut stats.luck, u16::from(self.luck) * 10, 999),
                ExStat::Intelligence => (&mut stats.intelligence, intelligence, 999),
                ExStat::MaxHp | ExStat::MaxTp => continue,
            };
            percent(target, base, bonus.percent.into(), 1000, cap);
        }
        stats
    }
}
