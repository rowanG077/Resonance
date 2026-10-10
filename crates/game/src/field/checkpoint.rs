use super::*;
use anyhow::Context;
use resonance_events::SavedProgress;

/// A normal field restart at the player's saved location, shared by both save UIs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldCheckpoint {
    #[serde(default)]
    pub allow_incomplete_scripts: bool,
    pub map_id: u32,
    pub position: [f32; 3],
    pub heading: f32,
    pub camera: Option<resonance_events::camera::CameraSettings>,
    pub progress: SavedProgress,
    pub played_ticks: u64,
}
impl FieldSession {
    pub(super) fn endgame_checkpoint(&self) -> Result<FieldCheckpoint> {
        let mut progress = self.events.save_progress()?;
        ensure!(
            progress.party.new_game_plus.cleared,
            "endgame menu requires clear data"
        );
        // The setup script selects its New Game Plus branch with story counter 1.
        const NEW_GAME_PLUS_STORY: usize = 0x40 / 4;
        progress.script_globals[NEW_GAME_PLUS_STORY] = 1;
        Ok(FieldCheckpoint {
            allow_incomplete_scripts: self.allow_incomplete_scripts,
            map_id: 5,
            position: [0.; 3],
            heading: 0.,
            camera: None,
            progress,
            played_ticks: self.play_time.total(),
        })
    }
    pub fn checkpoint(&self) -> Result<FieldCheckpoint> {
        let checkpoint = self.player_menu_checkpoint()?;
        ensure!(
            checkpoint.camera.is_some(),
            "quicksave requires the ordinary player-follow camera"
        );
        Ok(checkpoint)
    }

    pub(super) fn player_menu_checkpoint(&self) -> Result<FieldCheckpoint> {
        ensure!(
            !self.events.battle_pending(),
            "quicksave unavailable during battle"
        );
        ensure!(
            self.authored_entry.is_none(),
            "quicksave unavailable before an authored entry event"
        );
        ensure!(
            self.active_skit.is_none() && self.events.world.skit_request.is_none(),
            "quicksave unavailable during skit playback"
        );
        ensure!(
            !self.menu_is_open() && self.events.world.menu_request.is_none(),
            "quicksave unavailable while a menu is open"
        );
        let world = &self.events.world;
        ensure!(
            !world.menu_blocked(),
            "quicksave unavailable while a field effect owns the menu"
        );
        ensure!(
            world.field_transition.is_none() && world.world_transition.is_none(),
            "quicksave unavailable during a field transition"
        );
        ensure!(
            !world.blocked_by_movie(),
            "quicksave unavailable during a movie"
        );
        ensure!(
            self.events.player_has_control(),
            "quicksave unavailable during a scripted event"
        );
        ensure!(
            !self.events.control_handoff_pending(),
            "quicksave unavailable during a control handoff"
        );
        ensure!(
            self.dialogue.values().all(|d| d.closed)
                && world.dialogue.values().all(|d| !d.operation.is_pending())
                && world.choices.values().all(|c| !c.operation.is_pending()),
            "quicksave unavailable during dialogue or a choice"
        );
        ensure!(
            world
                .fade
                .as_ref()
                .is_none_or(|fade| world.tick >= fade.start_tick.saturating_add(fade.duration)),
            "quicksave unavailable during a scene fade"
        );
        self.menu_checkpoint()
    }

    /// Script-opened menus can edit party progress while an event owns the field.
    /// The player menu and quicksave apply their additional restrictions above.
    pub(super) fn menu_checkpoint(&self) -> Result<FieldCheckpoint> {
        let world = &self.events.world;
        let actor = world
            .actors
            .get(&world.controlled_actor)
            .context("controlled actor is missing")?;
        ensure!(
            actor.visible
                && actor.position.iter().all(|v| v.is_finite())
                && actor.heading.is_finite(),
            "controlled actor has no valid field position"
        );
        Ok(FieldCheckpoint {
            allow_incomplete_scripts: self.allow_incomplete_scripts,
            map_id: self.map_id,
            position: actor.position,
            heading: actor.heading.rem_euclid(360.),
            // Fixed event cameras do not prevent party management. Only the
            // restartable checkpoint requires a restorable follow camera.
            camera: world
                .field_camera
                .as_ref()
                .context("field camera is missing")?
                .settings(world.controlled_actor)
                .ok(),
            progress: self.events.save_progress()?,
            played_ticks: self.play_time.total(),
        })
    }
}
impl FieldCheckpoint {
    pub fn starts_new_game_plus(&self) -> bool {
        self.map_id == 5 && self.progress.party.new_game_plus.cleared
    }
    pub fn entry(
        self,
        assets: &FieldAssets,
        data: Arc<resonance_content::session::SessionData>,
        available_fields: std::collections::BTreeSet<u32>,
    ) -> Result<FieldEntry> {
        ensure!(
            self.map_id == assets.map_id
                && self.position.iter().all(|v| v.is_finite())
                && self.heading.is_finite()
                && (0.0..360.0).contains(&self.heading),
            "invalid saved field location"
        );
        let new_game_plus = self.starts_new_game_plus();
        if !new_game_plus {
            let ground = navigation::WalkMesh::new(&assets.ground)?;
            ensure!(
                self.camera.is_some() || self.map_id == 340,
                "saved field requires its entry camera settings"
            );
            ensure!(
                ground
                    .height(self.position, 32.)
                    .is_some_and(|height| (height - self.position[2]).abs() <= 32.),
                "saved player position is outside the field ground"
            );
        }
        let leader = i32::from(self.progress.party.field_leader);
        Ok(FieldEntry {
            effect_palette: Default::default(),
            rising_light_destination: None,
            services: None,
            attachments: Default::default(),
            allow_incomplete_scripts: self.allow_incomplete_scripts,
            kind: if new_game_plus {
                super::EntryKind::Arrival
            } else {
                super::EntryKind::Restore
            },
            play_time: crate::clock::PlayTime::resume(self.played_ticks),
            persistent: self.progress.into_state(&data)?,
            data: Some(data),
            menu_data: None,
            menu_files: Default::default(),
            skits: None,
            text: Default::default(),
            available_fields,
            available_movies: Default::default(),
            position: self.position,
            heading: self.heading,
            idle_animation: None,
            camera: self
                .camera
                .map(|camera| camera.entry(leader))
                .transpose()
                .map_err(anyhow::Error::msg)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FieldCheckpoint;
    use resonance_events::{
        GameplayRandom,
        camera::{CameraRig, EntryCamera},
        party::BattleStatistics,
    };

    #[test]
    fn checkpoint_roundtrip_requires_playtime_and_gameplay_random() {
        let mut camera = CameraRig::default();
        *camera.current_mut() = EntryCamera::following(1).camera;
        let saved = serde_json::json!({
            "map_id": 340, "position": [0., 0., 0.], "heading": 0.,
            "camera": camera.settings(1).unwrap(), "played_ticks": 1234,
            "progress": {
                "script_globals": vec![0; 256], "event_flags": [], "event_records": {},
                "random_state": 7, "gameplay_random": GameplayRandom::new(42), "tick": 12,
                "party": {
                    "battles": BattleStatistics::default(),
                    "members": [], "formation": [], "items": {}, "found_items": [],
                    "recent_items": [], "gald": 0, "spent_gald": 0,
                    "settings": {"battle_controls": [1, 2, 2, 2]}
                }
            }
        });
        let checkpoint: FieldCheckpoint = serde_json::from_value(saved).unwrap();
        let encoded = serde_json::to_value(&checkpoint).unwrap();
        let restored: FieldCheckpoint = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), encoded);
        assert_eq!(checkpoint.played_ticks, 1234);
        for (parent, field) in [("", "played_ticks"), ("/progress", "gameplay_random")] {
            let mut missing = encoded.clone();
            missing
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(
                serde_json::from_value::<FieldCheckpoint>(missing).is_err(),
                "{field}"
            );
        }
    }
}
