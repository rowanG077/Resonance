//! Input and saved ability selection; individual abilities are authored scripts.
use anyhow::{Context, Result};
use resonance_events::{
    EventRuntime,
    ring::{ITEM, SorcerersRing},
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Ring {
    pub event: Option<Arc<crate::authored::PreparedEvent>>,
    pub(super) active: Vec<i32>,
}

impl Ring {
    pub fn step(&mut self, events: &mut EventRuntime, pressed: bool) -> Result<()> {
        if !events.player_has_control() {
            return Ok(());
        }
        let Some(party) = events
            .world
            .party
            .as_mut()
            .filter(|party| party.items.get(&ITEM).is_some_and(|count| *count > 0))
        else {
            return Ok(());
        };
        let held = party.travel.sorcerers_ring == SorcerersRing::Sunlight
            && events
                .world
                .input
                .held
                .contains(resonance_events::input::Button::Ring);
        if !pressed && !held {
            return Ok(());
        }
        if party.travel.sorcerers_ring == SorcerersRing::Disabled {
            party.travel.sorcerers_ring = SorcerersRing::Fire;
        }
        const ELECTRIC_ORB_CAPACITY: usize = 2;
        let capacity = match party.travel.sorcerers_ring {
            SorcerersRing::ElectricOrb(_) => ELECTRIC_ORB_CAPACITY,
            _ => 1,
        };
        self.active
            .retain(|h| events.world.has_authored_controller(*h));
        if self.active.len() >= capacity {
            return Ok(());
        }
        self.active.push(
            self.event
                .as_ref()
                .context("ring script was not prepared")?
                .start_with_arguments(events, &[])?,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::field::CollisionGroup;
    use resonance_events::{
        Actor, Animation, AnimationClip, Enemy, ModelResource, ResourceLibrary,
    };
    use symphonia_script::Program;

    #[derive(Clone, Copy)]
    #[repr(i32)]
    enum Shot {
        Fire,
        Water,
        LongRangeFire = 3,
    }
    fn cast(kind: Shot) -> (EventRuntime, i32) {
        start("field::ring_projectile::cast", &[kind as i32])
    }

    fn start(task: &str, arguments: &[i32]) -> (EventRuntime, i32) {
        let legacy = Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap();
        start_with_legacy(task, arguments, legacy)
    }

    fn ring_program() -> Arc<Program> {
        let sources = symphonia_script_tools::SourceTree::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts"
        ))
        .unwrap();
        Arc::new(
            symphonia_script_compiler::compile(
                "field::ring",
                &sources,
                &resonance_events::authored::native_declarations(),
            )
            .unwrap()
            .program,
        )
    }

    fn start_with_legacy(task: &str, arguments: &[i32], legacy: Program) -> (EventRuntime, i32) {
        let mut resources = ResourceLibrary::default();
        resources.models.insert(
            1,
            ModelResource {
                clips: [
                    resonance_events::animation::slot::IDLE,
                    resonance_events::animation::slot::STAGGER,
                ]
                .into_iter()
                .map(|slot| {
                    (
                        slot,
                        AnimationClip {
                            duration_ticks: 30,
                            attachments: None,
                        },
                    )
                })
                .collect(),
                ..Default::default()
            },
        );
        resources.animations.insert(
            resonance_content::field::FIELD_SERVICE_MOTION_RESOURCE_BASE + 1,
            [(
                resonance_content::field::ServiceMotion::CastRing as u16,
                AnimationClip {
                    duration_ticks: 30,
                    attachments: None,
                },
            )]
            .into(),
        );
        resources.models.insert(
            resonance_content::field::LOCAL_MODEL_RESOURCES.start,
            ModelResource::default(),
        );
        resources.models.insert(
            resonance_content::field::LOCAL_MODEL_RESOURCES.start + 12,
            ModelResource::default(),
        );
        let mut events = EventRuntime::new(Arc::new(legacy), Arc::new(resources)).unwrap();
        events.world.controlled_actor = 1;
        events.world.input_enabled = true;
        let mut player = Actor::new(1, [0.; 3]);
        player.animation = Some(Animation::new(
            1,
            resonance_events::animation::slot::IDLE,
            60,
            0,
        ));
        events.world.insert_actor(1, player);
        let handle = events
            .start_authored(ring_program(), task, arguments)
            .unwrap();
        (events, handle)
    }

    fn solid_box(low: [f32; 3], high: [f32; 3]) -> Arc<resonance_content::field::ModelCollision> {
        Arc::new(resonance_content::field::ModelCollision {
            solids: vec![CollisionGroup {
                surface: 0,
                vertices: (0..8)
                    .map(|corner| {
                        std::array::from_fn(|axis| {
                            if corner & (1 << axis) == 0 {
                                low[axis]
                            } else {
                                high[axis]
                            }
                        })
                    })
                    .collect(),
                triangles: vec![
                    [0, 2, 1],
                    [1, 2, 3],
                    [4, 5, 6],
                    [5, 7, 6],
                    [0, 4, 2],
                    [2, 4, 6],
                    [1, 3, 5],
                    [3, 7, 5],
                    [0, 1, 4],
                    [1, 5, 4],
                    [2, 6, 3],
                    [3, 6, 7],
                ],
            }],
            ..Default::default()
        })
    }

    fn wall(events: &mut EventRuntime, front: f32) {
        let mut actor = Actor::new(resonance_content::field::SCENERY_RESOURCE_BASE, [0.; 3]);
        actor.model_collision = Some(solid_box([-100., front - 1., 0.], [100., front, 300.]));
        events.world.insert_actor(3, actor);
    }

    fn step(events: &mut EventRuntime) {
        events.step().unwrap();
    }

    #[test]
    fn cast_winds_up_then_miss_expires_and_releases_its_pose() {
        let (mut events, _) = cast(Shot::Fire);
        // Visible memory circles have no native contact shape and cannot stop a shot.
        let mut circle = Actor::new(
            resonance_content::field::SAVE_POINT_RESOURCE,
            [0., -30., 0.],
        );
        circle.contact = resonance_events::ActorContact::None;
        events.world.insert_actor(2, circle);
        for _ in 0..8 {
            step(&mut events);
        }
        assert!(events.world.projectiles.is_empty());
        assert!(!events.player_has_control());
        step(&mut events);
        let shot = events.world.projectiles.values().next().unwrap();
        assert_eq!(shot.position, [0., 0., 100.]);
        step(&mut events);
        assert_eq!(
            events.world.projectiles.values().next().unwrap().position,
            [0., -15., 100.]
        );
        assert!(!events.world.billboards.is_empty());
        for _ in 0..30 {
            step(&mut events);
        }
        assert!(events.player_has_control());
        assert!(events.world.projectiles.is_empty());
        assert_eq!(
            events.world.actors[&1].animation.as_ref().unwrap().slot,
            resonance_events::animation::slot::IDLE
        );
        assert!(!events.world.actors[&1].scripted_animation);
    }

    #[test]
    fn cancelling_a_flying_shot_retires_its_trail_and_restores_control() {
        let (mut events, handle) = cast(Shot::Fire);
        for _ in 0..10 {
            step(&mut events);
        }
        assert!(!events.world.billboards.is_empty());
        events.cancel_authored(handle).unwrap();
        assert!(events.world.projectiles.is_empty());
        assert!(events.world.billboards.is_empty());
        assert_eq!(
            events.world.actors[&1].animation.as_ref().unwrap().slot,
            resonance_events::animation::slot::IDLE
        );
        assert!(events.player_has_control());
    }

    #[test]
    fn model_barriers_follow_live_transforms_and_the_ring_contact_mask() {
        for (scale, offset, masked, blocks) in [
            (100, 0., false, false),
            (200, 0., false, true),
            (200, 200., false, false),
            (200, 0., true, false),
        ] {
            let (mut events, _) = cast(Shot::Fire);
            let mut prop = Actor::new(
                resonance_content::field::SCENERY_RESOURCE_BASE,
                [offset, -100., 0.],
            );
            prop.face(90.);
            prop.properties
                .extend([(31, scale), (48, i32::from(masked))]);
            prop.model_collision = Some(solid_box([-20., 20., 0.], [20., 30., 300.]));
            events.world.actors.get_mut(&1).unwrap().position[0] = -55.;
            events.world.insert_actor(2, prop);
            for _ in 0..18 {
                step(&mut events);
            }
            assert_eq!(events.world.projectiles.is_empty(), blocks);
            if !blocks {
                assert!(events.world.projectiles.values().next().unwrap().position[1] < -100.);
            }
        }
    }

    #[test]
    fn wall_blocks_enemy_hit_and_unobstructed_hit_stuns_until_recovery() {
        for (blocked, kind, stuns) in [
            (true, Shot::Fire, false),
            (true, Shot::LongRangeFire, false),
            (false, Shot::Water, false),
            (false, Shot::Fire, true),
            (false, Shot::LongRangeFire, true),
        ] {
            let (mut events, _) = cast(kind);
            if blocked {
                wall(&mut events, -100.);
            }
            let mut enemy = Actor::new(2, [0., -200., 0.]);
            enemy.autonomy = Some(resonance_events::Autonomy::new(
                resonance_events::Behavior::Stationary,
                0.,
                enemy.position,
            ));
            enemy.enemy = Some(Enemy {
                event: 0,
                behavior: 0,
                normal_speed: 0.,
                alert_speed: 0.,
                random_turns: false,
                chase_on_sight: false,
                sight_angle: 0.,
                sight_distance: 0.,
                alerted: false,
                event_parameters: [0; 2],
                contact_cooldown: 0,
                pause_effect_mode: 0,
                stun: None,
            });
            events.world.insert_actor(2, enemy);
            for _ in 0..40 {
                if matches!(kind, Shot::LongRangeFire) {
                    for shot in events.world.projectiles.values_mut() {
                        shot.velocity[1] = -1000.;
                    }
                }
                step(&mut events);
            }
            let stun = events.world.actors[&2].enemy.as_ref().unwrap().stun;
            assert_eq!(stun.is_some(), stuns);
            if let Some(remaining) = stun {
                assert!(!events.contact_enemy(2).unwrap());
                for _ in 1..remaining.remaining.get() {
                    step(&mut events);
                }
                assert!(
                    events.world.actors[&2]
                        .enemy
                        .as_ref()
                        .unwrap()
                        .stun
                        .is_some()
                );
                step(&mut events);
                assert!(
                    events.world.actors[&2]
                        .enemy
                        .as_ref()
                        .unwrap()
                        .stun
                        .is_none()
                );
            }
        }
    }
}
