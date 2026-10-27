use super::{AnimatedPose, Clock, Playback, secondary};
use crate::ResourceId;
use anyhow::{Context, Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec3};
use resonance_content::{
    animation::{Matrix, Motion, Skeleton},
    secondary_motion::{Chain, Simulation},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub struct EffectModelDefinition {
    pub resource: ResourceId,
    pub skeleton: Skeleton,
    pub motions: BTreeMap<u16, Motion>,
    pub secondary_motion: Vec<Chain>,
}

/// Shared model data with playback owned by one encounter instance.
#[derive(Debug, Clone)]
pub struct PreparedEffectModel {
    definition: Arc<EffectModelDefinition>,
    animation: Option<(u16, AnimatedPose, Arc<EffectModelDefinition>)>,
    layers: Vec<PreparedEffectModel>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectModelFrame {
    pub resource: ResourceId,
    pub clip: Option<u16>,
    pub frame: f32,
    pub world: Matrix,
    pub attachment: Option<EffectModelAttachment>,
    pub bones: Arc<resonance_content::animation::pose::Palette>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectModelParent {
    Actor {
        actor: crate::ActorId,
        bone: usize,
    },
    Particle {
        particle: crate::ParticleId,
        bone: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectModelAttachment {
    pub parent: EffectModelParent,
    pub local: Matrix,
}

impl PreparedEffectModel {
    pub fn resource(&self) -> ResourceId {
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
            layers: Vec::new(),
        })
    }

    /// Independent visual rigs share the particle transform, but own their animation clocks.
    pub fn with_layers(mut self, layers: Vec<Self>) -> Result<Self> {
        for layer in &layers {
            ensure!(layer.layers.is_empty(), "nested effect model layers");
            for &clip in self.definition.motions.keys() {
                layer.validate_clip(clip)?;
            }
        }
        self.layers = layers;
        Ok(self)
    }

    /// Start an equipment layer's own loop before any particle borrows the model.
    pub fn with_playback(mut self, playback: Playback) -> Result<Self> {
        self.play(resonance_content::battle_effect::ModelAnimation {
            model: 0,
            clip: playback.clip,
            frame: playback.frame,
            rate: playback.rate,
            repeat: playback.repeat,
            blend_ticks: 0,
        })?;
        Ok(self)
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

    /// A geometry-only selection keeps the declaration's motion resource.
    pub(crate) fn play_repeating_from(
        &mut self,
        source: &Self,
        clip: u16,
        repeat: bool,
    ) -> Result<()> {
        self.play_from(
            source.definition.clone(),
            resonance_content::battle_effect::ModelAnimation {
                model: 0,
                clip,
                frame: 0.,
                rate: 0.5,
                repeat,
                blend_ticks: 0,
            },
        )
    }

    pub(crate) fn play(
        &mut self,
        playback: resonance_content::battle_effect::ModelAnimation,
    ) -> Result<()> {
        self.play_from(self.definition.clone(), playback)
    }

    fn play_from(
        &mut self,
        source: Arc<EffectModelDefinition>,
        playback: resonance_content::battle_effect::ModelAnimation,
    ) -> Result<()> {
        playback.validate()?;
        let clip = playback.clip;
        let motion = source
            .motions
            .get(&clip)
            .context("unprepared effect model motion")?;
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
        if let Some((requested, animation, motion_source)) = &mut self.animation {
            if playback.blend_ticks > 1 {
                // Capture the outgoing effect pose from this update before blending.
                animation.advance(&self.definition.skeleton, &motion_source.motions[requested])?;
            }
            *requested = clip;
            *motion_source = source.clone();
            animation.start(Clock::new(
                play,
                motion.duration_frames,
                // Effect commands replace this update's sample when no blend is requested.
                playback.blend_ticks.max(1),
            )?);
        } else {
            ensure!(
                playback.blend_ticks == 0,
                "effect animation blend has no previous pose"
            );
            self.animation = Some((
                clip,
                AnimatedPose::new(&self.definition.skeleton, motion, play)?,
                source,
            ));
        }
        for layer in &mut self.layers {
            layer.play(playback)?;
        }
        Ok(())
    }

    pub(crate) fn reverse(&mut self) {
        if let Some((_, animation, _)) = &mut self.animation {
            animation.clock.rate = -animation.clock.rate;
            animation.clock.stopped = false;
            animation.clock.finished = false;
        }
        for layer in &mut self.layers {
            layer.reverse();
        }
    }

    pub(crate) fn advance(&mut self) -> Result<()> {
        if let Some((clip, animation, source)) = &mut self.animation {
            let skeleton = &self.definition.skeleton;
            let motion = &source.motions[clip];
            animation.advance(skeleton, motion)?;
        }
        for layer in &mut self.layers {
            layer.advance()?;
        }
        Ok(())
    }

    pub(crate) fn sample(
        &self,
        particle: &crate::ParticleFrame,
        secondary: &mut Vec<Simulation>,
        elevation: Option<f32>,
        ground_relative: bool,
        parent: Option<(EffectModelParent, Matrix)>,
        advance: bool,
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
        let local = if particle.owner_bone.is_some() {
            Mat4::from_scale_rotation_translation(scale, rotation, Vec3::from_array(state.offset))
        } else {
            Mat4::IDENTITY
        };
        let world = parent.map_or_else(
            || Mat4::from_scale_rotation_translation(scale, rotation, position),
            |(_, parent)| Mat4::from_cols_array_2d(&parent) * local,
        );
        let rotation = if parent.is_some() && world.determinant() != 0. {
            world.to_scale_rotation_translation().1
        } else {
            rotation
        };
        let mut frame = self.sample_at(world, rotation, secondary, advance)?;
        frame.attachment = parent.map(|(parent, _)| EffectModelAttachment {
            parent,
            local: local.to_cols_array_2d(),
        });
        Ok(frame)
    }

    pub(crate) fn sample_at(
        &self,
        world: Mat4,
        rotation: Quat,
        secondary: &mut Vec<Simulation>,
        advance: bool,
    ) -> Result<EffectModelFrame> {
        let secondary_count = std::iter::once(self)
            .chain(&self.layers)
            .map(|layer| layer.definition.secondary_motion.len())
            .sum();
        secondary.resize_with(secondary_count, Simulation::default);
        let (primary_secondary, mut simulations) =
            secondary.split_at_mut(self.definition.secondary_motion.len());
        let mut bones = self.sample_pose(world, rotation, primary_secondary, advance)?;
        for layer in &self.layers {
            let (layer_secondary, remaining) =
                simulations.split_at_mut(layer.definition.secondary_motion.len());
            simulations = remaining;
            bones.append(layer.sample_pose(world, rotation, layer_secondary, advance)?);
        }
        Ok(EffectModelFrame {
            resource: self.definition.resource,
            clip: self.animation.as_ref().map(|(clip, _, _)| *clip),
            frame: self
                .animation
                .as_ref()
                .map_or(0., |(_, animation, _)| animation.clock.frame),
            world: world.to_cols_array_2d(),
            attachment: None,
            bones: Arc::new(bones),
        })
    }

    fn sample_pose(
        &self,
        world: Mat4,
        rotation: Quat,
        secondary: &mut [Simulation],
        advance: bool,
    ) -> Result<resonance_content::animation::pose::Palette> {
        let mut pose = if let Some((_, animation, _)) = &self.animation {
            animation.pose(&self.definition.skeleton, [false; 3])?.0
        } else {
            resonance_content::animation::pose::Palette::bind(&self.definition.skeleton)?
        };
        if self.animation.is_some() && !self.definition.secondary_motion.is_empty() {
            let mut driven = pose.global.clone();
            secondary::apply(
                &self.definition.secondary_motion,
                secondary,
                &mut driven,
                secondary::Placement {
                    world: world.to_cols_array_2d(),
                    acceleration: Vec3::ZERO,
                    rotation,
                },
                advance,
            )?;
            pose.set_globals(driven);
        }
        Ok(pose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_effect::ModelAnimation;

    #[test]
    fn independent_layers_keep_their_rig_and_loop_length() -> Result<()> {
        use resonance_content::animation::{
            Track, TransformChannels, VectorCurve, VectorInterpolation,
        };
        let make = |duration, extra_bone| -> Result<_> {
            let mut source = crate::tests::model_definition(41, [(0, duration)]);
            source.motions.get_mut(&0).unwrap().tracks.push(Track {
                bone: 0,
                bind_channels: TransformChannels(8),
                period_frames: duration,
                times: vec![0., duration],
                translation: Some(VectorCurve {
                    interpolation: VectorInterpolation::Linear,
                    values: vec![[0.; 3], [duration, 0., 0.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            });
            if extra_bone {
                let mut child = source.skeleton.bones[0].clone();
                child.name = "child".into();
                child.parent = Some(0);
                child.bind.translation[1] = 5.;
                source.skeleton.bones.push(child);
            }
            PreparedEffectModel::new(Arc::new(EffectModelDefinition {
                resource: ResourceId(41),
                skeleton: source.skeleton,
                motions: source.motions,
                secondary_motion: vec![],
            }))
        };
        let mut model = make(4., false)?.with_layers(vec![make(8., true)?])?;
        model.play_repeating(0, true)?;
        for _ in 0..10 {
            model.advance()?;
        }
        let world = Mat4::from_translation(Vec3::new(10., 20., 30.));
        let frame = model.sample_at(world, Quat::IDENTITY, &mut vec![], true)?;
        assert_eq!(frame.world, world.to_cols_array_2d());
        assert_eq!(frame.bones.len(), 3);
        assert_eq!(frame.bones[0][3], [1., 0., 0., 1.]);
        assert_eq!(frame.bones[1][3], [5., 0., 0., 1.]);
        assert_eq!(frame.bones[2][3], [5., 5., 0., 1.]);

        let source = crate::tests::model_definition(41, [(0, 1.)]);
        let mut rigid = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
            resource: ResourceId(41),
            skeleton: source.skeleton,
            motions: BTreeMap::new(),
            secondary_motion: vec![],
        }))?
        .with_layers(vec![make(8., true)?.with_playback(Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        })?])?;
        for _ in 0..10 {
            rigid.advance()?;
        }
        let frame = rigid.sample_at(world, Quat::IDENTITY, &mut vec![], true)?;
        assert_eq!(frame.bones[0][3], [0., 0., 0., 1.]);
        assert_eq!(frame.bones[1][3], [5., 0., 0., 1.]);
        assert_eq!(frame.bones[2][3], [5., 5., 0., 1.]);

        let mut source = make(8., true)?;
        let definition = Arc::make_mut(&mut source.definition);
        let motion = definition.motions.get_mut(&0).unwrap();
        let mut child_track = motion.tracks[0].clone();
        child_track.bone = 1;
        motion.tracks.push(child_track);
        motion.validate(&definition.skeleton)?;
        let mut selected = make(4., false)?;
        Arc::make_mut(&mut selected.definition).resource = ResourceId(42);
        selected.play_repeating_from(&source, 0, true)?;
        for _ in 0..10 {
            selected.advance()?;
        }
        let frame = selected.sample_at(world, Quat::IDENTITY, &mut vec![], true)?;
        assert_eq!(frame.resource, ResourceId(42));
        assert_eq!(frame.bones.len(), 1);
        assert_eq!(frame.frame, 5.);
        assert_eq!(frame.bones[0][3], [5., 0., 0., 1.]);
        selected.play_repeating(0, true)?;
        for _ in 0..11 {
            selected.advance()?;
        }
        assert_eq!(selected.animation.as_ref().unwrap().1.clock.frame, 1.);
        Ok(())
    }

    #[test]
    fn effect_model_blends_freeze_frames_and_hold_or_loop() -> Result<()> {
        let source = crate::tests::model_definition(41, [(0, 3.), (1, 12.), (2, 25.)]);
        let mut model = PreparedEffectModel::new(Arc::new(EffectModelDefinition {
            resource: ResourceId(41),
            skeleton: source.skeleton,
            motions: source.motions,
            secondary_motion: vec![],
        }))?;
        model.play_repeating(0, false)?;
        for _ in 0..10 {
            model.advance()?;
        }
        assert_eq!(model.animation.as_ref().unwrap().1.clock.frame, 3.);
        model.reverse();
        for expected in [2.5, 2., 1.5, 1., 0.5, 0., 0.] {
            model.advance()?;
            assert_eq!(model.animation.as_ref().unwrap().1.clock.frame, expected);
        }
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
