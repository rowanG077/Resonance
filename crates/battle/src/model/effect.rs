use super::{AnimatedPose, Clock, Playback, secondary};
use anyhow::{Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec3};
use resonance_content::{
    animation::{Matrix, Motion, Skeleton},
    secondary_motion::{Chain, Simulation},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub struct EffectModelDefinition {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub secondary_motion: Vec<Chain>,
}

/// Shared model data with playback owned by one particle.
#[derive(Debug, Clone)]
pub struct PreparedEffectModel {
    definition: Arc<EffectModelDefinition>,
    animation: Option<(u16, AnimatedPose)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectModelFrame {
    pub resource: u32,
    pub clip: Option<u16>,
    pub frame: f32,
    pub world: Matrix,
    pub bones: Arc<Vec<Matrix>>,
}

impl PreparedEffectModel {
    pub fn resource(&self) -> u32 {
        self.definition.resource
    }
    pub fn new(definition: Arc<EffectModelDefinition>) -> Result<Self> {
        definition.skeleton.validate()?;
        for motion in definition.motions.values() {
            motion.validate(&definition.skeleton)?;
        }
        for chain in &definition.secondary_motion {
            chain.validate(definition.skeleton.bones.len())?;
        }
        Ok(Self {
            definition,
            animation: None,
        })
    }

    pub(crate) fn validate_clip(&self, clip: u16) -> Result<()> {
        ensure!(
            self.definition.motions.contains_key(&clip),
            "unprepared effect model motion"
        );
        Ok(())
    }

    pub(crate) fn play_repeating(&mut self, clip: u16, repeat: bool) -> Result<()> {
        self.validate_clip(clip)?;
        let play = Playback {
            clip,
            frame: 0.,
            rate: 0.5,
            repeat,
        };
        let motion = &self.definition.motions[&clip];
        if let Some((requested, animation)) = &mut self.animation {
            *requested = clip;
            animation.start(Clock::new(play, motion.duration_frames, 0)?);
        } else {
            self.animation = Some((
                clip,
                AnimatedPose::new(&self.definition.skeleton, motion, play)?,
            ));
        }
        Ok(())
    }

    pub(crate) fn advance(&mut self) -> Result<()> {
        if let Some((clip, animation)) = &mut self.animation {
            animation.advance(&self.definition.skeleton, &self.definition.motions[clip])?;
        }
        Ok(())
    }

    pub(crate) fn sample(
        &self,
        particle: &crate::ParticleFrame,
        secondary: &mut Vec<Simulation>,
        elevation: Option<f32>,
    ) -> Result<EffectModelFrame> {
        let (mut pose, sampled) = if let Some((clip, animation)) = &self.animation {
            (
                animation.pose(&self.definition.skeleton, [false; 3])?.0,
                Some((*clip, animation.clock.frame)),
            )
        } else {
            (self.definition.skeleton.bind_pose()?, None)
        };
        let state = &particle.state;
        let crate::ParticleGeometry::Size { value, .. } = state.geometry else {
            anyhow::bail!("effect model requires scale dimensions");
        };
        let [x, y, z] = state.angles.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
        let scale = Vec3::from_array(value);
        let mut position = Vec3::from_array(particle.origin) + Vec3::from_array(state.offset);
        if let Some(elevation) = elevation {
            position.y += elevation;
        }
        let world = Mat4::from_scale_rotation_translation(scale, rotation, position);
        if self.animation.is_some() {
            secondary.resize_with(self.definition.secondary_motion.len(), Simulation::default);
            secondary::apply(
                &self.definition.secondary_motion,
                secondary,
                &mut pose.global,
                secondary::Placement {
                    world: world.to_cols_array_2d(),
                    rotation,
                },
                true,
            )?;
        }
        Ok(EffectModelFrame {
            resource: self.definition.resource,
            clip: sampled.map(|(clip, _)| clip),
            frame: sampled.map_or(0., |(_, frame)| frame),
            world: world.to_cols_array_2d(),
            bones: Arc::new(pose.global),
        })
    }
}
