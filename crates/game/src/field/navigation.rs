//! Field movement over cooked triangles, independent of rendering and scripts.
use anyhow::{Result, ensure};
use resonance_content::field::CollisionGroup;

/// Lines touch the player's radius; area triggers test the player's center.
/// Both include the authored vertical span and the player's vertical radius.
pub fn touches_trigger(trigger: &resonance_events::Trigger, p: [f32; 3], radius: f32) -> bool {
    use resonance_events::TriggerShape;
    let points: &[[f32; 3]] = match &trigger.shape {
        TriggerShape::Line(points) => points,
        TriggerShape::Quad(points) => points,
    };
    let low = points.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min);
    let high = points
        .iter()
        .map(|v| v[2])
        .fold(f32::NEG_INFINITY, f32::max);
    if p[2] + radius < low || p[2] > high + trigger.height {
        return false;
    }
    if let TriggerShape::Quad(points) = &trigger.shape {
        let sides: [f32; 4] = std::array::from_fn(|i| {
            let (a, b) = (points[i], points[(i + 1) % 4]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        });
        return sides.iter().all(|s| *s >= 0.) || sides.iter().all(|s| *s <= 0.);
    }
    let (a, b) = (points[0], points[1]);
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    let t = if length_squared > 0. {
        ((p[0] - a[0]) * delta[0] + (p[1] - a[1]) * delta[1]) / length_squared
    } else {
        0.
    }
    .clamp(0., 1.);
    (p[0] - a[0] - t * delta[0]).hypot(p[1] - a[1] - t * delta[1]) <= radius
}

pub struct WalkMesh {
    triangles: Vec<([[f32; 3]; 3], u32)>,
}
#[derive(Debug, Clone, Copy)]
pub struct GroundSurface {
    pub height: f32,
    pub attributes: u32,
    /// Upward-facing unit normal in the authored Z-up coordinate space.
    pub normal: [f32; 3],
}
impl WalkMesh {
    pub fn new(groups: &[CollisionGroup]) -> Result<Self> {
        ensure!(!groups.is_empty(), "field has no walkable surface");
        let mut triangles = Vec::new();
        for group in groups {
            group.validate()?;
            triangles.extend(
                group
                    .triangles
                    .iter()
                    .map(|t| (t.map(|i| group.vertices[usize::from(i)]), group.surface)),
            );
        }
        Ok(Self { triangles })
    }
    /// Select the closest reachable floor. This also preserves authored ramps
    /// and raised platforms without importing the original collision engine.
    pub fn height(&self, point: [f32; 3], max_step: f32) -> Option<f32> {
        self.surface(point, max_step).map(|surface| surface.height)
    }
    /// Tilt horizontal intent onto the floor and resolve penetration along its
    /// normal. Player movement applies pitch before roll; NPCs use the reverse.
    pub fn resolve_motion(
        &self,
        start: [f32; 3],
        proposed: [f32; 3],
        player: bool,
    ) -> Option<[f32; 3]> {
        let surface = self.surface(proposed, 32.)?;
        let [nx, ny, nz] = surface.normal;
        let pitch = (-ny).clamp(-1., 1.).asin();
        let roll = nx.atan2(nz);
        let dx = proposed[0] - start[0];
        let dy = proposed[1] - start[1];
        let cross = pitch.sin() * roll.sin();
        Some([
            start[0] + dx * roll.cos() + if player { dy * cross } else { 0. },
            start[1] + dy * pitch.cos() + if player { 0. } else { dx * cross },
            start[2] + (surface.height - start[2]) * nz * nz,
        ])
    }
    pub fn surface(&self, point: [f32; 3], max_step: f32) -> Option<GroundSurface> {
        self.triangles
            .iter()
            .filter_map(|(triangle, attributes)| {
                height(*triangle, point).map(|z| (z, triangle, *attributes))
            })
            .filter(|(z, _, _)| (*z - point[2]).abs() <= max_step)
            .min_by(|(a, _, _), (b, _, _)| (a - point[2]).abs().total_cmp(&(b - point[2]).abs()))
            .map(|(height, [a, b, c], attributes)| {
                let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
                let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let length = cross
                    .iter()
                    .map(|v| v * v)
                    .sum::<f32>()
                    .sqrt()
                    .copysign(cross[2]);
                GroundSurface {
                    height,
                    attributes,
                    normal: cross.map(|v| v / length),
                }
            })
    }
    /// Check floor clearance in each intended direction, allowing the other
    /// axis to slide when one reaches an edge. `blocked` supplies actor collision.
    pub fn move_by(
        &self,
        start: [f32; 3],
        delta: [f32; 2],
        radius: f32,
        blocked: impl Fn([f32; 3]) -> bool,
    ) -> [f32; 3] {
        if !delta.iter().all(|v| v.is_finite()) {
            return start;
        }
        let distance = delta[0].hypot(delta[1]);
        let steps = (distance / 4.).ceil().clamp(1., 64.) as u32;
        let delta = delta.map(|v| v / steps as f32);
        let mut point = start;
        let fit = |mut p: [f32; 3]| {
            p[2] = self.height(p, 32.)?;
            (!blocked(p)).then_some(p)
        };
        for _ in 0..steps {
            let delta: [f32; 2] = std::array::from_fn(|axis| {
                let mut probe = point;
                probe[axis] += radius.copysign(delta[axis]);
                if delta[axis] != 0. && self.height(probe, 32.).is_some() {
                    delta[axis]
                } else {
                    0.
                }
            });
            if let Some(next) = fit([point[0] + delta[0], point[1] + delta[1], point[2]]) {
                point = next;
            } else {
                if let Some(next) = fit([point[0] + delta[0], point[1], point[2]]) {
                    point = next;
                }
                if let Some(next) = fit([point[0], point[1] + delta[1], point[2]]) {
                    point = next;
                }
            }
        }
        point
    }
}
fn height([a, b, c]: [[f32; 3]; 3], p: [f32; 3]) -> Option<f32> {
    let cross = |a: [f32; 2], b: [f32; 2]| a[0] * b[1] - a[1] * b[0];
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ac = [c[0] - a[0], c[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let determinant = cross(ab, ac);
    if determinant.abs() < 0.0001 {
        return None;
    }
    let u = cross(ap, ac) / determinant;
    let v = cross(ab, ap) / determinant;
    (u >= -0.0001 && v >= -0.0001 && u + v <= 1.0001)
        .then_some(a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn walking_intent_tilts_onto_single_and_double_axis_slopes() {
        let mesh = WalkMesh::new(&[CollisionGroup {
            surface: 96,
            vertices: vec![[0., -100., 0.], [100., -100., 75.], [0., 100., 0.]],
            triangles: vec![[0, 1, 2]],
        }])
        .unwrap();
        let resolved = mesh.resolve_motion([0.; 3], [10., 0., 0.], true).unwrap();
        // A 3:4:5 slope rotates ten horizontal units to eight. The initial
        // floor correction follows the normal rather than snapping vertically.
        for (actual, expected) in resolved.into_iter().zip([8., 0., 4.8]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        let mesh = WalkMesh::new(&[CollisionGroup {
            surface: 96,
            vertices: vec![[0., -100., -50.], [100., -100., 25.], [0., 100., 50.]],
            triangles: vec![[0, 1, 2]],
        }])
        .unwrap();
        // Pitching a sideways player step must not introduce forward drift.
        // NPC motion retains its separately authored orientation convention.
        for (player, expected) in [
            (true, [8., 0., 4.137931]),
            (false, [8., -2.228344, 4.137931]),
        ] {
            let actual = mesh.resolve_motion([0.; 3], [10., 0., 0.], player).unwrap();
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.0001);
            }
        }
    }
    #[test]
    fn doorway_height_does_not_expand_its_horizontal_reach() {
        let trigger = resonance_events::Trigger {
            key: 3001,
            shape: resonance_events::TriggerShape::Line([[-540., -264., 0.], [-540., -380., 0.]]),
            height: 200.,
            transition: None,
            touch_metadata: [0; 3],
        };
        assert!(!touches_trigger(&trigger, [-440., -320., 0.], 42.));
        assert!(touches_trigger(&trigger, [-499., -320., 0.], 42.));
        assert!(!touches_trigger(&trigger, [-499., -440., 0.], 42.));
        assert!(!touches_trigger(&trigger, [-499., -320., 201.], 42.));
        assert!(!touches_trigger(&trigger, [-499., -320., -43.], 42.));
    }
    #[test]
    fn area_trigger_uses_its_polygon_and_vertical_span() {
        let points = [
            [0., 0., 10.],
            [100., 0., 10.],
            [80., 60., 20.],
            [20., 60., 20.],
        ];
        let mut trigger = resonance_events::Trigger {
            key: 2002,
            shape: resonance_events::TriggerShape::Quad(points),
            height: 200.,
            transition: None,
            touch_metadata: [0; 3],
        };
        assert!(touches_trigger(&trigger, [50., 30., 0.], 42.));
        assert!(!touches_trigger(&trigger, [10., 59., 10.], 42.));
        assert!(!touches_trigger(&trigger, [50., 30., 221.], 42.));
        assert!(!touches_trigger(&trigger, [50., 30., -33.], 42.));
        let mut reversed = points;
        reversed.reverse();
        trigger.shape = resonance_events::TriggerShape::Quad(reversed);
        assert!(touches_trigger(&trigger, [50., 30., 10.], 42.));
    }
    fn square() -> WalkMesh {
        WalkMesh::new(&[CollisionGroup {
            surface: 96,
            vertices: vec![
                [0., 0., 0.],
                [100., 0., 0.],
                [100., 100., 20.],
                [0., 100., 20.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }])
        .unwrap()
    }
    #[test]
    fn ramp_height_and_shared_edge_are_continuous() {
        let mesh = square();
        assert_eq!(mesh.height([50., 50., 0.], 32.), Some(10.));
        assert_eq!(mesh.height([101., 50., 0.], 32.), None);
        assert_eq!(mesh.height([50., 50., 100.], 32.), None);
        let normal = mesh.surface([50., 50., 0.], 32.).unwrap().normal;
        assert!((normal[2] - 1. / 1.04f32.sqrt()).abs() < 0.00001);
        assert!((normal[1] + 0.2 / 1.04f32.sqrt()).abs() < 0.00001);
    }
    #[test]
    fn directional_clearance_preserves_small_slides_and_movement_away() {
        let mesh = square();
        let end = mesh.move_by([91., 20., 4.], [4., 0.4], 10., |_| false);
        assert_eq!(end[0], 91.);
        assert!((end[1] - 20.4).abs() < 0.001);
        assert!((end[2] - 4.08).abs() < 0.001);
        let away = mesh.move_by(end, [-4., 0.], 10., |_| false);
        assert_eq!(away[0], 87.);
        assert_eq!(away[1], end[1]);
    }
    #[test]
    fn actor_obstacle_is_respected_during_sweep() {
        let end = square().move_by([20., 50., 10.], [60., 0.], 5., |p| {
            (p[0] - 50.).hypot(p[1] - 50.) < 15.
        });
        assert!(end[0] <= 35. && end[0] >= 30.);
    }
}
