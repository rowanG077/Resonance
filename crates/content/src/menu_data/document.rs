//! Remaining menu owners. Numeric runtime tables are prepared from these names.
use super::*;
use crate::{library::Mappings, prepared::Files};
use serde_json::Value;
#[cfg(test)]
mod tests;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub grade_shop: crate::grade::Shop,
    pub crafting: crafting::document::Document,
    pub world_map: world_map::document::Document,
    pub presentation: Value,
}

impl Document {
    pub fn load(files: &Files) -> Result<Self> {
        let mut presentation: Value = files.json(PRESENTATION_PATH)?;
        let captions = presentation
            .as_object_mut()
            .context("menu presentation must be an object")?;
        ensure!(
            !captions.contains_key("monsters"),
            "monster catalogue belongs in game/menu/monster-book.json"
        );
        captions.insert("monsters".into(), files.json(MONSTER_BOOK_PATH)?);
        Ok(Self {
            grade_shop: files.json(GRADE_SHOP_PATH)?,
            crafting: files.json(CRAFTING_PATH)?,
            world_map: files.json(WORLD_MAP_PATH)?,
            presentation,
        })
    }

    pub fn from_data(data: &MenuData, items: &Mappings, fields: &Mappings) -> Result<Self> {
        // Project only the captions owned by this document. Do not serialize
        // gameplay tables merely to discard them again during publication.
        let mut presentation = serde_json::json!({
            "labels": data.presentation.labels,
            "items": data.presentation.items,
            "status": data.presentation.status,
            "monsters": data.presentation.monsters,
        });
        if let Some(labels) = presentation["labels"].as_object_mut() {
            labels.retain(|key, _| !key.starts_with("strategy_"));
        }
        if let Some(text) = presentation["status"].as_object_mut() {
            text.remove("equipment_effects");
        }
        if let Some(captions) = presentation["items"].as_object_mut() {
            captions.remove("items");
        }
        Ok(Self {
            grade_shop: data.grade_shop.clone(),
            crafting: crafting::document::Document::from_data(&data.crafting, items)?,
            world_map: world_map::document::Document::from_data(
                &data.world_map,
                data.presentation
                    .world_map
                    .as_ref()
                    .context("missing world captions")?,
                data.presentation
                    .shops
                    .as_ref()
                    .context("missing shop captions")?,
                items,
                fields,
            )?,
            presentation,
        })
    }
    pub(super) fn prepare(self, files: &Files, items: &Mappings) -> Result<MenuData> {
        let fields: Mappings = files.json(crate::library::FIELDS)?;
        let mut presentation = self.presentation;
        let captions = presentation
            .as_object_mut()
            .context("menu captions must be an object")?;
        for key in captions.keys() {
            ensure!(
                matches!(key.as_str(), "labels" | "items" | "status" | "monsters"),
                "{key} captions do not belong in game/menu/presentation.json"
            );
        }
        captions.insert(
            "shops".into(),
            serde_json::to_value(self.world_map.shop_names()?)?,
        );
        captions.insert(
            "world_map".into(),
            serde_json::json!({"names":self.world_map.names}),
        );
        Ok(MenuData {
            grade_shop: self.grade_shop,
            items: Default::default(),
            titles: Default::default(),
            title_awards: Default::default(),
            initial_names: Default::default(),
            techniques: Default::default(),
            strategy: Default::default(),
            cooking: Default::default(),
            status: Default::default(),
            ex_skills: Default::default(),
            crafting: self.crafting.prepare(items)?,
            world_map: self.world_map.prepare(items, &fields)?,
            presentation: MenuPresentation::decode(presentation, files)?,
        })
    }
}

/// Publication names follow captions; genuine variants get a small numbered suffix.
pub fn unique_name<T>(caption: &str, entries: &BTreeMap<String, T>) -> String {
    let base = caption
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() {
        "untitled".into()
    } else {
        base
    };
    let mut key = base.clone();
    let mut suffix = 2;
    while entries.contains_key(&key) {
        key = format!("{base}-{suffix}");
        suffix += 1;
    }
    key
}

pub(super) fn slots(mappings: &Mappings, limit: usize) -> Result<Vec<&str>> {
    let count = mappings.iter().count();
    ensure!(count <= limit, "too many menu selections");
    (0..count).map(|id| mappings.target(id as u32)).collect()
}

pub(super) fn item_ids(mappings: &Mappings) -> Result<BTreeMap<&str, u16>> {
    mappings
        .iter()
        .map(|(id, name)| Ok((name, u16::try_from(id)?)))
        .collect()
}
