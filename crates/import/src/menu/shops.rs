//! Independent checks of deployed shop stock against the original shop tables.
use super::*;
use crate::dol;
use resonance_content::menu_data::{Item, MenuData, ShopTrade, WorldMapData};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ShopInventoryValidation {
    pub executable_sha256: String,
    pub menu_data_sha256: String,
    pub stock_entries: usize,
    pub price_checks: usize,
    pub story_variants: usize,
    pub shops: Vec<ShopInventoryCheck>,
}

#[derive(Debug, Serialize)]
pub struct ShopInventoryCheck {
    pub id: u8,
    pub name: String,
    pub items: Vec<ShopItemCheck>,
}

#[derive(Debug, Serialize)]
pub struct ShopItemCheck {
    pub id: u16,
    pub name: String,
    pub buy: u32,
    pub sell: u32,
    pub personal_buy: u32,
    pub personal_sell: u32,
}

/// Check every shop, including unused stock and later story variants, without
/// recooking or modifying either input. The report contains the verified prices.
pub fn validate_shops(extracted: &Path, cooked: &Path) -> Result<ShopInventoryValidation> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let bytes = fs::read(cooked.join("game/menu-data.json"))?;
    let data: MenuData = serde_json::from_slice(&bytes)?;
    data.validate()?;
    let mut report = validate_catalogue(&executable, &data.items, &data.world_map)?;
    report.menu_data_sha256 = crate::digest(&bytes);
    Ok(report)
}

fn validate_catalogue(
    executable: &[u8],
    catalogue: &[Item],
    world_map: &WorldMapData,
) -> Result<ShopInventoryValidation> {
    ensure!(catalogue.len() == 528, "expected all 528 item definitions");
    world_map.validate(catalogue.len())?;
    let half = |address| -> Result<u16> {
        Ok(u16::from_be_bytes(
            dol::slice(executable, address, 2)?.try_into()?,
        ))
    };
    let word = |address| -> Result<u32> {
        Ok(u32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    // Read individual source fields here instead of using the cooking parser.
    // This catches omitted rows, changed ordering and incorrect record strides.
    ensure!(
        world_map.shops.len() == 52,
        "expected all 52 shop inventories"
    );
    for (id, item) in catalogue.iter().enumerate() {
        ensure!(
            item.price == word(0x801fad9c + id as u32 * 60)?,
            "item {id} sale price differs from source"
        );
        ensure!(
            item.name == dol::text(executable, word(0x801fad98 + id as u32 * 60)?)?,
            "item {id} name differs from source"
        );
    }
    let mut report = ShopInventoryValidation {
        executable_sha256: crate::digest(executable),
        menu_data_sha256: String::new(),
        stock_entries: 0,
        price_checks: catalogue.len(),
        story_variants: 0,
        shops: Vec::new(),
    };
    for (id, shop) in world_map.shops.iter().enumerate() {
        let address = 0x80230980 + id as u32 * 48;
        ensure!(
            shop.name == dol::text(executable, word(address)?)?,
            "shop {id} name differs from source"
        );
        let count = usize::from(half(address + 4)?);
        ensure!(
            (1..=21).contains(&count) && shop.items.len() == count,
            "shop {id} stock count differs from source"
        );
        let mut items = Vec::new();
        for (position, &item_id) in shop.items.iter().enumerate() {
            ensure!(
                item_id == half(address + 6 + position as u32 * 2)?,
                "shop {id} stock position {position} differs from source"
            );
            let item = &catalogue[usize::from(item_id)];
            let sale = word(0x801fad9c + u32::from(item_id) * 60)?;
            ensure!(sale > 0, "shop {id} stocks unsellable item {item_id}");
            let sale = u64::from(sale);
            let expected = [sale * 2, sale, sale * 180 / 100, sale * 110 / 100];
            let actual = [
                item.shop_price(ShopTrade::Buy, false),
                item.shop_price(ShopTrade::Sell, false),
                item.shop_price(ShopTrade::Buy, true),
                item.shop_price(ShopTrade::Sell, true),
            ];
            ensure!(
                actual.map(u64::from) == expected,
                "shop {id} item {item_id} trade prices differ from source"
            );
            items.push(ShopItemCheck {
                id: item_id,
                name: item.name.clone(),
                buy: actual[0],
                sell: actual[1],
                personal_buy: actual[2],
                personal_sell: actual[3],
            });
        }
        report.stock_entries += count;
        report.price_checks += count * 4;
        report.shops.push(ShopInventoryCheck {
            id: id as u8,
            name: shop.name.clone(),
            items,
        });
    }
    let list = |address| -> Result<Vec<u8>> {
        if address == 0 {
            return Ok(Vec::new());
        }
        let count = usize::from(dol::slice(executable, address, 1)?[0]);
        ensure!(count <= 8, "invalid source location shop list");
        Ok(dol::slice(executable, address + 1, count)?.to_vec())
    };
    for world in 0..2u32 {
        for town in 1..=10u32 {
            let id = (world * 256 + town) as u16;
            let location = world_map
                .locations
                .get(&id)
                .with_context(|| format!("missing shop location {id}"))?;
            let variant = match id {
                7 => Some((62, 500_000, 0x8035a190, 0x8035a194)),
                8 => Some((19, 301, 0x8035a198, 0x8035a19c)),
                262 => Some((16, 22_006_000, 0x8035a1b8, 0x8035a1bc)),
                _ => None,
            };
            let address = match variant {
                Some((_, _, before, _)) => before,
                None => word(0x80227f00 + world * 44 + town * 4)?,
            };
            ensure!(
                location.shops == list(address)?,
                "location {id} shop list differs from source"
            );
            if let Some((global, at_least, _, after)) = variant {
                ensure!(
                    location.shop_variants.len() == 1,
                    "location {id} shop variant count differs from source"
                );
                let value = &location.shop_variants[0];
                let after = list(after)?;
                ensure!(
                    value.global == global && value.at_least == at_least && value.shops == after,
                    "location {id} shop variant differs from source"
                );
                let mut globals = vec![0; 256];
                for (story, expected) in [
                    (at_least - 1, &location.shops),
                    (at_least, &after),
                    (at_least + 1, &after),
                ] {
                    globals[global] = story;
                    ensure!(
                        location.shops(&globals) == expected,
                        "location {id} shop variant threshold is incorrect"
                    );
                }
                report.story_variants += 1;
            } else {
                ensure!(
                    location.shop_variants.is_empty(),
                    "unexpected shop variant for location {id}"
                );
            }
        }
    }
    for (&id, location) in &world_map.locations {
        if id % 256 > 10 {
            ensure!(
                location.shops.is_empty() && location.shop_variants.is_empty(),
                "unexpected shops for location {id}"
            );
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the original extracted executable; no cooked assets"]
    fn every_shop_inventory_and_story_variant_matches_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let executable = fs::read(root.join("extracted/disc1/sys/main.dol")).unwrap();
        let mut catalogue = super::super::data::items(&executable).unwrap();
        let mut world_map = super::super::data::world_map::cook(
            &crate::all_assets::world_map::read(&executable).unwrap(),
            &crate::field_catalogue::read(&executable).unwrap(),
            &crate::all_assets::inventory_ui::read(&executable).unwrap(),
        )
        .unwrap();
        let report = validate_catalogue(&executable, &catalogue, &world_map).unwrap();
        assert_eq!(
            (
                report.shops.len(),
                report.stock_entries,
                report.story_variants
            ),
            (52, 608, 3)
        );
        assert_eq!(report.price_checks, 2960);
        assert_eq!(world_map.shops[25].items.len(), 20);
        assert_eq!(world_map.shops[25].items.last(), Some(&121));
        // Its unused twenty-first slot contains Red Satay, which is not for sale.
        world_map.shops[25].items.push(125);
        assert!(
            validate_catalogue(&executable, &catalogue, &world_map).is_err(),
            "stock beyond the declared count passed"
        );
        world_map.shops[25].items.pop();
        // Odd base prices catch per-unit truncation; multiplying a basket first
        // would incorrectly give 33 gald for six discounted Magic Lenses.
        let lens = &catalogue[37];
        assert_eq!(lens.shop_price(ShopTrade::Sell, true) * 6, 30);
        assert_eq!(catalogue[127].shop_price(ShopTrade::Sell, true), 27);

        world_map.shops[51].items.swap(0, 1);
        assert!(
            validate_catalogue(&executable, &catalogue, &world_map).is_err(),
            "reordered final shop passed"
        );
        world_map.shops[51].items.swap(0, 1);
        catalogue[37].price += 1;
        assert!(
            validate_catalogue(&executable, &catalogue, &world_map).is_err(),
            "changed price passed"
        );
        catalogue[37].price -= 1;
        world_map.locations.get_mut(&262).unwrap().shop_variants[0].at_least += 1;
        assert!(
            validate_catalogue(&executable, &catalogue, &world_map).is_err(),
            "wrong story threshold passed"
        );
        world_map.locations.get_mut(&262).unwrap().shop_variants[0].at_least -= 1;
        world_map.shops[1].items.push(1);
        assert!(
            world_map.validate(catalogue.len()).is_err(),
            "duplicate stock passed"
        );
        world_map.shops[1].items.pop();
        let final_shop = world_map.shops.pop().unwrap();
        assert!(
            validate_catalogue(&executable, &catalogue, &world_map).is_err(),
            "missing last shop passed"
        );
        world_map.shops.push(final_shop);
        assert!(
            validate_catalogue(&executable, &catalogue[..527], &world_map).is_err(),
            "missing item definition passed"
        );
        assert!(
            validate_catalogue(&executable[..256], &catalogue, &world_map).is_err(),
            "truncated executable passed"
        );
    }
}
