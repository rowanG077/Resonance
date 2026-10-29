//! Named titles: one owner for growth, costumes, captions and award parameters.
use crate::{diagnostics::Diagnostics, library::Mappings, menu_data, overworld::PartyCharacter};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PATH: &str = "game/titles.json";
mod awards;
pub use awards::*;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: BTreeMap<PartyCharacter, Mappings>,
    pub titles: BTreeMap<String, Entry>,
    pub awards: Awards,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// HP/TP are whole points; the other stats support tenths of a point.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub growth: BTreeMap<Statistic, f64>,
    /// Named costume in the character's existing battle/party mappings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costume: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub text: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Statistic {
    Hp,
    Tp,
    Strength,
    Defense,
    Intelligence,
    Evasion,
    Accuracy,
}
impl Statistic {
    pub const ALL: [Self; 7] = [
        Self::Hp,
        Self::Tp,
        Self::Strength,
        Self::Defense,
        Self::Intelligence,
        Self::Evasion,
        Self::Accuracy,
    ];
    pub(crate) fn scale(self) -> f64 {
        if matches!(self, Self::Hp | Self::Tp) {
            1.
        } else {
            10.
        }
    }
}

impl Entry {
    pub fn from_parts(rule: &menu_data::Title, text: Value, costume: Option<String>) -> Self {
        Self {
            growth: Statistic::ALL
                .into_iter()
                .zip(rule.growth)
                .filter(|(_, n)| *n != 0)
                .map(|(stat, n)| (stat, f64::from(n) / stat.scale()))
                .collect(),
            costume,
            text,
        }
    }
    fn growth(&self) -> Result<[u8; 7]> {
        let mut growth = [0; 7];
        for (i, stat) in Statistic::ALL.into_iter().enumerate() {
            let value = self.growth.get(&stat).copied().unwrap_or_default() * stat.scale();
            ensure!(
                value.is_finite()
                    && (0. ..=255.).contains(&value)
                    && (value - value.round()).abs() < 1e-9,
                "invalid title growth for {stat:?}: HP/TP use whole points, other stats use tenths"
            );
            growth[i] = value.round() as u8;
        }
        Ok(growth)
    }
}

pub struct Prepared {
    pub rules: Vec<Vec<menu_data::Title>>,
    pub awards: menu_data::TitleAwardRules,
    pub text: Vec<Vec<Value>>,
}
impl Prepared {
    pub fn captions(
        &self,
        diagnostics: &Diagnostics,
    ) -> Result<Vec<Vec<Option<menu_data::NamedText>>>> {
        self.text
            .iter()
            .enumerate()
            .map(|(character, rows)| {
                rows.iter()
                    .enumerate()
                    .map(|(id, text)| caption(text, character, id, diagnostics))
                    .collect()
            })
            .collect()
    }
}
fn caption(
    text: &Value,
    character: usize,
    id: usize,
    diagnostics: &Diagnostics,
) -> Result<Option<menu_data::NamedText>> {
    if text.is_null() {
        return Ok(None);
    }
    diagnostics.attempt(
        &format!("title caption {}:{}", character + 1, id + 1),
        serde_json::from_value(text.clone()).map_err(Into::into),
    )
}

impl Document {
    pub fn prepare(
        &self,
        items: &Mappings,
        mut costume: impl FnMut(PartyCharacter, &str) -> Result<menu_data::Costume>,
    ) -> Result<Prepared> {
        ensure!(
            self.mappings.len() == 9,
            "title mappings must cover the nine party members"
        );
        let mut rules = Vec::new();
        let mut text = Vec::new();
        for character in 1..=9 {
            let member = PartyCharacter::try_from(character)?;
            let mapping = self
                .mappings
                .get(&member)
                .context("missing character title mappings")?;
            let mut names = std::collections::BTreeSet::new();
            let mut rows = Vec::new();
            let mut captions = Vec::new();
            for (index, (id, name)) in mapping.iter().enumerate() {
                ensure!(
                    id as usize == index + 1 && id <= 255,
                    "title IDs must cover one through the last title"
                );
                ensure!(
                    names.insert(name),
                    "a character's title names must have unique IDs"
                );
                let entry = self
                    .titles
                    .get(name)
                    .with_context(|| format!("missing title {name}"))?;
                rows.push(menu_data::Title {
                    growth: entry.growth()?,
                    costume: entry
                        .costume
                        .as_deref()
                        .map(|name| costume(member, name))
                        .transpose()?,
                });
                captions.push(entry.text.clone());
            }
            ensure!(!rows.is_empty(), "character has no titles");
            rules.push(rows);
            text.push(captions);
        }
        let awards = self.awards.prepare(&self.mappings, items)?;
        awards.validate(&rules, items.iter().count().saturating_sub(1))?;
        Ok(Prepared {
            rules,
            awards,
            text,
        })
    }

    /// Dialogue needs labels only, without preparing costumes or gameplay.
    pub fn names(&self, diagnostics: &Diagnostics) -> Result<BTreeMap<u16, String>> {
        let mut names = BTreeMap::new();
        for (&character, mappings) in &self.mappings {
            for (id, name) in mappings.iter() {
                ensure!((1..=255).contains(&id), "invalid title ID");
                let entry = self
                    .titles
                    .get(name)
                    .with_context(|| format!("missing title {name}"))?;
                if entry.text.is_null() {
                    continue;
                }
                if let Some(label) = diagnostics.attempt(
                    &format!("title name {name}"),
                    serde_json::from_value::<String>(entry.text["name"].clone())
                        .map_err(Into::into),
                )? {
                    names.insert((u16::from(character as u8 - 1) << 8) | id as u16, label);
                }
            }
        }
        Ok(names)
    }
}

pub fn capture(snapshot: &mut crate::library::Snapshot) -> Result<()> {
    let document: Document = snapshot.json(PATH, crate::preparation::JSON_LIMIT)?;
    for (&character, mappings) in &document.mappings {
        if mappings.iter().any(|(_, name)| {
            document
                .titles
                .get(name)
                .is_some_and(|t| t.costume.is_some())
        }) {
            let path = snapshot.resolve(
                crate::battle_model::document::PARTY,
                character as u32,
                Some("mappings.json"),
            )?;
            snapshot.read(&path, crate::preparation::JSON_LIMIT)?;
        }
    }
    Ok(())
}

pub fn load(files: &crate::prepared::Files, items: &Mappings) -> Result<Prepared> {
    let document: Document = files.json(PATH)?;
    let mut costumes = BTreeMap::<PartyCharacter, Mappings>::new();
    document.prepare(items, |character, name| {
        if let std::collections::btree_map::Entry::Vacant(entry) = costumes.entry(character) {
            let parties: Mappings = files.json(crate::battle_model::document::PARTY)?;
            let path = parties.resolve(
                crate::battle_model::document::PARTY,
                character as u32,
                Some("mappings.json"),
            )?;
            entry.insert(files.json(&path)?);
        }
        let id = costumes[&character]
            .iter()
            .find(|(_, target)| *target == name)
            .with_context(|| format!("unknown title costume {character:?}/{name}"))?
            .0;
        Ok(match id {
            0 => menu_data::Costume::Standard,
            1 => menu_data::Costume::Variant1,
            2 => menu_data::Costume::Variant2,
            3 => menu_data::Costume::Story,
            4 => menu_data::Costume::Variant4,
            _ => anyhow::bail!("unsupported title costume slot {id}"),
        })
    })
}
