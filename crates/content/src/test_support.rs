//! Geometry fixtures for behavior tests in dependent crates.
use crate::field::{CollisionGroup, ModelCollision};
use std::sync::Arc;

pub fn cuboid(low: [f32; 3], high: [f32; 3]) -> CollisionGroup {
    CollisionGroup {
        surface: 0,
        vertices: (0..8)
            .map(|corner| {
                std::array::from_fn(|axis| {
                    if corner & (1 << axis) == 0 {
                        low[axis]
                    } else {
                        high[axis]
                    }
                })
            })
            .collect(),
        triangles: vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
        ],
    }
}

pub fn solid_box(low: [f32; 3], high: [f32; 3]) -> Arc<ModelCollision> {
    Arc::new(ModelCollision {
        solids: vec![cuboid(low, high)],
        floors: Vec::new(),
    })
}
