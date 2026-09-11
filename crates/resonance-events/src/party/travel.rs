use super::*;
use resonance_content::menu_data::WorldMapData;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Travel {
    pub current_location: Option<u16>,
    pub visited_locations: BTreeSet<u16>,
    pub visited_shops: BTreeSet<u8>,
}

impl Travel {
    pub fn enter_field(&mut self, data: &WorldMapData, field: u32) {
        if let Some(&id) = data.field_locations.get(&field) {
            self.current_location = Some(id);
            self.visited_locations
                .insert(data.locations[&id].visit_alias.unwrap_or(id));
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.visited_locations
                .iter()
                .chain(self.current_location.iter())
                .all(|id| (1..=98).contains(id) || (257..=337).contains(id))
                && self.visited_shops.iter().all(|&id| id < 52),
            "invalid saved travel history"
        );
        Ok(())
    }
}
