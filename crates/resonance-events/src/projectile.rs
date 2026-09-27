//! Scene-local projectile state. Scripts own lifetime and reactions; the field
//! queries each movement segment before the script advances it.
use crate::{ACTOR_CONTACT_HEIGHT, Animation, GameWorld, Operation, Outcome};

impl crate::Actor {
    pub fn projectile_target(&self) -> bool {
        // Native contact survives alpha-zero phases, including Ice's rising water.
        self.contact != crate::ActorContact::None
            && self.ring_contact_enabled()
            && (self.resource < resonance_content::field::SCENERY_RESOURCE_BASE
                || resonance_content::field::LOCAL_MODEL_RESOURCES.contains(&self.resource))
    }
    /// First contact along a segment with the actor's horizontal collision cylinder.
    fn projectile_contact(&self, start: [f32; 3], delta: [f32; 3], radius: f32) -> Option<f32> {
        if self.contact != crate::ActorContact::Cylinder {
            return None;
        }
        let offset: [f32; 3] = std::array::from_fn(|i| start[i] - self.position[i]);
        let radius = radius + self.radius;
        let speed = delta[0] * delta[0] + delta[1] * delta[1];
        let distance = offset[0] * offset[0] + offset[1] * offset[1] - radius * radius;
        let (mut enter, mut exit) = (0_f32, 1_f32);
        if speed == 0. {
            if distance > 0. {
                return None;
            }
        } else {
            let approach = offset[0] * delta[0] + offset[1] * delta[1];
            let discriminant = approach * approach - speed * distance;
            if discriminant < 0. {
                return None;
            }
            enter = enter.max((-approach - discriminant.sqrt()) / speed);
            exit = exit.min((-approach + discriminant.sqrt()) / speed);
        }
        if delta[2] == 0. {
            if offset[2].abs() > ACTOR_CONTACT_HEIGHT {
                return None;
            }
        } else {
            let bottom = (-ACTOR_CONTACT_HEIGHT - offset[2]) / delta[2];
            let top = (ACTOR_CONTACT_HEIGHT - offset[2]) / delta[2];
            enter = enter.max(bottom.min(top));
            exit = exit.min(bottom.max(top));
        }
        (enter <= exit).then_some(enter)
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
    pub shadow: Option<Shadow>,
    /// A script may retain an impact effect's velocity without advancing its origin.
    pub paused: bool,
    pub(crate) blocks_menu: bool,
    pub(crate) operation: Operation,
}

impl Projectile {
    fn movement(&self) -> [f32; 3] {
        if self.paused { [0.; 3] } else { self.velocity }
    }
    pub(crate) fn touches_actor(&self, actor: &crate::Actor, radius: f32) -> bool {
        actor
            .projectile_contact(self.position, self.movement(), radius)
            .is_some()
    }
    pub(crate) fn barrier(&self, world: &GameWorld) -> Option<f32> {
        let end = std::array::from_fn(|i| self.position[i] + self.movement()[i]);
        world
            .actors
            .iter()
            .filter(|(id, actor)| **id != self.source && actor.ring_contact_enabled())
            .filter_map(|(_, actor)| {
                actor.solid_contact(
                    self.position,
                    end,
                    resonance_content::field::CollisionQuery::All,
                )
            })
            .min_by(f32::total_cmp)
    }
    pub(crate) fn reaches(&self, actor: &crate::Actor, radius: f32, barrier: Option<f32>) -> bool {
        actor
            .projectile_contact(self.position, self.movement(), radius)
            .is_some_and(|time| barrier.is_none_or(|barrier| time < barrier))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_checks_the_entire_segment_including_vertical_motion() {
        let mut actor = crate::Actor::new(1, [0.; 3]);
        actor.radius = 1.;
        for (start, delta, hits) in [
            ([-10., -10., 0.], [20., 20., 0.], true),
            ([-10., -10., 0.], [20., 0., 0.], false),
            (
                [0., 0., 2. * ACTOR_CONTACT_HEIGHT],
                [0., 0., -2. * ACTOR_CONTACT_HEIGHT],
                true,
            ),
            ([10., 0., 0.], [20., 0., 0.], false),
            ([0.; 3], [0.; 3], true),
        ] {
            assert_eq!(actor.projectile_contact(start, delta, 1.).is_some(), hits);
        }
    }
}
