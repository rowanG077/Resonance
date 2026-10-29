//! Character names and fresh-game defaults, independent of source storage layouts.
use crate::{
    diagnostics::Diagnostics,
    library::Mappings,
    menu_data,
    session::{CharacterDefinition, StatGrowth},
    titles::{Slot, Statistic},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
mod initial;
pub use initial::*;
#[cfg(test)]
mod tests;

pub const PATH: &str = "game/characters.json";
pub type GrowthTable = BTreeMap<Statistic, Gain>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: Mappings,
    pub characters: BTreeMap<String, Entry>,
    /// Total experience required at each level. Level zero is not playable.
    pub experience: BTreeMap<u8, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub growth_profiles: BTreeMap<String, GrowthTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    /// Optional status-page format. The current saved name replaces {name}.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub full_name: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<Initial>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth: Option<Growth>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Growth {
    Shared(String),
    Inline(GrowthTable),
}

/// Inclusive gain, in stat points. A number is a fixed gain; a pair is min/max.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Gain {
    Fixed(f64),
    Range([f64; 2]),
}

impl Gain {
    fn from_parts(stat: Statistic, base: u8, random: u8) -> Self {
        let maximum = f64::from(u16::from(base) + u16::from(random)) / stat.scale();
        let base = f64::from(base) / stat.scale();
        if random == 0 {
            Self::Fixed(base)
        } else {
            Self::Range([base, maximum])
        }
    }
    fn prepare(&self, stat: Statistic, title_bonus: u8) -> Result<StatGrowth> {
        let [base, maximum] = match *self {
            Self::Fixed(n) => [n, n],
            Self::Range(range) => range,
        };
        let base = units(stat, base, 255)?;
        let maximum = units(stat, maximum, 510)?;
        let random = maximum
            .checked_sub(base)
            .context("growth maximum is below minimum")?;
        Ok(StatGrowth {
            base: base as u8,
            random: u8::try_from(random).context("growth range exceeds 255 steps")?,
            title_bonus,
        })
    }
}

fn units(stat: Statistic, points: f64, maximum: u16) -> Result<u16> {
    let value = points * stat.scale();
    ensure!(
        value.is_finite()
            && (0. ..=f64::from(maximum)).contains(&value)
            && (value - value.round()).abs() < 1e-8,
        "invalid {stat:?} value: HP/TP use whole points, other stats use tenths"
    );
    Ok(value.round() as u16)
}

impl Document {
    fn entry(&self, id: u32) -> Result<&Entry> {
        let name = self.mappings.target(id)?;
        self.characters
            .get(name)
            .with_context(|| format!("missing character {name}"))
    }
    pub fn names(&self) -> Result<BTreeMap<i32, String>> {
        self.mappings
            .iter()
            .map(|(id, _)| {
                let name = &self.entry(id)?.name;
                ensure!(
                    !name.is_empty() && !name.chars().any(char::is_control),
                    "invalid character name"
                );
                Ok((i32::try_from(id)?, name.clone()))
            })
            .collect()
    }
    pub fn initial_names(&self) -> Result<[String; 9]> {
        let mut names: [String; 9] = Default::default();
        for (index, name) in names.iter_mut().enumerate() {
            *name = self.entry(index as u32 + 1)?.name.clone();
        }
        Ok(names)
    }
    pub fn full_names(&self, diagnostics: &Diagnostics) -> Result<Option<Vec<String>>> {
        diagnostics.attempt(
            "character full names",
            (1..=9)
                .map(|id| {
                    let value = &self.entry(id)?.full_name;
                    if value.is_null() {
                        Ok("{name}".into())
                    } else {
                        serde_json::from_value(value.clone()).map_err(Into::into)
                    }
                })
                .collect(),
        )
    }
    pub fn experience_curve(&self) -> Result<Vec<u32>> {
        let mut curve = vec![0];
        for (&level, &amount) in &self.experience {
            ensure!(
                usize::from(level) == curve.len(),
                "experience levels must cover one through the last level"
            );
            ensure!(
                amount >= *curve.last().unwrap(),
                "experience curve decreases"
            );
            curve.push(amount);
        }
        ensure!(curve.len() >= 2, "experience curve is empty");
        Ok(curve)
    }
    pub fn prepare(
        &self,
        items: &Mappings,
        techniques: &crate::arte::document::Prepared,
        titles: &[Vec<menu_data::Title>],
        recipes: &Mappings,
        ex: &crate::ex_skills::Document,
    ) -> Result<Vec<CharacterDefinition>> {
        let items: BTreeMap<_, _> = items.iter().map(|(id, name)| (name, id)).collect();
        let tech_ids: BTreeMap<_, _> = techniques
            .mappings
            .iter()
            .map(|(id, name)| (name, id))
            .collect();
        let recipe_ids = recipes.iter().map(|(id, name)| (name, id)).collect();
        let mut result = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for (id, name) in self.mappings.iter() {
            ensure!(
                id > 0 && names.insert(name),
                "character mappings need unique nonzero identities"
            );
            let entry = self.entry(id)?;
            if id > 9 {
                ensure!(
                    entry.initial.is_none() && entry.growth.is_none(),
                    "only party characters have starting state"
                );
            }
        }
        for index in 0..9 {
            let entry = self.entry(index as u32 + 1)?;
            let initial = entry
                .initial
                .as_ref()
                .context("party character needs initial state")?;
            let growth = match entry
                .growth
                .as_ref()
                .context("party character needs growth")?
            {
                Growth::Inline(table) => table,
                Growth::Shared(name) => self
                    .growth_profiles
                    .get(name)
                    .with_context(|| format!("missing growth profile {name}"))?,
            };
            let bonus = titles
                .get(index)
                .and_then(|t| t.first())
                .context("missing initial title")?
                .growth;
            let mut gains = Vec::new();
            for (i, stat) in Statistic::ALL.into_iter().enumerate() {
                gains.push(
                    growth
                        .get(&stat)
                        .unwrap_or(&Gain::Fixed(0.))
                        .prepare(stat, bonus[i])?,
                );
            }
            let mut character = initial.prepare(
                &items,
                &tech_ids,
                &recipe_ids,
                ex,
                crate::overworld::PartyCharacter::try_from(index as u8 + 1)?,
            )?;
            character.growth = gains.try_into().unwrap();
            character.allowed_techniques =
                techniques.catalogue.learned_by(index as u8 + 1)?.to_vec();
            character.level_techniques = techniques.level_techniques(index as u8 + 1)?;
            result.push(character);
        }
        Ok(result)
    }

    pub fn from_parts(
        data: &crate::session::SessionData,
        names: &BTreeMap<i32, String>,
        full_names: &[String],
        mappings: Mappings,
        items: &Mappings,
        techniques: &Mappings,
        recipes: &Mappings,
        ex: &crate::ex_skills::Document,
    ) -> Result<Self> {
        ensure!(
            data.characters.len() == 9
                && full_names.len() == 9
                && data.experience.first() == Some(&0),
            "invalid character source shape"
        );
        let mut characters = BTreeMap::new();
        for (id, key) in mappings.iter() {
            let name = names
                .get(&i32::try_from(id)?)
                .context("missing character caption")?
                .clone();
            let character = if (1..=9).contains(&id) {
                let i = id as usize - 1;
                let source = &data.characters[i];
                Entry {
                    name,
                    full_name: full_names[i].clone().into(),
                    initial: Some(Initial::from_parts(
                        source,
                        items,
                        techniques,
                        recipes,
                        ex,
                        crate::overworld::PartyCharacter::try_from(id as u8)?,
                    )?),
                    growth: Some(Growth::Inline(
                        Statistic::ALL
                            .into_iter()
                            .zip(&source.growth)
                            .filter(|(_, g)| g.base != 0 || g.random != 0)
                            .map(|(stat, g)| (stat, Gain::from_parts(stat, g.base, g.random)))
                            .collect(),
                    )),
                }
            } else {
                Entry {
                    name,
                    full_name: Value::Null,
                    initial: None,
                    growth: None,
                }
            };
            ensure!(
                characters.insert(key.into(), character).is_none(),
                "duplicate character name"
            );
        }
        Ok(Self {
            mappings,
            characters,
            experience: data
                .experience
                .iter()
                .enumerate()
                .skip(1)
                .map(|(level, &xp)| Ok((u8::try_from(level)?, xp)))
                .collect::<Result<_>>()?,
            growth_profiles: Default::default(),
        })
    }
}
