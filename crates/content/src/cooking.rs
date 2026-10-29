//! Named recipes and ordered ingredients, independent of source table layout.
use crate::{
    diagnostics::Diagnostics, library::Mappings, menu_data as runtime, overworld::PartyCharacter,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const PATH: &str = "game/cooking.json";
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: Mappings,
    pub recipes: BTreeMap<String, Recipe>,
    pub groups: BTreeMap<String, Group>,
    pub preferences: BTreeMap<PartyCharacter, Preferences>,
    /// Named skill from the shared EX catalogue.
    pub genis_bonus_ex_skill: String,
    #[serde(default)]
    pub labels: Value,
    #[serde(default)]
    pub effect_names: BTreeMap<runtime::MealEffect, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[serde(default)]
    pub text: Value,
    pub effects: Vec<runtime::MealEffect>,
    pub required: Vec<Ingredient>,
    pub cooks: BTreeMap<PartyCharacter, Cook>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cook {
    pub stars: u8,
    /// Recovery at mastery 0–2, 3–5, and 6–8. Authored values, not a formula.
    pub recovery: [u8; 3],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<Ingredient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Ingredient {
    Item(String),
    Selection(Selection),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub any: Option<String>,
    #[serde(default, skip_serializing_if = "zero")]
    pub from_mastery: u8,
    #[serde(default = "eight", skip_serializing_if = "is_eight")]
    pub through_mastery: u8,
}
fn zero(n: &u8) -> bool {
    *n == 0
}
fn eight() -> u8 {
    8
}
fn is_eight(n: &u8) -> bool {
    *n == 8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    #[serde(default)]
    pub name: Value,
    pub icon: Icon,
    /// Order affects ingredient selection; repeated entries are meaningful.
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Icon {
    Meat,
    Fish,
    Vegetable,
    Fruit,
    Carbs,
    MiscellaneousFood,
    Greens,
}
impl Icon {
    pub fn from_index(index: u8) -> Result<Self> {
        Ok(match index {
            7 => Self::Meat,
            8 => Self::Fish,
            9 => Self::Vegetable,
            10 => Self::Fruit,
            11 => Self::Carbs,
            12 => Self::MiscellaneousFood,
            49 => Self::Greens,
            _ => anyhow::bail!("unsupported cooking icon {index}"),
        })
    }
    pub fn index(self) -> u8 {
        match self {
            Self::Meat => 7,
            Self::Fish => 8,
            Self::Vegetable => 9,
            Self::Fruit => 10,
            Self::Carbs => 11,
            Self::MiscellaneousFood => 12,
            Self::Greens => 49,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dislikes: Vec<String>,
}

impl Ingredient {
    fn prepare(
        &self,
        items: &BTreeMap<&str, u32>,
        groups: &BTreeMap<&str, u8>,
    ) -> Result<(runtime::Ingredient, u8, u8)> {
        let item = |name: &str| -> Result<runtime::Ingredient> {
            Ok(runtime::Ingredient::Item(item_id(items, name)?))
        };
        Ok(match self {
            Self::Item(name) => (item(name)?, 0, 8),
            Self::Selection(rule) => {
                ensure!(
                    matches!(rule.from_mastery, 0 | 3 | 6)
                        && matches!(rule.through_mastery, 2 | 5 | 8)
                        && rule.from_mastery <= rule.through_mastery,
                    "ingredient mastery bounds must cover complete 0–2, 3–5 or 6–8 grades"
                );
                let ingredient = match (&rule.item, &rule.any) {
                    (Some(name), None) => item(name)?,
                    (None, Some(name)) => runtime::Ingredient::Any(
                        *groups
                            .get(name.as_str())
                            .with_context(|| format!("unknown ingredient group {name}"))?,
                    ),
                    _ => anyhow::bail!("ingredient needs exactly one of item or any"),
                };
                (ingredient, rule.from_mastery, rule.through_mastery)
            }
        })
    }
}
fn item_id(items: &BTreeMap<&str, u32>, name: &str) -> Result<u16> {
    let id = *items
        .get(name)
        .with_context(|| format!("unknown cooking item {name}"))?;
    ensure!(id != 0, "empty item cannot be a cooking ingredient");
    Ok(id.try_into()?)
}

impl Document {
    pub fn prepare(&self, items: &Mappings, skills: &Mappings) -> Result<runtime::CookingData> {
        let item_count = items
            .iter()
            .map(|(id, _)| id as usize + 1)
            .max()
            .unwrap_or_default();
        let items: BTreeMap<_, _> = items.iter().map(|(id, name)| (name, id)).collect();
        let groups: BTreeMap<_, _> = self
            .groups
            .keys()
            .enumerate()
            .map(|(id, name)| Ok((name.as_str(), u8::try_from(id)?)))
            .collect::<Result<_>>()?;
        let mut recipes = Vec::new();
        let mut names = BTreeSet::new();
        for (id, name) in self.mappings.iter() {
            ensure!(
                id as usize == recipes.len() && names.insert(name),
                "recipe mappings must have distinct names and consecutive IDs from zero"
            );
            let recipe = self
                .recipes
                .get(name)
                .with_context(|| format!("missing recipe {name}"))?;
            let required = recipe
                .required
                .iter()
                .map(|ingredient| {
                    let (ingredient, from, through) = ingredient.prepare(&items, &groups)?;
                    ensure!(
                        from == 0 && through == 8,
                        "required ingredients cannot depend on mastery"
                    );
                    Ok(ingredient)
                })
                .collect::<Result<_>>()?;
            ensure!(
                recipe.cooks.len() == 9,
                "recipe {name} needs nine named cooks"
            );
            let cooks = (1..=9)
                .map(|id| {
                    let character = PartyCharacter::try_from(id)?;
                    let cook = recipe
                        .cooks
                        .get(&character)
                        .context("missing recipe cook")?;
                    let extras = cook
                        .extras
                        .iter()
                        .map(|ingredient| ingredient.prepare(&items, &groups))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(runtime::RecipeCook {
                        base_stars: cook.stars,
                        grades: std::array::from_fn(|grade| runtime::RecipeGrade {
                            effects: recipe.effects.clone(),
                            recovery: cook.recovery[grade],
                            extras: extras
                                .iter()
                                .filter(|(_, from, through)| {
                                    *from <= grade as u8 * 3 && *through >= grade as u8 * 3
                                })
                                .map(|(ingredient, _, _)| *ingredient)
                                .collect(),
                        }),
                    })
                })
                .collect::<Result<_>>()?;
            recipes.push(runtime::Recipe { required, cooks });
        }
        ensure!(names.len() == self.recipes.len(), "unmapped cooking recipe");
        let groups = self
            .groups
            .values()
            .map(|group| {
                Ok(runtime::IngredientGroup {
                    category: group.icon.index(),
                    items: group
                        .items
                        .iter()
                        .map(|name| item_id(&items, name))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        let preferences = (1..=9)
            .map(|id| {
                let character = PartyCharacter::try_from(id)?;
                let preference = self
                    .preferences
                    .get(&character)
                    .cloned()
                    .unwrap_or_default();
                Ok(runtime::FoodPreferences {
                    likes: preference
                        .likes
                        .iter()
                        .map(|name| item_id(&items, name))
                        .collect::<Result<_>>()?,
                    dislikes: preference
                        .dislikes
                        .iter()
                        .map(|name| item_id(&items, name))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        let data = runtime::CookingData {
            recipes,
            groups,
            preferences,
            bonus_skill: skills
                .iter()
                .find(|(_, name)| *name == self.genis_bonus_ex_skill)
                .context("unknown cooking bonus EX skill")?
                .0
                .try_into()?,
        };
        data.validate(item_count)?;
        Ok(data)
    }

    pub fn captions(&self, diagnostics: &Diagnostics) -> Result<Option<runtime::CookingText>> {
        let recipes = self
            .mappings
            .iter()
            .map(|(_, name)| {
                let text = &self.recipes.get(name).context("missing recipe")?.text;
                diagnostics.attempt(
                    &format!("recipe caption {name}"),
                    serde_json::from_value(text.clone()).map_err(Into::into),
                )
            })
            .collect::<Result<_>>()?;
        let groups = diagnostics.attempt(
            "Cooking group captions",
            self.groups
                .values()
                .map(|group| Ok(serde_json::from_value(group.name.clone())?))
                .collect::<Result<_>>(),
        )?;
        let labels = diagnostics.attempt(
            "Cooking labels",
            serde_json::from_value(self.labels.clone()).map_err(Into::into),
        )?;
        let effects = diagnostics.attempt(
            "Cooking effect captions",
            runtime::MealEffect::ALL
                .into_iter()
                .map(|effect| {
                    Ok(serde_json::from_value(
                        self.effect_names
                            .get(&effect)
                            .context("missing cooking effect caption")?
                            .clone(),
                    )?)
                })
                .collect::<Result<Vec<String>>>()
                .map(|values| values.try_into().unwrap()),
        )?;
        Ok(match (groups, labels) {
            (Some(groups), Some(labels)) => Some(runtime::CookingText {
                recipes,
                groups,
                labels,
                effects,
            }),
            _ => None,
        })
    }
}
