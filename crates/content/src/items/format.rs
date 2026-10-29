use super::*;

/// Category names preserve inventory ordering and implied equipment slots.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Category(u8);
impl Category {
    const NAMES: [&str; 48] = [
        "unclassified",
        "gel",
        "medicine",
        "bottle",
        "herb",
        "utility",
        "quartz",
        "meat",
        "fish",
        "vegetable",
        "fruit",
        "carbs",
        "miscellaneous_food",
        "dual_blades",
        "dagger",
        "chakram",
        "bow",
        "kendama",
        "staff",
        "card",
        "sword",
        "axe",
        "leg_guard",
        "armor",
        "guard",
        "cloak",
        "robe",
        "helmet",
        "hat",
        "ribbon",
        "circlet",
        "shield",
        "gauntlet",
        "bracelet",
        "gloves",
        "charm",
        "ring",
        "symbol",
        "cape",
        "boots",
        "jewel",
        "doll",
        "article",
        "collectible",
        "battle_item",
        "key_item",
        "trade_material",
        "other",
    ];
    pub fn from_index(index: u8) -> Result<Self> {
        ensure!(
            usize::from(index) < Self::NAMES.len(),
            "unknown item category {index}"
        );
        Ok(Self(index))
    }
    pub fn index(self) -> u8 {
        self.0
    }
    pub fn equipment_kind(self) -> Option<u8> {
        match self.0 {
            13..=22 => Some(0),
            23..=26 => Some(1),
            27..=30 => Some(2),
            31..=34 => Some(3),
            35..=42 => Some(4),
            _ => None,
        }
    }
}
impl TryFrom<String> for Category {
    type Error = anyhow::Error;
    fn try_from(name: String) -> Result<Self> {
        Ok(Self(
            Self::NAMES
                .iter()
                .position(|&value| value == name)
                .with_context(|| format!("unknown item category {name}"))? as u8,
        ))
    }
}
impl From<Category> for String {
    fn from(category: Category) -> Self {
        Category::NAMES[category.0 as usize].into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum Statistic {
    Slash,
    Thrust,
    Defense,
    Intelligence,
    Accuracy,
    Evasion,
    Luck,
}
impl Statistic {
    pub const ALL: [Self; 7] = [
        Self::Slash,
        Self::Thrust,
        Self::Defense,
        Self::Intelligence,
        Self::Accuracy,
        Self::Evasion,
        Self::Luck,
    ];
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterGroup {
    All,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Characters {
    Group(CharacterGroup),
    Named(Vec<PartyCharacter>),
}
impl Default for Characters {
    fn default() -> Self {
        Self::Named(Vec::new())
    }
}
impl Characters {
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Named(names) if names.is_empty())
    }
    pub fn from_mask(mask: u16) -> Result<Self> {
        ensure!(mask < 512, "unknown item character mask");
        if mask == 511 {
            return Ok(Self::Group(CharacterGroup::All));
        }
        Ok(Self::Named(
            (1..=9)
                .filter(|id| mask & (1 << (id - 1)) != 0)
                .map(|id| PartyCharacter::try_from(id).map_err(anyhow::Error::from))
                .collect::<Result<_>>()?,
        ))
    }
    pub fn mask(&self) -> Result<u16> {
        let Self::Named(names) = self else {
            return Ok(511);
        };
        let mut mask = 0;
        for character in names {
            let bit = 1 << (*character as u8 - 1);
            ensure!(mask & bit == 0, "duplicate item character");
            mask |= bit;
        }
        Ok(mask)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum GrowthStat {
    MaxHp,
    MaxTp,
    Strength,
    Defense,
    Intelligence,
    Evasion,
    Accuracy,
}
impl GrowthStat {
    const ALL: [Self; 7] = [
        Self::MaxHp,
        Self::MaxTp,
        Self::Strength,
        Self::Defense,
        Self::Intelligence,
        Self::Evasion,
        Self::Accuracy,
    ];
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncounterAura {
    Holy,
    Dark,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Recover {
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        hp: u8,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        tp: u8,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        party: bool,
    },
    Revive,
    Cure,
    Transform,
    /// Actual stat points, or a percentage when percent is true.
    Herb {
        stat: GrowthStat,
        amount: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        percent: bool,
    },
    EncounterRate {
        aura: EncounterAura,
    },
}
impl TryFrom<ItemUse> for Action {
    type Error = anyhow::Error;
    fn try_from(action: ItemUse) -> Result<Self> {
        Ok(match action {
            ItemUse::Recover { hp, tp, party } => Self::Recover { hp, tp, party },
            ItemUse::Revive => Self::Revive,
            ItemUse::Cure => Self::Cure,
            ItemUse::Transform => Self::Transform,
            ItemUse::Herb {
                stat,
                amount,
                percent,
            } => Self::Herb {
                stat: *GrowthStat::ALL
                    .get(stat)
                    .context("unknown herb statistic")?,
                amount: f64::from(amount) / if percent { 1.0 } else { 10.0 },
                percent,
            },
            ItemUse::EncounterRate { rate } => Self::EncounterRate {
                aura: match rate {
                    1 => EncounterAura::Holy,
                    2 => EncounterAura::Dark,
                    _ => anyhow::bail!("unknown encounter aura"),
                },
            },
        })
    }
}
impl TryFrom<Action> for ItemUse {
    type Error = anyhow::Error;
    fn try_from(action: Action) -> Result<Self> {
        Ok(match action {
            Action::Recover { hp, tp, party } => Self::Recover { hp, tp, party },
            Action::Revive => Self::Revive,
            Action::Cure => Self::Cure,
            Action::Transform => Self::Transform,
            Action::Herb {
                stat,
                amount,
                percent,
            } => {
                let value = amount * if percent { 1.0 } else { 10.0 };
                ensure!(
                    value.is_finite()
                        && (0.0..=f64::from(u16::MAX)).contains(&value)
                        && (value - value.round()).abs() < 1e-8,
                    "herb amounts require whole percentages or tenths of a stat point"
                );
                Self::Herb {
                    stat: stat as usize,
                    amount: value.round() as u16,
                    percent,
                }
            }
            Action::EncounterRate { aura } => Self::EncounterRate {
                rate: match aura {
                    EncounterAura::Holy => 1,
                    EncounterAura::Dark => 2,
                },
            },
        })
    }
}
