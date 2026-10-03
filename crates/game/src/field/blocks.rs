//! Shared block controls; room scripts retain ownership of puzzle responses.
use super::{FieldInput, navigation::WalkMesh};
use resonance_content::field::ServiceMotion;
use resonance_events::{Animation, EventRuntime, GameWorld, input::Button};

const GRIP_DISTANCE: f32 = 125.;
const CELL: f32 = 150.;
const STEP: f32 = 3.;
const MOVE_UPDATES: u8 = 50;
// fn_8006F648 latches a direction above 60 on the native 80-unit stick range.
const STICK_THRESHOLD: f32 = 60. / 80.;
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
pub(super) struct Blocks(Option<Grip>);
impl Blocks {
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
    pub fn moving(&self) -> Option<i32> {
        self.0
            .as_ref()
            .filter(|g| matches!(g.phase, Phase::Moving { .. }))
            .map(|g| g.block)
    }
    pub fn target(world: &GameWorld) -> Option<i32> {
        let player = world.actors.get(&world.controlled_actor)?;
        world.actor_order().iter().copied().find(|id| {
            world.actors.get(id).is_some_and(|block| {
                block.pushable() && block.visible && super::within_interaction_reach(player, block)
            })
        })
    }
    pub fn step(&mut self, events: &mut EventRuntime, mesh: &WalkMesh, input: FieldInput) {
        if !self.active() && (!events.player_has_control() || !input.interact) {
            return;
        }
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
        if self.0.is_none() {
            if !world.input_enabled || world.mapped_input_disabled || !input.interact {
                return;
            }
            let Some(id) = Self::target(world) else {
                return;
            };
            if !mesh.block_supported(world, id) {
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
            self.0 = Some(Grip {
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
        let grip = self.0.as_mut().unwrap();
        let valid = world.controlled_actor == grip.player
            && world
                .actors
                .get(&grip.block)
                .is_some_and(|a| a.instance == grip.instance && a.pushable())
            && world
                .actors
                .get(&grip.player)
                .is_some_and(|a| a.instance == grip.player_instance);
        let release = !valid
            || (matches!(grip.phase, Phase::Holding)
                && (!world.input.held.contains(Button::Accept)
                    || world.mapped_input_disabled
                    || !mesh.block_supported(world, grip.block)));
        if release {
            if let Some(player) = world
                .actors
                .get_mut(&grip.player)
                .filter(|a| a.instance == grip.player_instance)
            {
                player.scripted_animation = false;
            }
            self.0 = None;
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
                    world,
                    grip.block,
                    delta.map(|v| v / STEP * CELL),
                    matches!(movement, Movement::Pull),
                ) {
                    grip.phase = Phase::Moving {
                        delta,
                        remaining: MOVE_UPDATES,
                        movement,
                    };
                }
            }
        }
        // fn_80020658/80020348 finish the whole cell even if Accept is released.
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
        block.role = ActorRole::Pushable;
        block.radius = 50.;
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
        events.world.input.sample(
            input.held_buttons,
            input
                .interact
                .then_some(Button::Accept)
                .into_iter()
                .collect(),
        );
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
    #[test]
    fn pushing_and_pulling_select_the_native_motion_banks() {
        for (direction, slot, displacement) in [([0., -1.], 36, -150.), ([0., 1.], 40, 150.)] {
            let (mut events, mesh, mut blocks) = room();
            step(
                &mut events,
                &mesh,
                &mut blocks,
                FieldInput {
                    interact: true,
                    ..Default::default()
                },
            );
            for _ in 0..50 {
                step(&mut events, &mesh, &mut blocks, held(direction));
            }
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
        block.properties.insert(19, 1);
        step(
            &mut events,
            &mesh,
            &mut blocks,
            FieldInput {
                interact: true,
                ..Default::default()
            },
        );
        assert_eq!(events.world.grabbed_block, Some(2));
        for _ in 0..50 {
            step(&mut events, &mesh, &mut blocks, held([0., -1.]));
        }
        assert_eq!(events.world.actors[&2].position, [0., -150., 0.]);
        events
            .world
            .actors
            .get_mut(&2)
            .unwrap()
            .properties
            .insert(19, 0);
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
        step(
            &mut events,
            &mesh,
            &mut blocks,
            FieldInput {
                interact: true,
                ..Default::default()
            },
        );
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
    fn pulling_needs_retreat_floor_and_both_moves_need_headroom() {
        let (mut events, _, _) = room();
        let ledge = floor(200.);
        assert!(ledge.can_move_block(&events.world, 2, [0., 150.], false));
        assert!(!ledge.can_move_block(&events.world, 2, [0., 150.], true));
        let mut ceiling = Actor::new(3, [0., 0., 200.]);
        ceiling.model_collision = Some(Arc::new(ModelCollision {
            solids: vec![CollisionGroup {
                surface: 0,
                vertices: vec![
                    [-500., -500., 0.],
                    [500., -500., 0.],
                    [0., 500., 0.],
                    [0., 0., 100.],
                ],
                triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]],
            }],
            ..Default::default()
        }));
        events.world.insert_actor(3, ceiling);
        assert!(!ledge.can_move_block(&events.world, 2, [0., -150.], false));
        assert!(!floor(1000.).can_move_block(&events.world, 2, [0., 150.], true));
        events.world.actors.get_mut(&3).unwrap().position = [0., -150., 50.];
        assert!(!floor(1000.).can_move_block(&events.world, 2, [0., -150.], false));
        assert!(floor(1000.).can_move_block(&events.world, 2, [0., 150.], true));
    }

    #[test]
    fn blocks_cannot_be_pushed_into_each_other() {
        let (mut events, mesh, mut blocks) = room();
        let mut obstacle = Actor::new(3, [0., -150., 0.]);
        obstacle.role = ActorRole::Pushable;
        obstacle.radius = 50.;
        events.world.insert_actor(3, obstacle);
        let mut upper = Actor::new(4, [0., 0., 150.]);
        upper.role = ActorRole::Pushable;
        upper.radius = 50.;
        events.world.insert_actor(4, upper);
        mesh.settle_scenery(&mut events.world, None);
        assert_eq!(events.world.actors[&4].position[2], 150.);
        step(
            &mut events,
            &mesh,
            &mut blocks,
            FieldInput {
                interact: true,
                ..Default::default()
            },
        );
        for _ in 0..50 {
            step(&mut events, &mesh, &mut blocks, held([0., -1.]));
        }
        assert_eq!(events.world.actors[&2].position, [0., 0., 0.]);
    }
    #[test]
    fn pause_or_replaced_block_releases_the_grip() {
        for replace in [false, true] {
            let (mut events, mesh, mut blocks) = room();
            step(
                &mut events,
                &mesh,
                &mut blocks,
                FieldInput {
                    interact: true,
                    ..Default::default()
                },
            );
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
