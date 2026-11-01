use super::*;
use anyhow::Context;

pub(crate) fn initialize(actors: &mut [Actor]) {
    for side in [Side::Party, Side::Enemy] {
        let Some(origin) = actors
            .iter()
            .find(|actor| actor.side == side)
            .map(|actor| actor.position)
        else {
            continue;
        };
        for actor in actors.iter_mut().filter(|actor| actor.side == side) {
            actor.movement.steering.home = actor.position;
            actor.movement.steering.home_offset =
                std::array::from_fn(|i| actor.position[i] - origin[i]);
        }
    }
}

impl Battle {
    pub(crate) fn remember_idle_home(&mut self, index: usize) {
        if self
            .actors
            .iter()
            .position(|actor| actor.side == Side::Party)
            == Some(index)
        {
            self.actors[index].movement.steering.home = self.actors[index].position;
        }
    }

    /// Choose a clear formation position; the companion policy owns walk speed and animation.
    pub(crate) fn return_position(&mut self, owner: ActorId) -> Result<bool> {
        let actor = self.actor(owner)?;
        ensure!(
            actor.control == Control::Auto && actor.side == Side::Party,
            "return position requires an automatic companion"
        );
        // Idle companions keep their current spot when the formation point is
        // inside the allowed radius. Post-action return uses formation_home directly.
        let requested = self.requested_formation_home(owner)?;
        let limits = self
            .companion_policy(owner)
            .map(|policy| policy.positioning);
        if limits.is_some_and(|row| planar_length(requested) <= row.formation_radius) {
            self.actors[owner.index()].movement.steering.home = actor.position;
            return Ok(false);
        }
        let home = self.formation_home(owner)?;
        if let Some(limits) = limits
            && self.actors.iter().enumerate().any(|(index, other)| {
                index != owner.index()
                    && other.available()
                    && self.activity(ActorId(index as u8)) != crate::Activity::Approaching
                    && planar_length([home[0] - other.position[0], 0., home[2] - other.position[2]])
                        < limits.formation_occupancy_unit
                            * (f32::from(other.movement.steering.positioning_radius) + 1.)
            })
        {
            self.actors[owner.index()].movement.steering.home = self.actors[owner.index()].position;
            return Ok(false);
        }
        let destination = self.steer_to(owner, home, None)?;
        let arrival = limits.map_or(ARRIVAL_DISTANCE, |row| row.idle_arrival_distance);
        let actor = &mut self.actors[owner.index()];
        actor.movement.steering.home = home;
        if planar_length([home[0] - actor.position[0], 0., home[2] - actor.position[2]]) <= arrival
        {
            return Ok(false);
        }
        actor.movement.direction =
            distance::planar_direction(destination, actor.position, actor.movement.direction);
        Ok(true)
    }

    pub(crate) fn formation_home(&self, owner: ActorId) -> Result<[f32; 3]> {
        let actor = self.actor(owner)?;
        let home = self.requested_formation_home(owner)?;
        let position = match self
            .companion_policy(owner)
            .map(|policy| policy.positioning)
        {
            Some(row) => {
                let mut point = [home[0], 0., home[2]];
                let length = planar_length(point);
                if length > row.formation_radius {
                    point = point.map(|value| value * row.formation_radius / length);
                }
                Some(point)
            }
            None => recovery_position::select(&self.actors, owner, home),
        };
        Ok(position.unwrap_or(actor.position))
    }

    fn requested_formation_home(&self, owner: ActorId) -> Result<[f32; 3]> {
        let actor = self.actor(owner)?;
        let leader = self
            .actors
            .iter()
            .find(|other| other.side == actor.side)
            .context("missing formation leader")?;
        Ok(std::array::from_fn(|i| {
            leader.movement.steering.home[i] + actor.movement.steering.home_offset[i]
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{actor, prepared};

    fn battle() -> Battle {
        let mut leader = actor(Side::Party);
        leader.position = [-200., 0., 200.];
        let mut owner = actor(Side::Party);
        owner.control = Control::Auto;
        owner.position = [-400., 0., 0.];
        let mut enemy = actor(Side::Enemy);
        enemy.position = [500., 0., 0.];
        prepared(vec![leader, owner, enemy], 1).finish().unwrap()
    }

    #[test]
    fn formation_offset_follows_the_leaders_idle_home() -> Result<()> {
        let mut battle = battle();
        battle.actors[0].position = [200., 0., 300.];
        battle.remember_idle_home(0);
        assert_eq!(battle.formation_home(ActorId(1))?, [0., 0., 100.]);
        assert!(battle.return_position(ActorId(1))?);
        assert!(battle.actors[1].movement.direction[0] > 0.);
        battle.actors[1].position = [0., 0., 100.];
        assert!(!battle.return_position(ActorId(1))?);
        Ok(())
    }

    #[test]
    fn formation_destination_stays_inside_arena_without_changing_the_offset() -> Result<()> {
        let mut battle = battle();
        let offset = battle.actors[1].movement.steering.home_offset;
        battle.actors[0].movement.steering.home = [-1000., 0., 500.];
        let home = battle.formation_home(ActorId(1))?;
        assert!(planar_length(home) < ARENA_RADIUS);
        assert_eq!(battle.actors[1].movement.steering.home_offset, offset);
        assert!(battle.return_position(ActorId(1))?);
        Ok(())
    }
}
