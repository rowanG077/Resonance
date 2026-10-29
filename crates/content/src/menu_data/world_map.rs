use super::*;
use crate::overworld::{Landmark, Landmarks};
pub mod document;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldMapData {
    pub landmarks: String,
    /// Prepared from the shared landmark document; never written into menu data.
    #[serde(skip)]
    pub locations: BTreeMap<u16, Landmark>,
    pub field_locations: BTreeMap<u32, u16>,
    pub shops: Vec<Shop>,
    /// Opening any member records visits for the entire group, using saved shop IDs.
    pub shop_visit_groups: Vec<Vec<u8>>,
}

/// Ordered stock shared by the shop counter and world-map directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shop {
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
        let mut seen = std::collections::BTreeSet::new();
        ensure!(
            !self.items.is_empty()
                && self
                    .items
                    .iter()
                    .all(|&id| { id > 0 && usize::from(id) < item_count && seen.insert(id) }),
            "shop has empty, duplicate, or invalid stock"
        );
        Ok(())
    }
}

impl WorldMapData {
    pub fn prepare_locations(&mut self, files: &crate::prepared::Files) -> Result<()> {
        let landmarks: Landmarks = files.json(&self.landmarks)?;
        let mut locations = BTreeMap::new();
        for (world, entries) in landmarks.worlds.into_iter().enumerate() {
            for entry in entries {
                ensure!(
                    usize::from(entry.id / 256) == world && entry.id % 256 != 0,
                    "map location ID belongs to another world"
                );
                ensure!(
                    locations.insert(entry.id, entry).is_none(),
                    "duplicate map location ID"
                );
            }
        }
        self.locations = locations;
        Ok(())
    }

    pub fn validate_shop_visits(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        ensure!(self.shops.len() <= 52, "too many saved shop IDs");
        for group in &self.shop_visit_groups {
            ensure!(
                group.len() >= 2
                    && group.windows(2).all(|pair| pair[0] < pair[1])
                    && group
                        .iter()
                        .all(|&id| usize::from(id) < self.shops.len() && seen.insert(id)),
                "invalid or overlapping shop visit group"
            );
        }
        Ok(())
    }

    pub fn shop_visit_group(&self, id: u8) -> &[u8] {
        self.shop_visit_groups
            .iter()
            .find(|group| group.contains(&id))
            .map_or(&[], Vec::as_slice)
    }

    /// Field entry only needs the current location's travel-history identity.
    pub fn validate_field(&self, field: u32) -> Result<()> {
        if let Some(&id) = self.field_locations.get(&field) {
            let location = self.locations.get(&id).ok_or_else(|| {
                anyhow::anyhow!("field {field} references missing world location {id}")
            })?;
            ensure!(
                [id, location.visit_alias.unwrap_or(id)]
                    .into_iter()
                    .all(|id| (1..=98).contains(&id) || (257..=337).contains(&id)),
                "field {field} references invalid travel locations"
            );
        }
        Ok(())
    }

    pub fn validate(&self, item_count: usize) -> Result<()> {
        crate::library::validate_path(&self.landmarks)?;
        self.validate_shop_visits()?;
        for shop in &self.shops {
            shop.validate(item_count)?;
        }
        for (&id, location) in &self.locations {
            ensure!(
                (1..=98).contains(&id) || (257..=337).contains(&id),
                "invalid world location {id}"
            );
            ensure!(
                location
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
            let point = location.map_point();
            ensure!(
                !location.name.is_empty()
                    && location.position.iter().all(|value| value.is_finite())
                    && (0..384).contains(&point[0])
                    && (0..288).contains(&point[1]),
                "invalid world map position or name {id}"
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shop_visits_require_disjoint_bounded_groups() -> Result<()> {
        let mut data = WorldMapData {
            landmarks: "world/landmarks.json".into(),
            locations: BTreeMap::new(),
            field_locations: BTreeMap::new(),
            shops: vec![Shop { items: vec![1] }; 4],
            shop_visit_groups: vec![vec![0, 2, 3]],
        };
        data.validate(2)?;
        assert_eq!(data.shop_visit_group(2), [0, 2, 3]);
        assert!(data.shop_visit_group(1).is_empty());
        for groups in [
            vec![vec![0]],
            vec![vec![0, 4]],
            vec![vec![0, 0]],
            vec![vec![2, 0]],
            vec![vec![0, 1], vec![1, 2]],
        ] {
            data.shop_visit_groups = groups;
            assert!(data.validate(2).is_err());
        }
        Ok(())
    }

    #[test]
    fn field_travel_validates_only_its_selected_location() {
        let mut data = WorldMapData {
            landmarks: "world/landmarks.json".into(),
            locations: BTreeMap::new(),
            field_locations: [(4, 1)].into(),
            shops: Vec::new(),
            shop_visit_groups: Vec::new(),
        };
        assert!(data.validate_field(5).is_ok());
        assert!(data.validate_field(4).is_err());
        data.locations.insert(
            1,
            serde_json::from_value(serde_json::json!({
                "id": 1, "name": "Place", "position": [200.,400.], "radius": 50.
            }))
            .unwrap(),
        );
        assert!(data.validate_field(4).is_ok());
        data.locations.get_mut(&1).unwrap().shops.push(1);
        assert!(data.validate_field(4).is_ok());
        assert!(
            data.validate(528).is_err(),
            "the shop page validates its own references"
        );
    }

    #[test]
    fn map_uses_shared_location_edits_and_retains_captured_versions() -> Result<()> {
        use crate::{
            diagnostics::Diagnostics,
            library::{Descriptor, GAME, Library, VERSION},
            prepared::Files,
        };
        use serde_json::json;
        let root = tempfile::tempdir()?;
        let base = root.path().join("base");
        let modified = root.path().join("mod");
        for path in [&base, &modified] {
            std::fs::create_dir_all(path.join("world"))?;
        }
        std::fs::write(
            base.join("content.json"),
            serde_json::to_vec(&Descriptor {
                version: VERSION,
                game: GAME.into(),
            })?,
        )?;
        let path = "world/places.json";
        let original = json!({"sylvarant":[
            {"id":1,"name":"First","position":[199.75,400.],"radius":40.,"map_listed":true,"shops":[0]},
            {"id":2,"name":"Second","position":[800.,1000.],"radius":50.}
        ], "tethe_alla":[{"id":257,"name":"Other world","position":[1000.,1000.],"radius":50.}]});
        std::fs::write(base.join(path), serde_json::to_vec(&original)?)?;
        let library = Library::open_paths(&base, &[modified.clone()])?;
        let capture = || -> Result<Files> {
            let mut snapshot = library.snapshot();
            snapshot.read(path, crate::preparation::JSON_LIMIT)?;
            Files::from_snapshot(snapshot, || false, Diagnostics::new(true))
        };
        let retained = capture()?;
        let source = json!({"landmarks":path,"field_locations":{"4":2},"shops":[{"items":[1]},{"items":[2]}],"shop_visit_groups":[]});
        let prepare = |files: &Files| -> Result<WorldMapData> {
            let mut data: WorldMapData = serde_json::from_value(source.clone())?;
            data.prepare_locations(files)?;
            data.validate(3)?;
            Ok(data)
        };
        let before = prepare(&retained)?;
        assert_eq!(before.locations[&1].map_point(), [0, 2]);
        let mut edit = original.clone();
        edit["sylvarant"][0]["name"] = "Renamed town".into();
        edit["sylvarant"][0]["position"] = json!([4200., 6200.]);
        edit["sylvarant"][0]["map_listed"] = false.into();
        edit["sylvarant"][0]["shops"] = json!([1]);
        edit["sylvarant"][0]["shop_variants"] = json!([{"global":16,"at_least":10,"shops":[0,1]}]);
        edit["sylvarant"][1]["visit_alias"] = 1.into();
        edit["sylvarant"].as_array_mut().unwrap().swap(0, 1);
        std::fs::write(modified.join(path), serde_json::to_vec(&edit)?)?;
        let current = prepare(&capture()?)?;
        assert_eq!(current.locations[&1].name, "Renamed town");
        assert_eq!(current.locations[&1].map_point(), [21, 31]);
        assert!(!current.locations[&1].map_listed);
        assert_eq!(current.locations[&2].visit_alias, Some(1));
        assert_eq!(current.locations[&257], before.locations[&257]);
        let mut globals = [0; 256];
        globals[16] = 9;
        assert_eq!(current.locations[&1].shops(&globals), [1]);
        globals[16] = 10;
        assert_eq!(current.locations[&1].shops(&globals), [0, 1]);
        assert_eq!(prepare(&retained)?.locations, before.locations);
        assert_eq!(
            serde_json::to_value(&current)?,
            source,
            "prepared locations must not become another editable copy"
        );
        let mut old_format = source.clone();
        old_format["locations"] = json!({});
        assert!(serde_json::from_value::<WorldMapData>(old_format).is_err());
        let mut missing = Files::default();
        assert!(prepare(&missing).is_err());
        edit["sylvarant"][0]["id"] = 1.into();
        missing.insert(path.into(), serde_json::to_vec(&edit)?.into());
        assert!(
            prepare(&missing).is_err(),
            "duplicate IDs must not silently overwrite another location"
        );
        Ok(())
    }
}
