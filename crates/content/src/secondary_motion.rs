//! Cooked bone chains for hair and cloth, ready for the shared native solver.
mod simulation;
pub use simulation::{Environment, Simulation, UpAxis};

use anyhow::{Result, ensure};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

mod document;
pub use document::{ChainDefinition, Definition, Parameter, PlaneDefinition};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    /// Ordered skeleton joints, including the terminal guide joint.
    pub joints: Vec<Joint>,
    pub attraction: f32,
    pub preserve_rotation: bool,
    pub rotation_locks: [bool; 2],
    pub collision_plane: Option<CollisionPlane>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joint {
    pub node: u16,
    pub gravity: f32,
    /// Signed velocity feedback; negative values reverse the previous displacement.
    pub damping: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionPlane {
    /// Rotation comes from this animated bone; the chain root supplies position.
    pub anchor: u16,
    pub normal: [f32; 3],
    pub offset: f32,
    pub strength: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SegmentRotation(Quat);

impl SegmentRotation {
    pub fn orientation(self) -> Quat {
        self.0
    }

    pub fn update(&mut self, chain: &Chain, authored: Vec3, simulated: Vec3) -> Quat {
        self.0 = chain.rotation(self.0, authored, simulated);
        self.0
    }
}

/// Shortest rotation between unit directions, including an exactly reversed segment.
fn swing(from: Vec3, to: Vec3) -> (Quat, f32) {
    let cross = from.cross(to);
    let dot = from.dot(to);
    if let Some(axis) = cross.try_normalize() {
        let angle = cross.length().atan2(dot);
        (Quat::from_axis_angle(axis, angle), angle)
    } else if dot < 0. {
        let angle = std::f32::consts::PI;
        (
            Quat::from_axis_angle(from.any_orthonormal_vector(), angle),
            angle,
        )
    } else {
        (Quat::IDENTITY, 0.)
    }
}

impl Chain {
    /// Rigid correction between segment directions in actor coordinates. Taking
    /// the rotation directly avoids the azimuth jump at an Euler pitch pole.
    fn rotation(&self, previous: Quat, authored: Vec3, simulated: Vec3) -> Quat {
        if self.preserve_rotation {
            return Quat::IDENTITY;
        }
        let (Some(from), Some(to)) = (authored.try_normalize(), simulated.try_normalize()) else {
            return previous;
        };
        let axis = match self.rotation_locks {
            [false, false] => {
                // Transport the previous basis by the segment's actual change.
                // Choosing a fresh shortest arc from the rest pose loses twist
                // whenever the segment crosses its antipode.
                let direction = previous * from;
                if direction.abs_diff_eq(to, 1e-6) {
                    return previous;
                }
                let (direct, _) = swing(from, to);
                if previous == Quat::IDENTITY {
                    return direct;
                }
                let (step, travel) = swing(direction, to);
                let transported = (step * previous).normalize();
                // Recover the authored twist as the segment moves away from
                // reversal. A closed loop can otherwise leave a permanent roll.
                let recovery = travel * (1. + from.dot(to)).max(0.);
                return transported.rotate_towards(direct, recovery).normalize();
            }
            [false, true] => Vec3::X,
            [true, false] => Vec3::Y,
            [true, true] => return Quat::IDENTITY,
        };
        let from = from - axis * from.dot(axis);
        let to = to - axis * to.dot(axis);
        if from.length_squared() < 1e-12 || to.length_squared() < 1e-12 {
            return previous;
        }
        Quat::from_axis_angle(axis, axis.dot(from.cross(to)).atan2(from.dot(to)))
    }

    pub fn validate(&self, bone_count: usize) -> Result<()> {
        ensure!(
            (2..=128).contains(&self.joints.len()),
            "invalid secondary chain length"
        );
        ensure!(
            self.attraction.is_finite() && (0. ..=1.).contains(&self.attraction),
            "invalid chain attraction"
        );
        let mut seen = std::collections::BTreeSet::new();
        for joint in &self.joints {
            ensure!(
                usize::from(joint.node) < bone_count && seen.insert(joint.node),
                "invalid secondary chain joint"
            );
            ensure!(
                joint.gravity.is_finite()
                    && joint.gravity.abs() <= 100.
                    && joint.damping.is_finite()
                    && (-2. ..=2.).contains(&joint.damping),
                "invalid secondary chain dynamics"
            );
        }
        if let Some(plane) = &self.collision_plane {
            ensure!(
                usize::from(plane.anchor) < bone_count
                    && plane.normal.iter().all(|v| v.is_finite())
                    && (plane.normal.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.001,
                "invalid chain collision plane"
            );
            ensure!(
                plane.offset.is_finite()
                    && plane.offset.abs() <= 100.
                    && plane.strength.is_finite()
                    && (0. ..=1.).contains(&plane.strength),
                "invalid chain collision response"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_rotation_crosses_the_pitch_pole_without_flipping_and_respects_locks() {
        let mut chain = Chain {
            joints: vec![],
            attraction: 0.,
            preserve_rotation: false,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        // Lloyd's cape crosses actor-local -Y: less than seven degrees of
        // segment motion used to produce a 176-degree rotation of the cloth.
        let directions = [
            Vec3::new(0.0452571, -9.982_372, -0.0526428),
            Vec3::new(-0.7683182, -9.786_379, 0.7983093),
        ];
        let rotations = directions.map(|to| chain.rotation(Quat::IDENTITY, Vec3::Z, to));
        assert!(rotations[0].angle_between(rotations[1]) < 0.2);
        for direction in directions.into_iter().chain([Vec3::Z, -Vec3::Z]) {
            assert!(
                (chain.rotation(Quat::IDENTITY, Vec3::Z, direction) * Vec3::Z)
                    .abs_diff_eq(direction.normalize(), 1e-5)
            );
        }
        assert_eq!(
            chain.rotation(Quat::IDENTITY, Vec3::ZERO, Vec3::Z),
            Quat::IDENTITY
        );
        assert_eq!(
            chain.rotation(Quat::IDENTITY, Vec3::Z, Vec3::ZERO),
            Quat::IDENTITY
        );
        let mut previous = Quat::IDENTITY;
        for x in [-0.02, -0.01, 0., 0.01, 0.02] {
            let direction = Vec3::new(x, 0.01, -1.).normalize();
            let rotation = chain.rotation(previous, Vec3::Z, direction);
            assert!((rotation * Vec3::Z).abs_diff_eq(direction, 1e-5));
            if x > -0.02 {
                assert!(rotation.angle_between(previous) < 0.011);
            }
            previous = rotation;
        }
        for to in [Vec3::X, Vec3::Y, Vec3::Z] {
            previous = chain.rotation(previous, Vec3::Z, to);
        }
        assert!(previous.angle_between(Quat::IDENTITY) < 1e-4);
        for (locks, axis) in [([false, true], Vec3::X), ([true, false], Vec3::Y)] {
            chain.rotation_locks = locks;
            let expected = Quat::from_axis_angle(axis, 0.7);
            let rotation = chain.rotation(Quat::IDENTITY, Vec3::Z, expected * Vec3::Z + axis * 0.4);
            assert!(rotation.abs_diff_eq(expected, 1e-5));
            assert!((rotation * axis).abs_diff_eq(axis, 1e-5));
        }
        chain.rotation_locks = [true; 2];
        assert_eq!(
            chain.rotation(Quat::IDENTITY, Vec3::Z, Vec3::X),
            Quat::IDENTITY
        );
        chain.rotation_locks = [false; 2];
        chain.preserve_rotation = true;
        assert_eq!(
            chain.rotation(Quat::IDENTITY, Vec3::Z, Vec3::X),
            Quat::IDENTITY
        );
    }
}
