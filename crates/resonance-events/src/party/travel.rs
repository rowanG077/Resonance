use super::*;
use resonance_content::menu_data::WorldMapData;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Travel {
    /// Last world position and mount survive fields, battles and save/load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overworld: Option<resonance_content::overworld::TravelState>,
    /// Field ability mode and variant; ordinary world entry resets the ring.
    #[serde(default)]
    pub sorcerers_ring: crate::ring::SorcerersRing,
    #[serde(default)]
    pub ring_timer: u32,
    /// Native field clock: advances only while scenario input is unpaused.
    #[serde(default)]
    pub field_ticks: u32,
    /// Scenario clock also advances while player input is suspended.
    #[serde(default)]
    pub scenario_ticks: u32,
    /// Scenario countdown; continues while mapped input is disabled.
    #[serde(default)]
    pub field_countdown: u32,
    /// Saved by event command 0x89, independent of the current party order.
    #[serde(default)]
    pub saved_formation: Vec<u8>,
    pub current_location: Option<u16>,
    pub visited_locations: BTreeSet<u16>,
    pub visited_shops: BTreeSet<u8>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub opened_treasures: BTreeSet<u16>,
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
            self.opened_treasures.iter().all(|id| *id < 1024),
            "invalid treasure flag"
        );
        if let Some(state) = &self.overworld {
            state.validate_shape()?;
        }
        anyhow::ensure!(
            self.saved_formation.len() <= 8
                && self.saved_formation.iter().all(|id| (1..=9).contains(id))
                && self.saved_formation.iter().collect::<BTreeSet<_>>().len()
                    == self.saved_formation.len(),
            "invalid saved formation"
        );
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
