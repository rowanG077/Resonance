use super::*;
use crate::{all_assets::world_map as source, field_catalogue as phases};
use resonance_content::menu_data::{MapLocation, MapShopVariant, Shop, WorldMapData};
use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) fn cook(
    catalogue: &source::Catalogue,
    phases: &phases::Phases,
    ui: &inventory_ui::Catalogue,
) -> Result<WorldMapData> {
    Ok(read(catalogue, phases, ui)?.runtime)
}

pub(super) fn cook_source(
    catalogue: &source::Catalogue,
    phases: &phases::Phases,
    ui: &inventory_ui::Catalogue,
) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(read(catalogue, phases, ui)?)?)
}

#[derive(Serialize)]
struct SourceWorldMap<'a> {
    #[serde(flatten)]
    runtime: WorldMapData,
    phases: &'a phases::Phases,
    authored_shops: Vec<AuthoredShop>,
    authored_locations: [Vec<AuthoredLocation>; 2],
    item_rewards: Vec<&'a source::ItemReward>,
    exploration_party_requirements: Vec<&'a source::PartyRequirement>,
}

#[derive(Serialize)]
struct AuthoredLocation {
    position: [i32; 2],
    height: f32,
    radius: u16,
    listed: bool,
    interaction: source::Interaction,
    marker: source::Marker,
    text: Option<String>,
}

#[derive(Serialize)]
struct AuthoredShop {
    id: usize,
    name: String,
    active_count: usize,
    slots: [Option<u16>; 21],
}

impl AuthoredShop {
    fn active(&self) -> Result<Shop> {
        let shop = Shop {
            name: self.name.clone(),
            items: self
                .slots
                .get(..self.active_count)
                .context("shop stock count exceeds fixed slots")?
                .iter()
                .map(|item| item.context("empty active shop slot"))
                .collect::<Result<_>>()?,
        };
        shop.validate(528)?;
        Ok(shop)
    }
}

fn read<'a>(
    source: &'a source::Catalogue,
    phases: &'a phases::Phases,
    ui: &inventory_ui::Catalogue,
) -> Result<SourceWorldMap<'a>> {
    let authored_locations = source.locations.each_ref().map(|rows| {
        rows.iter()
            .map(|row| AuthoredLocation {
                position: row.position,
                height: row.height,
                radius: row.radius,
                listed: row.listed,
                interaction: row.interaction,
                marker: row.marker,
                text: row.text.map(|id| source.text(id).into()),
            })
            .collect()
    });
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
                visit_alias: matches!(id, 43 | 44).then_some(7),
                shops: if local < 11 {
                    source.shops(source.shop_bindings[world][local])?
                } else {
                    Vec::new()
                },
                shop_variants: Vec::new(),
            };
            let variant = match id {
                7 => Some((0x2e, 500_000, &source.story_shops.luin)),
                8 => Some((3, 301, &source.story_shops.hima)),
                262 => Some((0, 0x014fc8f0, &source.story_shops.flanoir)),
                _ => None,
            };
            if let Some((global, at_least, stock)) = variant {
                location.shops = source.shops(Some(stock.before))?;
                location.shop_variants.push(MapShopVariant {
                    global: global + 16,
                    at_least,
                    shops: source.shops(Some(stock.after))?,
                });
            }
            locations.insert(id, location);
        }
    }
    let item_rewards = source
        .item_rewards
        .iter()
        .take_while(|row| row.location != 0)
        .map(|row| {
            ensure!(
                locations.contains_key(&row.location) && (1..528).contains(&row.item),
                "invalid active world-map item reward"
            );
            Ok(row)
        })
        .collect::<Result<_>>()?;
    let exploration_party_requirements = source
        .party_requirements
        .iter()
        .take_while(|row| row.location != 0)
        .map(|row| {
            ensure!(
                locations.contains_key(&row.location) && (1..=9).contains(&row.required_character),
                "invalid active world-map party requirement"
            );
            Ok(row)
        })
        .collect::<Result<_>>()?;
    let authored_shops = source
        .shops
        .iter()
        .enumerate()
        .map(|(id, shop)| {
            Ok(AuthoredShop {
                id,
                name: source.required_text(shop.name)?.into(),
                active_count: usize::from(shop.active_count),
                slots: shop.slots,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let shops = authored_shops
        .iter()
        .map(AuthoredShop::active)
        .collect::<Result<_>>()?;
    Ok(SourceWorldMap {
        phases,
        authored_shops,
        authored_locations,
        item_rewards,
        exploration_party_requirements,
        runtime: WorldMapData {
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
        },
    })
}
