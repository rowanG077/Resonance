//! Translated title parameters; eligibility and saved achievements stay native.
use super::Title;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleAwardRules {
    /// Source order is also the order of award notices.
    pub levels: Vec<LevelTitleAward>,
    /// Evaluated from the completed battle ledger, after level awards.
    /// This preserves Resonance's all-eligible-title result policy.
    pub combos: Vec<ComboTitleAward>,
    pub equipment: Vec<EquipmentTitleAward>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LevelTitleAward {
    pub character: u8,
    pub title: u8,
    pub minimum: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ComboTitleAward {
    pub character: u8,
    pub title: u8,
    pub minimum: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentTitleAward {
    pub character: u8,
    pub title: u8,
    /// Every requirement must match the recipient's final equipment.
    pub requirements: Vec<EquipmentRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentRequirement {
    /// Party equipment order: armor, head, shield, accessory, accessory, weapon.
    /// Any listed slot may hold any listed item.
    pub slots: Vec<u8>,
    pub items: Vec<u16>,
}

impl EquipmentTitleAward {
    pub fn matches(&self, equipment: &[u16; 6]) -> bool {
        self.requirements.iter().all(|requirement| {
            requirement.slots.iter().any(|&slot| {
                equipment
                    .get(usize::from(slot))
                    .is_some_and(|item| requirement.items.contains(item))
            })
        })
    }
}

impl TitleAwardRules {
    pub fn validate(&self, titles: &[Vec<Title>], item_count: usize) -> Result<()> {
        let mut awarded = BTreeSet::new();
        for (character, title) in self
            .levels
            .iter()
            .map(|r| (r.character, r.title))
            .chain(self.combos.iter().map(|r| (r.character, r.title)))
            .chain(self.equipment.iter().map(|r| (r.character, r.title)))
        {
            let count = character
                .checked_sub(1)
                .and_then(|index| titles.get(usize::from(index)))
                .map_or(0, Vec::len);
            ensure!(
                title != 0 && usize::from(title) <= count,
                "title award refers to an unknown character or title"
            );
            ensure!(awarded.insert((character, title)), "duplicate title award");
        }
        ensure!(
            self.levels.iter().all(|r| (1..=250).contains(&r.minimum)),
            "invalid title award level"
        );
        ensure!(
            self.combos.iter().all(|r| r.minimum != 0),
            "invalid title award combo threshold"
        );
        for rule in &self.equipment {
            ensure!(
                !rule.requirements.is_empty() && rule.requirements.len() <= 6,
                "invalid title equipment requirements"
            );
            let mut used_slots = BTreeSet::new();
            for requirement in &rule.requirements {
                ensure!(
                    !requirement.slots.is_empty()
                        && requirement
                            .slots
                            .iter()
                            .all(|&slot| slot < 6 && used_slots.insert(slot)),
                    "invalid or repeated title equipment slot"
                );
                let mut items = BTreeSet::new();
                ensure!(
                    !requirement.items.is_empty()
                        && requirement.items.iter().all(|&item| item != 0
                            && usize::from(item) <= item_count
                            && items.insert(item)),
                    "invalid or repeated title equipment item"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipment_rules_validate_references_and_any_slot_alternatives() -> Result<()> {
        let titles = vec![
            vec![Title {
                growth: [0; 7],
                costume: None
            }];
            9
        ];
        let mut rules = TitleAwardRules {
            equipment: vec![EquipmentTitleAward {
                character: 5,
                title: 1,
                requirements: vec![
                    EquipmentRequirement {
                        slots: vec![3, 4],
                        items: vec![2],
                    },
                    EquipmentRequirement {
                        slots: vec![5],
                        items: vec![3, 4],
                    },
                ],
            }],
            ..Default::default()
        };
        rules.validate(&titles, 4)?;
        let rule = &rules.equipment[0];
        assert!(rule.matches(&[0, 0, 0, 2, 0, 3]));
        assert!(rule.matches(&[0, 0, 0, 0, 2, 4]));
        assert!(!rule.matches(&[2, 0, 0, 0, 0, 3]));
        assert!(!rule.matches(&[0, 0, 0, 2, 0, 2]));
        assert!(rules.validate(&titles, 3).is_err());
        for slots in [vec![], vec![6], vec![3, 3], vec![5]] {
            let mut changed = rules.clone();
            changed.equipment[0].requirements[0].slots = slots;
            assert!(changed.validate(&titles, 4).is_err());
        }
        for items in [vec![], vec![0], vec![5], vec![2, 2]] {
            let mut changed = rules.clone();
            changed.equipment[0].requirements[0].items = items;
            assert!(changed.validate(&titles, 4).is_err());
        }
        rules.equipment[0].requirements.clear();
        assert!(rules.validate(&titles, 4).is_err());
        Ok(())
    }

    #[test]
    fn invalid_awards_are_rejected_before_play() -> Result<()> {
        let titles = vec![
            vec![Title {
                growth: [0; 7],
                costume: None
            }];
            9
        ];
        let mut rules = TitleAwardRules {
            levels: vec![LevelTitleAward {
                character: 1,
                title: 1,
                minimum: 20,
            }],
            combos: vec![],
            equipment: vec![],
        };
        rules.validate(&titles, 10)?;
        for invalid in [
            LevelTitleAward {
                character: 0,
                ..rules.levels[0]
            },
            LevelTitleAward {
                character: 10,
                ..rules.levels[0]
            },
            LevelTitleAward {
                title: 0,
                ..rules.levels[0]
            },
            LevelTitleAward {
                title: 2,
                ..rules.levels[0]
            },
            LevelTitleAward {
                minimum: 0,
                ..rules.levels[0]
            },
            LevelTitleAward {
                minimum: 251,
                ..rules.levels[0]
            },
        ] {
            let mut changed = rules.clone();
            changed.levels[0] = invalid;
            assert!(changed.validate(&titles, 10).is_err());
        }
        rules.levels.push(rules.levels[0]);
        assert!(rules.validate(&titles, 10).is_err());
        rules.levels.pop();
        rules.combos.push(ComboTitleAward {
            character: 1,
            title: 1,
            minimum: 10,
        });
        assert!(
            rules.validate(&titles, 10).is_err(),
            "duplicate across rule kinds"
        );
        rules.levels.clear();
        rules.validate(&titles, 10)?;
        rules.combos[0].minimum = 0;
        assert!(rules.validate(&titles, 10).is_err());
        rules.combos[0].minimum = 1;
        rules.validate(&titles, 10)?;
        Ok(())
    }
}
