use super::*;
use crate::{
    library::Mappings,
    menu_data::document::{item_ids, slots, unique_name},
};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub landmarks: String,
    pub names: Value,
    pub field_locations: BTreeMap<String, u16>,
    pub mappings: Mappings,
    pub shops: BTreeMap<String, NamedShop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shop_visit_groups: Vec<Vec<String>>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedShop {
    pub name: Value,
    pub items: Vec<String>,
}
impl Document {
    pub fn from_data(
        data: &WorldMapData,
        names: &WorldMapText,
        captions: &[String],
        items: &Mappings,
        fields: &Mappings,
    ) -> Result<Self> {
        ensure!(
            captions.len() == data.shops.len(),
            "incomplete shop captions"
        );
        let mut mappings = BTreeMap::new();
        let mut shops = BTreeMap::new();
        for (id, (shop, caption)) in data.shops.iter().zip(captions).enumerate() {
            let key = unique_name(caption, &shops);
            shops.insert(
                key.clone(),
                NamedShop {
                    name: caption.clone().into(),
                    items: shop
                        .items
                        .iter()
                        .map(|&id| Ok(items.target(u32::from(id))?.to_owned()))
                        .collect::<Result<_>>()?,
                },
            );
            mappings.insert(u32::try_from(id)?, key);
        }
        let mappings = Mappings::new(mappings)?;
        Ok(Self {
            landmarks: data.landmarks.clone(),
            names: serde_json::to_value(names)?["names"].clone(),
            field_locations: fields
                .iter()
                .filter_map(|(id, name)| {
                    data.field_locations
                        .get(&id)
                        .map(|&location| (name.to_owned(), location))
                })
                .collect(),
            shop_visit_groups: data
                .shop_visit_groups
                .iter()
                .map(|group| {
                    group
                        .iter()
                        .map(|&id| Ok(mappings.target(u32::from(id))?.to_owned()))
                        .collect::<Result<_>>()
                })
                .collect::<Result<_>>()?,
            mappings,
            shops,
        })
    }
    pub fn shop_names(&self) -> Result<Vec<Value>> {
        slots(&self.mappings, 52)?
            .into_iter()
            .map(|key| {
                self.shops
                    .get(key)
                    .map(|shop| shop.name.clone())
                    .with_context(|| format!("missing shop {key}"))
            })
            .collect()
    }
    pub fn prepare(&self, items: &Mappings, fields: &Mappings) -> Result<WorldMapData> {
        let items = item_ids(items)?;
        let fields: BTreeMap<_, _> = fields.iter().map(|(id, name)| (name, id)).collect();
        let shop_ids: BTreeMap<_, _> = self
            .mappings
            .iter()
            .map(|(id, name)| (name, id as u8))
            .collect();
        ensure!(
            shop_ids.len() == self.mappings.iter().count(),
            "saved shop IDs need distinct names"
        );
        Ok(WorldMapData {
            landmarks: self.landmarks.clone(),
            locations: BTreeMap::new(),
            field_locations: self
                .field_locations
                .iter()
                .map(|(name, &location)| {
                    Ok((
                        *fields
                            .get(name.as_str())
                            .with_context(|| format!("unknown map field {name}"))?,
                        location,
                    ))
                })
                .collect::<Result<_>>()?,
            shops: slots(&self.mappings, 52)?
                .into_iter()
                .map(|key| {
                    let shop = self
                        .shops
                        .get(key)
                        .with_context(|| format!("missing shop {key}"))?;
                    Ok(Shop {
                        items: shop
                            .items
                            .iter()
                            .map(|name| {
                                items
                                    .get(name.as_str())
                                    .copied()
                                    .with_context(|| format!("unknown shop item {name}"))
                            })
                            .collect::<Result<_>>()?,
                    })
                })
                .collect::<Result<_>>()?,
            shop_visit_groups: self
                .shop_visit_groups
                .iter()
                .map(|group| {
                    let mut ids = group
                        .iter()
                        .map(|key| {
                            shop_ids
                                .get(key.as_str())
                                .copied()
                                .with_context(|| format!("unknown shop visit member {key}"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    ids.sort_unstable();
                    Ok(ids)
                })
                .collect::<Result<_>>()?,
        })
    }
}
