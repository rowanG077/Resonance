//! The no-action automatic position branch of 32298 (23DE8 / 22ACC).
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
            actor.movement.steering.home_valid = false;
        }
    }
}

impl Battle {
    /// 31290 refreshes only party slot zero's formation anchor before moving.
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

    /// Sets the ordinary shared movement direction and returns input walk bit1.
    /// Policy retains the idle countdown, RNG draw and motion/speed selection.
    pub(crate) fn return_position(&mut self, owner: ActorId) -> Result<bool> {
        let index = owner.index();
        let actor = self.actor(owner)?;
        ensure!(
            actor.control == Control::Auto && actor.side == Side::Party,
            "return position requires an automatic companion"
        );
        if !actor.movement.steering.home_valid {
            let (home, outside) = self.formation_home(owner)?;
            let blocked = outside && self.return_position_blocked(index, home);
            let actor = &mut self.actors[index];
            actor.movement.steering.home_valid = outside && !blocked;
            actor.movement.steering.home = if actor.movement.steering.home_valid {
                home
            } else {
                actor.position
            };
        }
        let target = self
            .target(owner)
            .context("return position requires an actor target")?;
        let destination =
            self.steer_approach(owner, target, self.actors[index].movement.steering.home)?;
        let actor = &mut self.actors[index];
        // 4DC18 includes height, whereas the actual movement direction 4DA50
        // discards it and retains its previous value below the planar threshold.
        let distance =
            distance::length(std::array::from_fn(|i| destination[i] - actor.position[i]));
        if distance > 10. {
            actor.movement.direction = crate::distance::planar_direction(
                destination,
                actor.position,
                actor.movement.direction,
            );
            Ok(true)
        } else {
            actor.movement.steering.home_valid = false;
            Ok(false)
        }
    }

    pub(crate) fn formation_home(&self, owner: ActorId) -> Result<([f32; 3], bool)> {
        let actor = self.actor(owner)?;
        let leader = self
            .actors
            .iter()
            .find(|other| other.side == actor.side)
            .context("missing formation leader")?;
        // 23DE8 rotates the original offset by zero radians.
        let mut home: [f32; 3] = std::array::from_fn(|i| {
            leader.movement.steering.home[i] + actor.movement.steering.home_offset[i]
        });
        let radius = distance::length([home[0], 0., home[2]]);
        let outside = radius > 807.5;
        if outside {
            let inward = distance::planar_direction([0.; 3], home, [0.; 3]);
            let excess = (radius - 807.5).abs();
            for i in 0..3 {
                home[i] += inward[i] * excess;
            }
        }
        Ok((home, outside))
    }

    fn return_position_blocked(&self, owner: usize, home: [f32; 3]) -> bool {
        self.actors.iter().enumerate().any(|(index, actor)| {
            index != owner
                && actor.available()
                && self.approach_blocks_home(index)
                && self.recovery_return_blocks_home(index)
                && distance::length([home[0] - actor.position[0], 0., home[2] - actor.position[2]])
                    < 100_f32.mul_add(f32::from(actor.movement.steering.clearance_category), 100.)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{actor, prepared};

    fn battle() -> Battle {
        let mut leader = actor(Side::Party);
        leader.position = [-200., 0., 500.];
        let mut owner = actor(Side::Party);
        owner.control = Control::Auto;
        owner.position = [-600., 0., 0.];
        let mut enemy = actor(Side::Enemy);
        enemy.position = [300., 0., 500.];
        Battle::new(prepared("pub task run() {}", vec![leader, owner, enemy], 1))
    }

    #[test]
    fn inside_formation_radius_keeps_current_position_without_rng() {
        let mut battle = battle();
        let position = battle.actors[1].position;
        let random = battle.random;
        assert_eq!(
            battle.actors[1].movement.steering.home_offset,
            [-400., 0., -500.]
        );
        assert!(!battle.return_position(ActorId(1)).unwrap());
        assert_eq!(battle.actors[1].movement.steering.home, position);
        assert!(!battle.actors[1].movement.steering.home_valid);
        assert_eq!(battle.random.state(), random.state());
    }

    #[test]
    fn outside_formation_destination_is_clamped_and_retained_while_walking() {
        let mut battle = battle();
        battle.actors[0].movement.steering.home = [-900., 0., 500.];
        assert!(battle.return_position(ActorId(1)).unwrap());
        let home = battle.actors[1].movement.steering.home;
        assert!((home[0] + 807.5).abs() < 0.001);
        assert_eq!(home[1..], [0., 0.]);
        assert!(battle.actors[1].movement.steering.home_valid);
        assert!(battle.actors[1].movement.direction[0] < -0.999);
        battle.actors[0].movement.steering.home = [100., 0., 500.];
        assert!(battle.return_position(ActorId(1)).unwrap());
        assert_eq!(battle.actors[1].movement.steering.home, home);
    }

    #[test]
    fn occupied_clamped_destination_uses_profile_clearance_and_availability() {
        let mut battle = battle();
        battle.actors[0].movement.steering.home = [-900., 0., 500.];
        battle.actors[2].position = [-807.5, 0., 199.];
        battle.actors[2].movement.steering.clearance_category = 1;
        assert!(!battle.return_position(ActorId(1)).unwrap());
        assert!(!battle.actors[1].movement.steering.home_valid);
        battle.actors[2].availability = crate::ActorAvailability::Dead;
        assert!(battle.return_position(ActorId(1)).unwrap());
    }

    #[test]
    fn arrival_uses_full_distance_but_keeps_existing_planar_direction() {
        let mut battle = battle();
        battle.actors[1].movement.steering.home_valid = true;
        battle.actors[1].movement.steering.home = [-610., 0., 0.];
        assert!(!battle.return_position(ActorId(1)).unwrap());
        battle.actors[1].movement.steering.home_valid = true;
        battle.actors[1].movement.steering.home = [-600., 11., 0.];
        battle.actors[1].movement.direction = [0., 0., 1.];
        assert!(battle.return_position(ActorId(1)).unwrap());
        assert_eq!(battle.actors[1].movement.direction, [0., 0., 1.]);
    }
}
