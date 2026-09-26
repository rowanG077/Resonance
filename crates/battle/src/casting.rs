//! Prepared casting parameters. Sequencing and clock changes live in casting.sym.
use crate::ResourceBinding;
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy)]
pub struct CastMotion {
    pub age: u16,
    /// Index in this action's resource bindings.
    pub motion: usize,
    pub blend: u8,
    pub frame: f32,
    pub rate: f32,
    pub repeat: bool,
    pub loop_start: f32,
}

impl CastMotion {
    pub(crate) fn values(self) -> [i32; 7] {
        [
            i32::from(self.age),
            self.motion as i32,
            i32::from(self.blend),
            self.frame.to_bits() as i32,
            self.rate.to_bits() as i32,
            i32::from(self.repeat),
            self.loop_start.to_bits() as i32,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct CastingDefinition {
    pub base: i16,
    pub extra: i16,
    pub recovery: u16,
    pub tp_cost: u16,
    pub release: CastMotion,
    pub chant: Vec<CastMotion>,
    pub pulse_member: u16,
    pub effect_scale: f32,
    pub tint: resonance_content::battle_effect::EffectTint,
}

impl CastingDefinition {
    pub(crate) fn validate(&self, resources: &[ResourceBinding], tp_cost: u16) -> Result<()> {
        ensure!(
            self.tp_cost == tp_cost,
            "casting admission cost differs from technique"
        );
        ensure!(
            self.recovery <= i16::MAX as u16
                && self.effect_scale.is_finite()
                && self.effect_scale >= 0.
                && !self.chant.is_empty()
                && self.chant[0].age == 0
                && self.chant.windows(2).all(|w| w[0].age < w[1].age),
            "invalid prepared casting parameters"
        );
        for pose in self.chant.iter().chain([&self.release]) {
            ensure!(
                pose.age <= i16::MAX as u16
                    && matches!(resources.get(pose.motion), Some(ResourceBinding::Motion(_)))
                    && pose.frame.is_finite()
                    && pose.frame >= 0.
                    && pose.rate.is_finite()
                    && pose.rate > 0.
                    && pose.loop_start.is_finite()
                    && pose.loop_start >= 0.,
                "invalid prepared casting motion"
            );
        }
        Ok(())
    }
}
