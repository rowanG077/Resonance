//! World view pitch and terrain clearance.
use super::{Position, collision::Terrain, travel::Travel};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Camera {
    angle: f32,
    velocity: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            angle: 120f32.to_radians(),
            velocity: 0.,
        }
    }
}
impl Camera {
    pub fn angle(&self) -> f32 {
        self.angle
    }
    pub fn lead(&self, alternate: bool) -> f32 {
        lead(self.angle, alternate)
    }

    pub(super) fn step(&mut self, travel: &Travel, terrain: &Terrain) -> Result<()> {
        let state = travel.state();
        let mount = travel.displayed_mount();
        let direction = [state.camera_yaw.sin(), state.camera_yaw.cos()];
        let radius = if mount == super::travel::Mount::Noishe {
            1300.
        } else {
            800.
        };
        let height = if mount.airborne() {
            state.altitude - 100.
        } else if mount == super::travel::Mount::Ship {
            157.
        } else {
            state.position.map()[2] + 85.
        };
        let center =
            Position::from_map([state.position.map()[0], state.position.map()[1], height])?
                .translated([-radius * direction[0], -radius * direction[1], 0.])?;
        let query = terrain.camera_query(center, radius)?;
        let bound = |angle| -> Result<f32> {
            let offset = lead(angle, state.alternate_perspective);
            let origin =
                state
                    .position
                    .translated([-offset * direction[0], -offset * direction[1], 0.])?;
            let [x, y] = center.displacement_to(origin);
            Ok(query.camera_angle_bound([x, y, state.altitude - height]))
        };
        let best = bound(self.angle)?;
        let change = (0.05 * (best - self.angle).abs()).min(self.velocity + 0.0005);
        let next = if best < self.angle {
            (self.angle - change).max(120f32.to_radians())
        } else {
            let next = (self.angle + change).min(170f32.to_radians());
            if bound(next)? >= next {
                next
            } else {
                self.angle
            }
        };
        self.velocity = (self.angle - next).abs();
        self.angle = next;
        Ok(())
    }
}
fn lead(angle: f32, alternate: bool) -> f32 {
    if alternate {
        0.
    } else {
        400. * (angle - 120f32.to_radians()) / 50f32.to_radians()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::{
        TileCoordinate,
        collision::{
            self,
            tests::{rectangle, tables},
        },
        travel::{
            Mount,
            tests::{parameters, state, terrain},
        },
    };

    #[test]
    fn open_terrain_settles_with_acceleration_and_alternate_view_has_no_lead() -> Result<()> {
        let travel = Travel::new(state(Mount::Foot), parameters())?;
        let terrain = terrain(1);
        let mut camera = Camera::default();
        let initial = camera.angle();
        camera.step(&travel, &terrain)?;
        assert!((camera.angle() - initial - 0.0005).abs() < 1e-6);
        for _ in 0..300 {
            camera.step(&travel, &terrain)?;
        }
        assert!((camera.angle().to_degrees() - 170.).abs() < 0.001);
        assert!((camera.lead(false) - 400.).abs() < 0.01);
        assert_eq!(camera.lead(true), 0.);
        Ok(())
    }

    #[test]
    fn nearby_high_geometry_raises_the_view_and_failure_keeps_camera_state() -> Result<()> {
        let travel = Travel::new(state(Mount::Foot), parameters())?;
        let open = terrain(1);
        let mut camera = Camera::default();
        for _ in 0..300 {
            camera.step(&travel, &open)?;
        }
        let high = collision::Terrain::new(
            [(
                TileCoordinate::new(0, 0)?,
                collision::Mesh::new(&[rectangle(0, [-100., -900.], [100., -700.], 1000.)])?,
            )],
            tables(),
        )?;
        for _ in 0..300 {
            camera.step(&travel, &high)?;
        }
        assert!(camera.angle().to_degrees() < 135.);
        let mut edge = state(Mount::Foot);
        edge.position = Position::from_map([0., 0., 0.])?;
        let edge = Travel::new(edge, parameters())?;
        let before = camera.angle;
        assert!(camera.step(&edge, &high).is_err());
        assert_eq!(camera.angle, before);
        Ok(())
    }
}
