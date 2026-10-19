//! Persistent scene state shared by menus and save loading.
use resonance_events::SavedProgress;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Checkpoint {
    Field(crate::field::FieldCheckpoint),
    World(crate::overworld::Checkpoint),
}

impl Checkpoint {
    pub fn progress(&self) -> &SavedProgress {
        match self {
            Self::Field(c) => &c.progress,
            Self::World(c) => &c.progress,
        }
    }
    pub fn progress_mut(&mut self) -> &mut SavedProgress {
        match self {
            Self::Field(c) => &mut c.progress,
            Self::World(c) => &mut c.progress,
        }
    }
    pub fn into_progress(self) -> SavedProgress {
        match self {
            Self::Field(c) => c.progress,
            Self::World(c) => c.progress,
        }
    }
    pub fn played_ticks(&self) -> u64 {
        match self {
            Self::Field(c) => c.played_ticks,
            Self::World(c) => c.played_ticks,
        }
    }
    pub fn set_played_ticks(&mut self, ticks: u64) {
        match self {
            Self::Field(c) => c.played_ticks = ticks,
            Self::World(c) => c.played_ticks = ticks,
        }
    }
    pub fn location(&self) -> String {
        match self {
            Self::Field(c) if c.starts_new_game_plus() => "Game cleared".into(),
            Self::Field(c) => format!("Field {}", c.map_id),
            Self::World(c) => match c.state.world {
                crate::overworld::World::Sylvarant => "Sylvarant".into(),
                crate::overworld::World::TetheAlla => "Tethe'alla".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::FieldCheckpoint;
    #[test]
    fn field_and_world_save_shapes_preserve_numeric_inventory_and_event_keys() -> anyhow::Result<()>
    {
        let field: FieldCheckpoint = serde_json::from_value(serde_json::json!({
            "map_id": 330, "position": [0, 0, 0], "heading": 0, "played_ticks": 100,
            "progress": {
                "script_globals": [], "event_flags": [22], "random_state": 0, "tick": 100,
                "gameplay_random": resonance_events::GameplayRandom::default(),
                "event_records": {"12": {"value": 1, "extra": 0, "tick": 50}},
                "party": {
                    "battles": resonance_events::party::BattleStatistics::default(),
                    "members": [], "formation": [], "items": {"58": 1},
                    "found_items": [58], "recent_items": [58], "gald": 0, "spent_gald": 0,
                    "settings": resonance_events::party::Settings::default()
                }
            }
        }))?;
        let state = resonance_content::overworld::TravelState {
            world: resonance_content::overworld::World::Sylvarant,
            position: resonance_content::overworld::Position::from_map([9770., 23500., 0.])?,
            heading: 0.,
            camera_yaw: 0.,
            alternate_perspective: false,
            map_display: Default::default(),
            mount: resonance_content::overworld::Mount::Rheairds,
            altitude: 600.,
        };
        let world = crate::overworld::Checkpoint {
            state,
            progress: field.progress.clone(),
            played_ticks: 0,
        };
        for checkpoint in [Checkpoint::Field(field), Checkpoint::World(world)] {
            let mut menu = crate::menu::Menu::new(crate::menu::Page::Main, Some(checkpoint), true);
            menu.checkpoint.as_mut().unwrap().progress_mut().party.gald = 123;
            menu.set_play_time(crate::clock::PlayTime::resume(250));
            let checkpoint = menu.checkpoint.unwrap();
            assert_eq!(checkpoint.progress().party.gald, 123);
            assert_eq!(checkpoint.played_ticks(), 250);
            let bytes = serde_json::to_vec(&checkpoint)?;
            let decoded: Checkpoint = serde_json::from_slice(&bytes)?;
            assert_eq!(
                serde_json::to_value(&checkpoint)?,
                serde_json::to_value(decoded)?
            );
        }
        Ok(())
    }
}
