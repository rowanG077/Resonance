//! Authored bone chains for hair and cloth; model behavior is prepared at runtime.
mod profiles;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Definition {
    /// Authored model identity, used by native character behavior.
    pub model: String,
    pub chains: Vec<Chain>,
}

impl Definition {
    /// Bind model policies to skeleton names without changing the authored chains.
    pub fn prepare(&self, names: &[String]) -> Result<Vec<Chain>> {
        profiles::prepare(self, names)
    }

    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }
}

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
