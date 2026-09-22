use super::*;
use anyhow::Context;
use resonance_events::SavedProgress;

/// A normal field restart at the player's saved location, shared by both save UIs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldCheckpoint {
    pub map_id: u32,
    pub position: [f32; 3],
    pub heading: f32,
    #[serde(default)]
    pub camera: Option<resonance_events::camera::CameraSettings>,
    pub progress: SavedProgress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub played_ticks: Option<u64>,
}
impl FieldSession {
    pub fn checkpoint(&self) -> Result<FieldCheckpoint> {
        ensure!(
            self.authored_entry.is_none(),
            "quicksave unavailable before an authored entry event"
        );
        ensure!(
            self.active_skit.is_none() && self.events.world.skit_request.is_none(),
            "quicksave unavailable during skit playback"
        );
        ensure!(
            self.menu.is_none() && self.shop.is_none() && self.events.world.menu_request.is_none(),
            "quicksave unavailable while a menu is open"
        );
        let world = &self.events.world;
        ensure!(
            world.field_transition.is_none(),
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

    /// Menus can edit party progress while a script owns the field. Only
    /// checkpoint() applies the additional restrictions for a restartable save.
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
            map_id: self.map_id,
            position: actor.position,
            heading: actor.heading.rem_euclid(360.),
            camera: Some(
                world
                    .field_camera
                    .as_ref()
                    .context("field camera is missing")?
                    .settings(world.controlled_actor)
                    .map_err(anyhow::Error::msg)?,
            ),
            progress: self.events.save_progress()?,
            played_ticks: Some(self.play_time.total()),
        })
    }
}
impl FieldCheckpoint {
    pub fn played_ticks(&self) -> u64 {
        self.played_ticks.unwrap_or(u64::from(self.progress.tick))
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
        let leader = i32::from(self.progress.party.field_leader);
        Ok(FieldEntry {
            kind: super::EntryKind::Restore,
            play_time: crate::clock::PlayTime::resume(self.played_ticks()),
            persistent: self.progress.into_state(&data)?,
            data: Some(data),
            menu_data: None,
            skits: None,
            text: Default::default(),
            available_fields,
            position: self.position,
            heading: self.heading,
            idle_animation: None,
            camera: self
                .camera
                .map(|camera| camera.entry(leader).map_err(anyhow::Error::msg))
                .transpose()?,
        })
    }
}
