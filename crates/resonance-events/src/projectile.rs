//! Ring contact geometry and scene-owned effect lifetimes.
use crate::{ACTOR_CONTACT_HEIGHT, GameWorld, Operation, Outcome};

impl crate::Actor {
    pub fn projectile_target(&self) -> bool {
        // Invisible puzzle targets can still receive ring hits.
        self.contact != crate::ActorContact::None
            && self.ring_contact_enabled()
            && (self.resource < resonance_content::field::SCENERY_RESOURCE_BASE
                || resonance_content::field::LOCAL_MODEL_RESOURCES.contains(&self.resource))
    }
    /// First contact along a segment with the actor's horizontal collision cylinder.
    pub(crate) fn projectile_contact(
        &self,
        start: [f32; 3],
        delta: [f32; 3],
        radius: f32,
    ) -> Option<f32> {
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

pub(crate) struct Shot {
    pub source: i32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub radius: f32,
}

impl Shot {
    pub fn advance(&mut self) {
        self.position = std::array::from_fn(|i| self.position[i] + self.velocity[i]);
    }
    pub(crate) fn touches_actor(&self, actor: &crate::Actor, radius: f32) -> bool {
        actor
            .projectile_contact(self.position, self.velocity, radius)
            .is_some()
    }
    pub(crate) fn barrier(&self, world: &GameWorld) -> Option<f32> {
        let end = std::array::from_fn(|i| self.position[i] + self.velocity[i]);
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
    pub(crate) fn targets<'a>(
        &'a self,
        world: &'a GameWorld,
        radius: f32,
    ) -> impl Iterator<Item = (i32, f32)> + 'a {
        let barrier = self.barrier(world);
        world.actors.iter().filter_map(move |(&id, actor)| {
            if id == self.source || !actor.projectile_target() {
                return None;
            }
            let time = actor.projectile_contact(self.position, self.velocity, radius)?;
            barrier
                .is_none_or(|barrier| time < barrier)
                .then_some((id, time))
        })
    }
    pub(crate) fn nearest_target(&self, world: &GameWorld, radius: f32) -> Option<(i32, f32)> {
        self.targets(world, radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
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
        self.ring.pose_tint(actor, self.effect_tick)
    }
    pub fn menu_blocked(&self) -> bool {
        self.menu_disabled || self.ring.blocks_menu()
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
