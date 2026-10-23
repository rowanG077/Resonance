//! Persistent attachments sample declared placement, motion and rendering layers.
use anyhow::{Context, Result};
use bevy::prelude::{Mat4, Vec3};
use resonance_battle::BattleFrame;
use resonance_content::animation::{Matrix, multiply, pose};
use resonance_content::random::Random;
use resonance_events::Animation;
use resonance_game::battle::encounter::{AccessoryStyle, ModelAccessory};

// Authored-frame delay and opacity, beginning with the current pose.
const WING_LAYERS: [(f32, u8); 3] = [(0., 111), (5., 191), (10., 111)];
const COLOR_SCROLL_PER_TICK: f32 = 0.01;
const LAYER_COLOR_OFFSET: f32 = 0.15;

const SPARKLE_INTERVAL: u32 = 5;
const SPARKLE_LIFETIME: u32 = 32;
const SPARKLE_PALETTE_TICKS: u32 = 4;
const SPARKLE_PALETTE_CYCLE: [u8; 8] = [27, 28, 29, 30, 31, 30, 29, 28];
const MAX_SPARKLES: usize = 256;

pub(super) struct Sparkle {
    pub position: Vec3,
    pub roll: f32,
    fall_speed: f32,
    remaining: u32,
}

impl Sparkle {
    pub fn palette(&self) -> u8 {
        SPARKLE_PALETTE_CYCLE[(7 - self.remaining / SPARKLE_PALETTE_TICKS) as usize]
    }

    pub fn alpha(&self) -> f32 {
        (self.remaining * 4) as f32 / 255.
    }
}

#[derive(Default)]
struct Sparkles {
    particles: Vec<Sparkle>,
    random: Random,
    parity: usize,
}

impl Sparkles {
    fn step(&mut self, tick: u32, wing: Option<&Frame>) {
        if tick.is_multiple_of(SPARKLE_INTERVAL)
            && let Some(wing) = wing
        {
            let world = Mat4::from_cols_array_2d(&wing.world);
            for bone in wing.bones.iter().skip(1 - self.parity).step_by(2) {
                if self.particles.len() == MAX_SPARKLES {
                    break;
                }
                let position =
                    (world * Mat4::from_cols_array_2d(bone)).transform_point3(Vec3::ZERO);
                let fall_speed = 0.2 + 0.1 * (self.random.next_u32() % 3) as f32;
                let roll = (self.random.next_u32() % 360) as f32;
                let jitter = Vec3::from_array(
                    [30, 15, 30].map(|range| (self.random.next_u32() as i32 % range) as f32),
                );
                self.particles.push(Sparkle {
                    position: position + jitter,
                    roll,
                    fall_speed,
                    remaining: SPARKLE_LIFETIME,
                });
            }
            self.parity ^= 1;
        }
        for particle in &mut self.particles {
            particle.position.y -= particle.fall_speed;
            particle.remaining -= 1;
        }
        self.particles.retain(|particle| particle.remaining != 0);
    }
}

pub(super) struct State {
    pub source: ModelAccessory,
    animation: Option<Animation>,
    update: Option<u64>,
    age: u32,
    sparkles: Sparkles,
    sparkles_visible: bool,
}

pub(super) struct Frame {
    pub pass: u8,
    pub world: Matrix,
    pub bones: Vec<Matrix>,
    pub tint: [u8; 4],
    pub depth_write: bool,
    pub material: resonance_battle::ModelMaterial,
    pub uv: Option<(usize, [f32; 2])>,
}

impl State {
    pub fn new(source: ModelAccessory) -> Self {
        let animation = source.motion.as_ref().map(|motion| {
            let mut animation = Animation::new(
                resonance_content::field::COLETTE_WINGS_RESOURCE,
                resonance_events::animation::slot::IDLE,
                (motion.duration_frames * resonance_content::ANIMATION_HZ
                    / resonance_content::animation::FRAME_HZ) as u32,
                0,
            );
            animation.blend_ticks = 0;
            animation.repeat = true;
            animation
        });
        Self {
            source,
            animation,
            update: None,
            age: 0,
            sparkles: Sparkles::default(),
            sparkles_visible: false,
        }
    }

    pub fn sample(&mut self, frame: &BattleFrame) -> Result<Vec<Frame>> {
        let owner = self.source.owner;
        let body = frame.models.iter().find(|model| model.actor == owner);
        let previous_age = self.age;
        // Hidden wings retain the encounter's animation phase.
        if let Some(previous) = self.update.replace(frame.update)
            && !frame.clock.feedback_paused()
            && !frame.paused_actors.contains(&owner)
        {
            self.age = self
                .age
                .saturating_add(frame.update.saturating_sub(previous) as u32);
        }
        self.sparkles_visible = self.source.visible
            && body.is_some_and(|body| body.visible)
            && super::actor_visible(frame, owner);
        if !self.sparkles_visible {
            return Ok(vec![]);
        }
        let body = body.unwrap();
        let world = if let Some(attachment) = self.source.attachment {
            let parent = multiply(
                body.world,
                *body
                    .bones
                    .get(attachment)
                    .context("attached model pose is missing")?,
            );
            // Native attachments retain their parent's translation and rotation;
            // the record already includes the actor's model scale.
            let parent = Mat4::from_cols_array_2d(&parent);
            let (_, rotation, translation) = parent.to_scale_rotation_translation();
            Mat4::from_rotation_translation(rotation, translation)
                * Mat4::from_cols_array_2d(&self.source.transform.matrix())
        } else {
            Mat4::from_cols_array_2d(&self.source.transform.matrix())
        };
        let duration = self
            .source
            .motion
            .as_ref()
            .map_or(0., |motion| motion.duration_frames);
        let time = self.animation.as_ref().map_or(0., |animation| {
            animation.sample(
                self.age,
                0,
                duration * resonance_content::ANIMATION_HZ / resonance_content::animation::FRAME_HZ,
            ) * resonance_content::animation::FRAME_HZ
                / resonance_content::ANIMATION_HZ
        });
        let ordinary = matches!(self.source.style, AccessoryStyle::Ordinary);
        let layers: &[(f32, u8)] = if ordinary { &[(0., 255)] } else { &WING_LAYERS };
        let layers = layers
            .iter()
            .copied()
            .enumerate()
            .map(|(pass, (delay, alpha))| {
                let mut sample = time - delay;
                if duration > 0. && (sample < 0. || sample > duration) {
                    let phase = sample.rem_euclid(duration);
                    sample = if phase == 0. && sample > 0. {
                        duration
                    } else {
                        phase
                    };
                }
                let local = if let Some(motion) = &self.source.motion {
                    pose::sample(&self.source.skeleton, motion, sample)?
                } else {
                    self.source
                        .skeleton
                        .bones
                        .iter()
                        .map(|bone| pose::sample_bone(bone.bind, None, 0.))
                        .collect::<Result<Vec<_>>>()?
                };
                let mut bones = Vec::with_capacity(local.len());
                for (bone, value) in self.source.skeleton.bones.iter().zip(local) {
                    let matrix = value.matrix();
                    bones.push(bone.parent.map_or(matrix, |parent| {
                        multiply(bones[usize::from(parent)], matrix)
                    }));
                }
                let mut tint = if self.source.inherit_tint {
                    body.tint
                } else {
                    [64, 64, 64, 255]
                };
                if !ordinary {
                    tint[3] = if pass == 0 && self.age == 0 {
                        255
                    } else {
                        alpha
                    };
                }
                Ok(Frame {
                    pass: pass as u8,
                    world: world.to_cols_array_2d(),
                    bones,
                    tint,
                    depth_write: self.source.depth_write && body.depth_write,
                    // The original renderer copies the material mode independently
                    // of flag 0x20, which controls color/alpha inheritance only.
                    material: body.material,
                    uv: match self.source.style {
                        AccessoryStyle::Ordinary => None,
                        AccessoryStyle::EnemyWings => Some((1, [self.age as f32 * 0.02, 0.])),
                        AccessoryStyle::Colette { .. } => Some((
                            0,
                            [
                                self.age as f32 * COLOR_SCROLL_PER_TICK
                                    - pass as f32 * LAYER_COLOR_OFFSET,
                                0.,
                            ],
                        )),
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if self.source.sparkle_texture().is_some() {
            for tick in previous_age + 1..=self.age {
                self.sparkles.step(tick, layers.last());
            }
        }
        Ok(layers)
    }

    pub fn sparkles(&self) -> &[Sparkle] {
        if self.sparkles_visible {
            &self.sparkles.particles
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{
        Actor, ActorId, BattleClock, Control, EquipmentAttributes, ModelFrame, PreparedBattle,
        ResourceId, Side,
    };
    use resonance_content::animation::{
        Bone, Motion, Skeleton, Track, Transform, TransformChannels, VectorCurve,
        VectorInterpolation,
    };
    use std::sync::Arc;

    fn fixture() -> Result<(BattleFrame, ModelAccessory)> {
        let owner = ActorId::from_index(0)?;
        let actor = Actor::new(
            Side::Party,
            Control::Manual,
            EquipmentAttributes::new(100, 50, Default::default()),
        );
        let core = PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1)?
            .finish()?;
        let mut frame = core.snapshot();
        frame.models.push(ModelFrame {
            actor: owner,
            visible: true,
            depth_write: true,
            tint: [32, 48, 64, 100],
            material: resonance_battle::ModelMaterial::Normal,
            texture_layers: [0; 4],
            texture_tick: 0,
            light: None,
            shadow: None,
            resource: ResourceId(1),
            clip: 0,
            frame: 0.,
            blend_weight: 1.,
            root_translation: [0.; 3],
            world: Mat4::from_scale_rotation_translation(
                Vec3::splat(3.),
                Default::default(),
                Vec3::X * 100.,
            )
            .to_cols_array_2d(),
            particle_world: Mat4::IDENTITY.to_cols_array_2d(),
            bones: Arc::new(vec![
                Mat4::from_translation(Vec3::X * 2.).to_cols_array_2d(),
            ]),
        });
        let source = ModelAccessory {
            owner,
            resource: ResourceId(2),
            attachment: Some(0),
            transform: Transform {
                translation: [1., 0., 0.],
                scale: [2.; 3],
                ..Default::default()
            },
            skeleton: Skeleton {
                bones: vec![Bone {
                    name: "root".into(),
                    parent: None,
                    bind_channels: TransformChannels(0),
                    bind: Transform::default(),
                }],
            },
            motion: None,
            style: AccessoryStyle::Ordinary,
            visible: true,
            inherit_tint: true,
            depth_write: true,
        };
        Ok((frame, source))
    }

    fn motion() -> Motion {
        Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                bind_channels: TransformChannels(0),
                period_frames: 30.,
                times: vec![0., 30.],
                translation: Some(VectorCurve {
                    interpolation: VectorInterpolation::Linear,
                    values: vec![[0.; 3], [30., 0., 0.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            }],
        }
    }

    #[test]
    fn ordinary_attachment_keeps_bind_pose_tint_and_record_scale() -> Result<()> {
        let (mut frame, source) = fixture()?;
        let mut state = State::new(source);
        let poses = state.sample(&frame)?;
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].tint, frame.models[0].tint);
        assert!(poses[0].depth_write && poses[0].uv.is_none());
        assert_eq!(poses[0].world[3][0], 107.);
        assert_eq!(poses[0].world[0][0], 2.);
        assert_eq!(poses[0].bones, [Mat4::IDENTITY.to_cols_array_2d()]);
        frame.models[0].visible = false;
        assert!(state.sample(&frame)?.is_empty());
        frame.models[0].visible = true;
        state.source.visible = false;
        assert!(state.sample(&frame)?.is_empty());
        Ok(())
    }

    #[test]
    fn independent_attachment_ignores_owner_placement_and_holds_its_motion() -> Result<()> {
        let (mut frame, mut source) = fixture()?;
        source.attachment = None;
        source.motion = Some(motion());
        let mut state = State::new(source);
        let initial = state.sample(&frame)?.remove(0);
        frame.update += 2;
        let moved = state.sample(&frame)?.remove(0);
        assert_ne!(moved.bones, initial.bones);
        assert_eq!(moved.world, initial.world);
        assert_eq!(moved.world[3][0], 1.);
        frame.update += 10;
        frame.clock = BattleClock::Held;
        frame.models[0].world = Mat4::from_translation(Vec3::Y * 700.).to_cols_array_2d();
        assert_eq!(state.sample(&frame)?[0].bones, moved.bones);
        frame.update += 1;
        frame.clock = BattleClock::Running;
        frame.paused_actors.push(state.source.owner);
        assert_eq!(state.sample(&frame)?[0].bones, moved.bones);
        frame.update += 1;
        frame.paused_actors.clear();
        assert_ne!(state.sample(&frame)?[0].bones, moved.bones);
        Ok(())
    }

    #[test]
    fn attachments_follow_owner_fade_and_visibility_without_resetting_animation() -> Result<()> {
        for independent in [false, true] {
            let (mut frame, mut source) = fixture()?;
            source.motion = Some(motion());
            if independent {
                source.attachment = None;
            }
            let mut state = State::new(source);
            state.sample(&frame)?;
            frame.update += 2;
            let before = state.sample(&frame)?.remove(0);
            frame.models[0].tint[3] = 17;
            assert_eq!(state.sample(&frame)?[0].tint[3], 17);
            frame.models[0].visible = false;
            frame.update += 2;
            assert!(state.sample(&frame)?.is_empty());
            frame.models[0].visible = true;
            frame.update += 2;
            let resumed = state.sample(&frame)?.remove(0);
            assert_ne!(resumed.bones, before.bones);
            frame.models.clear();
            assert!(
                state.sample(&frame)?.is_empty(),
                "retired owners leave no attachment"
            );
        }
        Ok(())
    }

    #[test]
    fn wing_layers_keep_separate_poses_alpha_and_color_scroll() -> Result<()> {
        let (mut frame, mut source) = fixture()?;
        source.motion = Some(motion());
        source.style = AccessoryStyle::EnemyWings;
        source.inherit_tint = false;
        source.depth_write = false;
        let mut state = State::new(source);
        assert_eq!(state.sample(&frame)?[0].tint[3], 255);
        frame.models[0].material = resonance_battle::ModelMaterial::RedChannel;
        frame.update += 24;
        let poses = state.sample(&frame)?;
        assert_eq!(poses.len(), 3);
        assert_eq!(
            poses.iter().map(|pose| pose.tint[3]).collect::<Vec<_>>(),
            [111, 191, 111]
        );
        assert!(poses.windows(2).all(|pair| pair[0].bones != pair[1].bones));
        assert!(poses.iter().all(|pose| {
            pose.material == resonance_battle::ModelMaterial::RedChannel
                && pose.tint[..3] == [64, 64, 64]
        }));
        assert!(
            poses
                .iter()
                .all(|pose| !pose.depth_write && pose.uv == Some((1, [24. * 0.02, 0.])))
        );
        Ok(())
    }

    #[test]
    fn wing_sparkles_alternate_bones_and_expire_without_new_births() {
        let identity = Mat4::IDENTITY.to_cols_array_2d();
        let wing = Frame {
            pass: 2,
            world: identity,
            bones: (0..4)
                .map(|i| Mat4::from_translation(Vec3::X * i as f32 * 100.).to_cols_array_2d())
                .collect(),
            tint: [64, 64, 64, 255],
            depth_write: false,
            material: resonance_battle::ModelMaterial::Normal,
            uv: Some((0, [0.; 2])),
        };
        let mut particles = Sparkles::default();
        particles.step(5, Some(&wing));
        assert_eq!(particles.particles.len(), 2);
        assert!((71. ..130.).contains(&particles.particles[0].position.x));
        assert!((271. ..330.).contains(&particles.particles[1].position.x));
        assert_eq!(particles.particles[0].palette(), 27);
        assert_eq!(particles.particles[0].alpha(), 124. / 255.);
        for tick in 6..=10 {
            particles.step(tick, Some(&wing));
        }
        assert_eq!(particles.particles.len(), 4);
        assert!((-30. ..30.).contains(&particles.particles[2].position.x));
        assert!((170. ..230.).contains(&particles.particles[3].position.x));
        for tick in 11..=42 {
            particles.step(tick, None);
        }
        assert!(particles.particles.is_empty());
    }
}
