//! Retained secondary-chain dynamics shared by field and battle models.
use super::Chain;
use glam::Vec3;

// Fixed-tick solver tuning in model units.
const GRAVITY_PER_TICK: f32 = 0.98;
const RECOVERY_DISTANCE: f32 = 100.;
const CONSTRAINT_PASSES: usize = 10;
const CONSTRAINT_RELAXATION: f32 = 0.45;

#[derive(Debug, Clone, Copy, Default)]
pub enum UpAxis {
    Y,
    #[default]
    Z,
}
impl UpAxis {
    fn index(self) -> usize {
        match self {
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Environment {
    pub up: UpAxis,
    /// Acceleration in simulation coordinates, such as wind.
    pub acceleration: Vec3,
    /// Minimum height for free joints, applied after body-plane collision.
    pub floor: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct Simulation {
    positions: Vec<Vec3>,
    velocity: Vec<Vec3>,
    targets: Vec<Vec3>,
}
impl Simulation {
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }
    pub fn targets(&self) -> &[Vec3] {
        &self.targets
    }
    pub fn velocity(&self) -> &[Vec3] {
        &self.velocity
    }

    pub fn reset(&mut self, targets: &[Vec3]) {
        self.positions = targets.to_vec();
        self.velocity = vec![Vec3::ZERO; targets.len()];
        self.targets = targets.to_vec();
    }
    pub fn advance(
        &mut self,
        chain: &Chain,
        targets: &[Vec3],
        plane: Option<(Vec3, f32, f32)>,
        attraction: f32,
        steps: u32,
        environment: Environment,
    ) {
        if self.positions.len() != targets.len() {
            self.reset(targets);
        }
        let previous_targets = std::mem::take(&mut self.targets);
        for step in 0..steps {
            // Catch-up ticks consume interpolated authored targets; rendering
            // additional frames without a field tick does not advance physics.
            let t = (step + 1) as f32 / steps as f32;
            let targets: Vec<_> = previous_targets
                .iter()
                .zip(targets)
                .map(|(a, b)| a.lerp(*b, t))
                .collect();
            let mut previous = self.positions.clone();
            for (index, joint) in chain.joints.iter().enumerate() {
                let position = self.positions[index];
                self.positions[index] += (targets[index] - position) * attraction;
                self.positions[index][environment.up.index()] -= GRAVITY_PER_TICK * joint.gravity;
            }
            // Attraction can pull a large displacement within the chain's
            // recovery radius. Test afterwards, before applying momentum.
            if self.positions[0].distance(targets[0]) > RECOVERY_DISTANCE {
                self.reset(&targets);
                previous.clone_from(&targets);
            } else {
                for (position, velocity) in self.positions.iter_mut().zip(&mut self.velocity) {
                    *velocity += environment.acceleration;
                    *position += *velocity;
                }
            }
            let lengths: Vec<_> = targets.windows(2).map(|p| p[0].distance(p[1])).collect();
            for _ in 0..CONSTRAINT_PASSES {
                self.positions[0] = targets[0];
                for (index, length) in lengths.iter().enumerate() {
                    let delta = self.positions[index + 1] - self.positions[index];
                    let distance = delta.length();
                    if distance > 1e-6 {
                        let correction =
                            delta * (CONSTRAINT_RELAXATION * (length - distance) / distance);
                        self.positions[index] -= correction;
                        self.positions[index + 1] += correction;
                    }
                }
            }
            self.positions[0] = targets[0];
            if let Some((normal, offset, strength)) = plane {
                let root = self.positions[0];
                for position in self.positions.iter_mut().skip(1) {
                    let distance = (*position - root).dot(normal) - offset;
                    if distance < 0. {
                        *position -= normal * distance * strength;
                    }
                }
            }
            if let Some(floor) = environment.floor {
                for position in self.positions.iter_mut().skip(1) {
                    position[environment.up.index()] = position[environment.up.index()].max(floor);
                }
            }
            for (index, joint) in chain.joints.iter().enumerate() {
                self.velocity[index] = (self.positions[index] - previous[index]) * joint.damping;
            }
            self.velocity[0] = Vec3::ZERO;
        }
        self.targets = targets.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secondary_motion::Joint;

    fn chain() -> Chain {
        Chain {
            joints: (0..3)
                .map(|node| Joint {
                    node,
                    gravity: 0.3,
                    damping: 0.7,
                })
                .collect(),
            attraction: 0.03,
            preserve_rotation: false,
            rotation_locks: [false; 2],
            collision_plane: None,
        }
    }

    #[test]
    fn root_stays_attached_and_free_joints_respect_floor_in_both_coordinate_systems() {
        let chain = chain();
        for (up, direction) in [(UpAxis::Y, Vec3::Y), (UpAxis::Z, Vec3::Z)] {
            let targets = [direction * 20., direction * 30., direction * 40.];
            let mut simulation = Simulation::default();
            simulation.advance(
                &chain,
                &targets,
                Some((-direction, 0., 1.)),
                0.,
                1,
                Environment {
                    up,
                    acceleration: Vec3::X,
                    floor: Some(25.),
                },
            );
            assert_eq!(simulation.positions()[0], targets[0]);
            assert!(
                simulation.positions()[1..]
                    .iter()
                    .all(|p| p.is_finite() && p[up.index()] >= 25.)
            );
            assert!(simulation.velocity().iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn paused_draws_preserve_motion_and_catch_up_matches_individual_ticks() {
        let chain = chain();
        let targets = [
            Vec3::Z * 30.,
            Vec3::new(8., 0., 30.),
            Vec3::new(16., 0., 30.),
        ];
        let mut batched = Simulation::default();
        batched.reset(&targets);
        let mut individual = batched.clone();
        let moved = targets.map(|point| point + Vec3::X * 12.);
        batched.advance(
            &chain,
            &moved,
            None,
            chain.attraction,
            12,
            Environment::default(),
        );
        for tick in 1..=12 {
            let targets: Vec<_> = targets
                .iter()
                .map(|&point| point + Vec3::X * tick as f32)
                .collect();
            individual.advance(
                &chain,
                &targets,
                None,
                chain.attraction,
                1,
                Environment::default(),
            );
        }
        assert!(
            batched
                .positions()
                .iter()
                .zip(individual.positions())
                .all(|(a, b)| a.abs_diff_eq(*b, 0.001))
        );
        let before = batched.clone();
        for _ in 0..20 {
            batched.advance(
                &chain,
                &moved,
                None,
                chain.attraction,
                0,
                Environment::default(),
            );
        }
        assert_eq!(batched.positions(), before.positions());
        assert_eq!(batched.velocity(), before.velocity());
    }

    #[test]
    fn chains_stay_bounded_under_gravity_and_wind_and_reset_after_teleport() {
        let chain = chain();
        let targets = [
            Vec3::Z * 30.,
            Vec3::new(8., 0., 30.),
            Vec3::new(16., 0., 30.),
        ];
        let mut simulation = Simulation::default();
        simulation.advance(
            &chain,
            &targets,
            None,
            chain.attraction,
            500,
            Environment {
                acceleration: Vec3::Y * 0.05,
                ..Default::default()
            },
        );
        assert_eq!(simulation.positions()[0], targets[0]);
        assert!(
            simulation
                .positions()
                .iter()
                .all(|p| p.is_finite() && p.distance(targets[0]) < 32.)
        );
        assert!(simulation.positions()[2].z < targets[2].z);
        let moved = targets.map(|point| point + Vec3::X * 1000.);
        simulation.advance(
            &chain,
            &moved,
            None,
            chain.attraction,
            1,
            Environment::default(),
        );
        assert_eq!(simulation.positions(), moved);
        assert!(
            simulation
                .velocity()
                .iter()
                .all(|&velocity| velocity == Vec3::ZERO)
        );
    }
}
