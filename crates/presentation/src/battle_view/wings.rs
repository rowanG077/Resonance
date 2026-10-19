//! Colette's battle wings use delayed translucent poses and a scrolling color texture.
use anyhow::{Context, Result};
use bevy::prelude::{Mat4, Quat, Vec3};
use resonance_battle::BattleFrame;
use resonance_content::animation::{Matrix, multiply, pose};
use resonance_content::random::Random;
use resonance_events::{Animation, Wings};
use resonance_game::battle::encounter::WingAccessory;

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
    pub source: WingAccessory,
    animation: Animation,
    update: Option<u64>,
    age: u32,
    sparkles: Sparkles,
    sparkles_visible: bool,
}

pub(super) struct Frame {
    pub pass: u8,
    pub world: Matrix,
    pub bones: Vec<Matrix>,
    pub alpha: u8,
    pub uv: (usize, [f32; 2]),
}

impl State {
    pub fn new(source: WingAccessory) -> Self {
        let mut animation = Animation::new(
            resonance_content::field::COLETTE_WINGS_RESOURCE,
            resonance_events::animation::slot::IDLE,
            (source.motion.duration_frames * resonance_content::ANIMATION_HZ
                / resonance_content::animation::FRAME_HZ) as u32,
            0,
        );
        animation.blend_ticks = 0;
        animation.repeat = true;
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
        self.sparkles_visible =
            body.is_some_and(|body| body.visible) && super::actor_visible(frame, owner);
        if !self.sparkles_visible {
            return Ok(vec![]);
        }
        let body = body.unwrap();
        let parent = multiply(
            body.world,
            *body
                .bones
                .get(self.source.attachment)
                .context("wing attachment pose is missing")?,
        );
        // Match field attachments: retain the named bone's translation and rotation.
        let parent = Mat4::from_cols_array_2d(&parent);
        let (_, rotation, translation) = parent.to_scale_rotation_translation();
        let [x, y] = Wings::FLIGHT_TILT.map(|angle| (angle as f32).to_radians());
        let world = Mat4::from_rotation_translation(rotation, translation)
            * Mat4::from_quat(Quat::from_rotation_y(y) * Quat::from_rotation_x(x));
        let duration = self.source.motion.duration_frames * resonance_content::ANIMATION_HZ
            / resonance_content::animation::FRAME_HZ;
        let time = self.animation.sample(self.age, 0, duration)
            * resonance_content::animation::FRAME_HZ
            / resonance_content::ANIMATION_HZ;
        let duration = self.source.motion.duration_frames;
        let layers = WING_LAYERS
            .into_iter()
            .enumerate()
            .map(|(pass, (delay, alpha))| {
                let mut sample = time - delay;
                if sample < 0. || sample > duration {
                    let phase = sample.rem_euclid(duration);
                    sample = if phase == 0. && sample > 0. {
                        duration
                    } else {
                        phase
                    };
                }
                let local = pose::sample(&self.source.skeleton, &self.source.motion, sample)?;
                let mut bones = Vec::with_capacity(local.len());
                for (bone, value) in self.source.skeleton.bones.iter().zip(local) {
                    let matrix = value.matrix();
                    bones.push(bone.parent.map_or(matrix, |parent| {
                        multiply(bones[usize::from(parent)], matrix)
                    }));
                }
                Ok(Frame {
                    pass: pass as u8,
                    world: world.to_cols_array_2d(),
                    bones,
                    alpha: if pass == 0 && self.age == 0 {
                        255
                    } else {
                        alpha
                    },
                    uv: (
                        0,
                        [
                            self.age as f32 * COLOR_SCROLL_PER_TICK
                                - pass as f32 * LAYER_COLOR_OFFSET,
                            0.,
                        ],
                    ),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for tick in previous_age + 1..=self.age {
            self.sparkles.step(tick, layers.last());
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

    #[test]
    fn wing_sparkles_alternate_bones_and_expire_without_new_births() {
        let identity = Mat4::IDENTITY.to_cols_array_2d();
        let wing = Frame {
            pass: 2,
            world: identity,
            bones: (0..4)
                .map(|i| Mat4::from_translation(Vec3::X * i as f32 * 100.).to_cols_array_2d())
                .collect(),
            alpha: 255,
            uv: (0, [0.; 2]),
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
