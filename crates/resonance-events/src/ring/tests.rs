use super::SorcerersRing as Ability;
use super::*;
use crate::{Actor, Animation, AnimationClip, EventRuntime, ModelResource, ResourceLibrary};
use resonance_content::field::{FIELD_SERVICE_MOTION_RESOURCE_BASE, ServiceMotion};
use resonance_content::test_support::solid_box;
use std::sync::Arc;
use symphonia_script::{NativeCall, Program, scenario};

fn field(ability: Ability, callback: Option<(u32, &str)>) -> EventRuntime {
    let header = match callback {
        Some((key, _)) => format!(
            ".code_base 10\n.word 10\n.word 0\n.word 0\n.word 1\n.word 0\n.word 0\n.word {}\n.word {}\n.word 0\n.word 1\n",
            key >> 16,
            key & 65535
        ),
        None => ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n".into(),
    };
    let source = scenario::assemble(&format!(
        ".scenario\n{header}end\n{}",
        callback.map_or("", |(_, source)| source)
    ))
    .unwrap();
    let clip = || AnimationClip {
        duration_ticks: 30,
        attachments: None,
    };
    let mut resources = ResourceLibrary::default();
    resources.models.insert(
        1,
        ModelResource {
            clips: [
                (crate::animation::slot::IDLE, clip()),
                (crate::animation::slot::STAGGER, clip()),
            ]
            .into(),
            ..Default::default()
        },
    );
    resources.models.insert(
        resonance_content::field::RING_BOMB_RESOURCE,
        ModelResource::default(),
    );
    resources.animations.insert(
        FIELD_SERVICE_MOTION_RESOURCE_BASE + 1,
        [(ServiceMotion::CastRing as u16, clip())].into(),
    );
    let mut events = EventRuntime::new(
        Arc::new(Program::decode(&source).unwrap()),
        Arc::new(resources),
    )
    .unwrap();
    events.world.current_field = Some(if ability == Ability::Bomb { 412 } else { 511 });
    events.world.controlled_actor = 1;
    events.world.input_enabled = true;
    let mut actor = Actor::new(1, [0.; 3]);
    actor.animation = Some(Animation::new(1, crate::animation::slot::IDLE, 30, 0));
    events.world.insert_actor(1, actor);
    let mut party =
        crate::party::Party::new(&crate::party::tests::data(), Default::default()).unwrap();
    party.items.insert(ITEM, 1);
    party.members[0].tp = 100;
    party.travel.sorcerers_ring = ability;
    party.travel.ring_timer = 2000;
    events.world.party = Some(party);
    events
}
fn cast(ability: Ability) -> EventRuntime {
    let mut events = field(ability, None);
    events.activate_ring(true).unwrap();
    events
}
fn steps(events: &mut EventRuntime, count: u32) {
    for _ in 0..count {
        events.step().unwrap();
    }
}
fn enemy(position: [f32; 3]) -> Actor {
    let mut actor = Actor::new(1, position);
    actor.autonomy = Some(crate::Autonomy::new(
        crate::Behavior::Stationary,
        0.,
        position,
    ));
    actor.enemy = Some(crate::Enemy {
        event: 0,
        behavior: 0,
        normal_speed: 0.,
        alert_speed: 0.,
        random_turns: 0,
        chase_on_sight: false,
        sight_angle: 0.,
        sight_distance: 0.,
        alerted: false,
        event_parameters: [0; 2],
        pause_ticks: 0,
        reaction: crate::effect::StunEffect::None,
    });
    actor
}
fn wall(events: &mut EventRuntime, front: f32) {
    let mut actor = Actor::new(resonance_content::field::SCENERY_RESOURCE_BASE, [0.; 3]);
    actor.model_collision = Some(solid_box([-100., front - 1., 0.], [100., front, 300.]));
    events.world.insert_actor(3, actor);
}

#[test]
fn every_projectile_emits_expires_and_restores_control() {
    for ability in [
        Ability::Fire,
        Ability::Water,
        Ability::Wind,
        Ability::LongRangeFire,
        Ability::Mana,
        Ability::Lightning(LightningColor::Blue),
        Ability::Ice,
        Ability::Darkness,
    ] {
        let mut events = cast(ability);
        assert!(!events.player_has_control());
        let mut emitted = false;
        for _ in 0..400 {
            events.step().unwrap();
            emitted |= !events.world.billboards.is_empty();
        }
        assert!(emitted, "{ability:?}");
        assert!(events.world.billboards.is_empty());
        assert!(events.player_has_control());
        assert!(!events.world.actors[&1].scripted_animation);
        assert_eq!(
            events.world.actors[&1].animation.as_ref().unwrap().slot,
            crate::animation::slot::IDLE
        );
        assert_eq!(
            events.world.party.as_ref().unwrap().members[0].tp,
            if ability == Ability::Mana { 90 } else { 100 }
        );
    }
}
#[test]
fn orb_pause_freezes_movement_interactions_and_expiry_together() {
    for kind in [ElectricOrbKind::Sylvarant, ElectricOrbKind::Tethealla] {
        let mut events = cast(Ability::ElectricOrb(kind));
        steps(&mut events, 12);
        let position = events.world.ring_shadows().next().unwrap();
        events.world.mapped_input_disabled = true;
        events.world.insert_actor(2, enemy(position));
        steps(&mut events, 400);
        assert_eq!(events.world.ring_shadows().next().unwrap(), position);
        assert_eq!(
            events.world.actors[&2].enemy.as_ref().unwrap().pause_ticks,
            0
        );
        events.world.mapped_input_disabled = false;
        steps(&mut events, 2);
        assert!(events.world.actors[&2].enemy.as_ref().unwrap().pause_ticks > 0);
        steps(&mut events, 400);
        assert!(events.player_has_control());
    }
}
#[test]
fn scene_cancellation_or_caster_removal_retires_particles() {
    for remove_caster in [false, true] {
        let mut events = cast(Ability::Fire);
        steps(&mut events, 12);
        assert!(!events.world.billboards.is_empty());
        if remove_caster {
            events.world.remove_actor(1);
            events.step().unwrap();
        } else {
            events.cancel();
            assert!(!events.world.actors[&1].scripted_animation);
            assert_eq!(
                events.world.actors[&1].animation.as_ref().unwrap().slot,
                crate::animation::slot::IDLE
            );
        }
        assert!(events.world.billboards.is_empty());
    }
}

#[test]
fn model_barriers_follow_live_transforms_and_the_ring_contact_mask() {
    for (scale, offset, masked, blocks) in [
        (100, 0., false, false),
        (200, 0., false, true),
        (200, 200., false, false),
        (200, 0., true, false),
    ] {
        let callback = format!(
            "push.s16 321\ncalc 0\narg\nproc {}\nend\n",
            NativeCall::SetEventBit as u8
        );
        let mut events = field(Ability::Fire, Some((CALLBACK, &callback)));
        events
            .world
            .insert_actor(4, Actor::new(1, [-55., -150., 0.]));
        events.activate_ring(true).unwrap();
        let mut prop = Actor::new(
            resonance_content::field::SCENERY_RESOURCE_BASE,
            [offset, -100., 0.],
        );
        prop.face(90.);
        prop.scale_percent[1] = scale;
        prop.ring_contact_disabled = masked;
        prop.model_collision = Some(solid_box([-20., 20., 0.], [20., 30., 300.]));
        events.world.actors.get_mut(&1).unwrap().position[0] = -55.;
        events.world.insert_actor(2, prop);
        steps(&mut events, 40);
        assert_eq!(events.world.event_flags.contains(&321), !blocks);
    }
}
#[test]
fn walls_prevent_stuns_and_unobstructed_enemies_recover() {
    for (blocked, ability, stuns) in [
        (true, Ability::Fire, false),
        (true, Ability::LongRangeFire, false),
        (false, Ability::Water, false),
        (false, Ability::Fire, true),
        (false, Ability::LongRangeFire, true),
    ] {
        let mut events = cast(ability);
        if blocked {
            wall(&mut events, -100.);
        }
        events.world.insert_actor(2, enemy([0., -200., 0.]));
        steps(&mut events, 40);
        assert_eq!(
            events.world.actors[&2].enemy.as_ref().unwrap().pause_ticks > 0,
            stuns
        );
        steps(&mut events, 300);
        assert_eq!(
            events.world.actors[&2].enemy.as_ref().unwrap().pause_ticks,
            0
        );
    }
}
#[test]
fn sunlight_reports_each_continuous_exposure_once() {
    let callback = format!(
        "push.s16 321\ncalc 0\narg\nproc {}\nproc {}\nend\n",
        NativeCall::SetEventBit as u8,
        NativeCall::GetEventActor as u8
    );
    for (key, exposure) in [(CALLBACK, 1), (SECONDARY_CALLBACK, 240)] {
        let mut events = field(Ability::Sunlight, Some((key, &callback)));
        events.world.input.held = [crate::input::Button::Ring].into_iter().collect();
        events.activate_ring(true).unwrap();
        assert!(events.player_has_control());
        assert!(events.world.menu_blocked());
        steps(&mut events, 30);
        for id in [16, 17] {
            let mut target = Actor::new(1, [0., -200., 100.]);
            target.role = crate::ActorRole::Interaction;
            events.world.insert_actor(id, target);
            steps(&mut events, exposure + 1);
            assert!(events.world.event_flags.contains(&321));
            assert_eq!(
                events
                    .memory()
                    .read(0x20, symphonia_script::Width::S32)
                    .unwrap(),
                id
            );
            events.world.event_flags.remove(&321);
            steps(&mut events, 30);
            assert!(!events.world.event_flags.contains(&321));
            events.world.actors.remove(&id);
        }
        // A gap must restart the long exposure, even for the same actor.
        events
            .world
            .insert_actor(17, Actor::new(1, [0., -200., 100.]));
        events.world.actors.get_mut(&17).unwrap().role = crate::ActorRole::Interaction;
        steps(&mut events, 1);
        events.world.actors.get_mut(&17).unwrap().position[0] = 1000.;
        steps(&mut events, 2);
        events.world.event_flags.remove(&321);
        events.world.actors.get_mut(&17).unwrap().position[0] = 0.;
        steps(&mut events, exposure.saturating_sub(1));
        assert!(!events.world.event_flags.contains(&321));
        steps(&mut events, 2);
        assert!(events.world.event_flags.contains(&321));
        assert_eq!(
            events
                .memory()
                .read(0x20, symphonia_script::Width::S32)
                .unwrap(),
            17
        );
        events.world.input.held = Default::default();
        steps(&mut events, 60);
        assert!(events.world.model_particles.is_empty());
        assert!(!events.world.menu_blocked());
    }
}
#[test]
fn ring_hit_uses_nearest_target_and_keeps_control_until_callback_finishes() {
    let callback = format!(
        "proc {}\npush.s8 0\ncalc 0\narg\npush.s8 60\ncalc 0\narg\nproc {}\nend\n",
        NativeCall::GetEventActor as u8,
        NativeCall::YieldCommand as u8
    );
    for order in [[2, 3], [3, 2]] {
        let mut events = field(Ability::Fire, Some((CALLBACK, &callback)));
        for id in order {
            let mut target = Actor::new(1, [0., if id == 2 { -80. } else { -50. }, 0.]);
            target.radius = 1.;
            events.world.insert_actor(id, target);
        }
        events.activate_ring(true).unwrap();
        steps(&mut events, 40);
        assert_eq!(
            events
                .memory()
                .read(0x20, symphonia_script::Width::S32)
                .unwrap(),
            3
        );
        assert!(!events.player_has_control());
        steps(&mut events, 60);
        assert!(events.player_has_control());
    }
}

#[test]
fn area_and_transformation_powers_finish_without_leaving_control_or_visuals() {
    for ability in [
        Ability::Shrink,
        Ability::Radar,
        Ability::Bomb,
        Ability::Earthquake,
        Ability::Sound,
        Ability::AnimalCall(CallColor::Pink),
        Ability::Bubble(BubblePhase::Release),
    ] {
        let mut events = cast(ability);
        let mut visible = false;
        for _ in 0..900 {
            events.step().unwrap();
            visible |= !events.world.billboards.is_empty()
                || !events.world.model_particles.is_empty()
                || events.world.fog().is_some()
                || events.world.player_size == crate::PlayerSize::Small;
        }
        assert!(visible, "{ability:?}");
        assert!(events.player_has_control(), "{ability:?}");
        assert!(events.world.billboards.is_empty());
        assert!(events.world.model_particles.is_empty());
        assert!(events.world.fog().is_none());
        assert!(!events.world.menu_blocked());
        assert_eq!(events.world.actors.len(), 1);
        assert!(!events.world.actors[&1].scripted_animation);
    }
}
#[test]
fn bubble_callback_can_keep_the_player_floating_before_release() {
    let callback = format!(
        "push.s8 19\ncalc 0\narg\npush.s8 1\ncalc 0\narg\nproc {}\nend\n",
        NativeCall::ConfigureSorcerersRing as u8
    );
    let mut events = field(
        Ability::Bubble(BubblePhase::Release),
        Some((CALLBACK, &callback)),
    );
    Arc::get_mut(&mut events.resources).unwrap().session_data =
        Some(Arc::new(crate::party::tests::data()));
    events.activate_ring(true).unwrap();
    steps(&mut events, 200);
    assert!(!events.world.model_particles.is_empty());
    let height = events.world.actors[&1].visual_lift.as_ref().unwrap().height;
    events.world.mapped_input_disabled = true;
    steps(&mut events, 20);
    assert_ne!(
        events.world.actors[&1].visual_lift.as_ref().unwrap().height,
        height
    );
    events.world.party.as_mut().unwrap().travel.sorcerers_ring =
        Ability::Bubble(BubblePhase::Release);
    steps(&mut events, 100);
    assert!(events.world.model_particles.is_empty());
    assert!(events.world.actors[&1].visual_lift.is_none());
    events.world.mapped_input_disabled = false;
    assert!(events.player_has_control());
}

#[test]
fn insufficient_mana_calls_only_the_secondary_callback_without_spending_tp() {
    let callback = format!(
        "push.s8 42\ncalc 0\narg\nproc {}\nend\n",
        NativeCall::SetEventBit as u8
    );
    let mut events = field(Ability::Mana, Some((SECONDARY_CALLBACK, &callback)));
    events.world.party.as_mut().unwrap().members[0].tp = 9;
    events.activate_ring(true).unwrap();
    steps(&mut events, 60);
    assert!(events.world.event_flags.contains(&42));
    assert_eq!(events.world.party.as_ref().unwrap().members[0].tp, 9);
    assert!(events.world.billboards.is_empty());
    assert!(events.player_has_control());
}
#[test]
fn callback_errors_retire_particles_and_restore_the_pose() {
    let mut events = field(Ability::Fire, Some((CALLBACK, "proc 118\nend\n")));
    events.world.insert_actor(2, Actor::new(1, [0., -60., 0.]));
    events.activate_ring(true).unwrap();
    let error = (0..100)
        .find_map(|_| events.step().err())
        .expect("callback must fail");
    assert!(error.to_string().contains("event"));
    assert_eq!(events.active_instances(), 0);
    assert!(!events.world.actors[&1].scripted_animation);
    assert!(events.world.billboards.is_empty());
    assert!(events.world.refractions.is_empty());
}

#[test]
fn bomb_callback_measures_the_placed_bomb_after_it_becomes_hidden() {
    let callback = format!(
        "push.s8 1\ncalc 0\narg\npush.s32 {SCRIPT_ACTOR}\ncalc 0\narg\npush.s8 2\ncalc 0\narg\nproc {}\nend\n",
        NativeCall::MeasureActorGeometry as u8
    );
    let mut events = field(Ability::Bomb, Some((CALLBACK, &callback)));
    events.world.insert_actor(2, Actor::new(1, [500., 0., 0.]));
    events.activate_ring(true).unwrap();
    steps(&mut events, 10);
    events.world.actors.get_mut(&1).unwrap().position[0] = 1000.;
    steps(&mut events, 190);
    assert_eq!(
        events
            .memory()
            .read(0x20, symphonia_script::Width::S32)
            .unwrap(),
        500
    );
    assert!(
        events
            .world
            .actors
            .values()
            .any(|a| a.resource == resonance_content::field::RING_BOMB_RESOURCE && !a.visible)
    );
    steps(&mut events, 100);
    assert_eq!(events.world.actors.len(), 2);
}
