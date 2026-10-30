use super::*;
use crate::{
    library::Mappings,
    menu_data::document::{item_ids, slots, unique_name},
};
use anyhow::Context;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: Mappings,
    pub vendors: BTreeMap<String, NamedVendor>,
    pub recipes: BTreeMap<String, NamedRecipe>,
    pub labels: Labels,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedVendor {
    pub name: String,
    pub recipes: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedRecipe {
    pub result: String,
    pub ingredients: BTreeMap<String, u16>,
}
impl Document {
    pub fn from_data(data: &Data, items: &Mappings) -> Result<Self> {
        let mut recipes = BTreeMap::new();
        let mut names = BTreeMap::new();
        for (id, recipe) in data.recipes.iter().enumerate() {
            if recipe.result == 0 && recipe.ingredients.is_empty() {
                continue;
            }
            let result = items.target(u32::from(recipe.result))?.to_owned();
            let name = unique_name(&result, &recipes);
            recipes.insert(
                name.clone(),
                NamedRecipe {
                    result,
                    ingredients: recipe
                        .ingredients
                        .iter()
                        .map(|(&id, &count)| Ok((items.target(u32::from(id))?.to_owned(), count)))
                        .collect::<Result<_>>()?,
                },
            );
            names.insert(id, name);
        }
        let mut mappings = BTreeMap::new();
        let mut vendors = BTreeMap::new();
        for (id, vendor) in data.vendors.iter().enumerate() {
            let key = unique_name(&vendor.name, &vendors);
            vendors.insert(
                key.clone(),
                NamedVendor {
                    name: vendor.name.clone(),
                    recipes: vendor
                        .recipes
                        .iter()
                        .map(|&id| {
                            names
                                .get(&usize::from(id))
                                .cloned()
                                .context("vendor requests an empty recipe")
                        })
                        .collect::<Result<_>>()?,
                },
            );
            mappings.insert(u32::try_from(id)?, key);
        }
        Ok(Self {
            mappings: Mappings::new(mappings)?,
            vendors,
            recipes,
            labels: data.labels.clone(),
        })
    }
    pub fn prepare(&self, items: &Mappings) -> Result<Data> {
        let items = item_ids(items)?;
        let resolve = |name: &str| {
            items
                .get(name)
                .copied()
                .with_context(|| format!("unknown crafting item {name}"))
        };
        let recipe_ids = self
            .recipes
            .keys()
            .enumerate()
            .map(|(id, name)| Ok((name.as_str(), u16::try_from(id)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let recipes = self
            .recipes
            .values()
            .map(|recipe| {
                Ok(Recipe {
                    result: resolve(&recipe.result)?,
                    ingredients: recipe
                        .ingredients
                        .iter()
                        .map(|(name, &count)| Ok((resolve(name)?, count)))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        let vendors = slots(&self.mappings, 256)?
            .into_iter()
            .map(|key| {
                let vendor = self
                    .vendors
                    .get(key)
                    .with_context(|| format!("missing crafting vendor {key}"))?;
                Ok(Vendor {
                    name: vendor.name.clone(),
                    recipes: vendor
                        .recipes
                        .iter()
                        .map(|key| {
                            recipe_ids
                                .get(key.as_str())
                                .copied()
                                .with_context(|| format!("unknown crafting recipe {key}"))
                        })
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Data {
            recipes,
            vendors,
            labels: self.labels.clone(),
        })
    }
}
