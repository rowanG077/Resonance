//! Retained secondary-chain dynamics shared by field and battle models.
use super::Chain;
use glam::Vec3;

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
    /// Original model-local acceleration, or global wind when that is zero.
    pub acceleration: Vec3,
    /// Native floor clamp, applied after the chain's collision callback.
    pub floor: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct Simulation {
    positions: Vec<Vec3>,
    previous: Vec<Vec3>,
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
        self.previous = targets.to_vec();
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
            // 80069088 seeds only non-root joints from the first authored pose.
            // The new root still starts at zero when the recovery-radius test
            // runs; distant models therefore reset the whole chain this visit.
            if let Some(root) = self.positions.first_mut() {
                *root = Vec3::ZERO;
                self.previous[0] = Vec3::ZERO;
            }
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
            for (index, joint) in chain.joints.iter().enumerate() {
                let position = self.positions[index];
                self.positions[index] += (targets[index] - position) * attraction;
                self.positions[index][environment.up.index()] -= 0.98 * joint.gravity;
            }
            // Attraction can pull a large displacement within the chain's
            // recovery radius. Test afterwards, before applying momentum.
            if self.positions[0].distance(targets[0]) > 100. {
                self.reset(&targets);
            } else {
                for (position, velocity) in self.positions.iter_mut().zip(&mut self.velocity) {
                    *velocity += environment.acceleration;
                    *position += *velocity;
                }
            }
            let lengths: Vec<_> = targets.windows(2).map(|p| p[0].distance(p[1])).collect();
            for _ in 0..10 {
                self.positions[0] = targets[0];
                self.previous[0] = targets[0];
                for (index, length) in lengths.iter().enumerate() {
                    let delta = self.positions[index + 1] - self.positions[index];
                    let distance = delta.length();
                    if distance > 1e-6 {
                        let correction = delta * (0.45 * (length - distance) / distance);
                        self.positions[index] -= correction;
                        self.positions[index + 1] += correction;
                    }
                }
            }
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
                self.velocity[index] =
                    (self.positions[index] - self.previous[index]) * joint.damping;
                self.previous[index] = self.positions[index];
            }
        }
        self.targets = targets.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secondary_motion::Joint;

    #[test]
    fn floor_follows_projection_and_pause_preserves_momentum_in_both_coordinate_systems() {
        let chain = Chain {
            joints: (0..3)
                .map(|node| Joint {
                    node,
                    gravity: 0.,
                    damping: 1.,
                })
                .collect(),
            attraction: 0.,
            preserve_rotation: false,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        for (up, direction) in [(UpAxis::Y, Vec3::Y), (UpAxis::Z, Vec3::Z)] {
            let targets = [direction * 20., direction * 30., direction * 40.];
            let mut simulation = Simulation::default();
            let environment = Environment {
                up,
                acceleration: Vec3::X,
                floor: Some(25.),
            };
            simulation.advance(
                &chain,
                &targets,
                Some((-direction, 0., 1.)),
                0.,
                1,
                environment,
            );
            assert!(simulation.positions[0][up.index()] < 25.);
            for (index, target) in targets.iter().enumerate().skip(1) {
                assert_eq!(simulation.positions[index][up.index()], 25.);
                assert_eq!(
                    simulation.velocity[index][up.index()],
                    25. - target[up.index()]
                );
            }
            assert!(simulation.positions[2].x > 0.);
            let before = simulation.clone();
            simulation.advance(&chain, &targets, None, 0., 0, environment);
            assert_eq!(simulation.positions, before.positions);
            assert_eq!(simulation.velocity, before.velocity);
        }
    }

    #[test]
    fn retained_hair_state_matches_original_dolphin_visits() -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Sample {
            tick: u32,
            flags: u32,
            blend_bits: u32,
            chain_flags: u8,
            before: Vec<[u32; 16]>,
            after: Vec<[u32; 16]>,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            samples: Vec<Sample>,
        }
        let fixture: Fixture = serde_json::from_str(include_str!("lloyd-hair.json"))?;
        let vector = |row: &[u32; 16], offset: usize| {
            Vec3::new(
                f32::from_bits(row[offset]),
                f32::from_bits(row[offset + 1]),
                f32::from_bits(row[offset + 2]),
            )
        };
        for continuous in [false, true] {
            let mut maximum = 0f32;
            let mut retained = None;
            let mut previous_tick = None;
            for sample in &fixture.samples {
                if let Some(tick) = previous_tick {
                    assert_eq!(sample.tick, tick + 1);
                }
                previous_tick = Some(sample.tick);
                let chain = Chain {
                    joints: sample
                        .after
                        .iter()
                        .map(|row| Joint {
                            node: row[15] as u16,
                            gravity: f32::from_bits(row[13]),
                            damping: f32::from_bits(row[14]),
                        })
                        .collect(),
                    attraction: f32::from_bits(sample.blend_bits),
                    preserve_rotation: true,
                    rotation_locks: [false; 2],
                    collision_plane: None,
                };
                let initial = Simulation {
                    positions: sample.before.iter().map(|r| vector(r, 0)).collect(),
                    previous: sample.before.iter().map(|r| vector(r, 3)).collect(),
                    targets: sample.before.iter().map(|r| vector(r, 6)).collect(),
                    velocity: sample.before.iter().map(|r| vector(r, 9)).collect(),
                };
                let simulation = if continuous {
                    retained.get_or_insert(initial)
                } else {
                    retained.insert(initial)
                };
                let targets: Vec<_> = sample.after.iter().map(|r| vector(r, 6)).collect();
                simulation.advance(
                    &chain,
                    &targets,
                    None,
                    if sample.chain_flags & 0x20 != 0 {
                        0.2
                    } else {
                        chain.attraction
                    },
                    sample.flags & 1,
                    Environment {
                        up: UpAxis::Y,
                        floor: Some(5.),
                        acceleration: Vec3::ZERO,
                    },
                );
                for (index, expected) in sample.after.iter().enumerate() {
                    for (actual, offset) in [
                        (simulation.positions[index], 0),
                        (simulation.previous[index], 3),
                        (simulation.velocity[index], 9),
                    ] {
                        let delta = (actual - vector(expected, offset)).abs().max_element();
                        maximum = maximum.max(delta);
                        assert!(
                            delta < 0.001,
                            "tick {} joint {index} field {offset}: {delta}",
                            sample.tick
                        );
                    }
                }
            }
            eprintln!("maximum hair state error (continuous={continuous}): {maximum}");
        }
        Ok(())
    }

    #[test]
    fn prepared_hair_and_coat_dynamics_match_dolphin() -> anyhow::Result<()> {
        #[derive(serde::Deserialize)]
        struct Sample {
            tick: u32,
            flags: Vec<u8>,
            before: Vec<Vec<[u32; 16]>>,
            after: Vec<Vec<[u32; 16]>>,
            matrices_before: Vec<[u32; 12]>,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            definition: crate::secondary_motion::Definition,
            names: Vec<String>,
            samples: Vec<Sample>,
        }
        let fixture: Fixture = serde_json::from_str(include_str!("lloyd-secondary.json"))?;
        let chains = fixture.definition.prepare(&fixture.names)?;
        let vector = |row: &[u32; 16], offset: usize| {
            Vec3::new(
                f32::from_bits(row[offset]),
                f32::from_bits(row[offset + 1]),
                f32::from_bits(row[offset + 2]),
            )
        };
        let mut maximum = 0f32;
        for sample in fixture.samples {
            for (index, chain) in chains.iter().enumerate() {
                let before = &sample.before[index];
                let after = &sample.after[index];
                assert_eq!(chain.joints.len(), before.len());
                for (joint, native) in chain.joints.iter().zip(before) {
                    assert_eq!(u32::from(joint.node), native[15]);
                    assert_eq!(joint.gravity.to_bits(), native[13]);
                    assert_eq!(joint.damping.to_bits(), native[14]);
                }
                let mut simulation = Simulation {
                    positions: before.iter().map(|r| vector(r, 0)).collect(),
                    previous: before.iter().map(|r| vector(r, 3)).collect(),
                    targets: before.iter().map(|r| vector(r, 6)).collect(),
                    velocity: before.iter().map(|r| vector(r, 9)).collect(),
                };
                let plane = chain.collision_plane.as_ref().map(|plane| {
                    let m = sample.matrices_before[usize::from(plane.anchor)].map(f32::from_bits);
                    let transform = |v: Vec3| {
                        Vec3::new(
                            m[0] * v.x + m[1] * v.y + m[2] * v.z,
                            m[4] * v.x + m[5] * v.y + m[6] * v.z,
                            m[8] * v.x + m[9] * v.y + m[10] * v.z,
                        )
                    };
                    let normal = Vec3::from_array(plane.normal);
                    let tangent = normal.any_orthonormal_vector();
                    let normal = transform(tangent)
                        .cross(transform(normal.cross(tangent)))
                        .normalize();
                    (normal, plane.offset, plane.strength)
                });
                let targets: Vec<_> = after.iter().map(|r| vector(r, 6)).collect();
                simulation.advance(
                    chain,
                    &targets,
                    plane,
                    if sample.flags[index] & 0x20 != 0 {
                        0.2
                    } else {
                        chain.attraction
                    },
                    1,
                    Environment {
                        up: UpAxis::Y,
                        acceleration: Vec3::ZERO,
                        floor: Some(5.),
                    },
                );
                for (joint, expected) in after.iter().enumerate() {
                    for (actual, offset) in [
                        (simulation.positions[joint], 0),
                        (simulation.previous[joint], 3),
                        (simulation.velocity[joint], 9),
                    ] {
                        let delta = (actual - vector(expected, offset)).abs().max_element();
                        maximum = maximum.max(delta);
                        assert!(
                            delta < 0.001,
                            "tick {} chain {index} joint {joint} field {offset}: {delta}",
                            sample.tick
                        );
                    }
                }
            }
        }
        eprintln!("maximum hair/coat state error: {maximum}");
        Ok(())
    }
}
