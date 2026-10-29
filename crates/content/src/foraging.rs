//! Named food pools and the fields where Presea can find them.
use crate::{library::Mappings, menu_data::FieldForaging};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const PATH: &str = "fields/foraging.json";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub pools: BTreeMap<String, Vec<String>>,
    pub areas: Vec<Area>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    pub fields: Vec<String>,
    /// Select a pool, then an item. Order and repetitions affect selection.
    pub pools: Vec<String>,
}
impl Document {
    pub fn prepare(&self, items: &Mappings, fields: &Mappings) -> Result<FieldForaging> {
        let item_ids: BTreeMap<_, _> = items.iter().map(|(id, name)| (name, id)).collect();
        let field_ids: BTreeMap<_, _> = fields.iter().map(|(id, name)| (name, id)).collect();
        let pool_ids: BTreeMap<_, _> = self
            .pools
            .keys()
            .enumerate()
            .map(|(id, name)| (name.as_str(), id))
            .collect();
        let pools = self
            .pools
            .values()
            .map(|items| {
                items
                    .iter()
                    .map(|name| {
                        Ok(u16::try_from(*item_ids.get(name.as_str()).with_context(
                            || format!("unknown foraging item {name}"),
                        )?)?)
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?;
        let mut locations = BTreeMap::new();
        for area in &self.areas {
            ensure!(
                !area.fields.is_empty() && !area.pools.is_empty(),
                "empty foraging area"
            );
            let choices = area
                .pools
                .iter()
                .map(|name| {
                    pool_ids
                        .get(name.as_str())
                        .copied()
                        .with_context(|| format!("unknown foraging pool {name}"))
                })
                .collect::<Result<Vec<_>>>()?;
            for name in &area.fields {
                let id = *field_ids
                    .get(name.as_str())
                    .with_context(|| format!("unknown foraging field {name}"))?;
                ensure!(
                    locations.insert(id, choices.clone()).is_none(),
                    "overlapping foraging areas for {name}"
                );
            }
        }
        Ok(FieldForaging {
            pools,
            fields: locations,
        })
    }
}
