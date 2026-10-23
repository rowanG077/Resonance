//! World view pitch and terrain clearance.
use super::{Position, collision::Terrain, travel::Travel};
use anyhow::Result;

pub(super) const MINIMUM_ANGLE: f32 = 120f32.to_radians();

#[derive(Debug, Clone)]
pub struct Camera {
    angle: f32,
    velocity: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            angle: MINIMUM_ANGLE,
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

    pub(super) fn step(
        &mut self,
        travel: &Travel,
        terrain: &Terrain,
        landmark_bound: Option<f32>,
    ) -> Result<()> {
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
            Ok(landmark_bound
                .unwrap_or_else(|| query.camera_angle_bound([x, y, state.altitude - height])))
        };
        let best = bound(self.angle)?;
        let change = (0.05 * (best - self.angle).abs()).min(self.velocity + 0.0005);
        let next = if best < self.angle {
            (self.angle - change).max(MINIMUM_ANGLE)
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
        400. * (angle - MINIMUM_ANGLE) / 50f32.to_radians()
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
        camera.step(&travel, &terrain, None)?;
        assert!((camera.angle() - initial - 0.0005).abs() < 1e-6);
        for _ in 0..300 {
            camera.step(&travel, &terrain, None)?;
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
            camera.step(&travel, &open, None)?;
        }
        let high = collision::Terrain::new(
            [(
                TileCoordinate::new(0, 0)?,
                collision::Mesh::new(&[rectangle(0, [-100., -900.], [100., -700.], 1000.)])?,
            )],
            tables(),
        )?;
        for _ in 0..300 {
            camera.step(&travel, &high, None)?;
        }
        assert!(camera.angle().to_degrees() < 135.);
        let mut edge = state(Mount::Foot);
        edge.position = Position::from_map([0., 0., 0.])?;
        let edge = Travel::new(edge, parameters())?;
        let before = camera.angle;
        assert!(camera.step(&edge, &high, None).is_err());
        assert_eq!(camera.angle, before);
        Ok(())
    }

    #[test]
    fn ship_camera_clears_floating_landmarks_without_affecting_flight() -> Result<()> {
        use crate::overworld::{World, landmarks::Locations};
        use resonance_content::overworld::{Interaction, ShipCameraClearance};
        use std::sync::Arc;

        let mut data = crate::overworld::landmarks::tests::definitions();
        let landmark = &mut data.worlds[1][9];
        landmark.position = [3200., 3200.];
        landmark.radius = 1000.;
        landmark.interaction = Interaction::Active;
        landmark.ship_camera_clearance = Some(ShipCameraClearance {
            near_margin: 300.,
            far_margin: 4250.,
            height: 885.,
        });
        let mut locations = Locations::new(
            Arc::new(data),
            Arc::default(),
            crate::overworld::scripts::fixture(),
        )?;
        let position = Position::from_map([3200., 3200., 0.])?;
        let near = locations.camera_angle_bound(World::TetheAlla, position, Mount::Ship);
        assert_eq!(near, Some(MINIMUM_ANGLE));
        let far = locations
            .camera_angle_bound(
                World::TetheAlla,
                position.translated([3000., 0., 0.])?,
                Mount::Ship,
            )
            .unwrap();
        assert!((far - (std::f32::consts::PI - 885f32.atan2(2000.))).abs() < 1e-6);
        assert_eq!(
            locations.camera_angle_bound(
                World::TetheAlla,
                position.translated([5251., 0., 0.])?,
                Mount::Ship,
            ),
            None
        );
        assert_eq!(
            locations.camera_angle_bound(World::TetheAlla, position, Mount::Rheairds),
            None
        );
        let mut camera = Camera {
            angle: 170f32.to_radians(),
            velocity: 0.,
        };
        let travel = Travel::new(state(Mount::Ship), parameters())?;
        let water = terrain(11);
        for _ in 0..300 {
            camera.step(&travel, &water, near)?;
        }
        assert!((camera.angle() - MINIMUM_ANGLE).abs() < 0.001);
        locations.appearances.get_mut(&266).unwrap().interaction = Interaction::Disabled;
        assert_eq!(
            locations.camera_angle_bound(World::TetheAlla, position, Mount::Ship),
            None
        );
        for _ in 0..300 {
            camera.step(&travel, &water, None)?;
        }
        assert!((camera.angle().to_degrees() - 170.).abs() < 0.001);
        Ok(())
    }
}
