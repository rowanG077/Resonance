use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldMapData {
    pub names: [String; 2],
    pub locations: BTreeMap<u16, MapLocation>,
    pub field_locations: BTreeMap<u32, u16>,
    pub shops: Vec<Shop>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapLocation {
    pub name: String,
    /// Position on the 384 × 288 map image.
    pub point: [i16; 2],
    pub listed: bool,
    pub visit_alias: Option<u16>,
    pub shops: Vec<u8>,
    pub shop_variants: Vec<MapShopVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapShopVariant {
    /// Word index in saved script_globals.
    pub global: usize,
    pub at_least: i32,
    pub shops: Vec<u8>,
}

/// Ordered stock shared by the shop counter and world-map directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shop {
    pub name: String,
    pub items: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopTrade {
    Buy,
    Sell,
}

impl Item {
    /// Prices are rounded per unit before the basket quantity is applied.
    pub fn shop_price(&self, trade: ShopTrade, personal: bool) -> u32 {
        let percent = match (trade, personal) {
            (ShopTrade::Buy, false) => 200,
            (ShopTrade::Buy, true) => 180,
            (ShopTrade::Sell, false) => 100,
            (ShopTrade::Sell, true) => 110,
        };
        u32::try_from(u64::from(self.price) * percent / 100).unwrap_or(u32::MAX)
    }
}

impl Shop {
    pub fn validate(&self, item_count: usize) -> Result<()> {
        ensure!(!self.name.is_empty(), "missing shop name");
        let mut seen = std::collections::BTreeSet::new();
        ensure!(
            !self.items.is_empty()
                && self
                    .items
                    .iter()
                    .all(|&id| { id > 0 && usize::from(id) < item_count && seen.insert(id) }),
            "shop {:?} has empty, duplicate, or invalid stock",
            self.name
        );
        Ok(())
    }
}

impl MapLocation {
    pub fn shops(&self, globals: &[i32]) -> &[u8] {
        self.shop_variants
            .iter()
            .rev()
            .find(|v| globals.get(v.global).is_some_and(|&g| g >= v.at_least))
            .map_or(&self.shops, |v| &v.shops)
    }
}

impl WorldMapData {
    pub fn validate(&self, item_count: usize) -> Result<()> {
        for shop in &self.shops {
            shop.validate(item_count)?;
        }
        ensure!(
            self.names.iter().all(|v| !v.is_empty()),
            "missing world map name"
        );
        for (&id, location) in &self.locations {
            ensure!(
                (1..=98).contains(&id) || (257..=337).contains(&id),
                "invalid world location {id}"
            );
            ensure!(
                (!location.listed || !location.name.is_empty())
                    && (0..384).contains(&location.point[0])
                    && (0..288).contains(&location.point[1])
                    && location
                        .visit_alias
                        .is_none_or(|v| self.locations.contains_key(&v))
                    && location.shop_variants.iter().all(|v| v.global < 256)
                    && location
                        .shops
                        .iter()
                        .chain(location.shop_variants.iter().flat_map(|v| &v.shops))
                        .all(|&v| usize::from(v) < self.shops.len()),
                "invalid world location data {id}"
            );
        }
        ensure!(
            self.field_locations
                .values()
                .all(|id| self.locations.contains_key(id)),
            "invalid world map field or shop reference"
        );
        Ok(())
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.names
            .iter()
            .chain(self.locations.values().map(|v| &v.name))
            .chain(self.shops.iter().map(|v| &v.name))
            .map(String::as_str)
    }
}
