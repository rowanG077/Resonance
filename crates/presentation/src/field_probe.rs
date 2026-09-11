//! Registered rendering diagnostics, separate from the normal New Game replay.
use anyhow::{Context, Result, ensure};
use resonance_game::field::{FieldInput, FieldSession};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Isolate stochastic effects using a documented seed at a naturally reached
/// script pose. The script still creates and animates every particle itself.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleProbe {
    pub anchor: resonance_game::field::replay::InputReplay,
    pub random_state: u32,
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
                interact: true,
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
            animation.binding_updates = 0;
            animation.start_tick = tick.saturating_sub(animation.blend_ticks);
            animation.phase_tick = tick;
        }
        Ok(())
    }
}
