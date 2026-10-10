//! New Game Plus purchases use named benefits and authored prices.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const UNITS_PER_GRADE: u32 = 100;
pub const MAX_GRADE: u32 = 99_999_999;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Benefit {
    ExSkills,
    ExGems,
    Affection,
    IncreasedTension,
    PlayTime,
    MemoryCircles,
    ThirtyItems,
    Gald,
    Recipes,
    CookingAbility,
    Titles,
    Figurines,
    MonsterList,
    CollectorsBook,
    WorldMap,
    MiniGames,
    BattleInfo,
    Tech,
    TechUsage,
    IncreasedHp,
    MinimumHp,
    ComboExperience,
    HalfExperience,
    DoubleExperience,
    TenfoldExperience,
    IncreasedGrade,
}
impl Benefit {
    pub const ALL: [Self; 26] = [
        Self::ExSkills,
        Self::ExGems,
        Self::Affection,
        Self::IncreasedTension,
        Self::PlayTime,
        Self::MemoryCircles,
        Self::ThirtyItems,
        Self::Gald,
        Self::Recipes,
        Self::CookingAbility,
        Self::Titles,
        Self::Figurines,
        Self::MonsterList,
        Self::CollectorsBook,
        Self::WorldMap,
        Self::MiniGames,
        Self::BattleInfo,
        Self::Tech,
        Self::TechUsage,
        Self::IncreasedHp,
        Self::MinimumHp,
        Self::ComboExperience,
        Self::HalfExperience,
        Self::DoubleExperience,
        Self::TenfoldExperience,
        Self::IncreasedGrade,
    ];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shop {
    pub options: Vec<Purchase>,
    pub labels: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Purchase {
    pub benefit: Benefit,
    pub price: u32,
    pub name: String,
    pub description: String,
    pub excludes: Vec<Benefit>,
}
impl Shop {
    pub fn validate(&self) -> Result<()> {
        let benefits: BTreeSet<_> = self.options.iter().map(|p| p.benefit).collect();
        ensure!(
            self.options.len() == Benefit::ALL.len() && benefits.len() == Benefit::ALL.len(),
            "incomplete Grade Shop"
        );
        for option in &self.options {
            ensure!(
                !option.name.is_empty()
                    && !option.description.is_empty()
                    && option.price <= MAX_GRADE / UNITS_PER_GRADE,
                "invalid Grade Shop purchase"
            );
            ensure!(
                !option.excludes.contains(&option.benefit),
                "Grade Shop purchase excludes itself"
            );
        }
        for label in [
            "heading",
            "grade",
            "finish",
            "confirmation",
            "yes",
            "no",
            "total_cost",
        ] {
            ensure!(
                self.labels.get(label).is_some_and(|s| !s.is_empty()),
                "missing Grade Shop label {label}"
            );
        }
        Ok(())
    }
    pub fn cost(&self, selected: &BTreeSet<Benefit>) -> Result<u32> {
        let mut total = 0u32;
        for benefit in selected {
            let option = self
                .options
                .iter()
                .find(|p| p.benefit == *benefit)
                .ok_or_else(|| anyhow::anyhow!("Grade Shop purchase is missing"))?;
            ensure!(
                option.excludes.iter().all(|b| !selected.contains(b)),
                "conflicting Grade Shop purchases"
            );
            total = total
                .checked_add(option.price * UNITS_PER_GRADE)
                .ok_or_else(|| anyhow::anyhow!("Grade Shop total overflow"))?;
        }
        Ok(total)
    }
}
