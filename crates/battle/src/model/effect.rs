use super::{AnimatedPose, Clock, Playback, secondary};
use anyhow::{Context, Result, ensure};
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

/// Shared model data with playback owned by one encounter instance.
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

    pub(crate) fn attachment_bone(&self, joint: u8) -> Result<usize> {
        let prefix = format!("kk0{joint}");
        self.definition
            .skeleton
            .bones
            .iter()
            .position(|bone| bone.name.starts_with(&prefix))
            .with_context(|| format!("effect model has no attachment joint {joint}"))
    }

    pub(crate) fn play_repeating(&mut self, clip: u16, repeat: bool) -> Result<()> {
        self.play(resonance_content::battle_effect::ModelAnimation {
            model: 0,
            clip,
            frame: 0.,
            rate: 0.5,
            repeat,
            blend_ticks: 0,
        })
    }

    pub(crate) fn play(
        &mut self,
        playback: resonance_content::battle_effect::ModelAnimation,
    ) -> Result<()> {
        playback.validate()?;
        let clip = playback.clip;
        self.validate_clip(clip)?;
        let motion = &self.definition.motions[&clip];
        let frame = if playback.repeat && playback.frame > motion.duration_frames {
            playback.frame.rem_euclid(motion.duration_frames)
        } else {
            playback.frame
        };
        let play = Playback {
            clip,
            frame,
            rate: playback.rate,
            repeat: playback.repeat,
        };
        if let Some((requested, animation)) = &mut self.animation {
            *requested = clip;
            animation.start(Clock::new(
                play,
                motion.duration_frames,
                playback.blend_ticks,
            )?);
        } else {
            ensure!(
                playback.blend_ticks == 0,
                "effect animation blend has no previous pose"
            );
            self.animation = Some((
                clip,
                AnimatedPose::new(&self.definition.skeleton, motion, play)?,
            ));
        }
        Ok(())
    }

    pub(crate) fn advance(&mut self) -> Result<()> {
        if let Some((clip, animation)) = &mut self.animation {
            let skeleton = &self.definition.skeleton;
            let motion = &self.definition.motions[clip];
            animation.advance(skeleton, motion)?;
        }
        Ok(())
    }

    pub(crate) fn sample(
        &self,
        particle: &crate::ParticleFrame,
        secondary: &mut Vec<Simulation>,
        elevation: Option<f32>,
        ground_relative: bool,
        parent: Option<Matrix>,
    ) -> Result<EffectModelFrame> {
        let state = &particle.state;
        let crate::ParticleGeometry::Size { value, .. } = state.geometry else {
            anyhow::bail!("effect model requires scale dimensions");
        };
        let [x, y, z] = state.angles.map(f32::to_radians);
        let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
        let scale = Vec3::from_array(value);
        let mut position = Vec3::from_array(particle.origin) + Vec3::from_array(state.offset);
        if ground_relative {
            position.y = state.offset[1];
        }
        if let Some(elevation) = elevation {
            position.y += elevation;
        }
        // Attached models inherit the joint's full affine placement, including shear.
        let world = parent.map_or_else(
            || Mat4::from_scale_rotation_translation(scale, rotation, position),
            |parent| {
                let parent = Mat4::from_cols_array_2d(&parent);
                if particle.owner_bone.is_some() {
                    parent
                        * Mat4::from_scale_rotation_translation(
                            scale,
                            rotation,
                            Vec3::from_array(state.offset),
                        )
                } else {
                    parent
                }
            },
        );
        let rotation = if parent.is_some() && world.determinant() != 0. {
            world.to_scale_rotation_translation().1
        } else {
            rotation
        };
        self.sample_at(world, rotation, secondary, true)
    }

    pub(crate) fn sample_at(
        &self,
        world: Mat4,
        rotation: Quat,
        secondary: &mut Vec<Simulation>,
        advance: bool,
    ) -> Result<EffectModelFrame> {
        let (mut pose, sampled) = if let Some((clip, animation)) = &self.animation {
            (
                animation.pose(&self.definition.skeleton, [false; 3])?.0,
                Some((*clip, animation.clock.frame)),
            )
        } else {
            (self.definition.skeleton.bind_pose()?, None)
        };
        if self.animation.is_some() && !self.definition.secondary_motion.is_empty() {
            secondary.resize_with(self.definition.secondary_motion.len(), Simulation::default);
            secondary::apply(
                &self.definition.secondary_motion,
                secondary,
                &mut pose.global,
                secondary::Placement {
                    world: world.to_cols_array_2d(),
                    acceleration: Vec3::ZERO,
                    rotation,
                },
                advance,
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

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_effect::ModelAnimation;

    #[test]
    fn effect_model_blends_freeze_frames_and_hold_or_loop() -> Result<()> {
        let source = crate::tests::model_definition(41, [(0, 3.), (1, 12.), (2, 25.)]);
        let mut model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
            resource: 41,
            skeleton: source.skeleton,
            motions: source.motions,
            secondary_motion: vec![],
        }))?;
        model.play_repeating(0, false)?;
        for _ in 0..10 {
            model.advance()?;
        }
        assert_eq!(model.animation.as_ref().unwrap().1.clock.frame, 3.);
        for repeat in [false, true] {
            model.play(ModelAnimation {
                model: 0,
                clip: 1,
                frame: 0.,
                blend_ticks: 2,
                rate: 0.4,
                repeat,
            })?;
            // The request update presents frame zero, followed by two blending visits.
            for _ in 0..3 {
                model.advance()?;
                assert_eq!(model.animation.as_ref().unwrap().1.clock.frame, 0.);
            }
            model.advance()?;
            assert_eq!(model.animation.as_ref().unwrap().1.clock.frame, 0.4);
            for _ in 0..31 {
                model.advance()?;
            }
            let frame = model.animation.as_ref().unwrap().1.clock.frame;
            if repeat {
                assert!((frame - 0.8).abs() < 0.00001);
            } else {
                assert_eq!(frame, 12.);
            }
        }
        for (frame, expected) in [(0., 0.), (25., 25.), (26., 1.), (50., 0.)] {
            model.play(ModelAnimation {
                model: 0,
                clip: 2,
                frame,
                blend_ticks: 0,
                rate: 0.,
                repeat: true,
            })?;
            for _ in 0..3 {
                model.advance()?;
                assert_eq!(model.animation.as_ref().unwrap().1.clock.frame, expected);
            }
        }
        Ok(())
    }
}
