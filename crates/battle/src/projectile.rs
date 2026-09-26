//! The standard projectile group, independent of its emitting task/action.
//! Sources: fn_1_14C24 (birth), fn_1_14064 (motion), fn_1_147E0 (age/retirement).
use crate::{ActionId, ActorId, Cue, HitShape};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectileId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectAppearance {
    pub resource: u32,
    pub member: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ProjectileSteering {
    pub blend: f32,
    pub start: u8,
    /// Exclusive; zero leaves steering active indefinitely.
    pub end: u8,
    pub planar: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectileMotion {
    /// Direction-times-speed integration; None uses ballistic velocity.
    pub speed: Option<f32>,
    pub steering: Option<ProjectileSteering>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileShadow {
    pub color: [u8; 4],
    pub additive: bool,
    pub radius: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectileEffects {
    pub trail: Option<(EffectAppearance, u16)>,
    pub ground: Option<EffectAppearance>,
    pub shadow: Option<ProjectileShadow>,
}

/// Prepared contact parameters for the standard projectile group.
#[derive(Debug, Clone)]
pub struct ProjectileContact {
    pub hit: crate::HitRule,
    pub cooldown: u8,
    /// Zero allows unlimited repeats. Count advances when a cooldown reaches one.
    pub repeat_limit: u8,
    pub radius: f32,
    pub height: f32,
    pub shape: HitShape,
    /// Added in world axes after movement (fn_1_14064).
    pub offset: [f32; 3],
    /// Growth happens after submission, before the shared contact resolver.
    pub radius_growth: f32,
    pub height_growth: f32,
    pub survives_contact: bool,
    /// Presence enables clash checking. The loader binds common effect 11.
    pub clash_effect: Option<EffectAppearance>,
}

/// Prepared standard motion parameters. Original table decoding stays outside
/// the simulation. More specialized controllers are not admitted by this type.
#[derive(Debug, Clone)]
pub struct ProjectileDefinition {
    /// Zero disables age expiry, as in the original table.
    pub lifetime: u16,
    pub velocity: [f32; 3],
    pub acceleration: [f32; 3],
    pub offset: [f32; 3],
    pub clamp_ground: bool,
    /// Inclusive interval; an original zero-length interval becomes None.
    pub active: Option<[u16; 2]>,
    pub birth: Option<EffectAppearance>,
    pub motion: ProjectileMotion,
    pub effects: ProjectileEffects,
    pub contact: Option<ProjectileContact>,
}

impl ProjectileDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.lifetime <= i16::MAX as u16
                && self
                    .active
                    .is_none_or(|[start, end]| start <= end && end <= i16::MAX as u16),
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
        ensure!(
            self.motion
                .speed
                .is_none_or(|speed| speed.is_finite() && speed > 0.)
                && self
                    .motion
                    .steering
                    .is_none_or(|steering| steering.blend.is_finite()
                        && (0.0..=1.0).contains(&steering.blend)
                        && (steering.end == 0 || steering.start < steering.end)),
            "invalid projectile steering"
        );
        ensure!(
            self.effects
                .trail
                .is_none_or(|(_, interval)| interval > 0 && interval <= i16::MAX as u16)
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
    pub age: i16,
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
    age: i16,
    pub initialized: bool,
    pub retiring: bool,
    pub radius: f32,
    pub height: f32,
    contact_received: bool,
    pub attack_power: u16,
    pub target_point: [f32; 3],
    ground_emitted: bool,
    cooldowns: [u8; 12],
    repeats: [u8; 12],
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
            definition,
            action,
            frame,
            age: 0,
            initialized: false,
            retiring: false,
            contact_received: false,
            attack_power: 100,
            target_point: [0.; 3],
            ground_emitted: false,
            cooldowns: [0; 12],
            repeats: [0; 12],
        }
    }

    /// Standard constructor, before age-zero motion. The caller runs the effect
    /// constructor immediately so its emissions/RNG precede the first update.
    pub fn initialize(&mut self, cues: &mut Vec<Cue>) -> Option<crate::effect::Spawn> {
        let offset = rotate(self.definition.offset, self.frame.heading);
        for (value, delta) in self.frame.position.iter_mut().zip(offset) {
            *value += delta;
        }
        cues.push(Cue::ProjectileStarted {
            projectile: self.frame.id,
            action: self.action,
        });
        self.definition
            .birth
            .map(|appearance| crate::effect::Spawn {
                scene: None,
                action: self.action,
                owner: self.frame.owner,
                target: self.frame.owner,
                appearance,
                origin: self.frame.position,
                heading: self.frame.heading,
                follow: Some(crate::effect::Follow::Projectile(self.frame.id)),
                scale: 1.,
                late: false,
                tint: Default::default(),
            })
    }

    pub fn step(&mut self) -> Result<()> {
        let fresh = !self.initialized;
        self.initialized = true;
        // 14064 reuses its direction-times-speed scratch vector as the homing
        // destination. A sub-threshold target difference leaves it untouched.
        let displacement = self
            .definition
            .motion
            .speed
            .map(|speed| self.velocity.map(|v| v * speed));
        for axis in 0..3 {
            if let Some(speed) = self.definition.motion.speed {
                self.frame.position[axis] += self.velocity[axis] * speed;
            } else {
                self.frame.position[axis] += self.velocity[axis];
                self.velocity[axis] += self.acceleration[axis];
            }
        }
        if let Some(steering) = self.definition.motion.steering
            && self.age >= i16::from(steering.start)
            && (steering.end == 0 || self.age < i16::from(steering.end))
        {
            let mut difference =
                std::array::from_fn(|i| self.target_point[i] - self.frame.position[i]);
            if steering.planar {
                difference[1] = 0.;
            }
            // 4DAF4/4D920 normalize the desired direction twice, then normalize
            // the weighted sum. The SDK estimate and separate operations matter.
            let threshold = if steering.planar { 0.5 } else { 0.1 };
            let desired = if crate::distance::length(difference) >= threshold {
                crate::distance::normalize(difference)
            } else {
                displacement.unwrap_or([0.; 3])
            };
            if crate::distance::length(desired) > 0.1 {
                let desired = crate::distance::normalize(desired);
                self.velocity = crate::distance::normalize(std::array::from_fn(|i| {
                    self.velocity[i] * (1. - steering.blend) + desired[i] * steering.blend
                }));
            }
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
            .is_none_or(|[start, end]| (start as i16..=end as i16).contains(&self.age));
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
            // fn_1_14064: decay follows submission and precedes the resolver,
            // including inactive windows. Preserve the byte counter's wrap.
            for (cooldown, repeats) in self.cooldowns.iter_mut().zip(&mut self.repeats) {
                if *cooldown != 0 {
                    if *cooldown == 1 {
                        *repeats = repeats.wrapping_add(1);
                    }
                    if contact.repeat_limit == 0 || *repeats < contact.repeat_limit {
                        *cooldown -= 1;
                    }
                }
            }
        }
        // Contacts and the final pose precede retirement. Cleanup dispatch is on
        // the following group update. Birth sets mode 1 after its initial update.
        self.retiring = !fresh
            && (contact_retirement
                || (self.definition.lifetime != 0 && self.age >= self.definition.lifetime as i16)
                || self.frame.position[0].hypot(self.frame.position[2]) > 1350.
                || self.frame.position[1] < -49.9);
        self.age = self.age.wrapping_add(1);
        Ok(())
    }

    /// 14064 emits ground feedback once, followed by the periodic trail after
    /// movement. Neither follows the projectile after construction.
    pub fn effects(&mut self) -> Vec<crate::effect::Spawn> {
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
            && self.frame.age % interval as i16 == 0
        {
            effects.push(self.effect(appearance, self.frame.position));
        }
        effects
    }

    fn effect(&self, appearance: EffectAppearance, origin: [f32; 3]) -> crate::effect::Spawn {
        crate::effect::Spawn {
            scene: None,
            action: self.action,
            owner: self.frame.owner,
            target: self.frame.owner,
            appearance,
            origin,
            heading: self.frame.heading,
            follow: None,
            scale: 1.,
            late: false,
            tint: Default::default(),
        }
    }

    pub fn set_velocity(&mut self, velocity: [f32; 3]) {
        self.velocity = rotate(velocity, self.frame.heading);
    }

    pub fn clash(&mut self) {
        self.frame.disarmed = true;
        self.contact_received = true;
    }

    pub fn can_hit(&self, actor: ActorId) -> bool {
        self.cooldowns[actor.index()] == 0
    }

    pub fn hit(&mut self, actor: ActorId) {
        self.cooldowns[actor.index()] = self.definition.contact.as_ref().unwrap().cooldown;
        self.contact_received = true;
    }
}

fn rotate([x, y, z]: [f32; 3], heading: f32) -> [f32; 3] {
    crate::geometry::rotate([x, y, z], heading)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectileContact;

    #[test]
    fn fire_ball_direction_times_speed_matches_original_adjacent_vis() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/fire-ball-motion.json")).unwrap();
        let vector = |value: &serde_json::Value| {
            std::array::from_fn(|i| f32::from_bits(value[i].as_u64().unwrap() as u32))
        };
        let rows = fixture["observations"].as_array().unwrap();
        assert!(!rows.is_empty());
        for row in rows {
            let definition = ProjectileDefinition {
                lifetime: 120,
                velocity: vector(&row["velocity"]),
                acceleration: [0.; 3],
                offset: [0.; 3],
                clamp_ground: false,
                active: None,
                birth: None,
                contact: None,
                motion: ProjectileMotion {
                    speed: Some(20.),
                    steering: Some(ProjectileSteering {
                        blend: 0.1,
                        start: 0,
                        end: 25,
                        planar: false,
                    }),
                },
                effects: Default::default(),
            };
            let mut projectile = Projectile::new(
                Arc::new(definition),
                ActionId(1),
                ProjectileFrame {
                    id: ProjectileId(1),
                    owner: ActorId(0),
                    target: ActorId(1),
                    position: vector(&row["before"]),
                    heading: 0.,
                    age: 0,
                    contact_active: false,
                    disarmed: false,
                    shadow: None,
                },
            );
            projectile.initialized = true;
            projectile.age = row["age"].as_i64().unwrap() as i16;
            projectile.step().unwrap();
            assert_eq!(
                projectile.frame.position.map(f32::to_bits),
                vector(&row["after"]).map(f32::to_bits),
                "VI {}",
                row["vi"]
            );
        }
    }

    #[test]
    fn fire_ball_copied_target_homing_matches_original_adjacent_updates() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/fire-ball-homing.json")).unwrap();
        let vector = |value: &serde_json::Value| {
            std::array::from_fn(|i| f32::from_bits(value[i].as_u64().unwrap() as u32))
        };
        let rows = fixture["observations"].as_array().unwrap();
        assert_eq!(rows.len(), 72);
        let check = |index: &serde_json::Value,
                     age: i16,
                     position: [f32; 3],
                     velocity: [f32; 3],
                     target: [f32; 3],
                     heading: f32,
                     after: [f32; 3],
                     after_velocity: [f32; 3]| {
            let mut projectile = Projectile::new(
                Arc::new(ProjectileDefinition {
                    lifetime: 120,
                    velocity,
                    acceleration: [0.; 3],
                    offset: [0.; 3],
                    clamp_ground: false,
                    active: None,
                    birth: None,
                    contact: None,
                    motion: ProjectileMotion {
                        speed: Some(20.),
                        steering: Some(ProjectileSteering {
                            blend: 0.1,
                            start: 0,
                            end: 25,
                            planar: false,
                        }),
                    },
                    effects: Default::default(),
                }),
                ActionId(1),
                ProjectileFrame {
                    id: ProjectileId(1),
                    owner: ActorId(0),
                    target: ActorId(1),
                    position,
                    heading,
                    age: 0,
                    contact_active: false,
                    disarmed: false,
                    shadow: None,
                },
            );
            if age != 0 {
                // Later snapshots hold world-space direction already.
                projectile.velocity = velocity;
                projectile.initialized = true;
            }
            projectile.target_point = target;
            projectile.age = age;
            projectile.step().unwrap();
            assert_eq!(
                projectile.frame.position.map(f32::to_bits),
                after.map(f32::to_bits),
                "position at original update {}",
                index
            );
            assert_eq!(
                projectile.velocity.map(f32::to_bits),
                after_velocity.map(f32::to_bits),
                "direction at original update {}",
                index
            );
        };
        for row in rows {
            check(
                &row["index"],
                row["age"].as_i64().unwrap() as i16,
                vector(&row["before"]),
                vector(&row["velocity"]),
                vector(&row["target_point"]),
                0.,
                vector(&row["after"]),
                vector(&row["after_velocity"]),
            );
        }
        let births = fixture["births"].as_array().unwrap();
        assert_eq!(births.len(), 3);
        for row in births {
            let before = &row["before"];
            let after = &row["after"];
            check(
                &row["index"],
                0,
                vector(&before["position_bits"]),
                vector(&before["velocity_bits"]),
                vector(&before["target_point_bits"]),
                f32::from_bits(before["heading_bits"].as_u64().unwrap() as u32),
                vector(&after["position_bits"]),
                vector(&after["velocity_bits"]),
            );
        }
    }

    #[test]
    fn latched_contact_keeps_its_last_motion_and_contact_before_retirement() {
        let trace: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-contact.json")).unwrap();
        let vector = |v: &serde_json::Value| std::array::from_fn(|i| v[i].as_f64().unwrap() as f32);
        let before = &trace["observations"][0];
        let after = &trace["observations"][1];
        let mut projectile = Projectile::new(
            Arc::new(ProjectileDefinition {
                motion: Default::default(),
                effects: Default::default(),
                lifetime: trace["lifetime"].as_u64().unwrap() as u16,
                velocity: vector(&before["velocity"]),
                acceleration: [0.; 3],
                offset: [0.; 3],
                clamp_ground: true,
                active: None,
                birth: None,
                contact: Some(ProjectileContact {
                    hit: crate::HitRule {
                        impact: None,
                        arte: false,
                        reaction: Default::default(),
                        kind: crate::DamageKind::Slash,
                        power: crate::Power::Normal,
                        element: crate::HitElement::Neutral,
                        prevents_defeat: false,
                        guard: crate::GuardRule::default(),
                    },
                    cooldown: 120,
                    repeat_limit: 0,
                    radius: trace["radius"].as_f64().unwrap() as f32,
                    height: 50.,
                    shape: HitShape::Box,
                    offset: vector(&trace["contact_offset"]),
                    radius_growth: 0.,
                    height_growth: 0.,
                    survives_contact: false,
                    clash_effect: None,
                }),
            }),
            ActionId(1),
            ProjectileFrame {
                shadow: None,
                id: ProjectileId(1),
                owner: ActorId(0),
                target: ActorId(1),
                position: vector(&before["position"]),
                heading: 0.,
                age: 0,
                contact_active: true,
                disarmed: false,
            },
        );
        // Register the original's already-latched contact; actor-hit admission
        // and damage are outside this comparison. Native stores the NEXT age.
        projectile.initialized = true;
        projectile.age = before["age"].as_i64().unwrap() as i16;
        projectile.contact_received = before["feedback"] != 0;
        if !projectile.initialized {
            projectile.initialize(&mut Vec::new());
        }
        projectile.step().unwrap();
        assert_eq!(
            projectile.frame.position.map(f32::to_bits),
            vector(&after["position"]).map(f32::to_bits)
        );
        assert_eq!(i64::from(projectile.age), after["age"].as_i64().unwrap());
        assert_eq!(projectile.retiring, after["mode"] == 2);
        assert!(projectile.frame.contact_active);
        assert_eq!(
            projectile.frame.disarmed,
            after["disarmed"].as_bool().unwrap()
        );
    }

    #[test]
    fn motion_and_terminal_age_match_the_original_opening_battle() {
        let trace: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-projectile.json"))
                .unwrap();
        let vector =
            |value: &serde_json::Value| std::array::from_fn(|i| value[i].as_f64().unwrap() as f32);
        let observations = trace["observations"].as_array().unwrap();
        let mut projectile = Projectile::new(
            Arc::new(ProjectileDefinition {
                motion: Default::default(),
                effects: Default::default(),
                lifetime: trace["lifetime"].as_u64().unwrap() as u16,
                velocity: vector(&trace["velocity"]),
                acceleration: vector(&trace["acceleration"]),
                offset: [0.; 3],
                clamp_ground: false,
                active: None,
                birth: None,
                contact: None,
            }),
            ActionId(1),
            ProjectileFrame {
                shadow: None,
                id: ProjectileId(1),
                owner: ActorId(0),
                target: ActorId(1),
                position: vector(&observations[0]["position"]),
                heading: 0.,
                age: 0,
                contact_active: false,
                disarmed: false,
            },
        );
        // Register the observed post-update state. Native stores the NEXT age;
        // presentation exposes the age actually processed. Initialization is not
        // part of this comparison, and no input/timing adjustment is applied.
        projectile.initialized = true;
        projectile.age = observations[0]["age"].as_i64().unwrap() as i16;
        for observation in &observations[1..] {
            if !projectile.initialized {
                projectile.initialize(&mut Vec::new());
            }
            projectile.step().unwrap();
            assert_eq!(
                projectile.frame.position.map(f32::to_bits),
                vector(&observation["position"]).map(f32::to_bits),
                "VI {}",
                observation["vi"]
            );
            assert_eq!(
                i64::from(projectile.age),
                observation["age"].as_i64().unwrap()
            );
            assert_eq!(projectile.retiring, observation["mode"] == 2);
        }
        assert!(projectile.retiring);
    }
}
