//! Field movement over cooked triangles, independent of rendering and scripts.
use anyhow::{Result, ensure};
use resonance_content::field::{CollisionGroup, CollisionQuery};
use resonance_events::collision::{Plane, body_contact, segment_contact};
use std::sync::Arc;

const MAX_STEP_HEIGHT: f32 = 32.;
const BLOCK_FALL_STEP: f32 = 9.;
const NO_BLOCK_SUPPORT: u32 = 1 << 22;
const BLOCK_FLOOR_REACH: f32 = 60.;

/// Lines and circles touch the player's radius; polygons test the player's center.
/// Both include the authored vertical span and the player's vertical radius.
pub fn touches_trigger(trigger: &resonance_events::Trigger, p: [f32; 3], radius: f32) -> bool {
    trigger.touches(p, radius)
}

type Surface = ([[f32; 3]; 3], u32);

pub struct WalkMesh {
    triangles: Arc<[Surface]>,
    model_floors: Vec<(u64, Surface)>,
    solids: Vec<Solid>,
}
struct Solid {
    owner: u64,
    surface: u32,
    planes: Vec<Plane>,
    body_planes: Option<Vec<Plane>>,
}
#[derive(Default)]
pub(super) enum PlayerFall {
    #[default]
    Supported,
    Falling {
        updates: u32,
    },
}
#[derive(Debug, Clone, Copy)]
pub struct GroundSurface {
    pub height: f32,
    pub attributes: u32,
    /// Upward-facing unit normal in the authored Z-up coordinate space.
    pub normal: [f32; 3],
}
impl WalkMesh {
    pub(super) fn update_enemy_sight(&self, world: &mut resonance_events::GameWorld) {
        let player = world
            .actors
            .get(&world.controlled_actor)
            .map(|a| a.position);
        let rate = world
            .party
            .as_ref()
            .and_then(|p| p.encounter_modifier.as_ref())
            .map_or(0, |m| m.rate);
        for actor in world.actors.values_mut() {
            if let Some(enemy) = &mut actor.enemy {
                enemy.alerted = player.is_some_and(|player| {
                    self.sees_player(
                        (actor.instance, actor.position),
                        actor.heading,
                        enemy.sight_angle,
                        enemy.sight_distance,
                        player,
                        rate,
                    )
                });
            }
        }
    }

    fn sees_player(
        &self,
        (owner, position): (u64, [f32; 3]),
        heading: f32,
        angle: f32,
        range: f32,
        player: [f32; 3],
        rate: u8,
    ) -> bool {
        // Holy bottles suppress detection; dark bottles widen the sight cone.
        if rate == 1 {
            return false;
        }
        let delta: [f32; 3] = std::array::from_fn(|i| player[i] - position[i]);
        let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        if distance >= range {
            return false;
        }
        let horizontal = delta[0].hypot(delta[1]);
        if horizontal == 0. {
            return false;
        }
        let heading = heading.to_radians();
        let dot = (delta[0] * heading.sin() - delta[1] * heading.cos()) / distance;
        let angle = (angle * if rate == 2 { 2. } else { 1. }).clamp(0., 360.);
        if dot < (angle.to_radians() * 0.5).cos() {
            return false;
        }
        const EYE_HEIGHT: f32 = 100.;
        let eye = |mut point: [f32; 3]| {
            point[2] += EYE_HEIGHT;
            point
        };
        let (start, end) = (eye(position), eye(player));
        !self.blocked_segment(start, end, CollisionQuery::Enemy, owner)
            && !self.triangles.iter().any(|(triangle, attributes)| {
                CollisionQuery::Enemy.accepts(*attributes)
                    && crosses_triangle(start, end, *triangle)
            })
    }

    pub(super) fn settle_scenery(
        &self,
        world: &mut resonance_events::GameWorld,
        moving: Option<i32>,
    ) {
        for (&id, actor) in &mut world.actors {
            if moving == Some(id) || !actor.pushable || actor.motion.is_some() {
                continue;
            }
            let next = actor.position[2] - BLOCK_FALL_STEP;
            actor.position[2] = self
                .block_surface(
                    actor.position,
                    actor.instance,
                    BLOCK_FLOOR_REACH,
                    NO_BLOCK_SUPPORT,
                )
                .map_or(next, |floor| next.max(floor.height));
        }
    }

    pub(super) fn block_supported(&self, actor: &resonance_events::Actor) -> bool {
        self.block_surface(
            actor.position,
            actor.instance,
            BLOCK_FLOOR_REACH,
            NO_BLOCK_SUPPORT,
        )
        .is_some_and(|floor| (floor.height - actor.position[2]).abs() < 0.01)
    }

    fn block_surface(
        &self,
        point: [f32; 3],
        owner: u64,
        drop: f32,
        excluded_attributes: u32,
    ) -> Option<GroundSurface> {
        self.surfaces(Some(owner))
            .filter(|(_, attributes)| attributes & excluded_attributes == 0)
            .filter_map(|(triangle, attributes)| {
                let z = height(*triangle, point)?;
                (z <= point[2] + MAX_STEP_HEIGHT && z >= point[2] - drop).then(|| GroundSurface {
                    height: z,
                    attributes: *attributes,
                    normal: surface_normal(*triangle),
                })
            })
            .max_by(|a, b| a.height.total_cmp(&b.height))
    }
    pub(super) fn can_move_block(
        &self,
        block: &resonance_events::Actor,
        player: &resonance_events::Actor,
        delta: [f32; 2],
    ) -> bool {
        const FLOOR_REACH: f32 = 60.;
        [
            (block, CollisionQuery::Block),
            (player, CollisionQuery::Player),
        ]
        .into_iter()
        .all(|(actor, query)| {
            let target = [
                actor.position[0] + delta[0],
                actor.position[1] + delta[1],
                actor.position[2],
            ];
            let surface = if actor.instance == block.instance {
                // Pit surfaces permit entry but do not support a block's weight.
                self.block_surface(target, block.instance, f32::INFINITY, 0)
            } else {
                self.surface_within(target, Some(block.instance), |z, attributes| {
                    query.accepts(attributes) && (z - target[2]).abs() <= FLOOR_REACH
                })
            };
            let Some(surface) = surface else {
                return false;
            };
            // A forbidden tile cannot be bypassed using a floor beneath it.
            if !query.accepts(surface.attributes) {
                return false;
            }
            let (low, high) = actor.collision_bounds();
            let mut center = std::array::from_fn(|i| (low[i] + high[i]) * 0.5);
            let mut half_size = std::array::from_fn(|i| (high[i] - low[i]) * 0.5);
            if actor.instance == block.instance {
                center[2] += (surface.height - target[2]).clamp(0., MAX_STEP_HEIGHT);
            }
            // Both bodies must clear a step at their leading edge, before their
            // centers reach it. Filled pits can sit just above the surrounding floor.
            center[2] += MAX_STEP_HEIGHT * 0.5;
            half_size[2] -= MAX_STEP_HEIGHT * 0.5;
            let end = [center[0] + delta[0], center[1] + delta[1], center[2]];
            !self.solids.iter().any(|solid| {
                solid.owner != block.instance
                    && solid.owner != player.instance
                    && query.accepts(solid.surface)
                    && body_contact(
                        solid
                            .body_planes
                            .as_ref()
                            .unwrap_or(&solid.planes)
                            .iter()
                            .copied(),
                        center,
                        end,
                        half_size,
                    )
            })
        })
    }

    fn surfaces(&self, excluding: Option<u64>) -> impl Iterator<Item = &Surface> {
        self.model_floors
            .iter()
            .filter(move |(owner, (triangle, _))| {
                Some(*owner) != excluding && surface_normal(*triangle)[2] > 0.
            })
            .map(|(_, surface)| surface)
            .chain(self.triangles.iter())
    }
    /// Snapshot live model floors without copying the map. Hidden scenery also
    /// carries floors; native property 48 only masks projectile contact.
    pub(super) fn with_actors<'a>(
        &self,
        actors: impl Iterator<Item = &'a resonance_events::Actor>,
    ) -> Self {
        let mut model_floors = Vec::new();
        let mut solids = Vec::new();
        for actor in actors {
            if let Some(mesh) = actor.model_collision.as_ref() {
                for group in &mesh.solids {
                    let planes = actor
                        .collision_triangles(group)
                        .map(Plane::triangle)
                        .collect();
                    // Block solids include clearance for walking characters. A swept
                    // body supplies its own clearance, so use the physical block faces.
                    let body_planes = (actor.pushable && !mesh.floors.is_empty()).then(|| {
                        mesh.floors
                            .iter()
                            .flat_map(|group| actor.collision_triangles(group))
                            .map(Plane::triangle)
                            .collect()
                    });
                    solids.push(Solid {
                        owner: actor.instance,
                        surface: group.surface,
                        planes,
                        body_planes,
                    });
                }
                for group in &mesh.floors {
                    model_floors.extend(
                        actor
                            .collision_triangles(group)
                            .map(|triangle| (actor.instance, (triangle, group.surface))),
                    );
                }
            }
        }
        Self {
            triangles: self.triangles.clone(),
            model_floors,
            solids,
        }
    }
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
        Ok(Self {
            triangles: triangles.into(),
            model_floors: Vec::new(),
            solids: Vec::new(),
        })
    }
    /// Keep an entrance on its floor, or use the nearest valid triangle when an
    /// unfinished setup script never placed the player.
    pub fn exploration_start(&self, point: [f32; 3]) -> Option<[f32; 3]> {
        if let Some(z) = self.height(point, f32::MAX) {
            return Some([point[0], point[1], z]);
        }
        self.surfaces(None)
            .filter(|(_, attributes)| CollisionQuery::Player.accepts(*attributes))
            .filter_map(|(vertices, _)| {
                let center =
                    std::array::from_fn(|axis| vertices.iter().map(|p| p[axis]).sum::<f32>() / 3.);
                height(*vertices, center).map(|z| [center[0], center[1], z])
            })
            .min_by(|a, b| {
                let distance = |p: &[f32; 3]| {
                    p.iter()
                        .zip(point)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f32>()
                };
                distance(a).total_cmp(&distance(b))
            })
    }
    /// Select the closest reachable floor, including ramps and raised platforms.
    pub fn height(&self, point: [f32; 3], max_step: f32) -> Option<f32> {
        self.walking_surface(point, |z| (z - point[2]).abs() <= max_step)
            .map(|surface| surface.height)
    }
    /// Ground reachable by a normal walking step, including raised platforms.
    pub fn ground_surface(&self, point: [f32; 3]) -> Option<GroundSurface> {
        self.surface(point, MAX_STEP_HEIGHT)
    }
    /// Place a horizontal destination on the closest reachable floor.
    pub fn resolve_motion(&self, proposed: [f32; 3]) -> Option<[f32; 3]> {
        let surface =
            self.walking_surface(proposed, |z| (z - proposed[2]).abs() <= MAX_STEP_HEIGHT)?;
        Some([proposed[0], proposed[1], surface.height])
    }
    /// An authored landing can end just outside the floor while the feet overlap it.
    pub(super) fn landing_near(&self, point: [f32; 3], radius: f32) -> Option<[f32; 3]> {
        let mut nearest = None;
        for (triangle, attributes) in self.surfaces(None) {
            if !CollisionQuery::Player.accepts(*attributes) {
                continue;
            }
            for edge in 0..3 {
                let (a, b) = (triangle[edge], triangle[(edge + 1) % 3]);
                let delta: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
                let length = delta[0] * delta[0] + delta[1] * delta[1];
                if length == 0. {
                    continue;
                }
                let t = (((point[0] - a[0]) * delta[0] + (point[1] - a[1]) * delta[1]) / length)
                    .clamp(0., 1.);
                let candidate: [f32; 3] = std::array::from_fn(|i| a[i] + t * delta[i]);
                let distance = (candidate[0] - point[0]).hypot(candidate[1] - point[1]);
                if distance <= radius
                    && (candidate[2] - point[2]).abs() <= MAX_STEP_HEIGHT
                    && nearest.is_none_or(|(best, _)| distance < best)
                {
                    nearest = Some((distance, candidate));
                }
            }
        }
        nearest.map(|(_, point)| point)
    }
    pub(super) fn blocked(&self, point: [f32; 3], query: CollisionQuery, owner: u64) -> bool {
        self.blocked_segment(point, point, query, owner)
    }
    fn blocked_segment(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        query: CollisionQuery,
        owner: u64,
    ) -> bool {
        self.solids.iter().any(|solid| {
            solid.owner != owner
                && query.accepts(solid.surface)
                && segment_contact(solid.planes.iter().copied(), start, end).is_some()
        })
    }

    pub(super) fn resolve_enemy(&self, proposed: [f32; 3], owner: u64) -> Option<[f32; 3]> {
        const BODY_PROBE_HEIGHT: f32 = 45.;
        if self.blocked(
            [proposed[0], proposed[1], proposed[2] + BODY_PROBE_HEIGHT],
            CollisionQuery::Enemy,
            owner,
        ) {
            return None;
        }
        let surface = self.surface_within(proposed, None, |z, attributes| {
            (z - proposed[2]).abs() <= MAX_STEP_HEIGHT && CollisionQuery::Enemy.accepts(attributes)
        })?;
        Some([proposed[0], proposed[1], surface.height])
    }
    pub(super) fn resolve_player(
        &self,
        start: [f32; 3],
        proposed: [f32; 3],
        fall: &mut PlayerFall,
        event_paused: bool,
    ) -> Option<[f32; 3]> {
        const ACCELERATION: f32 = 9.;
        const FLOOR_REACH: f32 = 60.;
        let surface = match fall {
            PlayerFall::Supported => {
                self.walking_surface(proposed, |z| (z - proposed[2]).abs() <= FLOOR_REACH)
            }
            PlayerFall::Falling { .. } => self.walking_surface(proposed, |z| {
                (0. ..=FLOOR_REACH).contains(&(z - proposed[2]))
            }),
        };
        if let Some(surface) = surface {
            *fall = PlayerFall::Supported;
            return Some([proposed[0], proposed[1], surface.height]);
        }
        let updates = match (event_paused, &*fall) {
            (true, _) => 0,
            (false, PlayerFall::Supported) => 1,
            (false, PlayerFall::Falling { updates }) => updates.saturating_add(1),
        };
        let position = [
            start[0],
            start[1],
            proposed[2] - ACCELERATION * updates as f32,
        ];
        if self
            .walking_surface(position, |z| z <= position[2])
            .is_some()
        {
            *fall = if event_paused {
                PlayerFall::Supported
            } else {
                PlayerFall::Falling { updates }
            };
            Some(position)
        } else {
            *fall = PlayerFall::Supported;
            None
        }
    }
    pub fn surface(&self, point: [f32; 3], max_step: f32) -> Option<GroundSurface> {
        self.surface_within(point, None, |z, _| (z - point[2]).abs() <= max_step)
    }
    pub fn surface_below(&self, point: [f32; 3]) -> Option<GroundSurface> {
        self.surface_within(point, None, |z, _| z <= point[2])
    }
    fn walking_surface(
        &self,
        point: [f32; 3],
        accepts: impl Fn(f32) -> bool,
    ) -> Option<GroundSurface> {
        self.surface_within(point, None, |z, attributes| {
            CollisionQuery::Player.accepts(attributes) && accepts(z)
        })
    }
    fn surface_within(
        &self,
        point: [f32; 3],
        excluding: Option<u64>,
        accepts: impl Fn(f32, u32) -> bool,
    ) -> Option<GroundSurface> {
        self.surfaces(excluding)
            .filter_map(|(triangle, attributes)| {
                height(*triangle, point).map(|z| (z, triangle, *attributes))
            })
            .filter(|(z, _, attributes)| accepts(*z, *attributes))
            .min_by(|(a, _, _), (b, _, _)| (a - point[2]).abs().total_cmp(&(b - point[2]).abs()))
            .map(|(height, [a, b, c], attributes)| {
                let normal = surface_normal([*a, *b, *c]);
                GroundSurface {
                    height,
                    attributes,
                    normal: normal.map(|v| v * normal[2].signum()),
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
                if delta[axis] == 0. {
                    return 0.;
                }
                // Follow the floor through the clearance probe. Comparing a
                // whole body radius against one 32-unit step rejects continuous
                // steep stairs, even though each actual walking step is valid.
                let probe_steps = (radius / 4.).ceil().max(1.) as u32;
                let mut probe = Some(point);
                for _ in 0..probe_steps {
                    probe = probe.and_then(|mut p| {
                        p[axis] += radius.copysign(delta[axis]) / probe_steps as f32;
                        p[2] = self.height(p, 32.)?;
                        Some(p)
                    });
                }
                if probe.is_some() { delta[axis] } else { 0. }
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
fn surface_normal([a, b, c]: [[f32; 3]; 3]) -> [f32; 3] {
    let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let v: [f32; 3] = std::array::from_fn(|i| c[i] - a[i]);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = cross.iter().map(|v| v * v).sum::<f32>().sqrt();
    cross.map(|v| v / length)
}
// Segment/triangle intersection also handles vertical walls in the field mesh.
fn crosses_triangle(start: [f32; 3], end: [f32; 3], triangle: [[f32; 3]; 3]) -> bool {
    let normal = surface_normal(triangle);
    let side = |point: [f32; 3]| {
        (0..3)
            .map(|i| (point[i] - triangle[0][i]) * normal[i])
            .sum::<f32>()
    };
    let (from, to) = (side(start), side(end));
    if from * to >= 0. {
        return false;
    }
    let t = from / (from - to);
    let point: [f32; 3] = std::array::from_fn(|i| start[i] + (end[i] - start[i]) * t);
    (0..3).all(|edge| {
        let (a, b) = (triangle[edge], triangle[(edge + 1) % 3]);
        let u: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let v: [f32; 3] = std::array::from_fn(|i| point[i] - a[i]);
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        (0..3).map(|i| cross[i] * normal[i]).sum::<f32>() >= 0.
    })
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
    fn walking_ignores_block_only_planes_when_falling_and_landing() {
        let mesh = WalkMesh::new(&[(0, 0.), ((1 << 19) | NO_BLOCK_SUPPORT, 150.)].map(
            |(surface, z)| CollisionGroup {
                surface,
                vertices: vec![[0., 0., z], [100., 0., z], [100., 100., z], [0., 100., z]],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
            },
        ))
        .unwrap();
        let mut position = [20., 20., 150.];
        assert_eq!(mesh.move_by(position, [4., 0.], 10., |_| false), position);
        let mut fall = PlayerFall::Supported;
        for z in [141., 123., 96., 60., 15., 15., 0.] {
            position = mesh
                .resolve_player(position, position, &mut fall, false)
                .unwrap_or(position);
            assert_eq!(position, [20., 20., z]);
        }
        assert_eq!(
            mesh.move_by(position, [4., 0.], 10., |_| false),
            [24., 20., 0.]
        );
    }
    #[test]
    fn event_pause_stops_a_fall_and_resumes_from_the_first_gravity_step() {
        let mesh = square();
        let mut fall = PlayerFall::Supported;
        let mut position = [0., 0., 200.];
        for (paused, height) in [(false, 191.), (false, 173.), (true, 173.), (false, 164.)] {
            position = mesh
                .resolve_player(position, position, &mut fall, paused)
                .unwrap();
            assert_eq!(position[2], height);
        }
    }
    #[test]
    fn unsupported_player_over_void_does_not_start_falling() {
        let mut fall = PlayerFall::default();
        let position = [200., 200., 100.];
        assert!(
            square()
                .resolve_player(position, position, &mut fall, false)
                .is_none()
        );
        assert!(matches!(fall, PlayerFall::Supported));
    }
    #[test]
    fn circular_trigger_uses_radius_and_vertical_span() {
        let trigger = resonance_events::Trigger {
            ring_barrier: false,
            activations: 0,
            key: 1,
            automatic_event: false,
            shape: resonance_events::TriggerShape::Circle {
                center: [0., 0., 10.],
                radius: 20.,
            },
            height: 40.,
            transition: None,
            touch_metadata: [0; 3],
        };
        assert!(touches_trigger(&trigger, [24., 0., 10.], 5.));
        assert!(!touches_trigger(&trigger, [20., 20., 10.], 5.));
        assert!(!touches_trigger(&trigger, [25., 0., 10.], 5.));
        assert!(touches_trigger(&trigger, [0., 0., 50.], 5.));
        assert!(!touches_trigger(&trigger, [0., 0., 51.], 5.));
        assert!(!touches_trigger(&trigger, [0., 0., 4.], 5.));
    }
    #[test]
    fn triangular_triggers_use_the_polygon_instead_of_its_bounding_box() {
        let mut trigger = resonance_events::Trigger {
            ring_barrier: false,
            activations: 0,
            key: 1,
            automatic_event: false,
            shape: resonance_events::TriggerShape::Triangle([
                [0., 0., 10.],
                [100., 0., 10.],
                [0., 100., 10.],
            ]),
            height: 50.,
            transition: Some([18, 0, 243]),
            touch_metadata: [0; 3],
        };
        for _ in 0..2 {
            assert!(touches_trigger(&trigger, [20., 20., 10.], 5.));
            assert!(!touches_trigger(&trigger, [80., 80., 10.], 5.));
            assert!(!touches_trigger(&trigger, [20., 20., 61.], 5.));
            assert!(!touches_trigger(&trigger, [20., 20., 4.], 5.));
            if let resonance_events::TriggerShape::Triangle(points) = &mut trigger.shape {
                points.reverse();
            }
        }
    }
    #[test]
    fn walking_follows_slopes_without_horizontal_drift() {
        let mesh = square();
        let target = [50., 50., 0.];
        let expected = [target[0], target[1], mesh.height(target, 32.).unwrap()];
        assert_eq!(mesh.resolve_motion(target), Some(expected));
        assert_eq!(mesh.resolve_enemy(target, 0), Some(expected));
        assert_eq!(
            mesh.resolve_player([50., 40., 8.], target, &mut PlayerFall::default(), false),
            Some(expected)
        );
    }
    #[test]
    fn doorway_height_does_not_expand_its_horizontal_reach() {
        let trigger = resonance_events::Trigger {
            ring_barrier: false,
            activations: 0,
            key: 3001,
            automatic_event: false,
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
            ring_barrier: false,
            activations: 0,
            key: 2002,
            automatic_event: false,
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
    fn enemy_sight_respects_range_and_cone() {
        let floor = |y: [f32; 2], surface| CollisionGroup {
            surface,
            vertices: vec![
                [-700., y[0], 0.],
                [700., y[0], 0.],
                [700., y[1], 0.],
                [-700., y[1], 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let mesh = WalkMesh::new(&[floor([-700., 700.], 0)]).unwrap();
        let sees = |point, rate| mesh.sees_player((0, [0.; 3]), 0., 90., 600., point, rate);
        assert!(sees([0., -300., 0.], 0));
        assert!(!sees([0., -300., 0.], 1));
        assert!(!sees([0., 300., 0.], 0));
        assert!(!sees([0., -600., 0.], 0));
        assert!(!sees([300., -100., 0.], 0));
        assert!(!sees([250., -150., 0.], 0));
        assert!(sees([150., -250., 0.], 0));
        assert!(sees([300., -100., 0.], 2));
        assert!(!mesh.sees_player((0, [0.; 3]), 0., 90., 600., [0., -100., 500.], 0));
        assert!(crosses_triangle(
            [0., 0., 100.],
            [0., -300., 100.],
            [[-100., -150., 0.], [100., -150., 0.], [0., -150., 300.]]
        ));
    }
    #[test]
    fn ramp_height_and_shared_edge_are_continuous() {
        let mesh = square();
        assert_eq!(mesh.height([50., 50., 0.], 32.), Some(10.));
        assert_eq!(mesh.height([101., 50., 0.], 32.), None);
        assert_eq!(mesh.height([50., 50., 100.], 32.), None);
        assert_eq!(mesh.surface_below([50., 50., 100.]).unwrap().height, 10.);
        assert!(mesh.surface_below([50., 50., 9.]).is_none());
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
