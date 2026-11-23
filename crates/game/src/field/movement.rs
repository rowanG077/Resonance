use super::navigation::WalkMesh;
use resonance_content::field::{CollisionQuery, SLIDING_SURFACE};
use resonance_events::{ACTOR_CONTACT_HEIGHT, Actor, ActorContact, GameWorld};

pub(super) const SLIDING_SPEED: f32 = 8.;

/// Allow a scripted NPC to separate from an overlapping body.
pub(super) fn body_blocks_step(
    start: [f32; 3],
    end: [f32; 3],
    obstacle: [f32; 3],
    radius: f32,
) -> bool {
    let offset = [start[0] - obstacle[0], start[1] - obstacle[1]];
    let distance_squared =
        |p: [f32; 3]| (p[0] - obstacle[0]).powi(2) + (p[1] - obstacle[1]).powi(2);
    distance_squared(end) < radius * radius
        && (distance_squared(start) >= radius * radius
            || (end[0] - start[0]) * offset[0] + (end[1] - start[1]) * offset[1] < 0.)
}

/// Motion retained between player-controlled field updates.
#[derive(Clone, Default)]
pub(super) struct PlayerMovement {
    instance: Option<u64>,
    position: [f32; 2],
    velocity: [f32; 2],
    blocked_updates: u8,
}

impl PlayerMovement {
    fn velocity(&self, actor: &Actor) -> [f32; 2] {
        if self.instance == Some(actor.instance) && self.position == actor.position[..2] {
            self.velocity
        } else {
            // Actor replacement and authored repositioning discard old momentum.
            [0.; 2]
        }
    }

    pub(super) fn sliding(&self, actor: &Actor) -> bool {
        actor.ground_attributes & SLIDING_SURFACE != 0 && self.velocity(actor) != [0.; 2]
    }

    pub(super) fn delta(&self, actor: &Actor, input: [f32; 2], sliding: bool) -> [f32; 2] {
        if !sliding {
            return input;
        }
        let previous = self.velocity(actor);
        let velocity = if previous == [0.; 2] { input } else { previous };
        let axis = usize::from(velocity[0].abs() <= velocity[1].abs());
        let mut delta = [0.; 2];
        if velocity[axis] != 0. {
            delta[axis] = velocity[axis].signum() * velocity[axis].abs().max(SLIDING_SPEED);
        }
        delta
    }

    pub(super) fn clear_contact_delay(&mut self) {
        self.blocked_updates = 0;
    }

    pub(super) fn step(
        &mut self,
        mesh: &WalkMesh,
        world: &GameWorld,
        player: &Actor,
        requested: [f32; 2],
    ) -> ([f32; 3], Option<i32>) {
        const SQUEEZE_DELAY: u8 = 60;
        const MODEL_PROBE_HEIGHT: f32 = 45.;
        let start = player.position;
        let terrain_step = |motion| {
            mesh.move_by(start, motion, world.player_size.floor_clearance(), |p| {
                mesh.blocked(
                    [p[0], p[1], p[2] + MODEL_PROBE_HEIGHT],
                    CollisionQuery::Player,
                    player.instance,
                )
            })
        };
        let mut motion = requested;
        let mut eased = false;
        let mut contact = None;
        if !player.collidable {
            return (terrain_step(requested), None);
        }
        if motion != [0.; 2] && player.contact == ActorContact::Cylinder {
            let mut nearby: Vec<_> = world
                .actors
                .iter()
                .filter(|&(&id, other)| {
                    let radius = player.radius + other.radius;
                    id != world.controlled_actor
                        && other.visible
                        && other.collidable
                        && other.contact == ActorContact::Cylinder
                        && (start[2] - other.position[2]).abs() <= ACTOR_CONTACT_HEIGHT
                        && (start[0] - other.position[0]).abs() <= radius + requested[0].abs()
                        && (start[1] - other.position[1]).abs() <= radius + requested[1].abs()
                })
                .collect();
            nearby.sort_by_key(|(_, other)| other.update_order());
            for (&id, other) in nearby {
                if motion == [0.; 2] {
                    break;
                }
                let radius = player.radius + other.radius;
                let incoming = motion;
                let (clipped, hit) = clip_cylinder(start, incoming, other.position, radius);
                if !hit {
                    continue;
                }
                if contact.is_none() && other.contact_event {
                    // An attempted touch still needs a supported approach to the actor.
                    let reachable = terrain_step(incoming);
                    let step = [reachable[0] - start[0], reachable[1] - start[1]];
                    if clip_cylinder(start, step, other.position, radius).1 {
                        contact = Some(id);
                    }
                }
                let approach = (start[0] - other.position[0])
                    .mul_add(incoming[0], (start[1] - other.position[1]) * incoming[1]);
                motion = if self.blocked_updates >= SQUEEZE_DELAY && approach > -600. {
                    eased = true;
                    clip_cylinder(
                        start,
                        incoming,
                        other.position,
                        other.radius + player.radius * 0.5,
                    )
                    .0
                } else {
                    clipped
                };
            }
        }
        // Only a stop caused by bodies counts toward the delay. The floor and
        // model probes below may still remove an otherwise permitted component.
        if requested != [0.; 2] && motion == [0.; 2] {
            self.blocked_updates = self.blocked_updates.saturating_add(1);
        } else if !eased {
            self.blocked_updates = 0;
        }
        (terrain_step(motion), contact)
    }

    pub(super) fn record(&mut self, actor: &Actor, target: [f32; 3]) {
        self.instance = Some(actor.instance);
        self.position = [target[0], target[1]];
        self.velocity = [target[0] - actor.position[0], target[1] - actor.position[1]];
    }
}

/// Clip X first, then test Y from the accepted X position.
fn clip_cylinder(
    start: [f32; 3],
    mut delta: [f32; 2],
    other: [f32; 3],
    radius: f32,
) -> ([f32; 2], bool) {
    let inside = |x: f32, y: f32| (x - other[0]).hypot(y - other[1]) < radius;
    let hit_x = inside(start[0] + delta[0], start[1]);
    if hit_x {
        delta[0] = 0.;
    }
    let hit_y = inside(start[0] + delta[0], start[1] + delta[1]);
    if hit_y {
        delta[1] = 0.;
    }
    (delta, hit_x || hit_y)
}

#[cfg(test)]
mod tests {
    use crate::field::{FieldInput, FieldSession, tests::empty_session};
    use resonance_content::field::CollisionGroup;
    use resonance_events::Actor;

    fn walking_from(start: [f32; 3], obstacles: &[[f32; 3]]) -> FieldSession {
        let mut field = empty_session();
        field.walkmesh = super::WalkMesh::new(&[CollisionGroup {
            surface: 0,
            vertices: vec![
                [-150., -300., 0.],
                [300., -300., 0.],
                [300., 120., 0.],
                [-70., 120., 0.],
                [-150., -60., 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3], [0, 3, 4]],
        }])
        .unwrap();
        let world = &mut field.events.world;
        world.input_enabled = true;
        world.controlled_actor = 1;
        world.insert_actor(1, Actor::new(1, start));
        for (index, &position) in obstacles.iter().enumerate() {
            let mut obstacle = Actor::new(2, position);
            obstacle.radius = 50.;
            world.insert_actor(100 - index as i32, obstacle);
        }
        field
    }

    fn step(field: &mut FieldSession, motion: [f32; 2]) -> [f32; 3] {
        field
            .step(FieldInput {
                direction: motion.map(|v| v / 12.),
                run: true,
                ..Default::default()
            })
            .unwrap();
        field.events.world.actors[&1].position
    }

    #[test]
    fn sustained_glancing_contact_releases_the_player_at_a_floor_edge() {
        let mut field = walking_from([-70., 60., 0.], &[[0.; 3]]);
        // Terrain stops the outward X step. The chest permits the Y component.
        let wedged = step(&mut field, [-8.5, -8.]);
        assert_eq!(wedged[0], -70.);
        assert!((wedged[1] - 52.).abs() < 0.001);
        for _ in 0..30 {
            assert_eq!(step(&mut field, [-8.5, -8.]), wedged);
        }
        for _ in 0..60 {
            assert_eq!(step(&mut field, [-1., -10.]), wedged);
        }
        assert!((step(&mut field, [-1., -10.])[1] - 42.).abs() < 0.001);
        for _ in 0..16 {
            let [x, y, _] = step(&mut field, [-1., -10.]);
            assert!(x.hypot(y) >= 71.);
        }
        assert!(step(&mut field, [-1., -10.])[1] < -92.);
    }

    #[test]
    fn open_floor_contact_eases_for_a_slow_head_on_approach() {
        let start = [93., 0., 0.];
        let mut field = walking_from(start, &[[0.; 3], [186., 0., 0.]]);
        for _ in 0..70 {
            assert_eq!(step(&mut field, [-12., 0.]), start);
        }
        step(&mut field, [0.; 2]);
        // Switching cylinders preserves the delay, as does pausing collision.
        for dx in [3., -3.] {
            for _ in 0..30 {
                assert_eq!(step(&mut field, [dx, 0.]), start);
            }
        }
        field.events.world.mapped_input_disabled = true;
        field.events.world.actors.get_mut(&1).unwrap().motion = None;
        assert_eq!(step(&mut field, [0.; 2]), start);
        field.events.world.mapped_input_disabled = false;
        assert_eq!(step(&mut field, [-3., 0.]), [90., 0., 0.]);
        for _ in 0..20 {
            assert!(step(&mut field, [-3., 0.])[0] >= 71.);
        }
        let inside = step(&mut field, [-3., 0.]);
        assert!(inside[0] < 74.);

        // A field update with player input locked interrupts the push.
        field.events.world.input_enabled = false;
        field.events.world.actors.get_mut(&1).unwrap().motion = None;
        assert_eq!(step(&mut field, [3., 0.]), inside);
        field.events.world.input_enabled = true;
        for _ in 0..60 {
            assert_eq!(step(&mut field, [3., 0.]), inside);
        }
        assert!(step(&mut field, [3., 0.])[0] > inside[0]);
    }

    #[test]
    fn cylinders_clip_both_axes_in_actor_slot_order_before_floor_clearance() {
        let start = [0., -150., 0.];
        let a = [-74., -94., 0.];
        let b = [96., -150., 0.];
        // B clips X. A only clips Y if it sees that change first.
        assert_eq!(
            step(&mut walking_from(start, &[a, b]), [8., 8.]),
            [0., -142., 0.]
        );
        assert_eq!(step(&mut walking_from(start, &[b, a]), [8., 8.]), start);

        // The cylinder clips X and the cliff clips Y. Holding this diagonal
        // cannot move the player or build up the delay for a later push.
        let start = [-70., 90., 0.];
        let mut field = walking_from(start, &[[0., 30., 0.]]);
        for _ in 0..70 {
            assert_eq!(step(&mut field, [0.6, 10.]), start);
        }
        assert_eq!(step(&mut field, [2., -2.]), start);
    }
}
