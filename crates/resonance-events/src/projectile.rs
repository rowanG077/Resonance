//! Scene-local projectile state. Scripts own lifetime and reactions; the field
//! resolves each movement segment against its geometry before the next VM update.
use crate::{ACTOR_CONTACT_HEIGHT, Animation, GameWorld, Operation, Outcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    Flying,
    Actor(i32),
    Barrier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Move to the first swept contact before the controller runs.
    Swept,
    /// Probe barriers, leaving overlap order and advancement to the script.
    Scripted,
}

// A swept contact can round just outside the cylinder when stored as f32.
const CONTACT_TOLERANCE: f32 = 0.001;
impl crate::Actor {
    pub fn projectile_target(&self) -> bool {
        // Native contact survives alpha-zero phases, including Ice's rising water.
        self.contact != crate::ActorContact::None
            && self.ring_contact_enabled()
            && (self.resource < resonance_content::field::SCENERY_RESOURCE_BASE
                || resonance_content::field::LOCAL_MODEL_RESOURCES.contains(&self.resource))
    }
    pub fn touches_projectile(&self, position: [f32; 3], radius: f32) -> bool {
        self.contact != crate::ActorContact::None
            && (position[2] - self.position[2]).abs() <= ACTOR_CONTACT_HEIGHT
            && (position[0] - self.position[0]).hypot(position[1] - self.position[1])
                <= radius + self.radius + CONTACT_TOLERANCE
    }
    pub fn touches_moving_projectile(
        &self,
        position: [f32; 3],
        velocity: [f32; 3],
        radius: f32,
    ) -> bool {
        let x = position[0] + velocity[0] - self.position[0];
        let y = position[1] - self.position[1];
        self.contact == crate::ActorContact::Cylinder
            && (position[2] + velocity[2] - self.position[2]).abs() <= ACTOR_CONTACT_HEIGHT
            && (x.hypot(y) < radius + self.radius
                || x.hypot(y + velocity[1]) < radius + self.radius)
    }
}

pub struct Projectile {
    /// Root task that owns this controller, independent of its input lease.
    pub task: i32,
    pub source: i32,
    pub source_instance: u64,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub radius: f32,
    pub contact: Contact,
    pub motion: Motion,
    pub shadow: Option<Shadow>,
    /// A script may retain an impact effect's velocity without advancing its origin.
    pub paused: bool,
    pub(crate) blocks_menu: bool,
    pub(crate) operation: Operation,
}

impl Projectile {
    /// Held effects retain velocity for particles, but contact stops advancing.
    pub(crate) fn touches_actor(&self, actor: &crate::Actor, radius: f32) -> bool {
        match self.motion {
            Motion::Scripted => actor.touches_moving_projectile(
                self.position,
                if self.paused { [0.; 3] } else { self.velocity },
                radius,
            ),
            Motion::Swept => actor.touches_projectile(self.position, radius),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    pub size: f32,
    pub rgba: [u8; 4],
}

pub(crate) struct OwnedPose {
    pub actor: i32,
    pub instance: u64,
    pub previous: Option<Animation>,
    pub scripted: bool,
    pub slot: u16,
    pub started: u32,
    pub tint: Option<[u8; 3]>,
    pub operation: Operation,
}

pub const CHAIN_HISTORY_TICKS: u32 = 16;

/// One model update's external acceleration, retained for render catch-up.
#[derive(Debug, Clone)]
pub struct ChainImpulse {
    pub acceleration: [f32; 3],
    pub(crate) operation: Operation,
}

#[derive(Debug, Clone)]
pub(crate) struct VisualLift {
    pub height: f32,
    pub operation: Operation,
}

impl GameWorld {
    pub fn pose_tint(&self, actor: i32) -> Option<[u8; 3]> {
        let instance = self.actors.get(&actor)?.instance;
        self.owned_poses.values().rev().find_map(|pose| {
            (pose.actor == actor && pose.instance == instance && pose.operation.is_pending())
                .then_some(pose.tint)
                .flatten()
        })
    }
    pub fn menu_blocked(&self) -> bool {
        self.menu_disabled
            || self
                .projectiles
                .values()
                .any(|p| p.blocks_menu && p.operation.is_pending())
    }
    /// Controllers can outlive their input lease; spent effects and visual tails
    /// do not prevent the next cast while a task finishes joining its children.
    pub fn has_authored_controller(&self, task: i32) -> bool {
        self.effect_contexts
            .values()
            .any(|c| c.task == task && c.operation.is_pending())
            || self
                .projectiles
                .values()
                .any(|p| p.task == task && p.operation.is_pending())
            || self
                .fog_effects
                .values()
                .any(|f| f.task == task && f.operation.is_pending())
    }

    pub(crate) fn reap_authored_resources(&mut self) {
        self.authored_actors.retain(|handle, id| {
            self.actors
                .get(id)
                .is_some_and(|actor| actor.authored_handle == Some(*handle))
        });
        self.actors
            .retain(|_, actor| actor.operation.as_ref().is_none_or(Operation::is_pending));
        self.fog_effects
            .retain(|_, effect| effect.operation.is_pending());
        self.projectiles.retain(|_, p| p.operation.is_pending());
        self.effect_contexts.retain(|_, c| c.operation.is_pending());
        self.model_particles
            .retain(|_, p| p.operation.as_ref().is_none_or(Operation::is_pending));
        self.billboards.retain(|_, p| {
            p.operation
                .as_ref()
                .is_none_or(|op| op.progress().outcome != Some(Outcome::Cancelled))
        });
        self.refractions.retain(|_, p| {
            p.operation
                .as_ref()
                .is_none_or(|op| op.progress().outcome != Some(Outcome::Cancelled))
        });
        for actor in self.actors.values_mut() {
            if actor
                .visual_lift
                .as_ref()
                .is_some_and(|lift| !lift.operation.is_pending())
            {
                actor.visual_lift = None;
            }
            actor.chain_impulses.retain(|tick, impulse| {
                self.tick.saturating_sub(*tick) < CHAIN_HISTORY_TICKS
                    && impulse.operation.progress().outcome != Some(Outcome::Cancelled)
            });
        }
        let finished: Vec<_> = self
            .owned_poses
            .iter()
            .filter_map(|(id, pose)| (!pose.operation.is_pending()).then_some(*id))
            .collect();
        for id in finished {
            let mut pose = self.owned_poses.remove(&id).unwrap();
            // A newer pose may have captured this one before it completed.
            // Unwind that saved baseline too, so rapid recasts cannot restore a spent pose.
            for successor in self.owned_poses.values_mut() {
                if successor.actor == pose.actor
                    && successor.instance == pose.instance
                    && successor
                        .previous
                        .as_ref()
                        .is_some_and(|a| a.slot == pose.slot && a.start_tick == pose.started)
                {
                    successor.previous = pose.previous.clone();
                    successor.scripted = pose.scripted;
                }
            }
            if let Some(actor) = self.actors.get_mut(&pose.actor)
                && actor.instance == pose.instance
                && actor
                    .animation
                    .as_ref()
                    .is_some_and(|a| a.slot == pose.slot && a.start_tick == pose.started)
            {
                actor.animation = pose.previous.take();
                actor.scripted_animation = pose.scripted;
            }
        }
    }
}
