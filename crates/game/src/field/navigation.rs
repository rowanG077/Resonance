//! Field movement over cooked triangles, independent of rendering and scripts.
use anyhow::{Result, ensure};
use resonance_content::field::{CollisionGroup, CollisionQuery};
use std::sync::Arc;

const BLOCK_FALL_STEP: f32 = 9.;
const NO_BLOCK_SUPPORT: u32 = 1 << 22;
const BLOCK_FLOOR_REACH: f32 = 60.;

/// Lines and circles touch the player's radius; polygons test the player's center.
/// Both include the authored vertical span and the player's vertical radius.
pub fn touches_trigger(trigger: &resonance_events::Trigger, p: [f32; 3], radius: f32) -> bool {
    trigger.touches(p, radius)
}

pub struct WalkMesh {
    triangles: Arc<[([[f32; 3]; 3], u32)]>,
    model_floors: Vec<([[f32; 3]; 3], u32)>,
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
                        actor.position,
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
        position: [f32; 3],
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
        if delta.iter().map(|v| v * v).sum::<f32>().sqrt() >= range {
            return false;
        }
        let horizontal = delta[0].hypot(delta[1]);
        if horizontal == 0. {
            return false;
        }
        let heading = heading.to_radians();
        let dot = (delta[0] * heading.sin() - delta[1] * heading.cos()) / horizontal;
        let angle = if rate == 2 { angle * 2. } else { angle }.clamp(0., 360.);
        if dot < (angle.to_radians() * 0.5).cos() {
            return false;
        }
        // Native query 0x41 samples static field triangles, excluding surface
        // bit 19, at head height with a 600-unit vertical reach. It does not
        // include actor collision models (query bit 4 is absent).
        (1..=16).all(|step| {
            let mut point = std::array::from_fn(|i| position[i] + delta[i] * (step as f32 / 16.));
            point[2] += 150.;
            self.triangles.iter().any(|(triangle, attributes)| {
                CollisionQuery::Player.accepts(*attributes)
                    && height(*triangle, point).is_some_and(|z| (z - point[2]).abs() <= 600.)
            })
        })
    }

    pub(super) fn settle_scenery(
        &self,
        world: &mut resonance_events::GameWorld,
        moving: Option<i32>,
        falling: &mut std::collections::BTreeMap<i32, u64>,
    ) {
        // fn_8002122C steps downward by nine, then resolves the penetration on
        // the next update. Original puzzle callbacks observe that landing dip.
        falling.retain(|id, instance| {
            world
                .actors
                .get(id)
                .is_some_and(|a| a.instance == *instance && a.pushable())
        });
        let positions: Vec<_> = world
            .actors
            .iter()
            .filter(|(id, actor)| {
                moving != Some(**id) && actor.pushable() && actor.motion.is_none()
            })
            .map(|(&id, actor)| {
                let floor = self
                    .with_actors(
                        world
                            .actors
                            .iter()
                            .filter_map(|(&other, a)| (other != id).then_some(a)),
                    )
                    .block_surface(actor.position, falling.contains_key(&id));
                let z = if let Some((height, _)) = floor.filter(|(_, a)| a & NO_BLOCK_SUPPORT == 0)
                {
                    falling.remove(&id);
                    height
                } else {
                    falling.insert(id, actor.instance);
                    actor.position[2] - BLOCK_FALL_STEP
                };
                (id, z)
            })
            .collect();
        for (id, z) in positions {
            world.actors.get_mut(&id).unwrap().position[2] = z;
        }
    }
    pub(super) fn block_supported(&self, world: &resonance_events::GameWorld, id: i32) -> bool {
        let position = world.actors[&id].position;
        self.with_actors(
            world
                .actors
                .iter()
                .filter_map(|(&other, a)| (other != id).then_some(a)),
        )
        .block_surface(position, false)
        .is_some_and(|(_, attributes)| attributes & NO_BLOCK_SUPPORT == 0)
    }
    fn block_surface(&self, point: [f32; 3], falling: bool) -> Option<(f32, u32)> {
        // fn_8002E5F4 / fn_8002E188 keep the highest reachable floor, not
        // the closest plane. fn_8002EFD4 clears bit 22 after a model hit:
        // a filled pit remains solid even where its no-support plane overlaps.
        let mut model_support = false;
        self.model_floors
            .iter()
            .map(|surface| (surface, true))
            .chain(self.triangles.iter().map(|surface| (surface, false)))
            .filter_map(|((triangle, attributes), model)| {
                // Native model floors require an upward normal (Z > 0.2).
                // Collision packages can also contain the cube's underside.
                if model && surface_normal(*triangle)[2] <= 0.2 {
                    return None;
                }
                let z = height(*triangle, point)?;
                if !CollisionQuery::Block.accepts(*attributes)
                    || (z - point[2]).abs() > BLOCK_FLOOR_REACH
                    || (falling && z < point[2])
                {
                    return None;
                }
                model_support |= model;
                Some((z, *attributes))
            })
            .max_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(height, attributes)| {
                (
                    height,
                    if model_support {
                        attributes & !NO_BLOCK_SUPPORT
                    } else {
                        attributes
                    },
                )
            })
    }
    pub(super) fn can_move_block(
        &self,
        world: &resonance_events::GameWorld,
        id: i32,
        delta: [f32; 2],
        pulling: bool,
    ) -> bool {
        const PROBE_HEIGHT: f32 = 100.;
        const OVERHEAD: f32 = 225.;
        const FLOOR_REACH: f32 = 60.;
        const FOOT_DEPTH: f32 = 20.;
        let position = world.actors[&id].position;
        let mesh = self.with_actors(
            world
                .actors
                .iter()
                .filter_map(|(&other, a)| (other != id).then_some(a)),
        );
        let blocked = |point: [f32; 3], mask| {
            world
                .actors
                .iter()
                .any(|(&other, actor)| other != id && actor.contains_solid(point, mask))
        };
        let target = [
            position[0] + delta[0],
            position[1] + delta[1],
            position[2] - FOOT_DEPTH,
        ];
        let support = |point: [f32; 3], query: CollisionQuery| {
            mesh.surface_within(point, |z, attributes| {
                (z - point[2]).abs() <= FLOOR_REACH && query.accepts(attributes)
            })
            .is_some()
        };
        if blocked(
            [position[0], position[1], position[2] + OVERHEAD],
            CollisionQuery::Block,
        ) || !support(target, CollisionQuery::Block)
        {
            return false;
        }
        if pulling {
            let behind = [
                position[0] + delta[0] * 2.,
                position[1] + delta[1] * 2.,
                position[2] - FOOT_DEPTH,
            ];
            !blocked(
                [behind[0], behind[1], position[2] + PROBE_HEIGHT],
                CollisionQuery::Player,
            ) && support(behind, CollisionQuery::Player)
        } else {
            !blocked(
                [target[0], target[1], position[2] + PROBE_HEIGHT],
                CollisionQuery::Block,
            )
        }
    }
    fn surfaces(&self) -> impl Iterator<Item = &([[f32; 3]; 3], u32)> {
        self.model_floors.iter().chain(self.triangles.iter())
    }
    /// Snapshot live model floors without copying the map. Hidden scenery also
    /// carries floors; native property 48 only masks projectile contact.
    pub(super) fn with_actors<'a>(
        &self,
        actors: impl Iterator<Item = &'a resonance_events::Actor>,
    ) -> Self {
        let mut model_floors = Vec::new();
        for actor in actors {
            if let Some(mesh) = actor.model_collision.as_ref() {
                for group in &mesh.floors {
                    model_floors.extend(group.triangles.iter().map(|triangle| {
                        (
                            triangle.map(|i| actor.collision_point(group.vertices[usize::from(i)])),
                            group.surface,
                        )
                    }));
                }
            }
        }
        Self {
            triangles: self.triangles.clone(),
            model_floors,
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
        })
    }
    /// Keep an entrance on its floor, or use the nearest valid triangle when an
    /// unfinished setup script never placed the player.
    pub fn exploration_start(&self, point: [f32; 3]) -> Option<[f32; 3]> {
        if let Some(z) = self.height(point, f32::MAX) {
            return Some([point[0], point[1], z]);
        }
        self.surfaces()
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
    /// Select the closest reachable floor. This also preserves authored ramps
    /// and raised platforms without importing the original collision engine.
    pub fn height(&self, point: [f32; 3], max_step: f32) -> Option<f32> {
        self.walking_surface(point, |z| (z - point[2]).abs() <= max_step)
            .map(|surface| surface.height)
    }
    /// Tilt horizontal intent onto the floor and resolve penetration along its
    /// normal. Player movement applies pitch before roll; NPCs use the reverse.
    pub fn resolve_motion(
        &self,
        start: [f32; 3],
        proposed: [f32; 3],
        player: bool,
    ) -> Option<[f32; 3]> {
        let surface = self.walking_surface(proposed, |z| (z - proposed[2]).abs() <= 32.)?;
        Some(Self::resolve_surface(start, proposed, player, surface))
    }
    pub(super) fn resolve_enemy(&self, start: [f32; 3], proposed: [f32; 3]) -> Option<[f32; 3]> {
        // fn_800111D4 uses query 0xC4: enemies also reject surface bit20.
        // Doorways remain walkable for the player and ordinary field actors.
        let surface = self.surface_within(proposed, |z, attributes| {
            (z - proposed[2]).abs() <= 32. && CollisionQuery::Enemy.accepts(attributes)
        })?;
        Some(Self::resolve_surface(start, proposed, false, surface))
    }
    pub(super) fn resolve_player(
        &self,
        start: [f32; 3],
        proposed: [f32; 3],
        fall: &mut PlayerFall,
        event_paused: bool,
    ) -> Option<[f32; 3]> {
        // fn_8001D5F4: falling probes accept penetration, not a floor still
        // below the feet. A downward ray keeps unsupported actors over voids
        // in place; it also rejects a step that would pass through the floor.
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
            return Some(Self::resolve_surface(start, proposed, true, surface));
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
    fn resolve_surface(
        start: [f32; 3],
        proposed: [f32; 3],
        player: bool,
        surface: GroundSurface,
    ) -> [f32; 3] {
        let [nx, ny, nz] = surface.normal;
        let pitch = (-ny).clamp(-1., 1.).asin();
        let roll = nx.atan2(nz);
        let dx = proposed[0] - start[0];
        let dy = proposed[1] - start[1];
        let cross = pitch.sin() * roll.sin();
        [
            start[0] + dx * roll.cos() + if player { dy * cross } else { 0. },
            start[1] + dy * pitch.cos() + if player { 0. } else { dx * cross },
            start[2] + (surface.height - start[2]) * nz * nz,
        ]
    }
    pub fn surface(&self, point: [f32; 3], max_step: f32) -> Option<GroundSurface> {
        self.surface_within(point, |z, _| (z - point[2]).abs() <= max_step)
    }
    pub fn surface_below(&self, point: [f32; 3]) -> Option<GroundSurface> {
        self.surface_within(point, |z, _| z <= point[2])
    }
    fn walking_surface(
        &self,
        point: [f32; 3],
        accepts: impl Fn(f32) -> bool,
    ) -> Option<GroundSurface> {
        // Native walking queries set 0x40; collision dispatch then excludes
        // surface bit 19. Block probes and other surface queries retain it.
        self.surface_within(point, |z, attributes| {
            CollisionQuery::Player.accepts(attributes) && accepts(z)
        })
    }
    fn surface_within(
        &self,
        point: [f32; 3],
        accepts: impl Fn(f32, u32) -> bool,
    ) -> Option<GroundSurface> {
        self.surfaces()
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
    fn enemy_sight_checks_range_heading_bottles_and_the_static_path() {
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
        let sees = |point, rate| mesh.sees_player([0.; 3], 0., 90., 600., point, rate);
        assert!(sees([0., -300., 0.], 0));
        assert!(!sees([0., -300., 0.], 1));
        assert!(!sees([0., 300., 0.], 0));
        assert!(!sees([0., -600., 0.], 0));
        assert!(!sees([300., -100., 0.], 0));
        assert!(sees([300., -100., 0.], 2));
        assert!(!mesh.sees_player([0.; 3], 0., 90., 1000., [0., -100., 700.], 0));
        let gap = WalkMesh::new(&[floor([-700., -150.], 0), floor([-100., 700.], 0)]).unwrap();
        assert!(!gap.sees_player([0.; 3], 0., 90., 600., [0., -300., 0.], 0));
        let masked = WalkMesh::new(&[floor([-700., 700.], 1 << 19)]).unwrap();
        assert!(!masked.sees_player([0.; 3], 0., 90., 600., [0., -300., 0.], 0));
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
