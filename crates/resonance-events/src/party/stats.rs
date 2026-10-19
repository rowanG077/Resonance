use super::Member;
use resonance_content::menu_data::{Element, ExStat, ExStatBonus, MenuData, TpDiscount};

pub(super) fn recover(current: &mut u16, maximum: u16, percent: u16) -> bool {
    let previous = *current;
    *current = (u32::from(previous) + u32::from(maximum) * u32::from(percent) / 100)
        .min(u32::from(maximum)) as u16;
    *current != previous
}

pub struct EquipmentTraits {
    pub attack_element: Option<Element>,
    pub neutral_resistance: i16,
    pub resistance: [i16; 8],
    /// Added critical probability in percentage points, capped at 100.
    pub critical_chance_bonus: u16,
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
    /// Seven ordered random draws for one level. The caller owns EXP,
    /// technique learning and whether a scripted level change heals the member.
    pub(super) fn grow_level(
        &mut self,
        definition: &resonance_content::session::CharacterDefinition,
        title_growth: Option<[u8; 7]>,
        random: &mut impl FnMut() -> u32,
    ) {
        self.level += 1;
        for (index, growth) in definition.growth.iter().enumerate() {
            let gain = u32::from(growth.base)
                + random() % (u32::from(growth.random) + 1)
                + u32::from(title_growth.map_or(growth.title_bonus, |title| title[index]));
            self.base_stats[index] = (u32::from(self.base_stats[index]) + gain).min(match index {
                0 => 9999,
                1 => 999,
                _ => 32767,
            }) as u16;
        }
    }

    pub(crate) fn clamp_vitals(&mut self) {
        let [hp, tp] = self.maximum_vitals();
        self.hp = self.hp.min(hp);
        self.tp = self.tp.min(tp);
    }

    pub fn equipment_traits(&self, data: &MenuData) -> EquipmentTraits {
        let properties = |slot: usize| &data.items[usize::from(self.equipment[slot])].properties;
        let mut traits = EquipmentTraits {
            attack_element: [0, 4, 3]
                .into_iter()
                .filter_map(|slot| properties(slot).attack_element)
                .next_back(),
            resistance: [0; 8],
            neutral_resistance: 0,
            critical_chance_bonus: 0,
        };
        for slot in 0..6 {
            if self.equipment[slot] != 0 {
                traits.neutral_resistance += i16::from(properties(slot).neutral_resistance);
                traits.critical_chance_bonus += u16::from(properties(slot).critical_chance_bonus);
                for (&element, &value) in &properties(slot).resistance {
                    traits.resistance[element as usize] += i16::from(value);
                }
            }
        }
        traits.critical_chance_bonus = traits.critical_chance_bonus.min(100);
        traits
    }
    pub fn equipment_captions(&self, data: &MenuData) -> anyhow::Result<Vec<u8>> {
        use anyhow::Context;
        let captions: Vec<_> = [0, 1, 2, 5, 3, 4]
            .into_iter()
            .filter(|&slot| self.equipment[slot] != 0)
            .flat_map(|slot| {
                data.items[usize::from(self.equipment[slot])]
                    .properties
                    .caption_ids
                    .iter()
                    .copied()
            })
            .collect();
        let suppressed = captions
            .iter()
            .map(|id| {
                data.status
                    .equipment_effects
                    .get(id)
                    .context("missing selected equipment caption rules")
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(captions
            .iter()
            .copied()
            .filter(|id| !suppressed.iter().any(|rule| rule.suppresses.contains(id)))
            .collect())
    }

    pub fn tp_discount(&self, data: &MenuData) -> TpDiscount {
        self.equipment
            .iter()
            .filter(|&&id| id != 0)
            .map(|&id| data.items[usize::from(id)].properties.tp_discount)
            .max()
            .unwrap_or_default()
    }

    pub fn preview_equipment(&self, data: &MenuData, slot: usize, id: u16) -> Stats {
        let mut preview = self.clone();
        preview.equipment[slot] = id;
        preview.stats(data)
    }
    pub fn maximum_vitals(&self) -> [u16; 2] {
        self.vitals_with_rules(
            self.rules.as_ref().map(|r| r.data.as_ref()),
            self.rules.as_ref().map_or(0, |r| r.character),
        )
    }
    pub(super) fn vitals_with_rules(&self, data: Option<&MenuData>, character: usize) -> [u16; 2] {
        let mut vitals = [self.base_stats[0].min(9999), self.base_stats[1].min(999)];
        let gear = data.into_iter().flat_map(|data| {
            self.equipment
                .iter()
                .filter(|&&id| id != 0)
                .flat_map(move |&id| &data.items[usize::from(id)].properties.stat_bonuses)
        });
        for bonus in gear.chain(self.ex_bonuses(data.map(|data| &data.ex_skills), character)) {
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
        self.stats_for(data, self.rules.as_ref().map_or(0, |r| r.character))
    }

    /// Derive statistics with an explicit zero-based roster identity. Battle
    /// preparation can use the loaded rules without rebinding saved members.
    pub fn stats_for(&self, data: &MenuData, character: usize) -> Stats {
        let [_, _, strength, defense, intelligence, evasion, accuracy] = self.base_stats;
        let [hp, tp] = self.vitals_with_rules(Some(data), character);
        let mut stats = Stats {
            hp,
            tp,
            strength: (strength / 10).min(3000),
            slash: (strength / 10).min(3000),
            thrust: (strength / 10).min(3000),
            defense: (defense / 10).min(3000),
            intelligence: (intelligence / 10).min(999),
            accuracy: (accuracy / 10).min(999),
            evasion: (evasion / 10).min(999),
            luck: u16::from(self.luck),
        };
        let add = |stat: &mut u16, bonus: i64, cap| {
            *stat = (i64::from(*stat) + bonus).clamp(0, cap) as u16
        };
        // Percent equipment bonuses are rounded from the base statistic, before
        // adding them to the derived value. Each accessory applies independently.
        let percent = |stat: &mut u16, base: u16, percentage: u16, divisor: u32, cap| {
            add(
                stat,
                (i64::from(base) * i64::from(percentage) + i64::from(divisor / 2))
                    / i64::from(divisor),
                cap,
            );
        };
        let apply_bonus = |stats: &mut Stats, bonus: &ExStatBonus| {
            let (target, base, cap) = match bonus.stat {
                ExStat::Strength => {
                    for value in [&mut stats.strength, &mut stats.slash, &mut stats.thrust] {
                        percent(value, strength, bonus.percent.into(), 1000, 3000);
                    }
                    return;
                }
                ExStat::Defense => (&mut stats.defense, defense, 3000),
                ExStat::Accuracy => (&mut stats.accuracy, accuracy, 999),
                ExStat::Evasion => (&mut stats.evasion, evasion, 999),
                ExStat::Luck => (&mut stats.luck, u16::from(self.luck) * 10, 999),
                ExStat::Intelligence => (&mut stats.intelligence, intelligence, 999),
                ExStat::MaxHp | ExStat::MaxTp => return,
            };
            percent(target, base, bonus.percent.into(), 1000, cap);
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
            add(&mut stats.slash, slash.into(), 3000);
            add(&mut stats.thrust, thrust.into(), 3000);
            add(&mut stats.defense, defense_bonus.into(), 3000);
            add(&mut stats.intelligence, intelligence.into(), 999);
            add(&mut stats.accuracy, accuracy.into(), 999);
            add(&mut stats.evasion, evasion.into(), 999);
            add(&mut stats.luck, luck.into(), 999);
            for bonus in &data.items[usize::from(id)].properties.stat_bonuses {
                apply_bonus(&mut stats, bonus);
            }
        }
        for bonus in self.ex_bonuses(Some(&data.ex_skills), character) {
            apply_bonus(&mut stats, bonus);
        }
        stats
    }
}
