//! Projectile motion, contacts, and retirement.
use crate::{ActionId, ActorId, Cue, HitShape};
use anyhow::{Result, ensure};
use glam::{Quat, Vec3};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectileId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectAppearance {
    pub resource: u32,
    pub member: u16,
}

pub use resonance_content::battle_projectile::{
    ProjectileMotion, ProjectileResponse, ProjectileShadow, ProjectileSteering,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileShadowFrame {
    pub projectile: ProjectileId,
    pub position: [f32; 3],
    pub appearance: ProjectileShadow,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectileEffects {
    pub trail: Option<(EffectAppearance, u32)>,
    pub ground: Option<EffectAppearance>,
    pub shadow: Option<ProjectileShadow>,
    pub clash: Option<EffectAppearance>,
}

/// Prepared contact parameters for the standard projectile group.
#[derive(Debug, Clone)]
pub struct ProjectileContact {
    pub hit: crate::HitRule,
    pub cooldown: u8,
    /// Maximum successful hits per target; zero allows unlimited repeats.
    pub repeat_limit: u8,
    pub radius: f32,
    pub height: f32,
    pub shape: HitShape,
    pub offset: [f32; 3],
    /// Growth happens after submission, before the shared contact resolver.
    pub radius_growth: f32,
    pub height_growth: f32,
    pub survives_contact: bool,
    /// Colliding with an opposing attack disarms this projectile.
    pub clashes: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectileDefinition {
    pub lifetime: Option<u32>,
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub offset: [f32; 3],
    pub clamp_ground: bool,
    pub active: Option<[u32; 2]>,
    pub birth: Option<EffectAppearance>,
    pub motion: ProjectileMotion,
    pub effects: ProjectileEffects,
    pub contact: Option<ProjectileContact>,
}

impl ProjectileDefinition {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime.is_none_or(|duration| duration > 0)
                && self.active.is_none_or(|[start, end]| start <= end),
            "invalid projectile clock interval"
        );
        ensure!(
            self.velocity
                .iter()
                .chain(&self.acceleration)
                .chain(&self.offset)
                .all(|v| v.is_finite()),
            "invalid projectile transform"
        );
        self.motion.validate_velocity_jitter()?;
        ensure!(
            self.velocity
                .iter()
                .zip(self.motion.velocity_jitter)
                .all(|(velocity, amplitude)| (velocity.abs() + amplitude.abs()).is_finite()),
            "projectile velocity jitter overflows its velocity"
        );
        ensure!(
            self.motion.response.is_none_or(|response| {
                (response.bounce || response.ricochet)
                    && response.restitution.is_finite()
                    && (!response.ricochet || self.contact.is_some())
            }),
            "invalid projectile response"
        );
        ensure!(
            self.motion
                .speed
                .is_none_or(|speed| speed.is_finite() && speed > 0.)
                && self
                    .motion
                    .steering
                    .is_none_or(|steering| steering.blend.is_finite()
                        && (0.0..=1.0).contains(&steering.blend)
                        && steering.end.is_none_or(|end| steering.start < end)),
            "invalid projectile steering"
        );
        ensure!(
            self.effects.trail.is_none_or(|(_, interval)| interval > 0)
                && self
                    .effects
                    .shadow
                    .is_none_or(|shadow| shadow.radius.is_finite() && shadow.radius >= 0.),
            "invalid projectile effect parameters"
        );
        if let Some(contact) = &self.contact {
            contact.hit.reaction.validate()?;
            ensure!(
                contact.radius.is_finite()
                    && contact.radius >= 0.
                    && contact.height.is_finite()
                    && contact.height >= 0.
                    && contact.radius_growth.is_finite()
                    && contact.height_growth.is_finite()
                    && match contact.shape {
                        HitShape::Ring { width } => width.is_finite(),
                        _ => true,
                    }
                    && contact.offset.iter().all(|v| v.is_finite()),
                "invalid projectile contact"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileFrame {
    pub id: ProjectileId,
    pub owner: ActorId,
    pub target: ActorId,
    pub position: [f32; 3],
    pub heading: f32,
    /// The age just observed by this update, independent of the action's age.
    pub age: u32,
    pub contact_active: bool,
    /// A clash disarms contact without stopping the object's motion or lifetime.
    pub disarmed: bool,
    pub shadow: Option<ProjectileShadow>,
}

pub(crate) struct Projectile {
    pub definition: Arc<ProjectileDefinition>,
    pub action: ActionId,
    pub frame: ProjectileFrame,
    pub velocity: [f32; 3],
    acceleration: [f32; 3],
    age: u32,
    pub retiring: bool,
    pub radius: f32,
    pub height: f32,
    contact_received: bool,
    bounce: bool,
    ricocheted: bool,
    pub attack_power: u16,
    pub target_point: [f32; 3],
    ground_emitted: bool,
    cooldowns: [u8; crate::ACTOR_CAPACITY],
    hits: [u8; crate::ACTOR_CAPACITY],
}

impl Projectile {
    pub fn new(
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        frame: ProjectileFrame,
    ) -> Self {
        Self {
            velocity: rotate(definition.velocity, frame.heading),
            acceleration: rotate(definition.acceleration, frame.heading),
            radius: definition.contact.as_ref().map_or(0., |c| c.radius),
            height: definition.contact.as_ref().map_or(0., |c| c.height),
            bounce: definition
                .motion
                .response
                .is_some_and(|response| response.bounce),
            ricocheted: false,
            definition,
            action,
            frame,
            age: 0,
            retiring: false,
            contact_received: false,
            attack_power: 100,
            target_point: [0.; 3],
            ground_emitted: false,
            cooldowns: [0; crate::ACTOR_CAPACITY],
            hits: [0; crate::ACTOR_CAPACITY],
        }
    }

    /// Resolve launch motion and birth feedback once, at emission.
    pub fn initialize(
        &mut self,
        cues: &mut Vec<Cue>,
        random: &mut crate::Random,
    ) -> Option<crate::EffectRequest> {
        let mut local_velocity = self.definition.velocity;
        for (velocity, amplitude) in local_velocity
            .iter_mut()
            .zip(self.definition.motion.velocity_jitter)
        {
            if amplitude != 0. {
                let sample = f32::from(random.next_u16()) / f32::from(u16::MAX) * 2. - 1.;
                *velocity += sample * amplitude.abs();
            }
        }
        self.velocity = rotate(local_velocity, self.frame.heading);
        let offset = rotate(self.definition.offset, self.frame.heading);
        for (value, delta) in self.frame.position.iter_mut().zip(offset) {
            *value += delta;
        }
        self.frame.shadow = self.definition.effects.shadow;
        cues.push(Cue::ProjectileStarted {
            projectile: self.frame.id,
            action: self.action,
        });
        self.definition
            .birth
            .map(|appearance| crate::EffectRequest {
                owner: self.frame.owner,
                target: self.frame.owner,
                appearance,
                origin: self.frame.position,
                heading: self.frame.heading,
                follow: Some(crate::EffectFollow::Projectile(self.frame.id)),
                scale: 1.,

                tint: Default::default(),
            })
    }

    pub(crate) fn shadow_frame(&self) -> Option<ProjectileShadowFrame> {
        let appearance = self.frame.shadow?;
        (self.frame.position[1] >= 0.).then_some(ProjectileShadowFrame {
            projectile: self.frame.id,
            position: [self.frame.position[0], 1.1, self.frame.position[2]],
            appearance,
        })
    }

    pub fn step(&mut self) -> Result<()> {
        let fresh = self.age == 0;
        for axis in 0..3 {
            if let Some(speed) = self.definition.motion.speed {
                self.frame.position[axis] += self.velocity[axis] * speed;
            } else {
                self.frame.position[axis] += self.velocity[axis];
                self.velocity[axis] += self.acceleration[axis];
            }
        }
        if let Some(steering) = self.definition.motion.steering
            && self.age >= steering.start
            && steering.end.is_none_or(|end| self.age < end)
        {
            self.velocity = steer_velocity(
                self.velocity,
                std::array::from_fn(|i| self.target_point[i] - self.frame.position[i]),
                steering.blend,
                steering.planar,
            );
        }

        if self.bounce && self.frame.position[1] < 0.1 {
            self.frame.position[1] = 0.;
            self.velocity[1] *= -self.definition.motion.response.unwrap().restitution;
        }
        if self.definition.clamp_ground && self.frame.position[1] < 0.1 {
            self.frame.position[1] = 0.;
        }
        ensure!(
            self.frame
                .position
                .iter()
                .chain(&self.velocity)
                .all(|v| v.is_finite()),
            "projectile motion overflow"
        );
        self.frame.age = self.age;
        let active = self
            .definition
            .active
            .is_none_or(|[start, end]| (start..=end).contains(&self.age));
        if active
            && self.contact_received
            && !self.ricocheted
            && self
                .definition
                .motion
                .response
                .is_some_and(|response| response.ricochet)
        {
            let mut planar = [self.velocity[0], 0., self.velocity[2]];
            if crate::distance::length(planar) >= 0.1 {
                planar = crate::distance::normalize(planar);
            }
            self.velocity = planar.map(|value| value * -2.);
            self.velocity[1] = 9.;
            ensure!(
                self.velocity.iter().all(|value| value.is_finite()),
                "projectile response overflow"
            );
            self.acceleration = [0., -0.3, 0.];
            self.bounce = true;
            self.ricocheted = true;
            self.frame.disarmed = true;
        }
        self.frame.contact_active = active && self.age != 0 && !self.frame.disarmed;
        let contact_retirement = active
            && self.contact_received
            && self
                .definition
                .contact
                .as_ref()
                .is_some_and(|c| !c.survives_contact);
        if let Some(contact) = &self.definition.contact {
            self.radius += contact.radius_growth;
            self.height += contact.height_growth;
            ensure!(
                self.radius.is_finite() && self.height.is_finite(),
                "projectile contact growth overflow"
            );
            for cooldown in &mut self.cooldowns {
                *cooldown = cooldown.saturating_sub(1);
            }
        }

        // Publish contacts and the final pose before retirement on the next update.
        self.retiring = !fresh
            && (contact_retirement
                || self
                    .definition
                    .lifetime
                    .is_some_and(|duration| self.age >= duration)
                || self.frame.position[0].hypot(self.frame.position[2]) > 1350.
                || self.frame.position[1] < -49.9);
        self.age = self.age.saturating_add(1);
        Ok(())
    }

    pub fn effects(&mut self) -> Vec<crate::EffectRequest> {
        let mut effects = Vec::with_capacity(2);
        if self.frame.position[1] < 0.1 && !self.ground_emitted {
            self.ground_emitted = true;
            if let Some(appearance) = self.definition.effects.ground {
                let mut origin = self.frame.position;
                origin[1] = 0.;
                effects.push(self.effect(appearance, origin));
            }
        }
        if let Some((appearance, interval)) = self.definition.effects.trail
            && self.frame.age.is_multiple_of(interval)
        {
            effects.push(self.effect(appearance, self.frame.position));
        }
        effects
    }

    fn effect(&self, appearance: EffectAppearance, origin: [f32; 3]) -> crate::EffectRequest {
        crate::EffectRequest {
            owner: self.frame.owner,
            target: self.frame.owner,
            appearance,
            origin,
            heading: self.frame.heading,
            follow: None,
            scale: 1.,

            tint: Default::default(),
        }
    }

    pub fn clash(&mut self) {
        self.frame.disarmed = true;
        self.frame.contact_active = false;
        self.contact_received = true;
    }

    pub fn can_hit(&self, actor: ActorId) -> bool {
        !self.frame.disarmed
            && self.cooldowns[actor.index()] == 0
            && self.definition.contact.as_ref().is_some_and(|contact| {
                contact.repeat_limit == 0 || self.hits[actor.index()] < contact.repeat_limit
            })
    }

    pub fn hit(&mut self, actor: ActorId) {
        self.cooldowns[actor.index()] = self.definition.contact.as_ref().unwrap().cooldown;
        self.hits[actor.index()] = self.hits[actor.index()].saturating_add(1);
        self.contact_received = true;
    }
}

/// Turn through a fraction of the remaining angle without changing speed.
fn steer_velocity(velocity: [f32; 3], offset: [f32; 3], fraction: f32, planar: bool) -> [f32; 3] {
    let mut current = Vec3::from_array(velocity);
    let mut desired = Vec3::from_array(offset);
    if planar {
        current.y = 0.;
        desired.y = 0.;
    }
    let speed = crate::distance::length(current.to_array());
    let distance = crate::distance::length(desired.to_array());
    if fraction == 0. || speed <= f32::EPSILON || distance <= f32::EPSILON {
        return velocity;
    }
    let current = current / speed;
    let desired = desired / distance;
    let cross = current.cross(desired);
    let sine = cross.length();
    let angle = sine.atan2(current.dot(desired).clamp(-1., 1.));
    let axis = if sine > f32::EPSILON {
        cross / sine
    } else if planar {
        Vec3::Y
    } else {
        // Opposite directions have no unique turn plane. Choose a stable one.
        let absolute = current.abs();
        let basis = if absolute.x <= absolute.y && absolute.x <= absolute.z {
            Vec3::X
        } else if absolute.y <= absolute.z {
            Vec3::Y
        } else {
            Vec3::Z
        };
        current.cross(basis).normalize()
    };
    let mut turned = Quat::from_axis_angle(axis, angle * fraction) * current * speed;
    if planar {
        turned.y = velocity[1];
    }
    turned.to_array()
}

fn rotate([x, y, z]: [f32; 3], heading: f32) -> [f32; 3] {
    crate::geometry::rotate([x, y, z], heading)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(velocity: [f32; 3], motion: ProjectileMotion) -> Projectile {
        Projectile::new(
            Arc::new(ProjectileDefinition {
                lifetime: None,
                velocity,
                acceleration: [0.; 3],
                offset: [0.; 3],
                clamp_ground: false,
                active: None,
                birth: None,
                contact: None,
                motion,
                effects: Default::default(),
            }),
            ActionId(1),
            ProjectileFrame {
                id: ProjectileId(1),
                owner: ActorId(0),
                target: ActorId(1),
                position: [0., 5., 0.],
                heading: 0.,
                age: 0,
                contact_active: false,
                disarmed: false,
                shadow: None,
            },
        )
    }

    #[test]
    fn homing_flight_steers_only_during_its_window_and_expires_at_its_lifetime() {
        let mut projectile = instance(
            [1., 0., 0.],
            ProjectileMotion {
                speed: Some(2.),
                steering: Some(ProjectileSteering {
                    blend: 0.25,
                    start: 2,
                    end: Some(6),
                    planar: false,
                }),
                ..Default::default()
            },
        );
        Arc::get_mut(&mut projectile.definition).unwrap().lifetime = Some(12);
        projectile.target_point = [50., 25., 50.];
        projectile.initialize(&mut Vec::new(), &mut crate::Random::new(1));
        for age in 0..=12 {
            let before = projectile.frame.position;
            let direction = projectile.velocity;
            projectile.step().unwrap();
            let travelled: [f32; 3] =
                std::array::from_fn(|i| projectile.frame.position[i] - before[i]);
            assert!((crate::distance::length(travelled) - 2.).abs() < 0.00001);
            assert!(projectile.frame.position.iter().all(|v| v.is_finite()));
            if (2..6).contains(&age) {
                assert!(projectile.velocity[1] > direction[1]);
                assert!(projectile.velocity[2] > direction[2]);
            } else {
                assert_eq!(projectile.velocity, direction);
            }
            assert_eq!(projectile.retiring, age == 12);
        }
        assert!(projectile.frame.position[1] > 5. && projectile.frame.position[2] > 0.);
    }

    #[test]
    fn homing_reverses_without_changing_speed_or_planar_vertical_motion() {
        for (planar, velocity) in [(false, [4., 0., 0.]), (true, [4., 3., 0.])] {
            for speed in [None, Some(2.)] {
                let mut projectile = instance(
                    velocity,
                    ProjectileMotion {
                        speed,
                        steering: Some(ProjectileSteering {
                            blend: 0.25,
                            start: 0,
                            end: None,
                            planar,
                        }),
                        ..Default::default()
                    },
                );
                projectile.target_point = [-100., 5., 0.];
                for _ in 0..8 {
                    let before = projectile.frame.position;
                    let heading = Vec3::from_array(projectile.velocity).normalize();
                    projectile.step().unwrap();
                    let travelled =
                        Vec3::from_array(projectile.frame.position) - Vec3::from_array(before);
                    let expected_speed = crate::distance::length(velocity);
                    assert!(
                        (travelled.length() - expected_speed * speed.unwrap_or(1.)).abs() < 0.0001
                    );
                    assert!(
                        (crate::distance::length(projectile.velocity) - expected_speed).abs()
                            < 0.0001
                    );
                    let next = Vec3::from_array(projectile.velocity).normalize();
                    assert!(
                        heading.dot(next) >= std::f32::consts::FRAC_1_SQRT_2 - 0.0001,
                        "homing turned more than a quarter of a half-circle in one update"
                    );
                    if planar {
                        assert_eq!(projectile.velocity[1], velocity[1]);
                    }
                }
                assert!(
                    projectile.velocity[0] < 0.,
                    "projectile never turned back toward its target"
                );
            }
        }
    }

    #[test]
    fn homing_preserves_stationary_and_coincident_velocities() {
        for planar in [false, true] {
            assert_eq!(
                steer_velocity([0.; 3], [10., 0., 0.], 0.25, planar),
                [0.; 3]
            );
            let velocity = [3., 4., 5.];
            assert_eq!(steer_velocity(velocity, [0.; 3], 0.25, planar), velocity);
            assert_eq!(
                steer_velocity(velocity, [-3., -4., -5.], 0., planar),
                velocity
            );
        }
    }

    #[test]
    fn hit_limits_count_contacts_independently_of_cooldown_and_never_reset() {
        for cooldown in [0, 2] {
            for limit in [0, 1, 3] {
                let mut projectile = instance([0.; 3], Default::default());
                Arc::get_mut(&mut projectile.definition).unwrap().contact =
                    Some(ProjectileContact {
                        hit: crate::HitRule {
                            kind: crate::DamageKind::Slash,
                            arte: false,
                            overlimit_pause: false,
                            power: crate::Power::Normal,
                            element: crate::HitElement::Inherited,
                            prevents_defeat: false,
                            guard: Default::default(),
                            reaction: Default::default(),
                            condition: None,
                        },
                        cooldown,
                        repeat_limit: limit,
                        radius: 1.,
                        height: 1.,
                        shape: HitShape::Sphere,
                        offset: [0.; 3],
                        radius_growth: 0.,
                        height_growth: 0.,
                        survives_contact: true,
                        clashes: false,
                    });
                projectile.definition.validate().unwrap();
                let target = ActorId(1);
                let admitted_hits = if limit == 0 {
                    u16::from(u8::MAX) + 1
                } else {
                    u16::from(limit)
                };
                for _ in 0..admitted_hits {
                    assert!(projectile.can_hit(target));
                    projectile.hit(target);
                    if cooldown != 0 {
                        assert!(!projectile.can_hit(target));
                        for _ in 1..cooldown {
                            projectile.step().unwrap();
                            assert!(!projectile.can_hit(target));
                        }
                        projectile.step().unwrap();
                    }
                    assert!(
                        projectile.can_hit(ActorId(2)),
                        "hit budget leaked between targets"
                    );
                }
                for _ in 0..=u16::from(u8::MAX) + 1 {
                    projectile.step().unwrap();
                    assert!(!projectile.retiring);
                    assert_eq!(projectile.can_hit(target), limit == 0);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "projectile_shadow_tests.rs"]
mod shadow_tests;

#[cfg(test)]
#[path = "projectile/jitter_tests.rs"]
mod jitter_tests;

#[cfg(test)]
mod response_tests;

#[cfg(test)]
mod clock_tests;
