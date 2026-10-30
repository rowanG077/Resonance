//! Named EX skills, shared combinations and stable saved character selections.
use crate::{
    diagnostics::Diagnostics, library::Mappings, menu_data as runtime, overworld::PartyCharacter,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const PATH: &str = "game/ex-skills.json";
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: Mappings,
    pub skills: BTreeMap<String, Skill>,
    pub characters: BTreeMap<PartyCharacter, Character>,
    pub gem_items: Gems,
    #[serde(default)]
    pub labels: Value,
    #[serde(default)]
    pub activation_labels: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    #[serde(default)]
    pub text: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stat_bonuses: Vec<runtime::ExStatBonus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_point_tp_cost: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tendency: Option<runtime::ExTendency>,
    #[serde(default = "constant", skip_serializing_if = "is_constant")]
    pub activation: runtime::ExActivation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub combinations: Vec<Combination>,
}
fn constant() -> runtime::ExActivation {
    runtime::ExActivation::Constant
}
fn is_constant(value: &runtime::ExActivation) -> bool {
    *value == constant()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Combination {
    pub characters: Vec<PartyCharacter>,
    pub requires: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    /// Four choices per gem level, in menu order.
    pub levels: BTreeMap<u8, [String; 4]>,
    /// Saved, character-local compound IDs, starting at zero.
    pub compounds: Mappings,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gems {
    #[serde(rename = "1")]
    pub level1: String,
    #[serde(rename = "2")]
    pub level2: String,
    #[serde(rename = "3")]
    pub level3: String,
    #[serde(rename = "4")]
    pub level4: String,
    pub max: String,
}
impl Gems {
    pub fn names(&self) -> [&str; 5] {
        [
            &self.level1,
            &self.level2,
            &self.level3,
            &self.level4,
            &self.max,
        ]
    }
}
impl Document {
    pub fn skill_id(&self, name: &str) -> Result<u8> {
        Ok(self
            .mappings
            .iter()
            .find(|(_, key)| *key == name)
            .with_context(|| format!("unknown EX skill {name}"))?
            .0
            .try_into()?)
    }
    pub fn compounds(&self, character: PartyCharacter) -> Result<&Mappings> {
        Ok(&self
            .characters
            .get(&character)
            .context("missing EX character")?
            .compounds)
    }
    pub fn compound_id(&self, character: PartyCharacter, name: &str) -> Result<u8> {
        Ok(self
            .compounds(character)?
            .iter()
            .find(|(_, key)| *key == name)
            .with_context(|| format!("unknown compound EX skill {name} for {character:?}"))?
            .0
            .try_into()?)
    }
    pub fn prepare(&self, items: &Mappings) -> Result<runtime::ExSkillData> {
        let mut skills = BTreeMap::new();
        let mut names = BTreeSet::new();
        for (id, name) in self.mappings.iter() {
            ensure!(
                id > 0 && names.insert(name),
                "EX skill mappings need distinct names and nonzero IDs"
            );
            let row = self
                .skills
                .get(name)
                .with_context(|| format!("missing EX skill {name}"))?;
            skills.insert(
                u8::try_from(id)?,
                runtime::ExSkill {
                    stat_bonuses: row.stat_bonuses.clone(),
                    save_point_tp_cost: row.save_point_tp_cost,
                    tendency: row.tendency,
                    activation: row.activation,
                },
            );
            let mut users = BTreeSet::new();
            for combination in &row.combinations {
                ensure!(
                    !combination.characters.is_empty(),
                    "EX combination has no characters"
                );
                for character in &combination.characters {
                    ensure!(
                        users.insert(*character),
                        "ambiguous EX combination for {character:?}: {name}"
                    );
                    ensure!(
                        self.compounds(*character)?
                            .iter()
                            .any(|(_, key)| key == name),
                        "unmapped EX combination for {character:?}: {name}"
                    );
                }
            }
        }
        ensure!(
            names.len() == self.skills.len() && self.characters.len() == 9,
            "unmapped EX skills or missing characters"
        );
        let mut characters = Vec::new();
        for id in 1..=9 {
            let character = PartyCharacter::try_from(id)?;
            let source = self
                .characters
                .get(&character)
                .context("missing EX character")?;
            ensure!(
                source.levels.len() == 4,
                "EX choices need levels one through four"
            );
            let mut levels = [[0; 4]; 4];
            for level in 1..=4 {
                let choices = source.levels.get(&level).context("missing EX gem level")?;
                for (slot, name) in choices.iter().enumerate() {
                    levels[usize::from(level - 1)][slot] = self.skill_id(name)?;
                }
            }
            let mut compounds = Vec::new();
            let mut names = BTreeSet::new();
            for (index, name) in source.compounds.iter() {
                ensure!(
                    index as usize == compounds.len() && names.insert(name),
                    "compound mappings need distinct names and consecutive IDs from zero"
                );
                let skill = self.skills.get(name).context("missing compound EX skill")?;
                let combination = skill
                    .combinations
                    .iter()
                    .find(|row| row.characters.contains(&character))
                    .with_context(|| format!("missing {character:?} combination for {name}"))?;
                compounds.push(runtime::CompoundExSkill {
                    skill: self.skill_id(name)?,
                    required: combination
                        .requires
                        .iter()
                        .map(|name| self.skill_id(name))
                        .collect::<Result<_>>()?,
                });
            }
            characters.push(runtime::CharacterExSkills { levels, compounds });
        }
        let mut gem_items = [0; 5];
        for (index, name) in self.gem_items.names().into_iter().enumerate() {
            gem_items[index] = items
                .iter()
                .find(|(_, key)| *key == name)
                .with_context(|| format!("unknown EX gem item {name}"))?
                .0
                .try_into()?;
        }
        let data = runtime::ExSkillData {
            skills,
            characters,
            gem_items,
            foraging: Default::default(),
        };
        data.validate_rules(
            items
                .iter()
                .map(|(id, _)| id as usize + 1)
                .max()
                .unwrap_or_default(),
        )?;
        Ok(data)
    }
    pub fn captions(&self, diagnostics: &Diagnostics) -> Result<Option<runtime::ExSkillText>> {
        diagnostics.attempt(
            "EX skill captions",
            (|| {
                Ok(runtime::ExSkillText {
                    skills: self
                        .mappings
                        .iter()
                        .map(|(id, name)| {
                            Ok((
                                u8::try_from(id)?,
                                serde_json::from_value(
                                    self.skills
                                        .get(name)
                                        .context("missing EX skill")?
                                        .text
                                        .clone(),
                                )?,
                            ))
                        })
                        .collect::<Result<_>>()?,
                    labels: serde_json::from_value(self.labels.clone())?,
                    activation_labels: serde_json::from_value(self.activation_labels.clone())?,
                })
            })(),
        )
    }
}
