//! Models shared by the particles of one stored scene (3F0C0/3FC24).
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

/// Verified, in-memory model template. A stored scene clones it on entry;
/// immutable geometry and clips remain shared while playback belongs to battle.
#[derive(Debug, Clone)]
pub struct PreparedEffectModel {
    definition: Arc<EffectModelDefinition>,
    animation: Option<(u16, AnimatedPose)>,
    sampled: Option<(u16, f32)>,
    secondary: Vec<Simulation>,
}

#[derive(Debug, Clone, Copy)]
pub struct EffectMotionBinding {
    pub bank: u32,
    pub model: u8,
    pub clip: u16,
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
            secondary: vec![Simulation::default(); definition.secondary_motion.len()],
            definition,
            animation: None,
            sampled: None,
        })
    }

    pub(crate) fn validate_clip(&self, clip: u16) -> Result<()> {
        ensure!(
            self.definition.motions.contains_key(&clip),
            "unprepared effect model motion"
        );
        Ok(())
    }

    pub(crate) fn play(&mut self, clip: u16) -> Result<()> {
        self.play_repeating(clip, true)
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
            animation.clock = Clock::new(play, motion.duration_frames, 0)?;
        } else {
            self.animation = Some((
                clip,
                AnimatedPose::new(&self.definition.skeleton, motion, play)?,
            ));
        }
        Ok(())
    }

    pub(crate) fn step(
        &mut self,
        particle: &crate::ParticleFrame,
        advance: bool,
    ) -> Result<EffectModelFrame> {
        let (mut pose, motion, sampled) = if let Some((clip, animation)) = &mut self.animation {
            let motion = &self.definition.motions[clip];
            if advance {
                animation.advance(&self.definition.skeleton, motion)?;
                self.sampled = Some((*clip, animation.clock.frame));
            }
            let (pose, _) = animation.pose(&self.definition.skeleton, [false; 3])?;
            (
                pose,
                Some(motion),
                self.sampled.unwrap_or((*clip, animation.clock.frame)),
            )
        } else {
            (self.definition.skeleton.bind_pose()?, None, (0, 0.))
        };
        let state = &particle.state;
        let crate::ParticleGeometry::Size { value, .. } = state.geometry else {
            anyhow::bail!("effect model requires scale dimensions");
        };
        let [x, y, z] = state.angles.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
        let scale = Vec3::from_array(value);
        let position = Vec3::from_array(particle.origin) + Vec3::from_array(state.offset);
        let world = Mat4::from_scale_rotation_translation(scale, rotation, position);
        if let Some(motion) = motion {
            secondary::apply(
                &self.definition.secondary_motion,
                motion,
                &mut self.secondary,
                &mut pose.global,
                secondary::Placement {
                    world: world.to_cols_array_2d(),
                    rotation,
                    scale,
                },
                advance,
                [0.; 3],
            )?;
        }
        let (clip, frame) = sampled;
        Ok(EffectModelFrame {
            resource: self.definition.resource,
            clip: motion.map(|_| clip),
            frame,
            world: world.to_cols_array_2d(),
            bones: Arc::new(pose.global),
        })
    }
}
