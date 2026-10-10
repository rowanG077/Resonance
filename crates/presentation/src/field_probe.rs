//! Field images and sequences use the ordinary application and native scenarios.
use crate::{CheckpointReplay, RunOptions, SaveOptions, saves};
use anyhow::{Context, Result, ensure};
use resonance_content::menu_data::CustomizeSettings;
use resonance_events::input::Button;
use resonance_game::field::{FieldInput, FieldSession};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A prepared player save is optional; without one the scenario starts at title.
/// Fresh games may choose initial preferences before field preparation.
/// Saved games retain their own preferences; all movement uses ordinary input.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldCapture {
    pub checkpoint: Option<PathBuf>,
    pub initial_preferences: Option<CustomizeSettings>,
    pub scenario: CheckpointReplay,
}

impl FieldCapture {
    fn validate(&self) -> Result<()> {
        self.scenario.validate()?;
        ensure!(
            self.checkpoint.is_none() || self.initial_preferences.is_none(),
            "initial preferences require a fresh New Game"
        );
        if let Some(preferences) = &self.initial_preferences {
            preferences.validate()?;
        }
        Ok(())
    }

    fn app(&self, root: &Path, output: &Path) -> Result<bevy::prelude::App> {
        self.validate()?;
        let (mut app, _) = crate::build_app_with_display(
            RunOptions {
                assets: root.into(),
                script_root: None,
                capture: Some(output.into()),
                capture_at: None,
                saves: SaveOptions {
                    directory: Some(output.with_extension("slots")),
                    load: self.checkpoint.clone(),
                    ..Default::default()
                },
                silent: true,
                paranoid: true,
                skip_intro: true,
                reveal: self.checkpoint.is_none(),
                selected: 0,
                record_playthrough: None,
                record_title_ticks: 0,
                skip_battles: false,
                allow_incomplete_scripts: false,
            },
            crate::Resolution::default(),
        )?;
        if let Some(preferences) = &self.initial_preferences {
            app.insert_resource(crate::new_game::InitialPreferences(preferences.clone()));
        }
        Ok(app)
    }
}

/// Render one scenario capture through the production held GPU readback.
pub fn capture_field(root: &Path, output: &Path, spec: &FieldCapture) -> Result<()> {
    saves::capture_image(spec.app(root, output)?, output, &spec.scenario)
}

/// Record the named scenario images and the real native audio between them.
pub fn capture_field_sequence(root: &Path, output: &Path, spec: &FieldCapture) -> Result<()> {
    saves::record_app(spec.app(root, output)?, output, &spec.scenario, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintained_field_captures_use_native_scenarios() {
        for json in [
            include_str!("../examples/scenarios/setup-capture.json"),
            include_str!("../examples/scenarios/classroom-capture.json"),
            include_str!("../examples/scenarios/classroom-dialogue-capture.json"),
            include_str!("../../../tools/oracle/cases/classroom-walking.json"),
            include_str!("../../../tools/oracle/cases/classroom-emotes.json"),
            include_str!("../../../tools/oracle/cases/eraser-particles.json"),
        ] {
            let spec: FieldCapture = serde_json::from_str(json).unwrap();
            spec.validate().unwrap();
        }
    }

    #[test]
    fn initial_preferences_belong_only_to_fresh_games() {
        let mut spec: FieldCapture = serde_json::from_str(include_str!(
            "../examples/scenarios/classroom-dialogue-capture.json"
        ))
        .unwrap();
        assert!(spec.initial_preferences.is_none());
        spec.validate().unwrap();
        spec.initial_preferences = Some(CustomizeSettings::default());
        spec.validate().unwrap();
        spec.initial_preferences.as_mut().unwrap().window = 3;
        assert!(spec.validate().is_err());
        spec.initial_preferences.as_mut().unwrap().window = 2;
        spec.checkpoint = Some("player-save.json".into());
        assert!(spec.validate().is_err());
        spec.initial_preferences = None;
        spec.validate().unwrap();
    }
}

/// Isolate an authored pose or dialogue at a documented observer position.
/// Samples select existing cooked clips; no observed bones or pixels are loaded.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassroomProbe {
    pub position: [f32; 3],
    pub heading: f32,
    /// Let the normal follow camera settle at the registered observer position
    /// before opening a conversation. Does not inject camera or actor poses.
    #[serde(default)]
    pub approach_settle_ticks: u32,
    pub settle_ticks: u32,
    pub interaction_actor: Option<i32>,
    /// Appearance applied after the real interaction reaches its captured state.
    #[serde(default)]
    pub preferences: Option<resonance_content::menu_data::CustomizeSettings>,
    #[serde(default)]
    pub animation_samples: BTreeMap<i32, f32>,
}

impl ClassroomProbe {
    pub(super) fn apply(&self, session: &mut FieldSession) -> Result<()> {
        ensure!(
            session.events.world.input_enabled,
            "probe requires field control"
        );
        ensure!(
            self.position
                .iter()
                .chain([&self.heading])
                .all(|v| v.is_finite())
                && self.settle_ticks <= 3600
                && self.approach_settle_ticks <= 3600,
            "invalid probe position or duration"
        );
        let player = session.events.world.controlled_actor;
        let actor = session
            .events
            .world
            .actors
            .get_mut(&player)
            .context("probe player")?;
        actor.position = self.position;
        actor.face(self.heading);
        actor.motion = None;
        for _ in 0..self.approach_settle_ticks {
            session.step(FieldInput::default())?;
            session.events.world.audio_commands.clear();
        }
        if let Some(target) = self.interaction_actor {
            ensure!(
                session.interaction_target() == Some(target),
                "probe interaction target differs"
            );
            session.step(FieldInput {
                pressed_buttons: [Button::Accept].into(),
                ..Default::default()
            })?;
        }
        for _ in 0..self.settle_ticks {
            session.step(FieldInput::default())?;
            session.events.world.audio_commands.clear();
        }
        let tick = session.events.tick();
        for (&id, &sample) in &self.animation_samples {
            let animation = session
                .events
                .world
                .actors
                .get_mut(&id)
                .with_context(|| format!("probe actor {id}"))?
                .animation
                .as_mut()
                .with_context(|| format!("probe actor {id} animation"))?;
            ensure!(
                sample.is_finite() && (0. ..=animation.duration_ticks as f32).contains(&sample),
                "probe animation sample exceeds authored clip for actor {id}"
            );
            animation.start_frame = sample;
            // This diagnostic explicitly chooses the already-evaluated pose;
            // it does not perform another native animation binding.
            animation.start_tick = tick.saturating_sub(animation.blend_ticks);
            animation.phase_tick = tick;
        }
        Ok(())
    }
}
