//! Authored screen geometry for the field-image shatter at battle entry.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const SHATTER_SOUND: u16 = 130;
pub const POINT_COUNT: usize = 43;
pub const TRIANGLE_COUNT: usize = 62;
pub const VERTEX_COUNT: usize = TRIANGLE_COUNT * 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntranceRecipe {
    pub points: Vec<[f32; 2]>,
    pub triangles: Vec<[u8; 3]>,
    pub native_size: [f32; 2],
    pub center_weight: f32,
    pub initial_expansion: f32,
    pub outward_speed: f32,
    pub angular_speed: f32,
    pub angular_step: f32,
    pub angular_choices: u16,
    pub radians_per_degree: f32,
    pub z_rotation_scale: f32,
}

impl EntranceRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.points.len() == POINT_COUNT
                && self.triangles.len() == TRIANGLE_COUNT
                && self.points.iter().flatten().all(|v| v.is_finite())
                && self.native_size.iter().all(|v| v.is_finite() && *v > 0.)
                && [
                    self.center_weight,
                    self.outward_speed,
                    self.angular_speed,
                    self.radians_per_degree,
                    self.z_rotation_scale
                ]
                .iter()
                .all(|v| v.is_finite() && *v > 0.)
                && [self.initial_expansion, self.angular_step]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.)
                && self.angular_choices > 0,
            "invalid battle entrance geometry or motion"
        );
        for &[a, b, c] in &self.triangles {
            ensure!(
                [a, b, c]
                    .iter()
                    .all(|&i| usize::from(i) < self.points.len())
                    && a != b
                    && b != c
                    && a != c,
                "invalid battle entrance triangle"
            );
            let [a, b, c] = [a, b, c].map(|i| self.points[usize::from(i)]);
            ensure!(
                (b[0] - a[0]) * (c[1] - a[1]) != (b[1] - a[1]) * (c[0] - a[0]),
                "degenerate battle entrance triangle"
            );
        }
        Ok(())
    }
}
