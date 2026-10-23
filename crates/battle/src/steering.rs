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
const ENEMY_ARENA_MARGIN: f32 = 35.;
const ARENA_TARGET_LINE_LIMIT: f32 = 552.5;
const DETOUR_DISTANCE: f32 = 300.;
const DETOUR_CORRIDOR: f32 = 400.;
const DETOUR_MARGIN: f32 = 50.;
const DETOUR_EDGE_RADIUS: f32 = ARENA_RADIUS * 0.75;
const MAX_DETOUR_VISITS: usize = 11;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArenaContact {
    #[default]
    None,
    /// Moved radially back to the boundary.
    Slid,
    /// Rejected the planar step and kept the previous position.
    Blocked,
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
    /// Radius of this body in the automatic positioning grid.
    pub positioning_radius: u8,
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
        if actor.control == Control::Manual {
            return Ok(destination);
        }
        let mut destination = destination;
        // The controller revisits the first obstructing actor in roster order.
        // Bound cyclic detours even when successive waypoints remain obstructed.
        for _ in 0..MAX_DETOUR_VISITS {
            let delta = [
                destination[0] - actor.position[0],
                0.,
                destination[2] - actor.position[2],
            ];
            let length = planar_length(delta);
            if length <= f32::EPSILON {
                break;
            }
            let direction = delta.map(|value| value / length);
            let normal = [-direction[2], 0., direction[0]];
            let detour = [Side::Party, Side::Enemy].into_iter().find_map(|side| {
                self.actors
                    .iter()
                    .enumerate()
                    .find_map(|(index, obstacle)| {
                        if obstacle.side != side
                            || index == owner.index()
                            || target.is_some_and(|id| id.index() == index)
                            || actor.control == Control::SemiAuto && actor.side == obstacle.side
                        {
                            return None;
                        }
                        detour_destination(actor, obstacle, destination, normal)
                    })
            });
            let Some(detour) = detour else {
                break;
            };
            destination = detour;
        }
        Ok(destination)
    }

    /// Keep the body on the floor and inside the arena. A placement without a
    /// previous position slides to the edge instead of rejecting the step.
    pub(crate) fn constrain_actor(&mut self, index: usize, previous: Option<[f32; 3]>) {
        crate::movement::floor(&mut self.actors[index]);
        self.actors[index].movement.steering.contact = ArenaContact::None;
        let actor = &self.actors[index];
        if !self.prepared.arena_boundary
            || self.terminal.result == Some(BattleResult::Escaped)
            || actor.movement.steering.unrestricted_arena
            || actor.movement.steering.leaving_arena
        {
            return;
        }
        let radius = ARENA_RADIUS
            + if actor.side == Side::Enemy {
                ENEMY_ARENA_MARGIN
            } else {
                0.
            };
        let length = planar_length(actor.position);
        if length <= radius {
            return;
        }
        let previous = previous.filter(|_| self.blocks_arena_slide(index));
        let actor = &mut self.actors[index];
        if let Some(previous) = previous {
            actor.position[0] = previous[0];
            actor.position[2] = previous[2];
            actor.movement.steering.contact = ArenaContact::Blocked;
        } else {
            let scale = radius / length;
            actor.position[0] *= scale;
            actor.position[2] *= scale;
            actor.movement.steering.contact = ArenaContact::Slid;
        }
    }

    fn blocks_arena_slide(&self, index: usize) -> bool {
        let actor = &self.actors[index];
        if actor.airborne() {
            return true;
        }
        let Some(leader) = self.preferred_party_leader() else {
            return false;
        };
        let Some(target) = self.target(leader) else {
            return false;
        };
        if target.index() == index {
            return true;
        }
        if leader.index() != index {
            return false;
        }
        let target = self.actors[target.index()].position;
        let direction = distance::normalize([
            target[0] - actor.position[0],
            0.,
            target[2] - actor.position[2],
        ]);
        let line_distance = actor.position[0] * direction[2] - actor.position[2] * direction[0];
        line_distance.abs() < ARENA_TARGET_LINE_LIMIT
    }
}

fn detour_destination(
    actor: &Actor,
    obstacle: &Actor,
    destination: [f32; 3],
    normal: [f32; 3],
) -> Option<[f32; 3]> {
    if !obstacle.available()
        || obstacle.movement.fixed_height
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
    let across = distance::dot(relative, normal);
    let in_bounds = [0, 2].into_iter().all(|axis| {
        (actor.position[axis].min(destination[axis]) - DETOUR_MARGIN
            ..=actor.position[axis].max(destination[axis]) + DETOUR_MARGIN)
            .contains(&obstacle.position[axis])
    });
    if !(across.abs() < DETOUR_CORRIDOR && in_bounds) {
        return None;
    }
    let side = match actor.movement.steering.side {
        DetourSide::Left => 1.,
        DetourSide::Right => -1.,
        DetourSide::Automatic if across <= 0. => 1.,
        DetourSide::Automatic => -1.,
    };
    Some(std::array::from_fn(|axis| {
        obstacle.position[axis] + normal[axis] * DETOUR_DISTANCE * side
    }))
}

fn approach_side(actor: &Actor, target: [f32; 3]) -> DetourSide {
    if actor.control == Control::SemiAuto || planar_length(actor.position) < DETOUR_EDGE_RADIUS {
        return DetourSide::Automatic;
    }
    let normal = [
        actor.position[2] - target[2],
        0.,
        target[0] - actor.position[0],
    ];
    let outward = distance::dot(actor.position, normal);
    if outward < 0. {
        DetourSide::Left
    } else {
        DetourSide::Right
    }
}

fn planar_length(position: [f32; 3]) -> f32 {
    distance::length([position[0], 0., position[2]])
}

#[cfg(test)]
mod tests;
