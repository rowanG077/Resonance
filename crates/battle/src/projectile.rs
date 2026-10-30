//! Projectile motion, contacts, and retirement.
use crate::{ActionId, ActorId, Cue, HitShape};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectileId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EffectAppearance {
    pub resource: crate::ResourceId,
    pub member: u16,
}

pub use resonance_content::battle_projectile::{
    ProjectileBoneDirection, ProjectileDeparture, ProjectileMotion, ProjectileOrigin,
    ProjectileResponse, ProjectileShadow, ProjectileSteering, ProjectileVelocityReset,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectileShadowFrame {
    pub projectile: ProjectileId,
    pub position: [f32; 3],
    pub appearance: ProjectileShadow,
}

#[derive(Debug, Clone, Copy)]
pub struct ProjectileContactFeedback {
    pub appearance: EffectAppearance,
    pub on_guard: bool,
    pub sound: Option<crate::Sound>,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectileEffects {
    pub trail: Option<(EffectAppearance, u32)>,
    pub ground: Option<EffectAppearance>,
    pub shadow: Option<ProjectileShadow>,
    pub clash: Option<EffectAppearance>,
    pub contact: Option<ProjectileContactFeedback>,
    pub seal: Option<crate::SealDefinition>,
    /// Emit once at the first struck actor when this projectile ricochets.
    pub rebound_projectile: Option<Arc<ProjectileDefinition>>,
    pub spin_on_rebound: bool,
    pub departure_animation: Option<resonance_content::battle_effect::ModelAnimation>,
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

/// Defaults to a stationary, persistent projectile with no contact or effects.
#[derive(Debug, Clone, Default)]
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
            self.motion.departure.is_none_or(|departure| {
                [
                    departure.return_speed,
                    departure.jump_speed,
                    departure.ground_speed,
                ]
                .into_iter()
                .all(|value| value.is_finite() && value > 0.)
                    && departure
                        .ground_reference
                        .iter()
                        .all(|value| value.is_finite())
                    && self.contact.is_some()
                    && self.motion.response.is_none()
                    && self.motion.speed.is_none()
                    && self.motion.steering.is_none()
                    && self.clamp_ground
            }),
            "invalid projectile departure"
        );
        ensure!(
            self.motion.velocity_reset.is_none_or(|reset| reset.age > 0
                && reset.acceleration_scale.is_finite()
                && self.motion.speed.is_none()),
            "invalid projectile velocity reset"
        );
        ensure!(
            (self.motion.contact_path.is_empty() || self.contact.is_some())
                && self.motion.contact_path.iter().all(|points| {
                    !points.is_empty() && points.iter().flatten().all(|v| v.is_finite())
                }),
            "invalid projectile contact path"
        );
        ensure!(
            self.velocity
                .iter()
                .zip(self.motion.velocity_jitter)
                .all(|(velocity, amplitude)| (velocity.abs() + amplitude.abs()).is_finite()),
            "projectile velocity jitter overflows its velocity"
        );
        ensure!(
            self.motion.response.is_none_or(|response| {
                (response.bounce
                    || response.ricochet
                    || response.ricochet_on_deflect
                    || response.ground_ricochet)
                    && response.restitution.is_finite()
                    && (!(response.ricochet || response.ricochet_on_deflect)
                        || self.contact.is_some())
            }),
            "invalid projectile response"
        );
        ensure!(
            self.motion.bone_direction.is_none_or(
                |direction| direction.from != direction.to && !self.motion.aim_at_target
            ),
            "invalid projectile bone direction"
        );
        ensure!(
            self.motion
                .speed
                .is_none_or(|speed| speed.is_finite() && speed > 0.)
                && self
                    .motion
                    .steering
                    .is_none_or(|steering| steering.blend.is_finite()
                        && steering.blend >= 0.
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
        if let Some(seal) = self.effects.seal {
            seal.validate()?;
            ensure!(self.contact.is_some(), "projectile seal has no contact");
        }
        if let Some(animation) = self.effects.departure_animation {
            animation.validate()?;
            ensure!(
                self.motion.departure.is_some(),
                "projectile departure animation has no departure"
            );
        }
        if let Some(projectile) = &self.effects.rebound_projectile {
            ensure!(
                self.motion
                    .response
                    .is_some_and(|response| response.ricochet),
                "follow-up projectile requires a contact ricochet"
            );
            projectile.validate()?;
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
    /// Current world-space flight vector for attached effect orientation.
    pub velocity: [f32; 3],
    pub heading: f32,
    /// The age just observed by this update, independent of the action's age.
    pub age: u32,
    pub contact_active: bool,
    /// A clash disarms contact without stopping the object's motion or lifetime.
    pub disarmed: bool,
    pub shadow: Option<ProjectileShadow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rebound {
    Contact,
    Ground,
    Bounce,
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
    deflected: bool,
    seal_target: Option<ActorId>,
    rebound_target: Option<ActorId>,
    bounce: bool,
    ricocheted: bool,
    pub departed: bool,
    rebound: Option<Rebound>,
    pub attack_power: u16,
    pub target_point: [f32; 3],
    pub pending_launch: bool,
    ground_emitted: bool,
    cooldowns: Vec<u8>,
    hits: Vec<u8>,
}

const AQUA_EDGE_TURN_TICKS: u32 = 30;
const AQUA_EDGE_BRAKING: f32 = 0.5;
const AQUA_EDGE_ACCELERATION: f32 = 0.75;

impl Projectile {
    pub fn new(
        definition: Arc<ProjectileDefinition>,
        action: ActionId,
        frame: ProjectileFrame,
        actor_count: usize,
    ) -> Self {
        Self {
            velocity: rotate(definition.velocity, frame.heading),
            acceleration: if definition.motion.world_direction {
                definition.acceleration
            } else {
                rotate(definition.acceleration, frame.heading)
            },
            radius: definition.contact.as_ref().map_or(0., |c| c.radius),
            height: definition.contact.as_ref().map_or(0., |c| c.height),
            bounce: definition
                .motion
                .response
                .is_some_and(|response| response.bounce),
            ricocheted: false,
            departed: false,
            rebound: None,
            definition,
            action,
            frame,
            age: 0,
            retiring: false,
            contact_received: false,
            deflected: false,
            seal_target: None,
            rebound_target: None,
            attack_power: 100,
            target_point: [0.; 3],
            pending_launch: false,
            ground_emitted: false,
            cooldowns: vec![0; actor_count],
            hits: vec![0; actor_count],
        }
    }

    /// Resolve launch motion and birth feedback once, at emission.
    pub fn initialize(
        &mut self,
        cues: &mut Vec<Cue>,
        draw: impl FnMut(usize, f32) -> Result<u16>,
        direction: Option<[f32; 3]>,
    ) -> Result<Option<crate::EffectRequest>> {
        if direction.is_none() {
            self.prepare_velocity(draw)?;
        }
        Ok(self.finish_launch(cues, direction))
    }

    /// Consume authored jitter in emission order, before any deferred bone sampling.
    pub(crate) fn prepare_velocity(
        &mut self,
        mut draw: impl FnMut(usize, f32) -> Result<u16>,
    ) -> Result<()> {
        if self.definition.motion.bone_direction.is_none() && !self.definition.motion.aim_at_target
        {
            let mut local_velocity = self.definition.velocity;
            for (axis, (velocity, amplitude)) in local_velocity
                .iter_mut()
                .zip(self.definition.motion.velocity_jitter)
                .enumerate()
            {
                if amplitude.abs() >= 1. {
                    let steps = (amplitude * 10.) as i32;
                    let sample = i32::from(draw(axis, amplitude)? as i16) % steps;
                    *velocity = (sample as f32).mul_add(0.1, *velocity);
                }
            }
            self.velocity = if self.definition.motion.world_direction {
                local_velocity
            } else {
                rotate(local_velocity, self.frame.heading)
            };
            if self.definition.motion.aqua_edge {
                self.velocity = rotate(self.velocity, self.frame.heading);
            }
        }
        Ok(())
    }

    pub(crate) fn finish_launch(
        &mut self,
        cues: &mut Vec<Cue>,
        direction: Option<[f32; 3]>,
    ) -> Option<crate::EffectRequest> {
        self.pending_launch = false;
        if let Some(direction) = direction {
            self.velocity = direction;
        } else if self.definition.motion.aim_at_target {
            self.velocity = crate::distance::normalize(std::array::from_fn(|axis| {
                self.target_point[axis] - self.frame.position[axis]
            }));
        }
        let offset = rotate(self.definition.offset, self.frame.heading);
        for (value, delta) in self.frame.position.iter_mut().zip(offset) {
            *value += delta;
        }
        self.frame.shadow = self.definition.effects.shadow;
        self.frame.velocity = self.velocity;
        cues.push(Cue::ProjectileStarted {
            projectile: self.frame.id,
            action: self.action,
            velocity: self.velocity,
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

    pub(crate) fn contact_origins(&self, offset: [f32; 3]) -> impl Iterator<Item = [f32; 3]> {
        const ORIGIN: [[f32; 3]; 1] = [[0.; 3]];
        let path = &self.definition.motion.contact_path;
        let points = path.get(self.frame.age as usize).or_else(|| path.last());
        points
            .map_or(ORIGIN.as_slice(), Vec::as_slice)
            .iter()
            .map(move |&point| {
                let point = rotate(point, self.frame.heading);
                std::array::from_fn(|i| self.frame.position[i] + point[i] + offset[i])
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

    pub fn take_rebound(&mut self) -> Option<Rebound> {
        self.rebound.take()
    }

    pub fn pending_rebound_target(&self) -> Option<ActorId> {
        self.rebound_target
    }

    pub fn take_rebound_projectile(&mut self) -> Option<(ActorId, Arc<ProjectileDefinition>)> {
        if !self.ricocheted {
            return None;
        }
        self.rebound_target
            .take()
            .zip(self.definition.effects.rebound_projectile.clone())
    }

    pub fn step(&mut self) -> Result<()> {
        let fresh = self.age == 0;
        for axis in 0..3 {
            if let Some(mut speed) = self.definition.motion.speed {
                if self.definition.motion.aqua_edge {
                    speed -= AQUA_EDGE_BRAKING * self.age.min(AQUA_EDGE_TURN_TICKS - 1) as f32;
                    speed += AQUA_EDGE_ACCELERATION
                        * self.age.saturating_sub(AQUA_EDGE_TURN_TICKS) as f32;
                }
                self.frame.position[axis] += self.velocity[axis] * speed;
            } else {
                self.frame.position[axis] += self.velocity[axis];
                self.velocity[axis] += self.acceleration[axis];
            }
        }
        if self.definition.motion.aqua_edge && self.age + 1 == AQUA_EDGE_TURN_TICKS {
            let direction = [
                self.target_point[0] - self.frame.position[0],
                0.,
                self.target_point[2] - self.frame.position[2],
            ];
            if crate::distance::length(direction) >= 0.5 {
                self.velocity = crate::distance::normalize(direction);
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

        if self.definition.motion.face_velocity && (self.velocity[0] > 0. || self.velocity[2] > 0.)
        {
            self.frame.heading = self.velocity[0].atan2(self.velocity[2]).to_degrees();
        }
        if let Some(departure) = self.definition.motion.departure
            && !self.departed
            && self.frame.position[1] < 0.1
        {
            let [x, _, z] = self.frame.position;
            let direction = crate::distance::normalize([
                z - departure.ground_reference[1],
                0.,
                departure.ground_reference[0] - x,
            ]);
            self.velocity = direction.map(|value| value * departure.ground_speed);
            self.departed = true;
        }
        if self.bounce && self.frame.position[1] < 0.1 {
            self.frame.position[1] = 0.;
            self.velocity[1] *= -self.definition.motion.response.unwrap().restitution;
            if self.ricocheted && self.definition.effects.spin_on_rebound {
                self.rebound = Some(Rebound::Bounce);
            }
        }
        if (self.definition.clamp_ground || self.definition.motion.stop_on_ground)
            && self.frame.position[1] < 0.1
        {
            self.frame.position[1] = 0.;
            if self.definition.motion.stop_on_ground {
                self.velocity = [0.; 3];
            }
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
        if let Some(reset) = self.definition.motion.velocity_reset
            && self.age == reset.age
        {
            self.velocity = self
                .acceleration
                .map(|value| value * reset.acceleration_scale);
        }
        let active = self
            .definition
            .active
            .is_none_or(|[start, end]| (start..=end).contains(&self.age));
        if let Some(departure) = self.definition.motion.departure
            && active
            && self.contact_received
            && !self.ricocheted
            && !self.departed
        {
            let direction = crate::distance::normalize([
                self.target_point[0] - self.frame.position[0],
                0.,
                self.target_point[2] - self.frame.position[2],
            ]);
            self.velocity = direction.map(|value| value * departure.return_speed);
            self.velocity[1] = departure.jump_speed;
            self.ricocheted = true;
            self.frame.disarmed = true;
        }
        if active
            && let Some(response) = self.definition.motion.response
            && ((((response.ricochet && self.contact_received)
                || (response.ricochet_on_deflect && self.deflected))
                && !self.ricocheted)
                || (response.ground_ricochet && self.frame.position[1] < 0.1))
        {
            let contact = ((response.ricochet && self.contact_received)
                || (response.ricochet_on_deflect && self.deflected))
                && !self.ricocheted;
            let mut planar = [self.velocity[0], 0., self.velocity[2]];
            if crate::distance::length(planar) >= 0.1 {
                planar = crate::distance::normalize(planar);
            }
            self.velocity = planar.map(|value| value * if contact { -2. } else { 2. });
            self.velocity[1] = 9.;
            ensure!(
                self.velocity.iter().all(|value| value.is_finite()),
                "projectile response overflow"
            );
            self.acceleration = [0., -0.3, 0.];
            self.ricocheted = true;
            if contact {
                self.bounce = true;
            }
            if self.definition.effects.spin_on_rebound {
                self.rebound = Some(if contact {
                    Rebound::Contact
                } else {
                    Rebound::Ground
                });
            }
            self.frame.disarmed = true;
        }
        self.frame.velocity = self.velocity;
        self.frame.contact_active = active && self.age != 0 && !self.frame.disarmed;
        let contact_retirement =
            active
                && self.contact_received
                && (self.ricocheted
                    || self.definition.motion.response.is_none_or(|response| {
                        !response.ricochet && !response.ricochet_on_deflect
                    }))
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
                || (!self.definition.motion.unlimited_range
                    && self.frame.position[0].hypot(self.frame.position[2]) > 1350.)
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
        self.deflected = true;
    }

    pub fn can_hit(&self, actor: ActorId) -> bool {
        !self.frame.disarmed
            && self.cooldowns[actor.index()] == 0
            && self.definition.contact.as_ref().is_some_and(|contact| {
                contact.repeat_limit == 0 || self.hits[actor.index()] < contact.repeat_limit
            })
    }

    pub fn hit(&mut self, actor: ActorId, guarded: bool) {
        if !self.contact_received && self.definition.effects.rebound_projectile.is_some() {
            self.rebound_target = Some(actor);
        }
        self.cooldowns[actor.index()] = self.definition.contact.as_ref().unwrap().cooldown;
        self.hits[actor.index()] = self.hits[actor.index()].saturating_add(1);
        self.contact_received = true;
        self.deflected |= guarded;
    }

    pub fn seal_contact(&mut self, actor: ActorId, effective: bool) {
        if !self.contact_received && effective && self.definition.effects.seal.is_some() {
            self.seal_target = Some(actor);
        }
    }

    pub fn take_seal(&mut self) -> Option<(ActorId, crate::SealDefinition)> {
        self.seal_target.take().zip(self.definition.effects.seal)
    }
}

/// Blend toward the retained target, then restore a unit flight direction.
fn steer_velocity(direction: [f32; 3], mut offset: [f32; 3], blend: f32, planar: bool) -> [f32; 3] {
    const MINIMUM_DIRECTION_LENGTH: f32 = 0.1;
    if planar {
        offset[1] = 0.;
    }
    let distance = crate::distance::length(offset);
    if distance < MINIMUM_DIRECTION_LENGTH {
        return direction;
    }
    let desired = offset.map(|value| value / distance);
    let next = std::array::from_fn(|axis| direction[axis] * (1. - blend) + desired[axis] * blend);
    let length = crate::distance::length(next);
    if length < MINIMUM_DIRECTION_LENGTH {
        direction
    } else {
        next.map(|value| value / length)
    }
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
                velocity,
                motion,
                ..Default::default()
            }),
            ActionId(1),
            ProjectileFrame {
                id: ProjectileId(1),
                owner: ActorId(0),
                target: ActorId(1),
                position: [0., 5., 0.],
                velocity: [0.; 3],
                heading: 0.,
                age: 0,
                contact_active: false,
                disarmed: false,
                shadow: None,
            },
            3,
        )
    }

    #[test]
    fn launch_policy_samples_target_or_bones_once_and_retains_actor_offset() -> Result<()> {
        use resonance_content::animation::{Bone, Transform};
        let bone_direction = Some(ProjectileBoneDirection { from: 0, to: 1 });
        for (origin, bone_direction, expected) in [
            (ProjectileOrigin::Actor, None, [90., -10., 50.]),
            (ProjectileOrigin::TargetFloor, None, [290., -10., -40.]),
            (ProjectileOrigin::Bone(1), None, [90., 30., 70.]),
            (ProjectileOrigin::Bone(1), bone_direction, [90., 30., 70.]),
            (ProjectileOrigin::Actor, bone_direction, [90., -10., 50.]),
        ] {
            let mut owner = crate::tests::actor(crate::Side::Party);
            owner.position = [100., 0., 50.];
            owner.heading = -90.;
            owner.body.scale = 2.;
            let mut target = crate::tests::actor(crate::Side::Enemy);
            target.position = [300., 80., -40.];
            let mut battle = crate::tests::prepared(vec![owner, target], 1).finish()?;
            let mut body = crate::tests::model_definition(7, [(0, 30.)]);
            body.skeleton.bones.push(Bone {
                name: "tip".into(),
                parent: Some(0),
                parent_transform: None,
                shadow_bounds: true,
                bind: Transform {
                    translation: [10., 0., 20.],
                    ..Default::default()
                },
            });
            battle.set_models(crate::Models::new(
                battle.actors(),
                vec![Some(Arc::new(body)), None],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )?);
            let mut definition = instance(
                [0., -5., 5.],
                ProjectileMotion {
                    origin,
                    bone_direction,
                    speed: Some(30.),
                    ..Default::default()
                },
            )
            .definition;
            Arc::make_mut(&mut definition).offset = [0., -10., 10.];
            definition.validate()?;
            let id = battle.emit(
                definition,
                ActionId(1),
                ActorId(0),
                ActorId(1),
                [100., 0., 50.],
            )?;
            let projectile = &battle.projectiles[&id];
            for (actual, expected) in projectile.frame.position.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.0001);
            }
            let direction = projectile.velocity;
            if bone_direction.is_some() {
                for (actual, expected) in direction.into_iter().zip([0., 0.8944272, 0.4472136]) {
                    assert!((actual - expected).abs() < 0.0001);
                }
            } else {
                for (actual, expected) in direction.into_iter().zip([-5., -5., 0.]) {
                    assert!((actual - expected).abs() < 0.0001);
                }
            }
            battle.actors[0].position = [-1000., 0., 0.];
            battle.actors[1].position = [900., 0., 900.];
            let mut frame = battle.publish(Vec::new());
            battle.advance_models(&mut frame, crate::BattleClock::Running, false)?;
            let projectile = battle.projectiles.get_mut(&id).unwrap();
            let position = projectile.frame.position;
            projectile.step()?;
            assert_eq!(projectile.velocity, direction);
            assert_eq!(
                projectile.frame.position,
                std::array::from_fn(|i| position[i] + direction[i] * 30.)
            );
        }
        Ok(())
    }

    #[test]
    fn velocity_reset_runs_after_motion_without_stopping_acceleration() -> Result<()> {
        let mut projectile = instance(
            [4., 0., 0.],
            ProjectileMotion {
                world_direction: true,
                face_velocity: true,
                velocity_reset: Some(ProjectileVelocityReset {
                    age: 2,
                    acceleration_scale: 0.25,
                }),
                ..Default::default()
            },
        );
        projectile.frame.heading = 180.;
        projectile.acceleration = [2., 0., 0.];
        let mut random = crate::Random::new(0);
        projectile
            .initialize(&mut Vec::new(), |_, _| Ok(random.next_u16()), None)
            .unwrap();
        for _ in 0..3 {
            projectile.step()?;
        }
        assert_eq!(projectile.frame.position, [18., 5., 0.]);
        assert_eq!(projectile.frame.heading, 90.);
        assert_eq!(projectile.velocity, [0.5, 0., 0.]);
        assert!(projectile.frame.contact_active && !projectile.retiring);
        projectile.step()?;
        assert_eq!(projectile.frame.position, [18.5, 5., 0.]);
        assert_eq!(projectile.velocity, [2.5, 0., 0.]);
        Ok(())
    }

    #[test]
    fn water_blade_brakes_then_turns_once_and_accelerates_beyond_the_arena() -> Result<()> {
        let mut projectile = instance(
            [0., 0., 1.],
            ProjectileMotion {
                aqua_edge: true,
                unlimited_range: true,
                speed: Some(15.),
                ..Default::default()
            },
        );
        projectile.frame.position = [-700., 0., 0.];
        projectile.frame.heading = 90.;
        Arc::make_mut(&mut projectile.definition).offset = [0., 35., 0.];
        let mut random = crate::Random::new(0);
        projectile
            .initialize(&mut Vec::new(), |_, _| Ok(random.next_u16()), None)
            .unwrap();
        projectile.target_point = [1200., 100., 0.];
        for _ in 0..30 {
            projectile.step()?;
        }
        assert!((projectile.frame.position[0] + 700.).abs() < 0.001);
        assert!((projectile.frame.position[2] + 232.5).abs() < 0.001);
        assert_eq!(projectile.frame.position[1], 35.);
        let heading = projectile.velocity;
        projectile.target_point = [-1200., 0., 0.];
        let previous = projectile.frame.position;
        projectile.step()?;
        assert!(
            (crate::distance::length(std::array::from_fn(
                |i| projectile.frame.position[i] - previous[i]
            )) - 0.5)
                .abs()
                < 0.001
        );
        assert_eq!(projectile.velocity, heading);
        for _ in 0..89 {
            projectile.step()?;
        }
        assert!(projectile.frame.position[0] > 2200.);
        assert!(!projectile.retiring);
        Ok(())
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
        let mut random = crate::Random::new(1);
        projectile
            .initialize(&mut Vec::new(), |_, _| Ok(random.next_u16()), None)
            .unwrap();
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
    fn extrapolating_steering_preserves_observed_guardian_transitions() -> Result<()> {
        for (position, velocity, next_position, next_velocity) in [
            (
                [-164.4708, 133.07022, -124.24034],
                [0.; 3],
                [-164.4708, 133.07022, -124.24034],
                [0.79792905, 0., 0.60275126],
            ),
            (
                [-154.89658, 133.07022, -117.00611],
                [0.8185776, 0., 0.57439584],
                [-151.62227, 133.07022, -114.70853],
                [-0.32153502, 0., 0.9468977],
            ),
        ] {
            let mut projectile = instance(
                velocity,
                ProjectileMotion {
                    speed: Some(4.),
                    steering: Some(ProjectileSteering {
                        blend: 90.,
                        start: 0,
                        end: None,
                        planar: true,
                    }),
                    ..Default::default()
                },
            );
            projectile.definition.validate()?;
            projectile.frame.position = position;
            projectile.target_point = [0., 80., 0.];
            let mut random = crate::Random::new(0);
            projectile
                .initialize(&mut Vec::new(), |_, _| Ok(random.next_u16()), None)
                .unwrap();
            projectile.step()?;
            for (actual, expected) in projectile
                .frame
                .position
                .into_iter()
                .zip(next_position)
                .chain(projectile.velocity.into_iter().zip(next_velocity))
            {
                assert!((actual - expected).abs() < 0.00002);
            }
        }
        Ok(())
    }

    #[test]
    fn feather_trajectory_matches_the_observed_launch_turn_and_ground_exit() -> Result<()> {
        let mut projectile = instance(
            [-0.3, -0.2, 0.5],
            ProjectileMotion {
                speed: Some(30.),
                steering: Some(ProjectileSteering {
                    blend: 0.1,
                    start: 0,
                    end: Some(30),
                    planar: false,
                }),
                ..Default::default()
            },
        );
        projectile.frame.position = [-500., 240., 0.];
        projectile.frame.heading = 90.00002;
        projectile.target_point = [-126.93901, 50., 45.05799];
        let mut random = crate::Random::new(0);
        let mut cues = Vec::new();
        projectile.initialize(&mut cues, |_, _| Ok(random.next_u16()), None)?;
        let launch_direction = cues
            .iter()
            .find_map(|cue| match cue {
                Cue::ProjectileStarted { velocity, .. } => Some(*velocity),
                _ => None,
            })
            .unwrap();
        for (actual, expected) in launch_direction.into_iter().zip([0.5, -0.2, 0.3]) {
            assert!((actual - expected).abs() < 0.000001);
        }
        // Observed positions after 1, 2, 10 and 23 movements, including the turn past the target.
        let anchors = [
            (1, [-485., 234., 8.999994]),
            (2, [-460.03058, 223.54453, 21.931]),
            (10, [-251.61644, 126.721504, 87.97628]),
            (23, [50.626827, -62.1069, -28.518032]),
        ];
        let mut moved = 0;
        for (at, expected) in anchors {
            while moved < at {
                projectile.step()?;
                moved += 1;
            }
            if at == 1 {
                assert_ne!(projectile.frame.velocity, launch_direction);
                for (actual, expected) in projectile
                    .frame
                    .velocity
                    .into_iter()
                    .zip([0.832314, -0.348516, 0.431034])
                {
                    assert!((actual - expected).abs() < 0.00001);
                }
            }
            for (actual, expected) in projectile.frame.position.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 0.002,
                    "movement {at}: {actual} != {expected}"
                );
            }
        }
        assert!(projectile.retiring);
        assert_eq!(
            steer_velocity([1., 0., 0.], [0.; 3], 0.1, false),
            [1., 0., 0.]
        );
        assert_eq!(
            steer_velocity([1., 0., 0.], [-1., 0., 0.], 0.5, false),
            [1., 0., 0.]
        );
        Ok(())
    }

    #[test]
    fn hit_limits_count_contacts_independently_of_cooldown_and_never_reset() {
        for cooldown in [0, 2] {
            for limit in [0, 1, 3] {
                let mut projectile = instance([0.; 3], Default::default());
                Arc::get_mut(&mut projectile.definition).unwrap().contact =
                    Some(ProjectileContact {
                        hit: crate::HitRule {
                            element: crate::HitElement::Inherited,
                            ..crate::tests::hit()
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
                let mut projectile = Projectile::new(
                    projectile.definition,
                    projectile.action,
                    projectile.frame,
                    70,
                );
                let target = ActorId(69);
                let admitted_hits = if limit == 0 {
                    u16::from(u8::MAX) + 1
                } else {
                    u16::from(limit)
                };
                for _ in 0..admitted_hits {
                    assert!(projectile.can_hit(target));
                    projectile.hit(target, false);
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
