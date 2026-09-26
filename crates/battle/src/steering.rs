//! Automatic obstacle detours and the ordinary circular arena (23730 / 23AD0).
//! These operate on the same actor positions as movement, contacts and framing.
use crate::{Actor, ActorId, Battle, BattleResult, Control, PreparedBattle, Side, distance};
use anyhow::{Result, ensure};

mod recovery_return;
pub(crate) mod return_position;
pub use recovery_return::RecoveryReturnDefinition;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArenaContact {
    #[default]
    None,
    /// Moved radially back to the boundary.
    Slid,
    /// Restored the horizontal position from before integration.
    Blocked,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BoundaryCorrection {
    /// The leader's outside-arena query evaluates its line against the origin.
    pub leader_outside: bool,
    /// Rounded radial correction, before addition to the world position.
    pub translation: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DetourSide {
    #[default]
    Automatic,
    Left,
    Right,
    /// Coincident edge admission left the original local direction unwritten.
    Unproved,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Steering {
    /// Ignore allied obstacles which also allow this passage.
    pub passes_allied_obstacles: bool,
    pub passable_for_allies: bool,
    pub unrestricted_arena: bool,
    /// Set by an actor's explicit escape controller.
    pub leaving_arena: bool,
    /// Original profile category at E7, also used for formation clearance.
    pub clearance_category: u8,
    pub(crate) home: [f32; 3],
    pub(crate) home_offset: [f32; 3],
    pub(crate) home_valid: bool,
    pub(crate) returning: Option<recovery_return::ReturnState>,
    pub(crate) side: DetourSide,
    pub(crate) contact: ArenaContact,
}

impl Steering {
    pub fn arena_contact(&self) -> ArenaContact {
        self.contact
    }
}

impl PreparedBattle {
    /// Enable the original ordinary arena. Small isolated action kernels need
    /// not impose arena placement on their supplied test geometry.
    pub fn with_arena_boundary(mut self) -> Self {
        self.arena_boundary = true;
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

    /// Return a destination, not a velocity; the caller retains its source
    /// interpolation, facing, integration and approach admission order.
    pub(crate) fn steer_approach(
        &self,
        owner: ActorId,
        target: ActorId,
        mut destination: [f32; 3],
    ) -> Result<[f32; 3]> {
        let actor = self.actor(owner)?;
        self.actor(target)?;
        ensure!(
            destination.iter().all(|v| v.is_finite()),
            "invalid steering destination"
        );
        if actor.control == Control::Manual {
            return Ok(destination);
        }
        // The native loop starts at zero, and leaves only after increment > 10.
        for _ in 0..11 {
            let Some(next) = self.steering_detour(owner, target, destination)? else {
                break;
            };
            destination = next;
        }
        Ok(destination)
    }

    fn steering_detour(
        &self,
        owner: ActorId,
        target: ActorId,
        destination: [f32; 3],
    ) -> Result<Option<[f32; 3]>> {
        let actor = &self.actors[owner.index()];
        for side in [Side::Party, Side::Enemy] {
            if actor.control == Control::SemiAuto && actor.side == side {
                continue;
            }
            for (index, obstacle) in self.actors.iter().enumerate() {
                if obstacle.side != side
                    || index == owner.index()
                    || index == target.index()
                    || !obstacle.available()
                    || obstacle.movement.fixed_height
                    || (actor.side == side
                        && actor.movement.steering.passes_allied_obstacles
                        && obstacle.movement.steering.passable_for_allies)
                {
                    continue;
                }
                // The native function computes line separation first, but only
                // reads the output normal inside this box. Outside it, even a
                // coincident source/destination is fully defined.
                let within = [0, 2].into_iter().all(|axis| {
                    obstacle.position[axis] >= actor.position[axis].min(destination[axis]) - 50.
                        && obstacle.position[axis]
                            <= actor.position[axis].max(destination[axis]) + 50.
                });
                if !within {
                    continue;
                }
                let Some((separation, mut normal)) =
                    line_separation(obstacle.position, actor.position, destination)
                else {
                    // 4D798 returns zero without writing its out-vector here.
                    // Do not invent the original caller's uninitialized stack.
                    anyhow::bail!("coincident steering endpoints need original-game validation");
                };
                if separation.abs() >= 400. {
                    continue;
                }
                // 23730's earlier side read only scales a local vector for
                // rejected obstacles. A later nondegenerate line overwrites
                // that vector, so only an actual detour needs a proved side.
                let preferred: i8 = match actor.movement.steering.side {
                    DetourSide::Automatic => 0,
                    DetourSide::Left => -1,
                    DetourSide::Right => 1,
                    DetourSide::Unproved => anyhow::bail!(
                        "coincident approach endpoints need original-game validation (actor {}, target {})",
                        owner.index(),
                        target.index()
                    ),
                };
                if preferred != 0 {
                    normal = normal.map(|v| v * f32::from(preferred));
                }
                let scale = if preferred == 0 && separation >= 0. {
                    -300.
                } else {
                    300.
                };
                return Ok(Some(std::array::from_fn(|axis| {
                    obstacle.position[axis] + normal[axis] * scale
                })));
            }
        }
        Ok(None)
    }

    /// Runs after the actor callback's integration and before its approach stop
    /// check. Other actors therefore observe this actor's corrected position.
    pub(crate) fn constrain_actor(&mut self, index: usize) -> BoundaryCorrection {
        crate::movement::floor(&mut self.actors[index]);
        self.actors[index].movement.steering.contact = ArenaContact::None;
        if !self.prepared.arena_boundary || self.terminal.result == Some(BattleResult::Escaped) {
            return BoundaryCorrection::default();
        }
        // B564 selects the first non-automatic party member, falling back to
        // the first formation member. Availability does not change this choice.
        let leader = self
            .actors
            .iter()
            .position(|a| a.side == Side::Party && a.control != Control::Auto)
            .or_else(|| self.actors.iter().position(|a| a.side == Side::Party));
        let leader_target = leader.and_then(|leader| self.target(ActorId(leader as u8)));
        let constrained = leader_target.is_some_and(|target| target.index() == index)
            || leader == Some(index)
                && leader_target.is_some_and(|target| {
                    line_separation(
                        [0.; 3],
                        self.actors[index].position,
                        self.actors[target.index()].position,
                    )
                    .map_or(0., |(distance, _)| distance)
                    .abs()
                        < 552.5
                });
        let translation = constrain(&mut self.actors[index], constrained);
        BoundaryCorrection {
            leader_outside: leader == Some(index)
                && self.actors[index].movement.steering.contact != ArenaContact::None,
            translation,
        }
    }
}

fn approach_side(actor: &Actor, target: [f32; 3]) -> DetourSide {
    if actor.control == Control::SemiAuto || planar_length(actor.position) < 637.5 {
        return DetourSide::Automatic;
    }
    let delta = [
        target[0] - actor.position[0],
        0.,
        target[2] - actor.position[2],
    ];
    let length = distance::length(delta);
    if length < 0.5 || length.is_nan() {
        // 4DA50 does not write its output. Retain that uncertainty until a
        // detour needs it; close recovery may finish without any consumer.
        return DetourSide::Unproved;
    }
    let direction = distance::normalize(delta);
    let normal = [-direction[2] * 10., 0., direction[0] * 10.];
    let first = std::array::from_fn(|i| actor.position[i] + normal[i]);
    let second = std::array::from_fn(|i| actor.position[i] - normal[i]);
    if planar_length(first) < planar_length(second) {
        DetourSide::Left
    } else {
        DetourSide::Right
    }
}

/// Signed distance to the infinite line and its native left-facing normal.
/// The helper subtracts end from start before rotating, which matters at ties.
fn line_separation(point: [f32; 3], start: [f32; 3], end: [f32; 3]) -> Option<(f32, [f32; 3])> {
    let delta = [start[0] - end[0], 0., start[2] - end[2]];
    if f64::from(distance::length(delta)) <= 0.01 {
        return None;
    }
    let direction = distance::normalize(delta);
    let normal = [-direction[2], 0., direction[0]];
    Some((
        distance::dot(std::array::from_fn(|i| point[i] - start[i]), normal),
        normal,
    ))
}

fn planar_length(position: [f32; 3]) -> f32 {
    distance::length([position[0], 0., position[2]])
}

fn constrain(actor: &mut Actor, constrained: bool) -> Option<[f32; 3]> {
    if actor.movement.steering.unrestricted_arena || actor.movement.steering.leaving_arena {
        return None;
    }
    let radius = 850. + if actor.side == Side::Enemy { 35. } else { 0. };
    let length = planar_length(actor.position);
    if length <= radius {
        return None;
    }
    if !constrained && actor.position[1] <= 0.1 {
        // 4DAF4 normalizes all three axes even though the radius is planar.
        let inward = distance::normalize(actor.position.map(|v| -v));
        let translation = inward.map(|direction| direction * (length - radius));
        for (position, correction) in actor.position.iter_mut().zip(translation) {
            *position += correction;
        }
        actor.movement.steering.contact = ArenaContact::Slid;
        Some(translation)
    } else {
        actor.position[0] = actor.movement.previous_position[0];
        actor.position[2] = actor.movement.previous_position[2];
        actor.movement.steering.contact = ArenaContact::Blocked;
        None
    }
}

#[cfg(test)]
mod tests;
