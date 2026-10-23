//! Encounter-owned artwork with its own animation and fixed world placement.
use super::{EffectModelFrame, PreparedEffectModel};
use anyhow::Result;
use glam::{Mat4, Quat};
use resonance_content::secondary_motion::Simulation;

pub(super) struct IndependentModel {
    model: PreparedEffectModel,
    secondary: Vec<Simulation>,
}

impl IndependentModel {
    pub(super) fn new(mut model: PreparedEffectModel) -> Result<Self> {
        model.play_repeating(0, true)?;
        Ok(Self {
            model,
            secondary: Vec::new(),
        })
    }

    pub(super) fn step(&mut self, advance: bool) -> Result<EffectModelFrame> {
        if advance {
            self.model.advance()?;
        }
        let rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        self.model.sample_at(
            Mat4::from_quat(rotation),
            rotation,
            &mut self.secondary,
            advance,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn independent_playback_loops_authored_duration_and_holds_when_paused() -> Result<()> {
        let source = crate::tests::model_definition(41, [(0, 3.)]);
        let mut model = IndependentModel::new(PreparedEffectModel::new(Arc::new(
            crate::EffectModelDefinition {
                resource: crate::ResourceId(41),
                skeleton: source.skeleton.clone(),
                motions: source.motions.clone(),
                secondary_motion: vec![],
            },
        ))?)?;
        let initial = model.step(false)?;
        for _ in 0..6 {
            model.step(true)?;
        }
        // The shared animation clock presents the endpoint before wrapping.
        assert_eq!(model.step(false)?.frame, 3.);
        let looped = model.step(true)?;
        assert_eq!(looped.clip, Some(0));
        assert_eq!(looped.frame, 0.5);
        assert_eq!(model.step(false)?, looped);
        assert_eq!(looped.world, initial.world);
        assert_eq!(model.step(true)?.frame, 1.);
        Ok(())
    }
}
