//! Shared block controls; room scripts retain ownership of puzzle responses.
use super::{FieldInput, navigation::WalkMesh};
use resonance_content::field::ServiceMotion;
use resonance_events::input::Button;
use resonance_events::{Animation, EventRuntime, GameWorld};

const GRIP_DISTANCE: f32 = 125.;
const CELL: f32 = 150.;
const STEP: f32 = CELL / MOVE_UPDATES as f32;
const MOVE_UPDATES: u8 = 50;
const STICK_THRESHOLD: f32 = 0.75;
const BLEND_UPDATES: u32 = 2;
const HOLD_CLIP: u16 = ServiceMotion::HoldBlock as u16;
const PUSH_CLIP: u16 = ServiceMotion::PushBlock as u16;
const PULL_CLIP: u16 = ServiceMotion::PullBlock as u16;

#[derive(Clone, Copy)]
enum Direction {
    North,
    East,
    South,
    West,
}
impl Direction {
    fn nearest([x, y]: [f32; 2]) -> Self {
        match (x.abs() > y.abs(), x > 0., y > 0.) {
            (true, true, _) => Self::East,
            (true, false, _) => Self::West,
            (false, _, true) => Self::South,
            (false, _, false) => Self::North,
        }
    }
    fn vector(self) -> [f32; 2] {
        match self {
            Self::North => [0., -1.],
            Self::East => [1., 0.],
            Self::South => [0., 1.],
            Self::West => [-1., 0.],
        }
    }
    fn heading(self) -> f32 {
        match self {
            Self::North => 0.,
            Self::East => 90.,
            Self::South => 180.,
            Self::West => 270.,
        }
    }
}
#[derive(Clone, Copy)]
enum Movement {
    Push,
    Pull,
}
enum Phase {
    Holding,
    Moving {
        delta: [f32; 2],
        remaining: u8,
        movement: Movement,
    },
}
struct Grip {
    block: i32,
    instance: u64,
    player: i32,
    player_instance: u64,
    facing: Direction,
    phase: Phase,
}
#[derive(Default)]
pub(super) struct Blocks {
    grip: Option<Grip>,
}
impl Blocks {
    pub fn settle(&mut self, world: &mut GameWorld, mesh: &WalkMesh) {
        mesh.with_actors(world.actors.values())
            .settle_scenery(world, self.moving());
    }
    pub fn active(&self) -> bool {
        self.grip.is_some()
    }
    pub fn moving(&self) -> Option<i32> {
        self.grip
            .as_ref()
            .filter(|g| matches!(g.phase, Phase::Moving { .. }))
            .map(|g| g.block)
    }
    pub fn target(world: &GameWorld) -> Option<i32> {
        let player = world.actors.get(&world.controlled_actor)?;
        world.actor_order().iter().copied().find(|id| {
            world.actors.get(id).is_some_and(|block| {
                block.pushable && block.visible && super::within_interaction_reach(player, block)
            })
        })
    }
    pub fn step(&mut self, events: &mut EventRuntime, mesh: &WalkMesh, input: FieldInput) {
        if !self.active() && (!events.player_has_control() || !input.pressed(Button::Accept)) {
            return;
        }
        let mesh = &mesh.with_actors(events.world.actors.values());
        let service_clips = [HOLD_CLIP, PUSH_CLIP, PULL_CLIP].map(|slot| {
            let duration = events
                .world
                .actors
                .get(&events.world.controlled_actor)
                .and_then(|a| {
                    events.resources().animations.get(
                        &(resonance_content::field::FIELD_SERVICE_MOTION_RESOURCE_BASE
                            + a.resource),
                    )
                })
                .and_then(|clips| clips.get(&slot))
                .map(|clip| clip.duration_ticks);
            (slot, duration)
        });
        let world = &mut events.world;
        if self.grip.is_none() {
            if !world.input_enabled || world.mapped_input_disabled || !input.pressed(Button::Accept)
            {
                return;
            }
            let Some(id) = Self::target(world) else {
                return;
            };
            if !mesh.block_supported(&world.actors[&id]) {
                return;
            }
            let block = &world.actors[&id];
            let (position, instance) = (block.position, block.instance);
            let player = world.actors.get_mut(&world.controlled_actor).unwrap();
            let facing = Direction::nearest([
                position[0] - player.position[0],
                position[1] - player.position[1],
            ]);
            let direction = facing.vector();
            for axis in 0..2 {
                player.position[axis] = position[axis] - direction[axis] * GRIP_DISTANCE;
            }
            player.face(facing.heading());
            player.motion = None;
            self.grip = Some(Grip {
                block: id,
                instance,
                player: world.controlled_actor,
                player_instance: player.instance,
                facing,
                phase: Phase::Holding,
            });
            world.grabbed_block = Some(id);
            world.input_enabled = false;
            return;
        }
        let grip = self.grip.as_mut().unwrap();
        let valid = world.controlled_actor == grip.player
            && world
                .actors
                .get(&grip.block)
                .is_some_and(|a| a.instance == grip.instance && a.pushable)
            && world
                .actors
                .get(&grip.player)
                .is_some_and(|a| a.instance == grip.player_instance);
        let release = !valid
            || (matches!(grip.phase, Phase::Holding)
                && (!world.input.held.contains(Button::Accept)
                    || world.mapped_input_disabled
                    || !mesh.block_supported(&world.actors[&grip.block])));
        if release {
            if let Some(player) = world
                .actors
                .get_mut(&grip.player)
                .filter(|a| a.instance == grip.player_instance)
            {
                player.scripted_animation = false;
            }
            self.grip = None;
            world.grabbed_block = None;
            world.input_enabled = true;
            return;
        }
        world.input_enabled = false;
        let mut clip = HOLD_CLIP;
        if matches!(grip.phase, Phase::Holding) {
            let forward = world.field_camera.as_ref().map_or([0., 1.], |c| {
                [c.target[0] - c.position[0], c.target[1] - c.position[1]]
            });
            let forward = Direction::nearest(forward).vector();
            let stick = input.direction;
            let intent = [
                forward[1] * stick[0] + forward[0] * stick[1],
                -forward[0] * stick[0] + forward[1] * stick[1],
            ];
            let direction = grip.facing.vector();
            let along = intent[0] * direction[0] + intent[1] * direction[1];
            let movement = if along > STICK_THRESHOLD {
                Some(Movement::Push)
            } else if along < -STICK_THRESHOLD {
                Some(Movement::Pull)
            } else {
                None
            };
            if let Some(movement) = movement {
                let delta = direction.map(|v| {
                    v * if matches!(movement, Movement::Push) {
                        STEP
                    } else {
                        -STEP
                    }
                });
                if mesh.can_move_block(
                    &world.actors[&grip.block],
                    &world.actors[&world.controlled_actor],
                    delta.map(|v| v / STEP * CELL),
                ) {
                    grip.phase = Phase::Moving {
                        delta,
                        remaining: MOVE_UPDATES,
                        movement,
                    };
                }
            }
        }
        if let Phase::Moving {
            delta,
            remaining,
            movement,
        } = &mut grip.phase
        {
            clip = match movement {
                Movement::Push => PUSH_CLIP,
                Movement::Pull => PULL_CLIP,
            };
            for id in [grip.block, grip.player] {
                let actor = world.actors.get_mut(&id).unwrap();
                for (position, distance) in actor.position.iter_mut().zip(delta.iter()) {
                    *position += distance;
                }
            }
            *remaining -= 1;
            if *remaining == 0 {
                grip.phase = Phase::Holding;
            }
        }
        let player = world.actors.get_mut(&grip.player).unwrap();
        player.motion = None;
        let resource =
            resonance_content::field::FIELD_SERVICE_MOTION_RESOURCE_BASE + player.resource;
        if player
            .animation
            .as_ref()
            .is_none_or(|a| a.resource != resource || a.slot != clip)
            && let Some(duration) = service_clips
                .into_iter()
                .find_map(|(slot, duration)| (slot == clip).then_some(duration).flatten())
        {
            let mut animation = Animation::new(resource, clip, duration, world.tick + 1);
            animation.source = resonance_events::animation::AnimationSource::Resource;
            animation.blend_ticks = BLEND_UPDATES;
            animation.repeat = clip != HOLD_CLIP;
            player.animation = Some(animation);
            player.scripted_animation = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::field::{
        CollisionGroup, FIELD_SERVICE_MOTION_RESOURCE_BASE, ModelCollision,
    };
    use resonance_events::{Actor, ActorRole, AnimationClip, ResourceLibrary};
    use std::sync::Arc;

    fn room() -> (EventRuntime, WalkMesh, Blocks) {
        let mut resources = ResourceLibrary::default();
        resources.animations.insert(
            FIELD_SERVICE_MOTION_RESOURCE_BASE + 1,
            [HOLD_CLIP, PUSH_CLIP, PULL_CLIP]
                .into_iter()
                .map(|slot| {
                    (
                        slot,
                        AnimationClip {
                            duration_ticks: 60,
                            attachments: None,
                        },
                    )
                })
                .collect(),
        );
        let program =
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap();
        let mut events = EventRuntime::new(Arc::new(program), Arc::new(resources)).unwrap();
        events.world.controlled_actor = 1;
        events.world.input_enabled = true;
        events.world.insert_actor(1, Actor::new(1, [0., 120., 0.]));
        let mut block = Actor::new(2, [0.; 3]);
        block.pushable = true;
        block.radius = 50.;
        let cube = resonance_content::test_support::cuboid([-75., -75., 0.], [75., 75., CELL]);
        block.model_collision = Some(Arc::new(ModelCollision {
            floors: vec![cube.clone()],
            solids: vec![cube],
        }));
        events.world.insert_actor(2, block);
        (events, floor(1000.), Blocks::default())
    }
    fn floor(edge: f32) -> WalkMesh {
        WalkMesh::new(&[CollisionGroup {
            surface: 0,
            vertices: vec![
                [-1000., -1000., 0.],
                [1000., -1000., 0.],
                [1000., edge, 0.],
                [-1000., edge, 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        }])
        .unwrap()
    }
    fn step(events: &mut EventRuntime, mesh: &WalkMesh, blocks: &mut Blocks, input: FieldInput) {
        events
            .world
            .input
            .sample(input.held_buttons, input.pressed_buttons);
        blocks.step(events, mesh, input);
        events.step().unwrap();
    }
    fn held(direction: [f32; 2]) -> FieldInput {
        FieldInput {
            held_buttons: [Button::Accept].into_iter().collect(),
            direction,
            ..Default::default()
        }
    }
    fn grab(events: &mut EventRuntime, mesh: &WalkMesh, blocks: &mut Blocks) {
        step(
            events,
            mesh,
            blocks,
            FieldInput {
                pressed_buttons: [Button::Accept].into(),
                ..Default::default()
            },
        );
    }
    fn hold_cell(
        events: &mut EventRuntime,
        mesh: &WalkMesh,
        blocks: &mut Blocks,
        direction: [f32; 2],
    ) {
        for _ in 0..MOVE_UPDATES {
            step(events, mesh, blocks, held(direction));
        }
    }
    #[test]
    fn pushing_and_pulling_select_the_native_motion_banks() {
        for (direction, slot, displacement) in [([0., -1.], 36, -150.), ([0., 1.], 40, 150.)] {
            let (mut events, mesh, mut blocks) = room();
            grab(&mut events, &mesh, &mut blocks);
            hold_cell(&mut events, &mesh, &mut blocks, direction);
            assert_eq!(events.world.actors[&2].position, [0., displacement, 0.]);
            let animation = events.world.actors[&1].animation.as_ref().unwrap();
            assert_eq!(animation.resource, FIELD_SERVICE_MOTION_RESOURCE_BASE + 1);
            assert_eq!(animation.slot, slot);
        }
    }
    #[test]
    fn property_enabled_block_can_be_grabbed_moved_and_disabled() {
        let (mut events, mesh, mut blocks) = room();
        let block = events.world.actors.get_mut(&2).unwrap();
        block.role = ActorRole::Ordinary;
        block.pushable = true;
        grab(&mut events, &mesh, &mut blocks);
        assert_eq!(events.world.grabbed_block, Some(2));
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0., -150., 0.]);
        events.world.actors.get_mut(&2).unwrap().pushable = false;
        step(&mut events, &mesh, &mut blocks, held([0.; 2]));
        assert_eq!(events.world.grabbed_block, None);
        assert!(events.player_has_control());
    }

    #[test]
    fn gripping_uses_the_closest_face() {
        for (offset, direction) in [
            ([89., -90.], [0., -1.]),
            ([90., -89.], [1., 0.]),
            ([-90., 89.], [-1., 0.]),
            ([89., 90.], [0., 1.]),
        ] {
            assert_eq!(Direction::nearest(offset).vector(), direction);
        }
    }
    #[test]
    fn released_button_finishes_the_cell_then_returns_control() {
        let (mut events, mesh, mut blocks) = room();
        grab(&mut events, &mesh, &mut blocks);
        assert_eq!(events.world.actors[&1].position, [0., 125., 0.]);
        step(&mut events, &mesh, &mut blocks, held([0., -1.]));
        let started = events.world.actors[&1]
            .animation
            .as_ref()
            .unwrap()
            .start_tick;
        for _ in 0..49 {
            step(&mut events, &mesh, &mut blocks, FieldInput::default());
        }
        assert_eq!(events.world.actors[&2].position, [0., -150., 0.]);
        assert_eq!(events.world.actors[&1].position, [0., -25., 0.]);
        assert_eq!(events.world.grabbed_block, Some(2));
        let animation = events.world.actors[&1].animation.as_ref().unwrap();
        assert_eq!((animation.slot, animation.start_tick), (PUSH_CLIP, started));
        assert!(!events.player_has_control());
        step(&mut events, &mesh, &mut blocks, FieldInput::default());
        assert_eq!(events.world.grabbed_block, None);
        assert!(events.player_has_control());
    }
    #[test]
    fn pulling_checks_the_players_body_and_actual_retreat_position() {
        for (floor_edge, obstacle, moves) in [
            (280., None, true),
            (270., None, false),
            (1000., Some(([-50., 230., 0.], [50., 240., 50.])), false),
            (1000., Some(([-50., 230., 160.], [50., 240., 250.])), true),
        ] {
            let (mut events, _, mut blocks) = room();
            let mesh = floor(floor_edge);
            if let Some((low, high)) = obstacle {
                let mut wall = Actor::new(3, [0.; 3]);
                wall.model_collision = Some(resonance_content::test_support::solid_box(low, high));
                events.world.insert_actor(3, wall);
            }
            grab(&mut events, &mesh, &mut blocks);
            hold_cell(&mut events, &mesh, &mut blocks, [0., 1.]);
            let moved = if moves { CELL } else { 0. };
            assert_eq!(events.world.actors[&2].position, [0., moved, 0.]);
            assert_eq!(
                events.world.actors[&1].position,
                [0., GRIP_DISTANCE + moved, 0.]
            );
        }
    }

    #[test]
    fn blocks_cannot_be_pushed_into_each_other() {
        let (mut events, mesh, mut blocks) = room();
        let mut obstacle = events.world.actors[&2].clone();
        obstacle.position = [0., -150., 0.];
        events.world.insert_actor(3, obstacle);
        grab(&mut events, &mesh, &mut blocks);
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0., 0., 0.]);
        events.world.actors.get_mut(&3).unwrap().model_collision = None;
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0., -150., 0.]);
    }

    #[test]
    fn blocks_cross_a_slightly_raised_filled_pit_and_leave_it_again() {
        let (mut events, mesh, mut blocks) = room();
        let mut filled = events.world.actors[&2].clone();
        filled.pushable = false;
        filled.position = [0., -2. * CELL, 1. - CELL];
        // Scenery solids already include clearance for a walking character.
        let collision = Arc::make_mut(filled.model_collision.as_mut().unwrap());
        for vertex in &mut collision.solids[0].vertices {
            vertex[0] *= 115. / 75.;
            vertex[1] *= 115. / 75.;
        }
        events.world.insert_actor(3, filled);
        grab(&mut events, &mesh, &mut blocks);
        for cell in 1..=3 {
            if cell == 2 {
                let mut ceiling = Actor::new(4, [0.; 3]);
                ceiling.model_collision = Some(resonance_content::test_support::solid_box(
                    [-75., -375., CELL + 0.5],
                    [75., -225., 2. * CELL],
                ));
                events.world.insert_actor(4, ceiling);
                hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
                assert_eq!(events.world.actors[&2].position[1], -CELL);
                events.world.actors.remove(&4);
            }
            for _ in 0..MOVE_UPDATES {
                blocks.settle(&mut events.world, &mesh);
                step(&mut events, &mesh, &mut blocks, held([0., -1.]));
            }
            blocks.settle(&mut events.world, &mesh);
            assert_eq!(events.world.actors[&2].position[1], -CELL * cell as f32);
        }
        assert_eq!(events.world.actors[&2].position[2], 0.);
    }

    #[test]
    fn a_pit_allows_a_push_and_then_a_fall_onto_the_floor_below() {
        let (mut events, _, mut blocks) = room();
        let lower = events.world.actors[&2].clone();
        events.world.insert_actor(3, lower);
        for id in [1, 2] {
            events.world.actors.get_mut(&id).unwrap().position[2] = CELL;
        }
        let mesh = WalkMesh::new(
            &[
                (resonance_content::field::CollisionQuery::Block as u32, 0.),
                ((1 << 19) | (1 << 22), CELL),
            ]
            .map(|(surface, z)| CollisionGroup {
                surface,
                vertices: vec![
                    [-1000., -1000., z],
                    [1000., -1000., z],
                    [1000., 1000., z],
                    [-1000., 1000., z],
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
            }),
        )
        .unwrap();
        grab(&mut events, &mesh, &mut blocks);
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0., -CELL, CELL]);
        for _ in 0..20 {
            blocks.settle(&mut events.world, &mesh);
            step(&mut events, &mesh, &mut blocks, held([0., -1.]));
        }
        assert_eq!(events.world.actors[&2].position, [0., -CELL, 0.]);
        assert!(events.player_has_control());
    }

    #[test]
    fn a_forbidden_surface_cannot_be_bypassed_using_the_floor_below() {
        let (mut events, _, mut blocks) = room();
        let mut ground = CollisionGroup {
            surface: 0,
            vertices: vec![
                [-1000., -1000., 0.],
                [1000., -1000., 0.],
                [1000., 1000., 0.],
                [-1000., 1000., 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let mut tile = ground.clone();
        tile.surface = resonance_content::field::CollisionQuery::Block as u32;
        tile.vertices = vec![
            [-100., -225., 20.],
            [100., -225., 20.],
            [100., -75., 20.],
            [-100., -75., 20.],
        ];
        let mesh = WalkMesh::new(&[ground.clone(), tile.clone()]).unwrap();
        grab(&mut events, &mesh, &mut blocks);
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0.; 3]);

        // An allowed platform above the tile supplies the actual supporting floor.
        for vertex in &mut ground.vertices {
            vertex[2] = 30.;
        }
        let mesh = WalkMesh::new(&[ground, tile]).unwrap();
        for id in [1, 2] {
            events.world.actors.get_mut(&id).unwrap().position[2] = 30.;
        }
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0., -CELL, 30.]);
    }

    #[test]
    fn the_moving_block_cannot_supply_the_players_floor_over_a_gap() {
        let (mut events, _, mut blocks) = room();
        let platform = CollisionGroup {
            surface: 0,
            vertices: vec![
                [-1000., 0., 0.],
                [1000., 0., 0.],
                [1000., 1000., 0.],
                [-1000., 1000., 0.],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let mut pit = platform.clone();
        for vertex in &mut pit.vertices {
            vertex[1] -= 1000.;
            vertex[2] -= CELL;
        }
        let mesh = WalkMesh::new(&[platform, pit]).unwrap();
        grab(&mut events, &mesh, &mut blocks);
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0.; 3]);
        assert_eq!(events.world.actors[&1].position, [0., GRIP_DISTANCE, 0.]);
    }

    #[test]
    fn pushing_cannot_cross_a_thin_wall() {
        let (mut events, mesh, mut blocks) = room();
        let mut wall = events.world.actors[&2].clone();
        wall.position = [0., -100., 0.];
        let geometry = Arc::make_mut(wall.model_collision.as_mut().unwrap());
        geometry.floors.clear();
        for vertex in &mut geometry.solids[0].vertices {
            vertex[1] *= 0.02;
        }
        events.world.insert_actor(3, wall);
        grab(&mut events, &mesh, &mut blocks);
        hold_cell(&mut events, &mesh, &mut blocks, [0., -1.]);
        assert_eq!(events.world.actors[&2].position, [0.; 3]);
    }

    #[test]
    fn model_support_overrides_pit_planes_but_disabled_collision_does_not() {
        for plane in [149., 150., 151.] {
            let (mut events, _, mut blocks) = room();
            let mesh = WalkMesh::new(&[CollisionGroup {
                surface: (1 << 19) | (1 << 22),
                vertices: vec![
                    [-100., -100., plane],
                    [100., -100., plane],
                    [0., 100., plane],
                ],
                triangles: vec![[0, 1, 2]],
            }])
            .unwrap();
            let lower = events.world.actors.get_mut(&2).unwrap();
            lower.pushable = false;
            let mut upper = lower.clone();
            upper.pushable = true;
            upper.position[2] = 149.;
            events.world.insert_actor(3, upper);
            blocks.settle(&mut events.world, &mesh);
            assert_eq!(events.world.actors[&3].position[2], 150.);
            assert!(
                mesh.with_actors(events.world.actors.values())
                    .block_supported(&events.world.actors[&3])
            );
            events.world.actors.get_mut(&2).unwrap().model_collision = None;
            events.world.actors.get_mut(&3).unwrap().position[2] = plane;
            assert!(
                !mesh
                    .with_actors(events.world.actors.values())
                    .block_supported(&events.world.actors[&3])
            );
            blocks.settle(&mut events.world, &mesh);
            assert_eq!(events.world.actors[&3].position[2], plane - 9.);
        }
    }

    #[test]
    fn falling_blocks_land_without_overshoot_and_ignore_model_undersides() {
        let (mut events, mesh, mut blocks) = room();
        events.world.actors.get_mut(&2).unwrap().position[2] = 70.;
        for tick in 0..10 {
            blocks.settle(&mut events.world, &mesh);
            assert!(events.world.actors[&2].position[2] >= 0.);
            if tick == 1 {
                grab(&mut events, &mesh, &mut blocks);
                assert_eq!(events.world.grabbed_block, None);
            }
        }
        assert_eq!(events.world.actors[&2].position[2], 0.);
        let mut ceiling = events.world.actors[&2].clone();
        ceiling.position[2] = 50.;
        ceiling.pushable = false;
        events.world.insert_actor(3, ceiling);
        events.world.actors.get_mut(&2).unwrap().position[2] = 5.;
        blocks.settle(&mut events.world, &mesh);
        assert_eq!(events.world.actors[&2].position[2], 0.);
    }

    #[test]
    #[ignore = "requires locally cooked fields; no devices"]
    fn martel_block_stays_on_a_filled_pit_after_pushing() -> anyhow::Result<()> {
        let root = std::env::var_os("RESONANCE_WORLD_ASSETS")
            .ok_or_else(|| anyhow::anyhow!("set RESONANCE_WORLD_ASSETS"))?;
        let field: resonance_content::field::FieldAssets = serde_json::from_slice(&std::fs::read(
            std::path::Path::new(&root).join("fields/map-308.json"),
        )?)?;
        let collision = Arc::new(
            field
                .actors
                .iter()
                .find(|a| a.resource == 267)
                .unwrap()
                .collision
                .clone(),
        );
        assert!(!collision.floors.is_empty());
        assert!(!collision.solids.is_empty());
        let mesh = WalkMesh::new(&field.ground)?;
        let (mut events, _, mut blocks) = room();
        let z = mesh.height([-1190., -1375., -800.], 10.).unwrap();
        let block = events.world.actors.get_mut(&2).unwrap();
        block.position = [-1190., -1375., z];
        block.model_collision = Some(collision.clone());
        let player = events.world.actors.get_mut(&1).unwrap();
        player.position = [-1065., -1375., z];
        player.face(270.);
        // The west pit is filled at this progression checkpoint.
        let mut filled = Actor::new(267, [-1340., -1375., -949.]);
        filled.model_collision = Some(collision);
        events.world.insert_actor(5001, filled);
        grab(&mut events, &mesh, &mut blocks);
        assert_eq!(events.world.grabbed_block, Some(2));
        for _ in 0..50 {
            blocks.settle(&mut events.world, &mesh);
            step(&mut events, &mesh, &mut blocks, held([-1., 0.]));
        }
        assert_eq!(events.world.actors[&2].position[..2], [-1340., -1375.]);
        for _ in 0..60 {
            blocks.settle(&mut events.world, &mesh);
            let z = events.world.actors[&2].position[2];
            assert!(
                (z + 799.).abs() < 0.01,
                "block sank into the filled pit: {z}"
            );
        }
        hold_cell(&mut events, &mesh, &mut blocks, [1., 0.]);
        assert_eq!(events.world.actors[&2].position[..2], [-1190., -1375.]);
        Ok(())
    }
    #[test]
    fn pause_or_replaced_block_releases_the_grip() {
        for replace in [false, true] {
            let (mut events, mesh, mut blocks) = room();
            grab(&mut events, &mesh, &mut blocks);
            step(&mut events, &mesh, &mut blocks, held([0.; 2]));
            assert_eq!(
                events.world.actors[&1].animation.as_ref().unwrap().slot,
                HOLD_CLIP
            );
            if replace {
                let replacement = events.world.actors[&2].clone();
                events.world.insert_actor(2, replacement);
            } else {
                events.world.mapped_input_disabled = true;
            }
            step(&mut events, &mesh, &mut blocks, held([0., -1.]));
            assert_eq!(events.world.grabbed_block, None);
            assert!(!blocks.active());
            assert!(events.world.input_enabled);
        }
    }
}
