use super::*;
use crate::{all_assets::world_map as source, field_catalogue as phases};
use resonance_content::menu_data::{MapLocation, MapShopVariant, Shop, WorldMapData};
use std::collections::BTreeMap;

pub(crate) fn cook(
    source: &source::Catalogue,
    phases: &phases::Phases,
    ui: &inventory_ui::Catalogue,
) -> Result<WorldMapData> {
    let mut locations = BTreeMap::new();
    for world in 0..2 {
        for (local, row) in source
            .world(world)?
            .iter()
            .take_while(|row| !row.is_terminator())
            .enumerate()
            .skip(1)
        {
            let id = world as u16 * 256 + local as u16;
            let mut location = MapLocation {
                name: row
                    .text
                    .map(|id| source.text(id))
                    .unwrap_or_default()
                    .into(),
                point: row.position.map(|v| (v / 200) as i16),
                listed: row.listed,
                // Ruined and rebuilt Luin share the town's visited flag.
                visit_alias: matches!(id, 43 | 44).then_some(7),
                shops: source.shop_bindings[world]
                    .get(local)
                    .cloned()
                    .flatten()
                    .unwrap_or_default(),
                shop_variants: Vec::new(),
            };
            // These three stock changes are native story rules; the location
            // tables only contain the referenced before/after shop lists.
            let variant = match id {
                7 => Some((0x2e, 500_000, &source.story_shops.luin)),
                8 => Some((3, 301, &source.story_shops.hima)),
                262 => Some((0, 0x014fc8f0, &source.story_shops.flanoir)),
                _ => None,
            };
            if let Some((global, at_least, stock)) = variant {
                location.shops = stock.before.clone();
                location.shop_variants.push(MapShopVariant {
                    global: global + 16,
                    at_least,
                    shops: stock.after.clone(),
                });
            }
            locations.insert(id, location);
        }
    }
    let shops = source
        .shops
        .iter()
        .map(|shop| {
            Ok(Shop {
                name: source.required_text(shop.name)?.into(),
                items: shop.items.clone(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(WorldMapData {
        names: ui
            .inventory
            .worlds
            .map(|reference| ui.text(reference).to_owned()),
        locations,
        field_locations: phases
            .records
            .iter()
            .filter_map(|phase| {
                (!matches!(phase.location, 0 | 0x100 | 0x200))
                    .then_some((phase.id as u32, phase.location))
            })
            .collect(),
        shops,
    })
}
