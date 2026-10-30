//! One editable owner for item rules, equipment eligibility and captions.
use crate::{
    diagnostics::Diagnostics,
    library::Mappings,
    menu_data::{self, EquipmentProperties, Item, ItemText, ItemUse},
    overworld::PartyCharacter,
    session::ItemDefinition,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PATH: &str = "game/items.json";
mod format;
pub use format::*;
mod descriptions;
pub use descriptions::{Description, SuppressingDescription};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: Mappings,
    pub items: BTreeMap<String, Entry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub equipment_descriptions: BTreeMap<String, Description>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub category: Category,
    #[serde(default, skip_serializing_if = "Characters::is_empty")]
    pub characters: Characters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_limit: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub field_usable: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub battle_usable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<menu_data::ItemView>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stats: BTreeMap<Statistic, i16>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub properties: EquipmentProperties,
    /// Base sale value; shops apply their existing purchase multipliers.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub price: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transforms_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_use: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<menu_data::ItemAttention>,
    /// Caption failure remains independent of gameplay admission.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub text: Value,
}

pub struct Prepared {
    pub mappings: Mappings,
    pub menu: Vec<Item>,
    pub inventory: Vec<ItemDefinition>,
    pub text: Vec<Value>,
    pub status: menu_data::StatusData,
    pub description_text: BTreeMap<String, Value>,
}

pub fn load(files: &crate::prepared::Files) -> Result<Prepared> {
    files.json::<Document>(PATH)?.prepare()
}
impl Prepared {
    pub fn captions(&self, diagnostics: &Diagnostics) -> Result<Vec<Option<ItemText>>> {
        self.text
            .iter()
            .enumerate()
            .map(|(id, value)| {
                if value.is_null() {
                    return Ok(None);
                }
                diagnostics.attempt(
                    &format!("item caption {id}"),
                    serde_json::from_value::<ItemText>(value.clone()).map_err(Into::into),
                )
            })
            .collect()
    }
    pub fn names(&self, diagnostics: &Diagnostics) -> Result<BTreeMap<u16, String>> {
        let mut names = BTreeMap::new();
        for (id, text) in self.text.iter().enumerate() {
            if text.is_null() {
                continue;
            }
            if let Some(name) = diagnostics.attempt(
                &format!("item name {id}"),
                serde_json::from_value::<String>(text["name"].clone()).map_err(Into::into),
            )? {
                names.insert(id as u16, name);
            }
        }
        Ok(names)
    }
}

impl Document {
    pub fn from_parts(
        menu: &[Item],
        inventory: &[ItemDefinition],
        text: &[Option<ItemText>],
        mappings: Mappings,
        equipment_descriptions: BTreeMap<String, Description>,
    ) -> Result<Self> {
        ensure!(
            menu.len() == inventory.len() && menu.len() == text.len(),
            "item rule and caption counts differ"
        );
        ensure!(
            mappings.iter().count() == menu.len(),
            "item mappings must cover the source catalogue"
        );
        let mut items = BTreeMap::new();
        for (id, name) in mappings.iter() {
            let index = id as usize;
            let item = menu
                .get(index)
                .context("item mapping exceeds source catalogue")?;
            let inventory = &inventory[index];
            let category = Category::from_index(item.category)?;
            ensure!(
                inventory.equipment_kind == category.equipment_kind(),
                "item category and equipment slot disagree for {name}"
            );
            let entry = Entry {
                category,
                characters: Characters::from_mask(inventory.allowed_characters)?,
                stack_limit: (inventory.stack_limit
                    != crate::session::item_stack_limit(item.category))
                .then_some(inventory.stack_limit),
                field_usable: item.field_usable,
                battle_usable: item.battle_usable,
                view: item.view,
                stats: Statistic::ALL
                    .into_iter()
                    .zip(item.equipment_stats)
                    .filter(|(_, value)| *value != 0)
                    .collect(),
                properties: item.properties.clone(),
                price: item.price,
                transforms_to: if item.transforms_to == 0 {
                    None
                } else {
                    Some(mappings.target(u32::from(item.transforms_to))?.into())
                },
                field_use: item.field_use.map(Action::try_from).transpose()?,
                attention: item.attention,
                text: text[index]
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()?
                    .unwrap_or_default(),
            };
            ensure!(
                items.insert(name.into(), entry).is_none(),
                "item names must be unique"
            );
        }
        let document = Self {
            mappings,
            items,
            equipment_descriptions,
        };
        document.prepare()?;
        Ok(document)
    }

    pub fn prepare(&self) -> Result<Prepared> {
        let mut identities = BTreeMap::new();
        for (index, (id, name)) in self.mappings.iter().enumerate() {
            ensure!(
                id as usize == index && id < 4096,
                "item IDs must cover zero through the last item"
            );
            ensure!(
                identities.insert(name, id as u16).is_none(),
                "item names must have unique IDs"
            );
        }
        ensure!(!identities.is_empty(), "item catalogue is empty");
        let mut result = Prepared {
            mappings: self.mappings.clone(),
            menu: Vec::new(),
            inventory: Vec::new(),
            text: Vec::new(),
            status: Default::default(),
            description_text: BTreeMap::new(),
        };
        for (name, description) in &self.equipment_descriptions {
            let (suppresses, text) = description.parts()?;
            result.status.equipment_effects.insert(
                name.clone(),
                menu_data::EquipmentEffect {
                    suppresses: suppresses.to_vec(),
                },
            );
            result.description_text.insert(name.clone(), text.clone());
        }
        for (_, name) in self.mappings.iter() {
            let item = self
                .items
                .get(name)
                .with_context(|| format!("missing item {name}"))?;
            let category = item.category.index();
            let stack_limit = item
                .stack_limit
                .unwrap_or_else(|| crate::session::item_stack_limit(category));
            ensure!(
                (1..=99).contains(&stack_limit),
                "invalid item stack limit for {name}"
            );
            let mut stats = [0; 7];
            for (&stat, &value) in &item.stats {
                stats[stat as usize] = value;
            }
            result.menu.push(Item {
                category,
                field_usable: item.field_usable,
                battle_usable: item.battle_usable,
                view: item.view,
                equipment_stats: stats,
                properties: item.properties.clone(),
                price: item.price,
                transforms_to: item
                    .transforms_to
                    .as_deref()
                    .map(|name| {
                        identities
                            .get(name)
                            .copied()
                            .with_context(|| format!("unknown item transformation {name}"))
                    })
                    .transpose()?
                    .unwrap_or(0),
                field_use: item.field_use.map(ItemUse::try_from).transpose()?,
                attention: item.attention,
            });
            result.inventory.push(ItemDefinition {
                equipment_kind: item.category.equipment_kind(),
                allowed_characters: item.characters.mask()?,
                stack_limit,
            });
            result.text.push(item.text.clone());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
