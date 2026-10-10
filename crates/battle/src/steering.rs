//! Automatic obstacle detours and circular arena boundaries.
use crate::{Actor, ActorId, Battle, BattleResult, Control, PreparedBattle, Side, distance};
use anyhow::{Result, ensure};

mod recovery_position;
mod recovery_return;
pub(crate) mod return_position;
pub use recovery_return::RecoveryReturnDefinition;
pub(crate) use recovery_return::ReturnState;

const ARRIVAL_DISTANCE: f32 = 8.;
const ARENA_RADIUS: f32 = 850.;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArenaContact {
    #[default]
    None,
    /// Moved radially back to the boundary.
    Slid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DetourSide {
    #[default]
    Automatic,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Steering {
    /// Ignore allied obstacles which also allow this passage.
    pub passes_allied_obstacles: bool,
    pub passable_for_allies: bool,
    pub unrestricted_arena: bool,
    /// Other actors do not push against this body.
    pub push_obstacle_disabled: bool,
    pub push_immovable: bool,
    /// Set by an actor's explicit escape controller.
    pub leaving_arena: bool,
    pub(crate) home: [f32; 3],
    pub(crate) home_offset: [f32; 3],
    pub(crate) side: DetourSide,
    pub(crate) contact: ArenaContact,
}

impl Steering {
    pub fn arena_contact(&self) -> ArenaContact {
        self.contact
    }
}

impl PreparedBattle {
    pub fn with_arena_boundary(mut self) -> Self {
        self.resources.arena_boundary = true;
        self
    }
}

impl Battle {
    /// Admission chooses a stable side around obstacles near the arena edge.
    pub(crate) fn begin_approach_steering(
        &mut self,
        owner: ActorId,
        target: ActorId,
    ) -> Result<()> {
        let actor = self.actor(owner)?;
        let target = self.actor(target)?;
        let side = approach_side(actor, target.position);
        if actor.control != Control::Enemy {
            self.actors[owner.index()].movement.steering.home = self.actors[owner.index()].position;
        }
        self.actors[owner.index()].movement.steering.side = side;
        Ok(())
    }

    pub(crate) fn steer_approach(
        &self,
        owner: ActorId,
        target: ActorId,
        destination: [f32; 3],
    ) -> Result<[f32; 3]> {
        self.actor(target)?;
        self.steer_to(owner, destination, Some(target))
    }

    fn steer_to(
        &self,
        owner: ActorId,
        destination: [f32; 3],
        target: Option<ActorId>,
    ) -> Result<[f32; 3]> {
        let actor = self.actor(owner)?;
        ensure!(
            destination.iter().all(|v| v.is_finite()),
            "invalid steering destination"
        );
        let delta = [
            destination[0] - actor.position[0],
            0.,
            destination[2] - actor.position[2],
        ];
        let length = planar_length(delta);
        if actor.control == Control::Manual || length <= f32::EPSILON {
            return Ok(destination);
        }
        let direction = delta.map(|v| v / length);
        let normal = [-direction[2], 0., direction[0]];
        let blocker = self
            .actors
            .iter()
            .enumerate()
            .filter_map(|(index, obstacle)| {
                if index == owner.index()
                    || target.is_some_and(|id| id.index() == index)
                    || !obstacle.available()
                    || obstacle.movement.fixed_height
                    || actor.control == Control::SemiAuto && actor.side == obstacle.side
                    || actor.side == obstacle.side
                        && actor.movement.steering.passes_allied_obstacles
                        && obstacle.movement.steering.passable_for_allies
                {
                    return None;
                }
                let relative = [
                    obstacle.position[0] - actor.position[0],
                    0.,
                    obstacle.position[2] - actor.position[2],
                ];
                let along = distance::dot(relative, direction);
                let across = distance::dot(relative, normal);
                let clearance = recovery_position::clearance(actor, obstacle);
                (along > 0. && along < length && across.abs() < clearance)
                    .then_some((along, across, clearance, obstacle))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, across, clearance, obstacle)) = blocker else {
            return Ok(destination);
        };
        let side = match actor.movement.steering.side {
            DetourSide::Left => 1.,
            DetourSide::Right => -1.,
            DetourSide::Automatic => {
                if across > 0. {
                    -1.
                } else {
                    1.
                }
            }
        };
        Ok(std::array::from_fn(|axis| {
            obstacle.position[axis] - direction[axis] * clearance + normal[axis] * clearance * side
        }))
    }

    /// Keep the body on the floor. Escaping actors may leave the arena.
    pub(crate) fn constrain_actor(&mut self, index: usize) {
        crate::movement::floor(&mut self.actors[index]);
        self.actors[index].movement.steering.contact = ArenaContact::None;
        if self.prepared.arena_boundary && self.terminal.result != Some(BattleResult::Escaped) {
            constrain(&mut self.actors[index]);
        }
    }
}

fn approach_side(actor: &Actor, target: [f32; 3]) -> DetourSide {
    if actor.control == Control::SemiAuto {
        return DetourSide::Automatic;
    }
    let normal = [
        actor.position[2] - target[2],
        0.,
        target[0] - actor.position[0],
    ];
    let outward = distance::dot(actor.position, normal);
    if outward.abs() <= f32::EPSILON {
        DetourSide::Automatic
    } else if outward < 0. {
        DetourSide::Left
    } else {
        DetourSide::Right
    }
}

fn planar_length(position: [f32; 3]) -> f32 {
    distance::length([position[0], 0., position[2]])
}

fn constrain(actor: &mut Actor) {
    if actor.movement.steering.unrestricted_arena || actor.movement.steering.leaving_arena {
        return;
    }
    let radius = (ARENA_RADIUS - actor.body_radius()).max(0.);
    let length = planar_length(actor.position);
    if length > radius {
        let scale = radius / length;
        actor.position[0] *= scale;
        actor.position[2] *= scale;
        actor.movement.steering.contact = ArenaContact::Slid;
    }
}

#[cfg(test)]
mod tests;
