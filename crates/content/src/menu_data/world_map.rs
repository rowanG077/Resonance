use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldMapData {
    pub names: [String; 2],
    pub locations: BTreeMap<u16, MapLocation>,
    pub field_locations: BTreeMap<u32, u16>,
    pub shops: Vec<MapShop>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapShop {
    pub name: String,
    pub items: Vec<u16>,
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
                .all(|id| self.locations.contains_key(id))
                && self.shops.iter().all(|shop| !shop.name.is_empty()
                    && shop
                        .items
                        .iter()
                        .all(|&id| id > 0 && usize::from(id) < item_count)),
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
