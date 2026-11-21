//! Field movement over cooked triangles, independent of rendering and scripts.
use anyhow::{Result, ensure};
use resonance_content::field::{CollisionGroup, CollisionQuery};
use resonance_events::collision::{Plane, body_contact, segment_contact};
use std::sync::Arc;

const MAX_STEP_HEIGHT: f32 = 32.;
// Adjacent floor pieces can leave fractional seams. Use world units so the
// clearance does not grow with the size of the supporting triangle.
const FLOOR_EDGE_TOLERANCE: f32 = 0.25;
// Join cracks narrower than one movement substep, only between two floor edges.
const MOVEMENT_SUBSTEP: f32 = 4.;
const BLOCK_FALL_STEP: f32 = 9.;
const NO_BLOCK_SUPPORT: u32 = 1 << 22;
const BLOCK_FLOOR_REACH: f32 = 60.;

/// Lines and circles touch the player's radius; polygons test the player's center.
/// Both include the authored vertical span and the player's vertical radius.
pub fn touches_trigger(trigger: &resonance_events::Trigger, p: [f32; 3], radius: f32) -> bool {
    trigger.touches(p, radius)
}

type Surface = ([[f32; 3]; 3], u32);

#[derive(Clone)]
pub struct WalkMesh {
    triangles: Arc<[Surface]>,
    model_floors: Vec<(u64, [f32; 3], Surface)>,
    solids: Vec<Solid>,
}
#[derive(Clone)]
struct Solid {
    owner: u64,
    surface: u32,
    planes: Vec<Plane>,
    body_planes: Option<Vec<Plane>>,
}
#[derive(Default, Clone)]
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
    /// Carry grounded feet by their support's displacement, including attached floors.
    /// Turning the collision model must not swing riders around its origin.
    pub(super) fn carry_riders(&self, world: &mut resonance_events::GameWorld) {
        let riders: Vec<_> = world
            .actors
            .iter()
            .filter(|&(id, actor)| {
                ((id == &world.controlled_actor && actor.grounded)
                    || (actor.pushable && actor.motion.is_none()))
                    && actor.attachment.is_none()
                    && !actor.interaction_anchor
            })
            .filter_map(|(&id, actor)| {
                self.carried_position(world, actor)
                    .map(|position| (id, position))
            })
            .collect();
        for (id, position) in riders {
            world.actors.get_mut(&id).unwrap().position = position;
        }
    }

    fn carried_position(
        &self,
        world: &resonance_events::GameWorld,
        actor: &resonance_events::Actor,
    ) -> Option<[f32; 3]> {
        let point = actor.position;
        let accepts = |attributes| {
            if actor.pushable {
                attributes & NO_BLOCK_SUPPORT == 0
            } else {
                CollisionQuery::Player.accepts(attributes)
            }
        };
        let support =
            self.model_floors
                .iter()
                .find_map(|&(owner, origin, (triangle, attributes))| {
                    if owner == actor.instance
                        || !accepts(attributes)
                        || surface_normal(triangle)[2] <= 0.
                    {
                        return None;
                    }
                    let z = height(triangle, point)?;
                    ((z - point[2]).abs() <= 0.01).then_some((owner, origin))
                });
        let (owner, previous) = support?;
        let platform = world
            .actors
            .values()
            .find(|actor| actor.instance == owner && actor.model_collision.is_some())?;
        let current = platform.collision_point([0.; 3]);
        if current == previous {
            return None;
        }
        // Where a dock overlaps the platform, the fixed floor wins the same
        // height tie as normal walking. Stepping ashore ends the ride.
        if self.triangles.iter().any(|&(triangle, attributes)| {
            accepts(attributes)
                && height(triangle, point).is_some_and(|z| (z - point[2]).abs() <= 0.01)
        }) {
            return None;
        }
        Some(std::array::from_fn(|axis| {
            point[axis] + (current[axis] - previous[axis])
        }))
    }

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
            .filter(move |(owner, _, (triangle, _))| {
                Some(*owner) != excluding && surface_normal(*triangle)[2] > 0.
            })
            .map(|(_, _, surface)| surface)
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
                    // Solid meshes already include walking clearance. When the floor
                    // mesh describes a closed body, sweep against its physical faces.
                    // This applies to fixed barriers as well as movable blocks.
                    let body_planes = (mesh.solids.len() == 1 && mesh.floors.len() == 1)
                        .then(|| &mesh.floors[0])
                        .filter(|group| closed_volume(group))
                        .map(|group| {
                            actor
                                .collision_triangles(group)
                                .map(Plane::triangle)
                                .collect()
                        })
                        .or_else(|| {
                            actor
                                .block_mesh_bounds()
                                .map(|(low, high)| Plane::bounds(low, high).to_vec())
                        });
                    solids.push(Solid {
                        owner: actor.instance,
                        surface: group.surface,
                        planes,
                        body_planes,
                    });
                }
                let origin = actor.collision_point([0.; 3]);
                model_floors.extend(
                    mesh.floors
                        .iter()
                        .flat_map(|group| {
                            actor
                                .collision_triangles(group)
                                .map(|triangle| (triangle, group.surface))
                        })
                        .map(|surface| (actor.instance, origin, surface)),
                );
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
        self.surface_within(point, None, |z, attributes| {
            CollisionQuery::Player.accepts(attributes) && (z - point[2]).abs() <= max_step
        })
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
    /// Ground actor movement along the slope, including horizontal correction.
    pub fn resolve_actor_motion(&self, proposed: [f32; 3]) -> Option<[f32; 3]> {
        let surface =
            self.walking_surface(proposed, |z| (z - proposed[2]).abs() <= MAX_STEP_HEIGHT)?;
        let distance = (proposed[2] - surface.height) * surface.normal[2];
        Some(std::array::from_fn(|axis| {
            proposed[axis] - distance * surface.normal[axis]
        }))
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

    pub(super) fn enemy_floor_ahead(&self, point: [f32; 3]) -> bool {
        const FLOOR_REACH: f32 = 60.;
        self.surface_within(point, None, |z, attributes| {
            (z - point[2]).abs() <= FLOOR_REACH && CollisionQuery::Enemy.accepts(attributes)
        })
        .is_some()
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
        // A ramp can overlap the floor beneath it. Step onto the highest
        // reachable surface rather than staying on that lower floor.
        self.surfaces_at(point, None)
            .filter(|surface| {
                CollisionQuery::Player.accepts(surface.attributes) && accepts(surface.height)
            })
            .max_by(|a, b| a.height.total_cmp(&b.height))
            .or_else(|| {
                self.seam_surface(point, None, |z, attributes| {
                    CollisionQuery::Player.accepts(attributes) && accepts(z)
                })
            })
    }
    fn surface_within(
        &self,
        point: [f32; 3],
        excluding: Option<u64>,
        accepts: impl Fn(f32, u32) -> bool,
    ) -> Option<GroundSurface> {
        self.surfaces_at(point, excluding)
            .filter(|surface| accepts(surface.height, surface.attributes))
            .min_by(|a, b| {
                (a.height - point[2])
                    .abs()
                    .total_cmp(&(b.height - point[2]).abs())
            })
            .or_else(|| self.seam_surface(point, excluding, accepts))
    }

    fn seam_surface(
        &self,
        point: [f32; 3],
        excluding: Option<u64>,
        accepts: impl Fn(f32, u32) -> bool,
    ) -> Option<GroundSurface> {
        // An authored floor that rejected this query is not a crack to fill.
        if self
            .surfaces_at(point, excluding)
            .any(|surface| (surface.height - point[2]).abs() <= MAX_STEP_HEIGHT)
        {
            return None;
        }
        let distance_squared =
            |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
        let edge = |&(triangle, attributes): &Surface| {
            // Degenerate and vertical triangles cannot provide floor support.
            barycentric(triangle, triangle[0])?;
            let normal = surface_normal(triangle);
            let nearest = (0..3)
                .filter_map(|i| closest_edge_point(triangle[i], triangle[(i + 1) % 3], point))
                .min_by(|a, b| {
                    distance_squared(*a, point).total_cmp(&distance_squared(*b, point))
                })?;
            (distance_squared(nearest, point) <= MOVEMENT_SUBSTEP.powi(2)
                && accepts(nearest[2], attributes))
            .then_some((nearest, attributes, normal.map(|v| v * normal[2].signum())))
        };
        let (a, attributes, normal) = self
            .surfaces(excluding)
            .filter_map(edge)
            .min_by(|a, b| distance_squared(a.0, point).total_cmp(&distance_squared(b.0, point)))?;
        self.surfaces(excluding)
            .filter_map(edge)
            .find_map(|(b, _, _)| {
                let width = distance_squared(a, b);
                if width == 0.
                    || width > MOVEMENT_SUBSTEP.powi(2)
                    || (a[2] - b[2]).abs() > MAX_STEP_HEIGHT
                {
                    return None;
                }
                // Both sides must support this point. A nearby outer edge alone
                // must not enlarge the walkable floor or bridge a real gap.
                let along =
                    ((point[0] - a[0]) * (b[0] - a[0]) + (point[1] - a[1]) * (b[1] - a[1])) / width;
                if !(0. ..=1.).contains(&along) {
                    return None;
                }
                let joined = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * along);
                (distance_squared(joined, point) <= FLOOR_EDGE_TOLERANCE.powi(2)).then_some(
                    GroundSurface {
                        height: joined[2],
                        attributes,
                        normal,
                    },
                )
            })
    }

    fn surfaces_at(
        &self,
        point: [f32; 3],
        excluding: Option<u64>,
    ) -> impl Iterator<Item = GroundSurface> + '_ {
        self.surfaces(excluding)
            .filter_map(move |(triangle, attributes)| {
                height(*triangle, point).map(|z| (z, triangle, *attributes))
            })
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
        let steps = (distance / MOVEMENT_SUBSTEP).ceil().clamp(1., 64.) as u32;
        let delta = delta.map(|v| v / steps as f32);
        let mut point = start;
        let fit = |p: [f32; 3]| {
            let p = self.resolve_motion(p)?;
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
                let probe_steps = (radius / MOVEMENT_SUBSTEP).ceil().max(1.) as u32;
                let mut probe = Some(point);
                for _ in 0..probe_steps {
                    probe = probe.and_then(|mut p| {
                        p[axis] += radius.copysign(delta[axis]) / probe_steps as f32;
                        self.resolve_motion(p)
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

fn closed_volume(group: &CollisionGroup) -> bool {
    !group.triangles.is_empty()
        && group.triangles.iter().all(|&[a, b, c]| {
            [(a, b), (b, c), (c, a)].into_iter().all(|(from, to)| {
                group
                    .triangles
                    .iter()
                    .any(|&[x, y, z]| [(x, y), (y, z), (z, x)].contains(&(to, from)))
            })
        })
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

fn height(triangle: [[f32; 3]; 3], p: [f32; 3]) -> Option<f32> {
    let [a, b, c] = triangle;
    let [u, v] = barycentric(triangle, p)?;
    Some(a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]))
}
fn closest_edge_point(a: [f32; 3], b: [f32; 3], point: [f32; 3]) -> Option<[f32; 3]> {
    let delta: [f32; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    if length_squared == 0. {
        return None;
    }
    let t = (((point[0] - a[0]) * delta[0] + (point[1] - a[1]) * delta[1]) / length_squared)
        .clamp(0., 1.);
    Some(std::array::from_fn(|i| a[i] + delta[i] * t))
}
fn barycentric([a, b, c]: [[f32; 3]; 3], p: [f32; 3]) -> Option<[f32; 2]> {
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
    if u >= 0. && v >= 0. && u + v <= 1. {
        return Some([u, v]);
    }
    let near_edge = [(a, b), (b, c), (c, a)].into_iter().any(|(a, b)| {
        let edge = [b[0] - a[0], b[1] - a[1]];
        let point = [p[0] - a[0], p[1] - a[1]];
        let length_squared = edge[0] * edge[0] + edge[1] * edge[1];
        let t = ((point[0] * edge[0] + point[1] * edge[1]) / length_squared).clamp(0., 1.);
        let distance_squared = (point[0] - t * edge[0]).powi(2) + (point[1] - t * edge[1]).powi(2);
        distance_squared <= FLOOR_EDGE_TOLERANCE * FLOOR_EDGE_TOLERANCE
    });
    near_edge.then_some([u, v])
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
    fn walking_climbs_and_descends_a_ramp_above_an_overlapping_floor() {
        let mut floor = CollisionGroup {
            surface: 0,
            vertices: vec![
                [0., -100., 0.],
                [200., -100., 0.],
                [200., 300., 0.],
                [0., 300., 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let ramp = CollisionGroup {
            vertices: vec![
                [0., 0., 0.],
                [200., 0., 0.],
                [200., 200., 200.],
                [0., 200., 200.],
            ],
            ..floor.clone()
        };
        let ground = floor.clone();
        for point in &mut floor.vertices {
            point[2] = 300.;
        }
        let mesh = WalkMesh::new(&[ground, ramp, floor]).unwrap();
        let mut position = [100., 0., 0.];
        for direction in [1., -1.] {
            for _ in 0..30 {
                let next = mesh.move_by(position, [0., 4. * direction], 40., |_| false);
                assert!((next[1] - position[1] - 4. * direction).abs() < 0.001);
                assert!((next[2] - next[1]).abs() < 0.001, "{next:?}");
                assert_eq!(mesh.resolve_motion(next), Some(next));
                position = next;
            }
        }
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
        let projected = mesh.resolve_actor_motion([50., 50., 0.]).unwrap();
        assert!((projected[1] - 48.076923).abs() < 0.00001);
        assert!((projected[2] - 9.615385).abs() < 0.00001);
        assert!((mesh.height(projected, 32.).unwrap() - projected[2]).abs() < 0.00001);
    }
    #[test]
    fn clearance_crosses_floor_seams_without_extending_edges_or_bridging_gaps() {
        for width in [50., 3000.] {
            for angle in [0_f32, 0.61, 1.57] {
                let rotate = |[x, y, z]: [f32; 3]| {
                    [
                        x * angle.cos() - y * angle.sin(),
                        x * angle.sin() + y * angle.cos(),
                        z,
                    ]
                };
                for gap in [0.2, 2., 3.5, 8.] {
                    let floor = |low, high, z, surface| CollisionGroup {
                        surface,
                        vertices: [
                            [-width, low, z],
                            [width, low, z],
                            [width, high, z],
                            [-width, high, z],
                        ]
                        .map(rotate)
                        .to_vec(),
                        triangles: vec![[0, 1, 2], [0, 2, 3]],
                    };
                    let a = floor(-200., -gap * 0.5, 0., 1);
                    let b = floor(gap * 0.5, 200., 0., 2);
                    for reverse in [false, true] {
                        let mut groups = [a.clone(), b.clone()];
                        if reverse {
                            for group in &mut groups {
                                for triangle in &mut group.triangles {
                                    triangle.reverse();
                                }
                            }
                        }
                        let mesh = WalkMesh::new(&groups).unwrap();
                        for direction in [-1., 1.] {
                            let mut position = rotate([0., -60. * direction, 0.]);
                            let delta = rotate([0., 4. * direction, 0.]);
                            for _ in 0..30 {
                                position =
                                    mesh.move_by(position, [delta[0], delta[1]], 40., |_| false);
                            }
                            let along = (-position[0] * angle.sin() + position[1] * angle.cos())
                                * direction;
                            if gap <= MOVEMENT_SUBSTEP {
                                assert!((along - 60.).abs() < 0.01, "{gap} {angle} {position:?}");
                                let surface =
                                    mesh.ground_surface(rotate([0., -gap * 0.2, 0.])).unwrap();
                                assert_eq!(surface.attributes, 1);
                                assert!(surface.normal.iter().all(|n| n.is_finite()));
                            } else {
                                assert!(along < 0.);
                            }
                        }
                        assert!(
                            mesh.ground_surface(rotate([width + 1., -100., 0.]))
                                .is_none()
                        );
                    }
                    if gap > FLOOR_EDGE_TOLERANCE * 2. {
                        let barrier = floor(-gap * 0.5, gap * 0.5, 0., 1 << 19);
                        let mesh = WalkMesh::new(&[a.clone(), b, barrier]).unwrap();
                        assert!(mesh.resolve_motion(rotate([0., 0., 0.])).is_none());
                    }
                    // A high ledge and a forbidden floor cannot complete a seam.
                    for b in [
                        floor(gap * 0.5, 200., 100., 2),
                        floor(gap * 0.5, 200., 0., 1 << 19),
                    ] {
                        let mesh = WalkMesh::new(&[a.clone(), b]).unwrap();
                        if gap > FLOOR_EDGE_TOLERANCE * 2. {
                            assert!(mesh.resolve_motion(rotate([0., 0., 0.])).is_none());
                        }
                    }
                }
            }
        }
        let floor = CollisionGroup {
            surface: 0,
            vertices: vec![
                [-100., -100., 0.],
                [0., -100., 0.],
                [0., 100., 0.],
                [-100., 100., 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let line = CollisionGroup {
            surface: 0,
            vertices: vec![[3., -20., 0.], [3., 0., 0.], [3., 20., 0.]],
            triangles: vec![[0, 1, 2]],
        };
        assert!(
            WalkMesh::new(&[floor, line])
                .unwrap()
                .ground_surface([1.5, 0., 0.])
                .is_none()
        );
        // A tolerance must not extend an acute corner into a long strip.
        assert!(
            height(
                [[0., 0., 0.], [3000., 0., 0.], [3000., 1., 0.]],
                [-1., 0., 0.]
            )
            .is_none()
        );
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
