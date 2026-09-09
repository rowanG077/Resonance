//! Cooked bone chains for hair and cloth. No source node-name parsing at runtime.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

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

impl Chain {
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
                    && (0. ..=2.).contains(&joint.damping),
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
