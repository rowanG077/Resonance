//! Development input registration against a naturally reached actor pose.
//! Replays supply controls only; they never overwrite scene state or assets.
use super::FieldSession;
use anyhow::{Result, ensure};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputReplay {
    pub actor: i32,
    pub position: [f32; 3],
    pub animation_slot: u16,
    pub animation_sample: f32,
    #[serde(default = "default_duration")]
    pub duration_updates: u32,
    pub accept_updates: Vec<u32>,
}
fn default_duration() -> u32 {
    30000
}
impl InputReplay {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.position.iter().all(|v| v.is_finite())
                && self.animation_sample.is_finite()
                && self.animation_sample >= 0.,
            "invalid replay anchor"
        );
        ensure!(
            (1..=30000).contains(&self.duration_updates)
                && self.accept_updates.len() <= 30000
                && self
                    .accept_updates
                    .iter()
                    .all(|t| *t > 0 && *t <= self.duration_updates),
            "invalid replay input range"
        );
        ensure!(
            self.accept_updates.windows(2).all(|w| w[0] < w[1]),
            "replay inputs must be strictly ordered"
        );
        Ok(())
    }
    pub fn matches(&self, session: &FieldSession) -> bool {
        session
            .events
            .world
            .actors
            .get(&self.actor)
            .is_some_and(|actor| {
                actor
                    .position
                    .iter()
                    .zip(self.position)
                    .all(|(a, b)| (*a - b).abs() < 0.0001)
                    && actor.animation.as_ref().is_some_and(|animation| {
                        animation.slot == self.animation_slot
                            && (animation.sample(
                                session.events.tick(),
                                0,
                                animation.duration_ticks as f32,
                            ) - self.animation_sample)
                                .abs()
                                < 0.001
                    })
            })
    }
    /// None returns control to the caller's ordinary replay policy.
    pub fn accept_at(&self, update: u32) -> Option<bool> {
        (update <= self.duration_updates)
            .then(|| self.accept_updates.binary_search(&update).is_ok())
    }
}
