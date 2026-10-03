//! Equipment customization recipes and vendor labels.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Data {
    pub recipes: Vec<Recipe>,
    pub vendors: Vec<Vendor>,
    pub labels: Labels,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Labels {
    pub heading: String,
    pub confirmation: String,
    pub yes: String,
    pub no: String,
    pub missing_materials: String,
    pub inventory_full: String,
}
impl Labels {
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        [
            &self.heading,
            &self.confirmation,
            &self.yes,
            &self.no,
            &self.missing_materials,
            &self.inventory_full,
        ]
        .into_iter()
        .map(String::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub result: u16,
    pub ingredients: BTreeMap<u16, u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vendor {
    pub name: String,
    pub recipes: Vec<u16>,
}

impl Data {
    pub fn validate(&self, items: usize) -> Result<()> {
        ensure!(
            self.labels.texts().all(|s| !s.is_empty()),
            "missing crafting labels"
        );
        for recipe in &self.recipes {
            ensure!(
                usize::from(recipe.result) < items
                    && recipe.ingredients.iter().all(|(&item, &count)| item != 0
                        && usize::from(item) < items
                        && count != 0),
                "invalid crafting recipe"
            );
        }
        for vendor in &self.vendors {
            ensure!(
                !vendor.name.is_empty()
                    && vendor.recipes.iter().all(|&id| {
                        self.recipes
                            .get(usize::from(id))
                            .is_some_and(|r| r.result != 0)
                    }),
                "invalid crafting vendor"
            );
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.labels
            .texts()
            .chain(self.vendors.iter().map(|v| v.name.as_str()))
    }
}
